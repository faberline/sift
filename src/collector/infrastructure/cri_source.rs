//! The CRI source: every pod log under the root, read in discovery order, with
//! partial frames joined, rotation followed by device and inode, and lost bytes
//! reported as rejections.
mod state;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use service_collector::CollectorSource as _;

use crate::collector::domain::config::CriSourceConfig;
use crate::collector::domain::cri::{enrichment, parse_cri_frame, CriTag, DiscoveredFile};
use crate::collector::domain::quarantine::QuarantineEntry;
use crate::collector::domain::record::{FileAnchor, RawRecord, SourceCursor, SourceRejection};
use crate::collector::infrastructure::cri_checkpoint::{CriCheckpoint, CriFileCheckpoint};
use crate::collector::infrastructure::cri_discovery::{discover, file_identity};
use crate::collector::infrastructure::file_anchor;
use crate::collector::infrastructure::source::{read_bounded_line, CommitStats, ReadOutcome};

pub(crate) struct CriSource {
    config: CriSourceConfig,
    root: PathBuf,
    checkpoint_path: PathBuf,
    checkpoint: CriCheckpoint,
    discovered: Vec<DiscoveredFile>,
    read_positions: HashMap<String, ReadPosition>,
    pending_losses: VecDeque<SourceRejection>,
    staged_losses: HashSet<String>,
    start_offset: u64,
}

#[derive(Clone, Default)]
struct ReadPosition {
    offset: u64,
    line: u64,
    anchor: Option<FileAnchor>,
    generation: u64,
}

fn source_identity(identity: &str, generation: u64) -> String {
    if generation == 0 {
        format!("cri:{identity}")
    } else {
        format!("cri:{identity}:{generation}")
    }
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
            .map(|(identity, file)| {
                (
                    identity.clone(),
                    ReadPosition {
                        offset: file.offset,
                        line: file.line,
                        anchor: file.anchor.clone(),
                        generation: file.generation,
                    },
                )
            })
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
        if self.staged_losses.contains(&discovered.identity) {
            return Ok(ReadOutcome::Pending);
        }
        let position = self
            .read_positions
            .get(&discovered.identity)
            .cloned()
            .unwrap_or_default();
        let (start_offset, start_line) = (position.offset, position.line);
        let mut file = match File::open(&discovered.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadOutcome::Pending);
            }
            Err(error) => return Err(error).context("open CRI source"),
        };
        let metadata = file.metadata()?;
        if file_identity(&metadata)? != discovered.physical_identity {
            return Ok(ReadOutcome::Pending);
        }
        if metadata.len() < start_offset
            || !file_anchor::matches(&file, start_offset, position.anchor.as_ref())?
        {
            self.staged_losses.insert(discovered.identity.clone());
            let saved = &self.checkpoint.files[&discovered.identity];
            return Ok(ReadOutcome::Rejection(SourceRejection {
                entry: QuarantineEntry::invalid_line(
                    &source_identity(&discovered.identity, position.generation),
                    saved.line,
                    saved.offset,
                    "source_lost",
                    "CRI file was truncated or overwritten; observed uncommitted bytes were lost",
                    &[],
                ),
                cursor: SourceCursor::CriLoss {
                    identity: discovered.identity.clone(),
                    lost_bytes: saved.observed_len.saturating_sub(saved.offset),
                    generation: Some(position.generation),
                },
            }));
        }
        let mut observed = discovered.clone();
        observed.len = metadata.len();
        let discovered = &observed;
        let saved = self
            .checkpoint
            .files
            .get_mut(&discovered.identity)
            .context("missing CRI source checkpoint")?;
        if discovered.len > saved.observed_len {
            saved.observed_len = discovered.len;
            self.checkpoint.save(&self.checkpoint_path)?;
        }
        if start_offset >= discovered.len {
            return Ok(ReadOutcome::Exhausted);
        }
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
                file_anchor::from_tail(&first.tail),
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
                    file_anchor::from_tail(&first.tail),
                ));
            }
        };

        let stream = first_frame.stream;
        let mut content = first_frame.content;
        let mut preview = first.preview;
        let mut tail = first.tail;
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
            file_anchor::keep_tail(&mut tail, &fragment.tail);
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
                        file_anchor::from_tail(&tail),
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
                    file_anchor::from_tail(&tail),
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
                file_anchor::from_tail(&tail),
            ));
        }

        let anchor = file_anchor::from_tail(&tail);
        self.read_positions.insert(
            discovered.identity.clone(),
            ReadPosition {
                offset: next_offset,
                line: next_line,
                anchor: anchor.clone(),
                generation: position.generation,
            },
        );
        let mut metadata = enrichment(&self.config, discovered, stream);
        metadata.attributes.insert(
            "collector.cri.timestamp".into(),
            crate::AttributeValue::String(first_frame.timestamp),
        );
        Ok(ReadOutcome::Record(RawRecord {
            source_id: source_identity(&discovered.identity, position.generation),
            line: start_line + 1,
            offset: start_offset,
            bytes: content,
            cursor: SourceCursor::Cri {
                identity: discovered.identity.clone(),
                next_offset,
                next_line,
                observed_len: discovered.len,
                anchor,
                generation: Some(position.generation),
            },
            enrichment: metadata,
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
        anchor: Option<FileAnchor>,
    ) -> ReadOutcome {
        let generation = self.checkpoint.files[&discovered.identity].generation;
        self.read_positions.insert(
            discovered.identity.clone(),
            ReadPosition {
                offset: next_offset,
                line: next_line,
                anchor: anchor.clone(),
                generation,
            },
        );
        ReadOutcome::Rejection(SourceRejection {
            entry: QuarantineEntry::invalid_line(
                &source_identity(&discovered.identity, generation),
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
                anchor,
                generation: Some(generation),
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
        self.commit_cursors(cursors, stats)
    }

    fn refresh(&mut self) -> Result<()> {
        self.refresh_files()
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
