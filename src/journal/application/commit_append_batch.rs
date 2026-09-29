//! Committing a governed batch: reserving local capacity, short-circuiting
//! duplicates, and proposing the append command.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::event::EventEnvelope;
use crate::ingest::domain::storage_reservation::storage_reservation;
use crate::journal::domain::append_result::AppendResult;
use crate::journal::infrastructure::durable_journal::DurableJournal;

#[derive(Clone)]
pub(crate) struct CommitContext {
    pub(crate) journal: Arc<DurableJournal>,
    pub(crate) raft: Option<Arc<raft_runtime::RaftHost>>,
    pub(crate) state_machine:
        Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    pub(crate) local_command: Arc<tokio::sync::Mutex<()>>,
    pub(crate) local_capacity: Arc<crate::storage::LocalCapacity>,
}

impl CommitContext {
    pub(crate) async fn append_governed_batch(
        &self,
        governed: Vec<EventEnvelope>,
    ) -> Result<Vec<AppendResult>> {
        if governed.is_empty() {
            bail!("Sift Raft batch must not be empty");
        }
        let encoded_bytes = governed.iter().try_fold(0u64, |total, event| {
            let bytes = serde_json::to_vec(event)
                .context("encode governed Sift event for local capacity reservation")?;
            anyhow::Ok(total.saturating_add(bytes.len() as u64))
        })?;
        let capacity_reservation = self
            .local_capacity
            .reserve(storage_reservation(encoded_bytes, governed.len()))
            .context("reserve local WAL and segment capacity before Raft admission")?;
        let acknowledged_at = Utc::now();
        let current_commit = self.state_machine.applied_commit_index();
        let mut duplicate_results = Vec::with_capacity(governed.len());
        let mut all_duplicates = true;
        for event in &governed {
            match self
                .journal
                .result_for_at(&event.project, &event.event_id, acknowledged_at)?
            {
                Some(result) => duplicate_results.push(result.with_commit_index(current_commit)),
                None => {
                    all_duplicates = false;
                    break;
                }
            }
        }
        if all_duplicates {
            return Ok(duplicate_results);
        }
        let event_ids = governed
            .iter()
            .map(|event| (event.project.clone(), event.event_id.clone()))
            .collect::<Vec<_>>();
        let commit_index = self
            .commit_command(
                crate::journal::domain::sift_command::SiftCommandV1::append_events_at(
                    governed,
                    acknowledged_at,
                ),
            )
            .await?;
        let results = if let Some(results) = self.state_machine.take_append_outcomes(commit_index) {
            results
        } else {
            let mut recovered = Vec::with_capacity(event_ids.len());
            for (project, event_id) in event_ids {
                recovered.push(
                    self.journal
                        .result_for_at(&project, &event_id, acknowledged_at)?
                        .map(|result| result.with_commit_index(commit_index))
                        .context(
                            "state-machine commit completed without applying the Sift batch",
                        )?,
                );
            }
            recovered
        };
        capacity_reservation.commit();
        Ok(results)
    }
}
