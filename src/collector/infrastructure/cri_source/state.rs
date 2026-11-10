//! Durable file positions, generations, and source loss accounting.
use super::*;

impl CriSource {
    pub(super) fn commit_cursors(
        &mut self,
        cursors: &[SourceCursor],
        stats: CommitStats,
    ) -> Result<()> {
        for cursor in cursors {
            match cursor {
                SourceCursor::Cri {
                    identity,
                    next_offset,
                    next_line,
                    observed_len,
                    anchor,
                    generation,
                } => {
                    let file =
                        self.checkpoint.files.get_mut(identity).with_context(|| {
                            format!("missing CRI checkpoint identity {identity}")
                        })?;
                    if !file_anchor::current_generation(*generation, file.generation)?
                        || *next_offset <= file.offset
                    {
                        continue;
                    }
                    file.offset = *next_offset;
                    file.line = *next_line;
                    file.observed_len = file.observed_len.max(*observed_len);
                    file.anchor = anchor.clone();
                }
                SourceCursor::CriLoss {
                    identity,
                    lost_bytes,
                    generation,
                } => {
                    let current_len = self.current_file_length(identity)?;
                    let file = self
                        .checkpoint
                        .files
                        .get_mut(identity)
                        .with_context(|| format!("missing lost CRI identity {identity}"))?;
                    if !file_anchor::current_generation(*generation, file.generation)? {
                        continue;
                    }
                    if !file.loss_reported {
                        file.loss_reported = true;
                        let lost = (*lost_bytes).min(file.observed_len.saturating_sub(file.offset));
                        self.checkpoint.lost_bytes = self
                            .checkpoint
                            .lost_bytes
                            .checked_add(lost)
                            .context("CRI lost byte counter overflow")?;
                        self.checkpoint.lost_sources = self
                            .checkpoint
                            .lost_sources
                            .checked_add(1)
                            .context("CRI lost source counter overflow")?;
                    }
                    if let Some(len) = current_len {
                        file.generation = file
                            .generation
                            .checked_add(1)
                            .context("CRI file generation overflow")?;
                        file.offset = 0;
                        file.line = 0;
                        file.observed_len = len;
                        file.anchor = None;
                        file.retired = false;
                        file.loss_reported = false;
                        self.read_positions.insert(
                            identity.clone(),
                            ReadPosition {
                                generation: file.generation,
                                ..ReadPosition::default()
                            },
                        );
                    } else {
                        file.retired = true;
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

    pub(super) fn refresh_files(&mut self) -> Result<()> {
        let mut discovered = discover(&self.root, &self.checkpoint.files)?;
        discovered.retain(|file| file.workload.in_namespace(self.config.namespace.as_deref()));
        let present: HashSet<_> = discovered
            .iter()
            .map(|file| file.identity.clone())
            .collect();

        for file in &discovered {
            let intact = self.observed_position_intact(file)?;
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
                    generation: 0,
                    anchor: None,
                    birth_nanos: file.birth_nanos,
                });
            if intact {
                entry.observed_len = entry.observed_len.max(file.len);
            }
            entry.relative_path = file.relative_path.clone();
            entry.workload = file.workload.clone();
            entry.birth_nanos = file.birth_nanos;
            entry.retired = false;
            self.read_positions
                .entry(file.identity.clone())
                .or_insert_with(|| ReadPosition {
                    offset: entry.offset,
                    line: entry.line,
                    anchor: entry.anchor.clone(),
                    generation: entry.generation,
                });
        }

        for (identity, file) in &mut self.checkpoint.files {
            if !file.workload.in_namespace(self.config.namespace.as_deref()) {
                continue;
            }
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
                    generation: Some(file.generation),
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

    fn current_file_length(&self, identity: &str) -> Result<Option<u64>> {
        let Some(discovered) = self
            .discovered
            .iter()
            .find(|file| file.identity == identity)
        else {
            return Ok(None);
        };
        let file = match File::open(&discovered.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("open lost CRI file"),
        };
        let metadata = file.metadata()?;
        Ok((file_identity(&metadata)? == discovered.physical_identity).then_some(metadata.len()))
    }

    fn observed_position_intact(&self, discovered: &DiscoveredFile) -> Result<bool> {
        let Some(position) = self.read_positions.get(&discovered.identity) else {
            return Ok(true);
        };
        let file = match File::open(&discovered.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error).context("open CRI file before updating its observed length")
            }
        };
        let metadata = file.metadata()?;
        Ok(file_identity(&metadata)? == discovered.physical_identity
            && metadata.len() >= position.offset
            && file_anchor::matches(&file, position.offset, position.anchor.as_ref())?)
    }
}
