//! The per-signal write-ahead log: framed event batches for logs, metrics and
//! traces, their recovery, paged reads and compaction.

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{shared_kernel::stored_event::StoredEvent, SignalKind};

use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;

pub struct SignalWal {
    logs: WalFile,
    metrics: WalFile,
    traces: WalFile,
    max_decoded_bytes: usize,
}

/// Stateful, globally ordered reader over the three signal WAL files.
///
/// Each file keeps one decoded Raft batch at most. A batch whose decoded events
/// pass SIFT_WAL_DECODED_BYTES is refused. Repeated pages continue
/// from the current byte offsets instead of rescanning the WAL prefix.
pub struct SignalWalReader {
    streams: Vec<WalEventStream>,
    peeked: Vec<Option<StoredEvent>>,
}

struct WalEventStream {
    signal: SignalKind,
    path: PathBuf,
    frames: storage_durable::FramedLogCursor,
    events: VecDeque<StoredEvent>,
    after: u64,
    max_decoded_bytes: usize,
}

struct WalFile {
    signal: SignalKind,
    path: PathBuf,
    writer: Mutex<storage_durable::FramedLogWriter>,
}

const WAL_BATCH_FORMAT_VERSION: u16 = 1;

pub(crate) const DEFAULT_WAL_DECODED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WalBatch {
    format_version: u16,
    first_cursor: u64,
    last_cursor: u64,
    events: Vec<StoredEvent>,
}

impl SignalWal {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().join("wal");
        let max_decoded_bytes = decoded_bytes_from_env()?;
        Ok(Self {
            logs: WalFile::open(&root, "logs", SignalKind::Log)?,
            metrics: WalFile::open(&root, "metrics", SignalKind::Metric)?,
            traces: WalFile::open(&root, "traces", SignalKind::Span)?,
            max_decoded_bytes,
        })
    }

    #[cfg(test)]
    fn with_max_decoded_bytes(mut self, bytes: usize) -> Result<Self> {
        if bytes == 0 {
            bail!("SIFT_WAL_DECODED_BYTES must be a positive integer");
        }
        self.max_decoded_bytes = bytes;
        Ok(self)
    }

    pub fn append(&self, event: &StoredEvent) -> Result<()> {
        self.append_batch(std::slice::from_ref(event))
    }

    /// Write one signal batch as one CRC frame and one fsync boundary.
    pub fn append_batch(&self, events: &[StoredEvent]) -> Result<()> {
        let signal = events
            .first()
            .context("signal WAL batch must not be empty")?
            .event
            .signal;
        self.file(signal)?.append_batch(events)
    }

    pub fn recovered_events(&self) -> Result<Vec<StoredEvent>> {
        let mut by_cursor = BTreeMap::<u64, StoredEvent>::new();
        let mut event_ids = HashSet::new();
        for file in [&self.logs, &self.metrics, &self.traces] {
            for event in file.recovered_events()? {
                if !event_ids.insert(event.event.event_id.clone()) {
                    bail!("WAL contains duplicate event_id {}", event.event.event_id);
                }
                if let Some(existing) = by_cursor.insert(event.cursor, event.clone()) {
                    bail!(
                        "WAL cursor {} belongs to both {} and {}",
                        event.cursor,
                        existing.event.event_id,
                        event.event.event_id
                    );
                }
            }
        }
        Ok(by_cursor.into_values().collect())
    }

    #[doc(hidden)]
    pub fn reader(&self, after: u64) -> Result<SignalWalReader> {
        let streams = [&self.logs, &self.metrics, &self.traces]
            .into_iter()
            .map(|file| {
                WalEventStream::open(
                    file.signal,
                    file.path.clone(),
                    after,
                    self.max_decoded_bytes,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SignalWalReader {
            peeked: vec![None; streams.len()],
            streams,
        })
    }

    pub(crate) fn compact_through(&self, watermarks: ArchiveWatermarks) -> Result<()> {
        for file in [&self.logs, &self.metrics, &self.traces] {
            file.compact_through(watermarks.through(file.signal))?;
        }
        Ok(())
    }

    fn file(&self, signal: SignalKind) -> Result<&WalFile> {
        match signal {
            SignalKind::Log => Ok(&self.logs),
            SignalKind::Metric => Ok(&self.metrics),
            SignalKind::Span => Ok(&self.traces),
        }
    }
}

impl SignalWalReader {
    /// Read at most `item_limit` events and approximately `byte_limit` encoded
    /// bytes. One event is always allowed so a large valid record can make
    /// progress.
    pub fn read_page(&mut self, item_limit: usize, byte_limit: usize) -> Result<Vec<StoredEvent>> {
        if item_limit == 0 || byte_limit == 0 {
            return Ok(Vec::new());
        }
        let mut page = Vec::with_capacity(item_limit.min(1_000));
        let mut bytes = 0_usize;
        while page.len() < item_limit {
            for index in 0..self.streams.len() {
                if self.peeked[index].is_none() {
                    self.peeked[index] = self.streams[index].next_event()?;
                }
            }
            let Some((selected, cursor)) = self
                .peeked
                .iter()
                .enumerate()
                .filter_map(|(index, event)| event.as_ref().map(|event| (index, event.cursor)))
                .min_by_key(|(_, cursor)| *cursor)
            else {
                break;
            };
            let encoded = serde_json::to_vec(
                self.peeked[selected]
                    .as_ref()
                    .expect("selected WAL event exists"),
            )?
            .len();
            if !page.is_empty() && bytes.saturating_add(encoded) > byte_limit {
                break;
            }
            let event = self.peeked[selected]
                .take()
                .expect("selected WAL event exists");
            for (index, duplicate) in self.peeked.iter_mut().enumerate() {
                if index == selected || duplicate.as_ref().map(|row| row.cursor) != Some(cursor) {
                    continue;
                }
                let duplicate = duplicate.take().expect("matched WAL event exists");
                if duplicate != event {
                    bail!("signal WAL files disagree at cursor {cursor}");
                }
            }
            bytes = bytes.saturating_add(encoded);
            page.push(event);
        }
        Ok(page)
    }

    #[doc(hidden)]
    pub fn buffered_event_count_for_diagnostics(&self) -> usize {
        self.peeked.iter().filter(|event| event.is_some()).count()
            + self
                .streams
                .iter()
                .map(|stream| stream.events.len())
                .sum::<usize>()
    }
}

impl WalEventStream {
    fn open(
        signal: SignalKind,
        path: PathBuf,
        after: u64,
        max_decoded_bytes: usize,
    ) -> Result<Self> {
        Ok(Self {
            signal,
            frames: storage_durable::FramedLogCursor::open(&path)?,
            path,
            events: VecDeque::new(),
            after,
            max_decoded_bytes,
        })
    }

    fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        loop {
            if let Some(event) = self.events.pop_front() {
                if event.cursor > self.after {
                    self.after = event.cursor;
                    return Ok(Some(event));
                }
                continue;
            }
            let Some(frame) = self.frames.next_frame()? else {
                return Ok(None);
            };
            let batch: WalBatch = serde_json::from_slice(frame.payload())
                .with_context(|| format!("decode signal WAL batch frame {}", frame.seq()))?;
            validate_batch(self.signal, &self.path, frame.seq(), &batch)?;
            let decoded = decoded_batch_bytes(&batch.events);
            if decoded > self.max_decoded_bytes {
                bail!(
                    "signal WAL batch {}..{} decodes to {decoded} bytes, over SIFT_WAL_DECODED_BYTES = {}",
                    batch.first_cursor,
                    batch.last_cursor,
                    self.max_decoded_bytes
                );
            }
            self.events = batch.events.into();
        }
    }
}

impl WalFile {
    fn open(root: &Path, name: &str, signal: SignalKind) -> Result<Self> {
        let directory = root.join(name);
        fs::create_dir_all(&directory)
            .with_context(|| format!("create signal WAL directory {}", directory.display()))?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("set signal WAL directory mode {}", directory.display()))?;
        let path = directory.join("events.framed");
        let writer =
            storage_durable::FramedLogWriter::open(&path, storage_durable::FsyncPolicy::Interval)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set signal WAL file mode {}", path.display()))?;
        Ok(Self {
            signal,
            path,
            writer: Mutex::new(writer),
        })
    }

    fn append_batch(&self, events: &[StoredEvent]) -> Result<()> {
        let first = events
            .first()
            .context("signal WAL batch must not be empty")?;
        if events.iter().any(|event| event.event.signal != self.signal) {
            bail!("event signal does not match WAL signal");
        }
        if events
            .windows(2)
            .any(|pair| pair[0].cursor.checked_add(1) != Some(pair[1].cursor))
        {
            bail!("signal WAL batch cursors must be contiguous");
        }
        let last = events.last().expect("non-empty signal WAL batch");
        let batch = WalBatch {
            format_version: WAL_BATCH_FORMAT_VERSION,
            first_cursor: first.cursor,
            last_cursor: last.cursor,
            events: events.to_vec(),
        };
        let mut writer = self.writer.lock().expect("signal WAL writer lock poisoned");
        writer
            .append(last.cursor, &serde_json::to_vec(&batch)?)
            .with_context(|| format!("append signal WAL batch {}", self.path.display()))?;
        writer
            .sync()
            .with_context(|| format!("fsync signal WAL batch {}", self.path.display()))?;
        Ok(())
    }

    fn recovered_events(&self) -> Result<Vec<StoredEvent>> {
        let mut events = Vec::new();
        for frame in storage_durable::FramedLogReader::read_frames(&self.path, 0)? {
            let batch: WalBatch = serde_json::from_slice(frame.payload())
                .with_context(|| format!("decode signal WAL batch frame {}", frame.seq()))?;
            if batch.format_version != WAL_BATCH_FORMAT_VERSION {
                bail!(
                    "unsupported signal WAL batch format {}",
                    batch.format_version
                );
            }
            let first = batch.events.first().with_context(|| {
                format!("signal WAL frame {} contains an empty batch", frame.seq())
            })?;
            let last = batch.events.last().expect("validated non-empty WAL batch");
            if batch.first_cursor != first.cursor
                || batch.last_cursor != last.cursor
                || batch.last_cursor != frame.seq()
            {
                bail!(
                    "signal WAL frame sequence {} does not match batch cursors {}..{}",
                    frame.seq(),
                    batch.first_cursor,
                    batch.last_cursor
                );
            }
            if batch
                .events
                .windows(2)
                .any(|pair| pair[0].cursor.checked_add(1) != Some(pair[1].cursor))
            {
                bail!("signal WAL batch cursors are not contiguous");
            }
            if batch
                .events
                .iter()
                .any(|event| event.event.signal != self.signal)
            {
                bail!(
                    "signal WAL {} contains the wrong signal",
                    self.path.display()
                );
            }
            events.extend(batch.events);
        }
        Ok(events)
    }

    fn compact_through(&self, cursor: u64) -> Result<()> {
        self.writer
            .lock()
            .expect("signal WAL writer lock poisoned")
            .truncate_through(cursor)
            .with_context(|| {
                format!(
                    "compact signal WAL {} through committed archive cursor {cursor}",
                    self.path.display()
                )
            })?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
}

/// The JSON size of the events of one decoded batch: what the byte bound counts.
fn decoded_batch_bytes(events: &[StoredEvent]) -> usize {
    events.iter().fold(0_usize, |total, event| {
        total.saturating_add(
            serde_json::to_vec(event)
                .map(|bytes| bytes.len())
                .unwrap_or(0),
        )
    })
}

fn decoded_bytes_from_env() -> Result<usize> {
    bytes_from(std::env::var("SIFT_WAL_DECODED_BYTES").ok().as_deref())
}

fn bytes_from(value: Option<&str>) -> Result<usize> {
    let Some(text) = value else {
        return Ok(DEFAULT_WAL_DECODED_BYTES);
    };
    let bytes: usize = text
        .parse()
        .context("SIFT_WAL_DECODED_BYTES must be a positive integer")?;
    if bytes == 0 {
        bail!("SIFT_WAL_DECODED_BYTES must be a positive integer");
    }
    Ok(bytes)
}

fn validate_batch(
    signal: SignalKind,
    path: &Path,
    frame_sequence: u64,
    batch: &WalBatch,
) -> Result<()> {
    if batch.format_version != WAL_BATCH_FORMAT_VERSION {
        bail!(
            "unsupported signal WAL batch format {}",
            batch.format_version
        );
    }
    let first = batch
        .events
        .first()
        .with_context(|| format!("signal WAL frame {frame_sequence} contains an empty batch"))?;
    let last = batch.events.last().expect("validated non-empty WAL batch");
    if batch.first_cursor != first.cursor
        || batch.last_cursor != last.cursor
        || batch.last_cursor != frame_sequence
    {
        bail!(
            "signal WAL frame sequence {frame_sequence} does not match batch cursors {}..{}",
            batch.first_cursor,
            batch.last_cursor
        );
    }
    if batch
        .events
        .windows(2)
        .any(|pair| pair[0].cursor.checked_add(1) != Some(pair[1].cursor))
    {
        bail!("signal WAL batch cursors are not contiguous");
    }
    if batch
        .events
        .iter()
        .any(|event| event.event.signal != signal)
    {
        bail!("signal WAL {} contains the wrong signal", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod wal_decoded_tests {
    use super::*;

    fn stored(cursor: u64, pad: usize) -> StoredEvent {
        let mut event = crate::EventEnvelope::for_project(
            "p",
            "prod",
            format!("e{cursor}"),
            SignalKind::Log,
            serde_json::json!({"message": "x".repeat(pad)}),
        );
        event.resource =
            std::collections::BTreeMap::from([("service.name".to_string(), "svc".to_string())]);
        StoredEvent {
            cursor,
            acknowledged_at: event.observed_at.clone(),
            event,
        }
    }

    fn three_events() -> Vec<StoredEvent> {
        vec![stored(1, 1000), stored(2, 1000), stored(3, 1000)]
    }

    #[test]
    fn batch_over_the_bound_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let events = three_events();
        let size = decoded_batch_bytes(&events);
        let wal = SignalWal::open(dir.path())
            .unwrap()
            .with_max_decoded_bytes(size - 1)
            .unwrap();
        wal.append_batch(&events).unwrap();
        let mut reader = wal.reader(0).unwrap();
        let error = reader.read_page(10, 1 << 20).unwrap_err().to_string();
        assert!(error.contains("SIFT_WAL_DECODED_BYTES"), "{error}");
        assert!(error.contains("1..3"), "{error}");
        assert!(error.contains(&size.to_string()), "{error}");
        assert!(error.contains(&(size - 1).to_string()), "{error}");
    }

    #[test]
    fn batch_at_the_bound_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let events = three_events();
        let size = decoded_batch_bytes(&events);
        let wal = SignalWal::open(dir.path())
            .unwrap()
            .with_max_decoded_bytes(size)
            .unwrap();
        wal.append_batch(&events).unwrap();
        let mut reader = wal.reader(0).unwrap();
        let page = reader.read_page(10, 1 << 20).unwrap();
        let cursors: Vec<u64> = page.iter().map(|event| event.cursor).collect();
        assert_eq!(cursors, vec![1, 2, 3]);
    }

    #[test]
    fn batch_reads_with_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let events = three_events();
        let wal = SignalWal::open(dir.path()).unwrap();
        wal.append_batch(&events).unwrap();
        let mut reader = wal.reader(0).unwrap();
        let page = reader.read_page(10, 1 << 20).unwrap();
        assert_eq!(page.len(), 3);
    }

    #[test]
    fn one_event_batches_are_each_charged() {
        let dir = tempfile::tempdir().unwrap();
        let limit = decoded_batch_bytes(&[stored(1, 1000)]);
        let wal = SignalWal::open(dir.path())
            .unwrap()
            .with_max_decoded_bytes(limit)
            .unwrap();
        for cursor in 1..=3 {
            wal.append_batch(&[stored(cursor, 1000)]).unwrap();
        }
        let mut reader = wal.reader(0).unwrap();
        let page = reader.read_page(10, 1 << 20).unwrap();
        assert_eq!(page.len(), 3);
    }

    #[test]
    fn zero_bound_is_refused_by_the_builder() {
        let dir = tempfile::tempdir().unwrap();
        let wal = SignalWal::open(dir.path()).unwrap();
        assert!(wal.with_max_decoded_bytes(0).is_err());
    }

    #[test]
    fn bytes_from_reads_the_setting() {
        assert_eq!(bytes_from(None).unwrap(), 67_108_864);
        assert_eq!(bytes_from(Some("4096")).unwrap(), 4096);
        assert!(bytes_from(Some("0")).is_err());
        assert!(bytes_from(Some("abc")).is_err());
        assert!(bytes_from(Some("-1")).is_err());
    }

    #[test]
    fn decoded_bytes_from_env_defaults_without_the_variable() {
        if std::env::var("SIFT_WAL_DECODED_BYTES").is_err() {
            assert_eq!(decoded_bytes_from_env().unwrap(), DEFAULT_WAL_DECODED_BYTES);
        }
    }
}
