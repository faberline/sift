//! Drain a renamed file before following its replacement. Persist inode offsets.
mod checkpoint;
use super::{
    checkpoint::CollectorCheckpoint,
    source::{read_bounded_line, CommitStats, ReadOutcome},
};
use crate::collector::domain::{
    quarantine::QuarantineEntry,
    record::{RawRecord, RecordEnrichment, SourceCursor, SourceRejection},
};
use anyhow::{bail, Context, Result};
use checkpoint::{identity, FileCheckpoint, Position};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, Seek, SeekFrom},
    path::PathBuf,
};

struct OpenFile {
    reader: BufReader<File>,
    position: Position,
}
pub(super) struct FileSource {
    path: PathBuf,
    checkpoint_path: PathBuf,
    state: FileCheckpoint,
    open: BTreeMap<String, OpenFile>,
    lost: Option<SourceRejection>,
    start: u64,
}
impl FileSource {
    pub fn open(path: PathBuf, source_id: String, checkpoint_path: PathBuf) -> Result<Self> {
        if checkpoint_path.exists() && std::fs::metadata(&checkpoint_path)?.len() > 4 * 1024 * 1024
        {
            bail!("file checkpoint exceeds 4 MiB");
        }
        let legacy = CollectorCheckpoint::load(&checkpoint_path, &source_id)?;
        let state = if checkpoint_path.exists() {
            serde_json::from_slice(&std::fs::read(&checkpoint_path)?)?
        } else {
            FileCheckpoint {
                legacy,
                files: BTreeMap::new(),
                lost_bytes: 0,
                lost_sources: 0,
            }
        };
        let mut source = Self {
            path,
            checkpoint_path,
            start: state.legacy.offset,
            state,
            open: BTreeMap::new(),
            lost: None,
        };
        if source.state.files.len() > 4096
            || source
                .state
                .files
                .values()
                .any(|p| p.offset > p.observed_len)
        {
            bail!("invalid file checkpoint positions or identity quota");
        }
        if source.state.files.is_empty() && !source.path.exists() {
            bail!("collector source is missing and has no file checkpoint");
        }
        source.discover()?;
        Ok(source)
    }
    fn save(&self) -> Result<()> {
        storage_durable::atomic_write(
            &self.checkpoint_path,
            &serde_json::to_vec(&self.state)?,
            storage_durable::FsyncPolicy::Always,
        )
    }
    fn discover(&mut self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let mut paths = vec![self.path.clone()];
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                paths.push(entry.path());
            }
        }
        paths.sort();
        paths.dedup();
        for path in paths {
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                continue;
            }
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if !file.metadata()?.is_file() {
                continue;
            }
            let id = identity(&file)?;
            if path != self.path && !self.state.files.contains_key(&id) {
                continue;
            }
            let len = file.metadata()?.len();
            if self.state.files.is_empty() && self.state.legacy.offset > len {
                bail!("collector source was truncated or rotated below its legacy checkpoint");
            }
            if !self.state.files.contains_key(&id) {
                if self.state.files.len() >= 4096 {
                    bail!("collector file identity quota exceeds 4096");
                }
                let offset = if self.state.files.is_empty() {
                    self.state.legacy.offset
                } else {
                    0
                };
                let line = if self.state.files.is_empty() {
                    self.state.legacy.line
                } else {
                    0
                };
                self.state.files.insert(
                    id.clone(),
                    Position {
                        offset,
                        line,
                        observed_len: len,
                        retired: false,
                        generation: 0,
                    },
                );
            }
            let position = self.state.files.get_mut(&id).expect("file position");
            if position.retired {
                continue;
            }
            position.observed_len = position.observed_len.max(len);
            if let std::collections::btree_map::Entry::Vacant(entry) = self.open.entry(id) {
                let mut reader = BufReader::new(file);
                reader.seek(SeekFrom::Start(position.offset))?;
                entry.insert(OpenFile {
                    reader,
                    position: position.clone(),
                });
            }
        }
        self.save()?;
        Ok(())
    }
    fn loss(source_id: &str, id: &str, position: &Position, message: &str) -> SourceRejection {
        SourceRejection {
            entry: QuarantineEntry::invalid_line(
                source_id,
                position.line,
                position.offset,
                "source_lost",
                message,
                &[],
            ),
            cursor: SourceCursor::CriLoss {
                identity: id.into(),
                lost_bytes: position.observed_len.saturating_sub(position.offset),
            },
        }
    }
}
impl service_collector::CollectorSource for FileSource {
    type Cursor = SourceCursor;
    type Error = anyhow::Error;
    type Record = RawRecord;
    type Rejection = SourceRejection;
    fn next_record(&mut self, max_bytes: usize) -> Result<ReadOutcome> {
        if let Some(loss) = &self.lost {
            return Ok(ReadOutcome::Rejection(loss.clone()));
        }
        for (id, file) in &mut self.open {
            let len = file.reader.get_ref().metadata()?.len();
            if len < file.position.offset {
                let position = self.state.files.get(id).expect("checkpoint file");
                let loss = Self::loss(
                    &self.state.legacy.source_id,
                    id,
                    position,
                    "file was truncated; observed uncommitted bytes were lost",
                );
                self.lost = Some(loss.clone());
                return Ok(ReadOutcome::Rejection(loss));
            }
            if len > self.state.files[id].observed_len {
                self.state
                    .files
                    .get_mut(id)
                    .expect("file position")
                    .observed_len = len;
                // File lengths are committed before their bytes are read.
                storage_durable::atomic_write(
                    &self.checkpoint_path,
                    &serde_json::to_vec(&self.state)?,
                    storage_durable::FsyncPolicy::Always,
                )?;
            }
            let offset = file.position.offset;
            let read = read_bounded_line(&mut file.reader, max_bytes)?;
            if read.bytes_read == 0 {
                continue;
            }
            file.position.offset = file
                .position
                .offset
                .checked_add(read.bytes_read)
                .context("file offset overflow")?;
            file.position.line = file
                .position
                .line
                .checked_add(1)
                .context("file line overflow")?;
            let cursor = SourceCursor::Cri {
                identity: id.clone(),
                next_offset: file.position.offset,
                next_line: file.position.line,
                observed_len: len,
            };
            let source_id = format!(
                "{}:{id}:{}",
                self.state.legacy.source_id, file.position.generation
            );
            if read.oversized {
                return Ok(ReadOutcome::Rejection(SourceRejection {
                    entry: QuarantineEntry::invalid_line(
                        &source_id,
                        file.position.line,
                        offset,
                        "line_too_large",
                        "file log exceeds the record byte limit",
                        &read.preview,
                    ),
                    cursor,
                }));
            }
            return Ok(ReadOutcome::Record(RawRecord {
                source_id,
                line: file.position.line,
                offset,
                bytes: read.preview,
                cursor,
                enrichment: RecordEnrichment::default(),
            }));
        }
        for (id, p) in &self.state.files {
            if !p.retired && !self.open.contains_key(id) && p.observed_len > p.offset {
                let loss = Self::loss(
                    &self.state.legacy.source_id,
                    id,
                    p,
                    "source disappeared with observed uncommitted bytes",
                );
                self.lost = Some(loss.clone());
                return Ok(ReadOutcome::Rejection(loss));
            }
        }
        Ok(ReadOutcome::Exhausted)
    }
    fn commit(&mut self, cursors: &[SourceCursor], stats: CommitStats) -> Result<()> {
        for cursor in cursors {
            match cursor {
                SourceCursor::Cri {
                    identity,
                    next_offset,
                    next_line,
                    ..
                } => {
                    let p = self
                        .state
                        .files
                        .get_mut(identity)
                        .context("unknown file cursor")?;
                    if *next_offset < p.offset {
                        bail!("file checkpoint moves backwards");
                    }
                    self.state.legacy.offset = self
                        .state
                        .legacy
                        .offset
                        .checked_add(next_offset - p.offset)
                        .context("file progress offset overflow")?;
                    self.state.legacy.line = self
                        .state
                        .legacy
                        .line
                        .checked_add(next_line.saturating_sub(p.line))
                        .context("file progress line overflow")?;
                    p.offset = *next_offset;
                    p.line = *next_line;
                }
                SourceCursor::CriLoss {
                    identity,
                    lost_bytes,
                } => {
                    let p = self
                        .state
                        .files
                        .get_mut(identity)
                        .context("unknown lost file")?;
                    // Framed records committed before this loss already cover
                    // part of the observed suffix. Count only the uncovered part.
                    let lost = (*lost_bytes).min(p.observed_len.saturating_sub(p.offset));
                    self.state.lost_bytes = self
                        .state
                        .lost_bytes
                        .checked_add(lost)
                        .context("lost byte counter overflow")?;
                    self.state.lost_sources = self
                        .state
                        .lost_sources
                        .checked_add(1)
                        .context("lost source counter overflow")?;
                    if let Some(file) = self.open.get_mut(identity) {
                        let len = file.reader.get_ref().metadata()?.len();
                        p.generation = p
                            .generation
                            .checked_add(1)
                            .context("file generation overflow")?;
                        p.offset = 0;
                        p.line = 0;
                        p.observed_len = len;
                        file.reader.seek(SeekFrom::Start(0))?;
                        file.position = p.clone();
                    } else {
                        p.retired = true;
                    }
                    self.lost = None;
                }
                SourceCursor::Linear { .. } => {
                    bail!("linear cursor committed to a rotation-aware source")
                }
            }
        }
        self.state.legacy.accepted = self
            .state
            .legacy
            .accepted
            .checked_add(stats.accepted)
            .context("accepted count overflow")?;
        self.state.legacy.duplicates = self
            .state
            .legacy
            .duplicates
            .checked_add(stats.duplicates)
            .context("duplicate count overflow")?;
        self.state.legacy.rejected = self
            .state
            .legacy
            .rejected
            .checked_add(stats.rejected)
            .context("rejected count overflow")?;
        self.save()
    }
    fn refresh(&mut self) -> Result<()> {
        self.discover()
    }
    fn progress(&self) -> service_collector::SourceProgress {
        service_collector::SourceProgress {
            start_offset: self.start,
            final_offset: self.state.legacy.offset,
            lost_bytes: self.state.lost_bytes,
            lost_sources: self.state.lost_sources,
        }
    }
}

#[cfg(test)]
mod tests;
