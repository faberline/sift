//! Persist framed records before returning them to the delivery loop.
use super::source::{CommitStats, DynCollectorSource, ReadOutcome};
use crate::collector::domain::record::{RawRecord, SourceCursor, SourceRejection};
use anyhow::{bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use service_collector::CollectorSource;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Default, Deserialize, Serialize)]
struct FramingState {
    ready: Vec<RawRecord>,
    pending: Option<RawRecord>,
    #[serde(default)]
    rejection: Option<SourceRejection>,
}

pub(crate) struct MultilineSource {
    source: Box<DynCollectorSource>,
    start: Option<Regex>,
    path: PathBuf,
    state: FramingState,
    emitted: usize,
    replay_end: BTreeMap<String, u64>,
    follow: bool,
    changed: Instant,
    rejection_emitted: bool,
}

fn end(cursor: &SourceCursor) -> u64 {
    match cursor {
        SourceCursor::Linear { next_offset, .. } | SourceCursor::Cri { next_offset, .. } => {
            *next_offset
        }
        SourceCursor::CriLoss { .. } => 0,
    }
}

impl MultilineSource {
    pub fn open(
        source: Box<DynCollectorSource>,
        path: PathBuf,
        pattern: Option<&str>,
        follow: bool,
    ) -> Result<Self> {
        let state: FramingState = if path.exists() {
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
                bail!("collector framing queue must be a regular file of at most 64 MiB");
            }
            serde_json::from_slice(&std::fs::read(&path)?)?
        } else {
            FramingState::default()
        };
        let mut replay_end = BTreeMap::new();
        for record in state.ready.iter().chain(state.pending.iter()) {
            let watermark = replay_end.entry(record.source_id.clone()).or_insert(0);
            *watermark = (*watermark).max(end(&record.cursor));
        }
        Ok(Self {
            source,
            path,
            state,
            start: pattern.map(Regex::new).transpose()?,
            emitted: 0,
            replay_end,
            follow,
            changed: Instant::now(),
            rejection_emitted: false,
        })
    }
    fn save(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.state)?;
        if bytes.len() > 64 * 1024 * 1024 {
            bail!("collector framing queue exceeds 64 MiB");
        }
        storage_durable::atomic_write(&self.path, &bytes, storage_durable::FsyncPolicy::Always)?;
        Ok(())
    }
    fn finish(&mut self) -> Result<()> {
        if let Some(record) = self.state.pending.take() {
            self.state.ready.push(record);
        }
        self.save()
    }
}

impl CollectorSource for MultilineSource {
    type Cursor = SourceCursor;
    type Error = anyhow::Error;
    type Record = RawRecord;
    type Rejection = SourceRejection;

    fn next_record(&mut self, max_bytes: usize) -> Result<ReadOutcome> {
        loop {
            if let Some(record) = self.state.ready.get(self.emitted).cloned() {
                self.emitted += 1;
                if record.bytes.len() > max_bytes {
                    return Ok(ReadOutcome::Rejection(SourceRejection {
                        entry: crate::collector::domain::quarantine::QuarantineEntry::invalid_line(
                            &record.source_id,
                            record.line,
                            record.offset,
                            "record_too_large",
                            "assembled log exceeds the record byte limit",
                            &record.bytes,
                        ),
                        cursor: record.cursor,
                    }));
                }
                return Ok(ReadOutcome::Record(record));
            }
            if let Some(rejection) = &self.state.rejection {
                if self.rejection_emitted {
                    return Ok(ReadOutcome::Pending);
                }
                self.rejection_emitted = true;
                return Ok(ReadOutcome::Rejection(rejection.clone()));
            }
            match self.source.next_record(max_bytes)? {
                ReadOutcome::Record(mut record) => {
                    if self
                        .replay_end
                        .get(&record.source_id)
                        .is_some_and(|limit| end(&record.cursor) <= *limit)
                    {
                        continue;
                    }
                    record
                        .enrichment
                        .attributes
                        .entry("collector.observed_at".into())
                        .or_insert_with(|| {
                            crate::AttributeValue::String(chrono::Utc::now().to_rfc3339())
                        });
                    let starts = self.start.as_ref().is_none_or(|pattern| {
                        pattern.is_match(&String::from_utf8_lossy(&record.bytes))
                    });
                    let same_source = self
                        .state
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.source_id == record.source_id);
                    if starts || !same_source {
                        self.finish()?;
                    }
                    if let Some(pending) = &mut self.state.pending {
                        if !pending.bytes.ends_with(b"\n") {
                            pending.bytes.push(b'\n');
                        }
                        let remaining = max_bytes
                            .saturating_add(1)
                            .saturating_sub(pending.bytes.len());
                        pending
                            .bytes
                            .extend_from_slice(&record.bytes[..record.bytes.len().min(remaining)]);
                        pending.cursor = record.cursor;
                    } else {
                        self.state.pending = Some(record);
                    }
                    self.changed = Instant::now();
                    if self.start.is_none() {
                        self.finish()?;
                    } else {
                        self.save()?;
                    }
                }
                ReadOutcome::Pending => {
                    if self.state.pending.is_some()
                        && self.changed.elapsed() >= Duration::from_secs(1)
                    {
                        self.finish()?;
                        continue;
                    }
                    return Ok(ReadOutcome::Pending);
                }
                ReadOutcome::Exhausted => {
                    if self.state.pending.is_some()
                        && (!self.follow || self.changed.elapsed() >= Duration::from_secs(1))
                    {
                        self.finish()?;
                        continue;
                    }
                    return Ok(ReadOutcome::Exhausted);
                }
                ReadOutcome::Rejection(rejection) => {
                    // Flush preceding text first. Do not commit a loss cursor
                    // while its preceding group is still unacknowledged.
                    if self.state.pending.is_some() {
                        self.finish()?;
                    }
                    self.state.rejection = Some(rejection);
                    self.save()?;
                }
            }
        }
    }
    fn commit(&mut self, cursors: &[SourceCursor], stats: CommitStats) -> Result<()> {
        self.source.commit(cursors, stats)?;
        self.state.ready.drain(..self.emitted);
        self.emitted = 0;
        if self.rejection_emitted {
            self.state.rejection = None;
            self.rejection_emitted = false;
        }
        self.save()
    }
    fn refresh(&mut self) -> Result<()> {
        self.source.refresh()
    }
    fn progress(&self) -> service_collector::SourceProgress {
        self.source.progress()
    }
}
