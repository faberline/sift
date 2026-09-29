//! The on-disk store of sealed metric chunks: one file per chunk named by its
//! series, span and digest, and a framed ledger of obsolete keys removed once a
//! checkpoint commits.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use crate::projection::domain::metric::{MetricChunkV1, ResidentMetricChunk};
use crate::projection::domain::metric_memory_size::metric_chunk_memory_bytes;
use crate::projection::domain::metric_series::{sha256, validate_series_id, SealedMetricChunk};

pub(super) struct MetricChunkStore {
    root: PathBuf,
    obsolete: Mutex<storage_durable::FramedLogWriter>,
}

impl MetricChunkStore {
    pub(super) fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("create metric chunk root {}", root.display()))?;
        storage_durable::set_private_directory_mode(&root)?;
        let obsolete = storage_durable::FramedLogWriter::open(
            root.join("obsolete.framed"),
            storage_durable::FsyncPolicy::Always,
        )?;
        Ok(Self {
            root,
            obsolete: Mutex::new(obsolete),
        })
    }

    pub(super) fn write(
        &self,
        series_id: &str,
        chunk: &ResidentMetricChunk,
    ) -> Result<SealedMetricChunk> {
        if chunk.point_count == 0
            || chunk.point_count != chunk.points.len()
            || chunk.materialized_bytes == 0
        {
            bail!("resident metric chunk metadata mismatch before seal");
        }
        let first = chunk
            .points
            .first()
            .context("cannot seal an empty metric chunk")?;
        let last = chunk.points.last().context("metric chunk lost its tail")?;
        validate_series_id(series_id)?;
        let bytes = serde_json::to_vec(&chunk.chunk).context("encode sealed metric chunk")?;
        let digest = sha256(&bytes);
        let key = format!(
            "{series_id}/{:020}-{:020}-{digest}.json",
            first.cursor, last.cursor
        );
        let path = self.path_for(&key)?;
        let parent = path
            .parent()
            .context("metric chunk path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("create metric series directory {}", parent.display()))?;
        storage_durable::set_private_directory_mode(parent)?;
        storage_durable::atomic_write(&path, &bytes, storage_durable::FsyncPolicy::Always)
            .with_context(|| format!("persist sealed metric chunk {}", path.display()))?;
        storage_durable::set_private_file_mode(&path)?;
        Ok(SealedMetricChunk {
            key,
            start_time_unix_nano: chunk.start_time_unix_nano,
            end_time_unix_nano: chunk.end_time_unix_nano,
            point_count: chunk.point_count,
            encoded_bytes: bytes.len(),
            materialized_bytes: chunk.materialized_bytes,
            sha256: digest,
        })
    }

    pub(super) fn read(&self, sealed: &SealedMetricChunk) -> Result<MetricChunkV1> {
        let path = self.path_for(&sealed.key)?;
        let bytes = fs::read(&path)
            .with_context(|| format!("read sealed metric chunk {}", path.display()))?;
        if sha256(&bytes) != sealed.sha256 {
            bail!("sealed metric chunk {} checksum mismatch", sealed.key);
        }
        let chunk: MetricChunkV1 =
            serde_json::from_slice(&bytes).context("decode sealed metric chunk")?;
        if chunk.points.len() != sealed.point_count
            || bytes.len() != sealed.encoded_bytes
            || metric_chunk_memory_bytes(&chunk)? != sealed.materialized_bytes
            || chunk.start_time_unix_nano != sealed.start_time_unix_nano
            || chunk.end_time_unix_nano != sealed.end_time_unix_nano
            || chunk.points.is_empty()
        {
            bail!("sealed metric chunk {} metadata mismatch", sealed.key);
        }
        Ok(chunk)
    }

    pub(super) fn mark_obsolete(&self, cursor: u64, keys: &[String]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut ledger = self
            .obsolete
            .lock()
            .expect("metric obsolete ledger lock poisoned");
        for key in keys {
            self.path_for(key)?;
            ledger.append(cursor, key.as_bytes())?;
        }
        ledger.sync_strict()
    }

    pub(super) fn cleanup_obsolete(
        &self,
        committed_cursor: u64,
        referenced: &BTreeSet<String>,
    ) -> Result<()> {
        let mut ledger = self
            .obsolete
            .lock()
            .expect("metric obsolete ledger lock poisoned");
        ledger.flush()?;
        let ledger_path = self.root.join("obsolete.framed");
        let mut cursor = storage_durable::FramedLogCursor::open(&ledger_path)?;
        let mut safe_through = None;
        while let Some(frame) = cursor.next_frame()? {
            if frame.seq > committed_cursor {
                break;
            }
            let key =
                String::from_utf8(frame.payload).context("decode obsolete metric chunk key")?;
            if !referenced.contains(&key) {
                self.remove_key(&key)?;
            }
            safe_through = Some(frame.seq);
        }
        drop(cursor);
        if let Some(through) = safe_through {
            ledger.truncate_through(through)?;
        }
        Ok(())
    }

    fn remove_key(&self, key: &str) -> Result<()> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path) {
            Ok(()) => storage_durable::sync_parent_dir(&path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("remove metric chunk {}", path.display()))
            }
        }
    }

    fn path_for(&self, key: &str) -> Result<PathBuf> {
        let relative = Path::new(key);
        let components = relative.components().collect::<Vec<_>>();
        if components.len() != 2
            || components
                .iter()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("metric chunk key is not a safe relative path");
        }
        let series_id = components[0].as_os_str().to_string_lossy();
        validate_series_id(&series_id)?;
        Ok(self.root.join(relative))
    }
}
