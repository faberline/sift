//! The CRI source: every pod log under the root, read in discovery order, with
//! partial frames joined, rotation followed by device and inode, and lost bytes
//! reported as rejections.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use service_collector::CollectorSource as _;

use crate::collector::domain::config::CriSourceConfig;
use crate::collector::domain::cri::{enrichment, parse_cri_frame, CriTag, DiscoveredFile};
use crate::collector::domain::quarantine::QuarantineEntry;
use crate::collector::domain::record::{RawRecord, SourceCursor, SourceRejection};
use crate::collector::infrastructure::cri_checkpoint::{CriCheckpoint, CriFileCheckpoint};
use crate::collector::infrastructure::cri_discovery::discover;
use crate::collector::infrastructure::source::{read_bounded_line, CommitStats, ReadOutcome};

pub(crate) struct CriSource {
    config: CriSourceConfig,
    root: PathBuf,
    checkpoint_path: PathBuf,
    checkpoint: CriCheckpoint,
    discovered: Vec<DiscoveredFile>,
    read_positions: HashMap<String, (u64, u64)>,
    pending_losses: VecDeque<SourceRejection>,
    staged_losses: HashSet<String>,
    start_offset: u64,
}

impl CriSource {
    pub(crate) fn open(config: CriSourceConfig, checkpoint_path: PathBuf) -> Result<Self> {
        let root = std::fs::canonicalize(&config.root)
            .with_context(|| format!("resolve CRI root {}", config.root.display()))?;
        if !std::fs::metadata(&root)?.is_dir() {
            bail!("collector CRI root must be a directory");
        }
        let root_text = root.to_string_lossy().to_string();
        let checkpoint = CriCheckpoint::load(&checkpoint_path, &root_text)?;
        let start_offset = checkpoint.files.values().map(|file| file.offset).sum();
        let read_positions = checkpoint
            .files
            .iter()
            .map(|(identity, file)| (identity.clone(), (file.offset, file.line)))
            .collect();
        let mut source = Self {
            config,
            root,
            checkpoint_path,
            checkpoint,
            discovered: Vec::new(),
            read_positions,
            pending_losses: VecDeque::new(),
            staged_losses: HashSet::new(),
            start_offset,
        };
        source.refresh()?;
        Ok(source)
    }

    fn read_file(&mut self, discovered: &DiscoveredFile, max_bytes: usize) -> Result<ReadOutcome> {
        let (start_offset, start_line) = self
            .read_positions
            .get(&discovered.identity)
            .copied()
            .unwrap_or((0, 0));
        if start_offset >= discovered.len {
            return Ok(ReadOutcome::Exhausted);
        }

        let mut file = File::open(&discovered.path)
            .with_context(|| format!("open CRI source {}", discovered.path.display()))?;
        file.seek(SeekFrom::Start(start_offset))
            .with_context(|| format!("seek CRI source {}", discovered.path.display()))?;
        let mut reader = BufReader::new(file);
        let first = read_bounded_line(&mut reader, max_bytes)?;
        if first.bytes_read == 0 {
            return Ok(ReadOutcome::Exhausted);
        }

        let mut next_offset = start_offset
            .checked_add(first.bytes_read)
            .context("CRI byte offset overflow")?;
        let mut next_line = start_line.checked_add(1).context("CRI line overflow")?;
        if first.oversized {
            return Ok(self.rejection(
                discovered,
                start_offset,
                next_offset,
                next_line,
                "cri_line_too_large",
                format!("CRI physical record exceeds {max_bytes} bytes"),
                &first.preview,
            ));
        }
        let first_frame = match parse_cri_frame(&first.preview) {
            Ok(frame) => frame,
            Err(error) => {
                return Ok(self.rejection(
                    discovered,
                    start_offset,
                    next_offset,
                    next_line,
                    "invalid_cri_envelope",
                    error.to_string(),
                    &first.preview,
                ));
            }
        };

        let stream = first_frame.stream;
        let mut content = first_frame.content;
        let mut preview = first.preview;
        let mut tag = first_frame.tag;
        let mut oversized = content.len() > max_bytes;

        while tag == CriTag::Partial {
            let fragment = read_bounded_line(&mut reader, max_bytes)?;
            if fragment.bytes_read == 0 {
                return Ok(ReadOutcome::Pending);
            }
            next_offset = next_offset
                .checked_add(fragment.bytes_read)
                .context("CRI byte offset overflow")?;
            next_line = next_line.checked_add(1).context("CRI line overflow")?;
            if preview.len() < 1024 {
                preview.extend_from_slice(
                    &fragment.preview[..fragment.preview.len().min(1024 - preview.len())],
                );
            }
            if fragment.oversized {
                oversized = true;
            }
            let frame = match parse_cri_frame(&fragment.preview) {
                Ok(frame) => frame,
                Err(error) => {
                    return Ok(self.rejection(
                        discovered,
                        start_offset,
                        next_offset,
                        next_line,
                        "invalid_cri_partial",
                        error.to_string(),
                        &preview,
                    ));
                }
            };
            if frame.stream != stream {
                return Ok(self.rejection(
                    discovered,
                    start_offset,
                    next_offset,
                    next_line,
                    "interleaved_cri_partial",
                    "CRI partial fragments must remain on one stream",
                    &preview,
                ));
            }
            if content.len().saturating_add(frame.content.len()) > max_bytes {
                oversized = true;
            } else if !oversized {
                content.extend_from_slice(&frame.content);
            }
            tag = frame.tag;
        }

        if oversized {
            return Ok(self.rejection(
                discovered,
                start_offset,
                next_offset,
                next_line,
                "cri_record_too_large",
                format!("assembled CRI record exceeds {max_bytes} bytes"),
                &preview,
            ));
        }

        self.read_positions
            .insert(discovered.identity.clone(), (next_offset, next_line));
        Ok(ReadOutcome::Record(RawRecord {
            source_id: format!("cri:{}", discovered.identity),
            line: start_line + 1,
            offset: start_offset,
            bytes: content,
            cursor: SourceCursor::Cri {
                identity: discovered.identity.clone(),
                next_offset,
                next_line,
                observed_len: discovered.len,
            },
            enrichment: enrichment(&self.config, discovered, stream),
        }))
    }

    fn rejection(
        &mut self,
        discovered: &DiscoveredFile,
        start_offset: u64,
        next_offset: u64,
        next_line: u64,
        code: &str,
        message: impl AsRef<str>,
        preview: &[u8],
    ) -> ReadOutcome {
        self.read_positions
            .insert(discovered.identity.clone(), (next_offset, next_line));
        ReadOutcome::Rejection(SourceRejection {
            entry: QuarantineEntry::invalid_line(
                &format!("cri:{}", discovered.identity),
                next_line,
                start_offset,
                code,
                message,
                preview,
            ),
            cursor: SourceCursor::Cri {
                identity: discovered.identity.clone(),
                next_offset,
                next_line,
                observed_len: discovered.len,
            },
        })
    }
}

impl service_collector::CollectorSource for CriSource {
    type Cursor = SourceCursor;
    type Error = anyhow::Error;
    type Record = RawRecord;
    type Rejection = SourceRejection;

    fn next_record(&mut self, max_bytes: usize) -> Result<ReadOutcome> {
        if let Some(loss) = self.pending_losses.pop_front() {
            if let SourceCursor::CriLoss { identity, .. } = &loss.cursor {
                self.staged_losses.insert(identity.clone());
            }
            return Ok(ReadOutcome::Rejection(loss));
        }
        let mut saw_pending = false;
        for discovered in self.discovered.clone() {
            match self.read_file(&discovered, max_bytes)? {
                ReadOutcome::Exhausted => {}
                ReadOutcome::Pending => saw_pending = true,
                outcome => return Ok(outcome),
            }
        }
        Ok(if saw_pending {
            ReadOutcome::Pending
        } else {
            ReadOutcome::Exhausted
        })
    }

    fn commit(&mut self, cursors: &[SourceCursor], stats: CommitStats) -> Result<()> {
        for cursor in cursors {
            match cursor {
                SourceCursor::Cri {
                    identity,
                    next_offset,
                    next_line,
                    observed_len,
                } => {
                    let file =
                        self.checkpoint.files.get_mut(identity).with_context(|| {
                            format!("missing CRI checkpoint identity {identity}")
                        })?;
                    file.offset = *next_offset;
                    file.line = *next_line;
                    file.observed_len = file.observed_len.max(*observed_len);
                }
                SourceCursor::CriLoss {
                    identity,
                    lost_bytes,
                } => {
                    let file = self
                        .checkpoint
                        .files
                        .get_mut(identity)
                        .with_context(|| format!("missing lost CRI identity {identity}"))?;
                    if !file.loss_reported {
                        file.retired = true;
                        file.loss_reported = true;
                        self.checkpoint.lost_bytes = self
                            .checkpoint
                            .lost_bytes
                            .checked_add(*lost_bytes)
                            .context("CRI lost byte counter overflow")?;
                        self.checkpoint.lost_sources = self
                            .checkpoint
                            .lost_sources
                            .checked_add(1)
                            .context("CRI lost source counter overflow")?;
                    }
                    self.staged_losses.remove(identity);
                }
                SourceCursor::Linear { .. } => bail!("linear cursor committed to CRI source"),
            }
        }
        self.checkpoint.accepted = self.checkpoint.accepted.saturating_add(stats.accepted);
        self.checkpoint.duplicates = self.checkpoint.duplicates.saturating_add(stats.duplicates);
        self.checkpoint.rejected = self.checkpoint.rejected.saturating_add(stats.rejected);
        self.checkpoint.save(&self.checkpoint_path)
    }

    fn refresh(&mut self) -> Result<()> {
        let mut discovered = discover(&self.root, &self.checkpoint.files)?;
        let present: HashSet<_> = discovered
            .iter()
            .map(|file| file.identity.clone())
            .collect();

        for file in &discovered {
            let entry = self
                .checkpoint
                .files
                .entry(file.identity.clone())
                .or_insert_with(|| CriFileCheckpoint {
                    offset: 0,
                    line: 0,
                    observed_len: file.len,
                    relative_path: file.relative_path.clone(),
                    workload: file.workload.clone(),
                    retired: false,
                    loss_reported: false,
                });
            entry.observed_len = entry.observed_len.max(file.len);
            entry.relative_path = file.relative_path.clone();
            entry.workload = file.workload.clone();
            entry.retired = false;
            self.read_positions
                .entry(file.identity.clone())
                .or_insert((entry.offset, entry.line));
        }

        for (identity, file) in &mut self.checkpoint.files {
            if present.contains(identity) || file.retired || self.staged_losses.contains(identity) {
                continue;
            }
            let lost_bytes = file.observed_len.saturating_sub(file.offset);
            if lost_bytes == 0 {
                file.retired = true;
                continue;
            }
            self.pending_losses.push_back(SourceRejection {
                entry: QuarantineEntry::invalid_line(
                    &format!("cri:{identity}"),
                    file.line,
                    file.offset,
                    "source_lost",
                    format!(
                        "CRI source disappeared with {lost_bytes} observed uncommitted bytes at {}",
                        file.relative_path
                    ),
                    &[],
                ),
                cursor: SourceCursor::CriLoss {
                    identity: identity.clone(),
                    lost_bytes,
                },
            });
        }

        discovered.sort_by(|left, right| {
            (!left.known_before, &left.relative_path, &left.identity).cmp(&(
                !right.known_before,
                &right.relative_path,
                &right.identity,
            ))
        });
        self.discovered = discovered;
        self.checkpoint.save(&self.checkpoint_path)
    }

    fn progress(&self) -> service_collector::SourceProgress {
        service_collector::SourceProgress {
            start_offset: self.start_offset,
            final_offset: self.checkpoint.files.values().map(|file| file.offset).sum(),
            lost_bytes: self.checkpoint.lost_bytes,
            lost_sources: self.checkpoint.lost_sources,
        }
    }
}
