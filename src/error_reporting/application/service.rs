//! A durable source cursor prevents counting a replayed log twice.
use crate::control::application::service::{Change, ControlService, RecordKind};
pub use crate::error_reporting::domain::group::{ErrorGroup, ErrorStatus};
use crate::shared_kernel::projection_log::LogRecordV1;
use std::{collections::BTreeMap, sync::Arc};

pub struct ErrorReportingService {
    control: Arc<ControlService>,
}
impl ErrorReportingService {
    pub fn new(control: Arc<ControlService>) -> Self {
        Self { control }
    }
    pub async fn cursor(&self, project: &str) -> Result<u64, String> {
        Ok(self
            .control
            .list(project, RecordKind::Cursor)
            .await?
            .into_iter()
            .find(|r| r.id == "error-feed")
            .and_then(|r| r.value)
            .and_then(|v| v.as_u64())
            .unwrap_or(0))
    }
    pub(crate) async fn ingest(
        &self,
        project: &str,
        from: u64,
        through: u64,
        records: &[LogRecordV1],
    ) -> Result<(), String> {
        if records.len() > 500
            || through < from
            || records
                .iter()
                .any(|r| r.project != project || r.cursor <= from || r.cursor > through)
        {
            return Err("invalid error feed page".into());
        }
        let cursor = self
            .control
            .list(project, RecordKind::Cursor)
            .await?
            .into_iter()
            .find(|r| r.id == "error-feed");
        if cursor
            .as_ref()
            .and_then(|r| r.value.as_ref())
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            != from
        {
            return Err("error source cursor changed; read and retry".into());
        }
        let existing = if records.is_empty() {
            Vec::new()
        } else {
            self.control.list(project, RecordKind::ErrorGroup).await?
        };
        let mut groups = BTreeMap::new();
        for record in records {
            let Some(candidate) = ErrorGroup::from_log(record) else {
                continue;
            };
            if !groups.contains_key(&candidate.id) {
                let previous = existing.iter().find(|r| r.id == candidate.id);
                let revision = previous.map_or(0, |r| r.revision);
                let mut group = match previous.and_then(|r| r.value.clone()) {
                    Some(value) => {
                        serde_json::from_value::<ErrorGroup>(value).map_err(|e| e.to_string())?
                    }
                    None => {
                        groups.insert(candidate.id.clone(), (revision, candidate));
                        continue;
                    }
                };
                group.observe(record)?;
                groups.insert(group.id.clone(), (revision, group));
            } else {
                groups
                    .get_mut(&candidate.id)
                    .expect("error group exists")
                    .1
                    .observe(record)?;
            }
        }
        let mut changes = groups
            .into_values()
            .map(|(revision, group)| {
                Ok(Change {
                    project: project.into(),
                    kind: RecordKind::ErrorGroup,
                    id: group.id.clone(),
                    expected_revision: revision,
                    value: Some(serde_json::to_value(group).map_err(|e| e.to_string())?),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        changes.push(Change {
            project: project.into(),
            kind: RecordKind::Cursor,
            id: "error-feed".into(),
            expected_revision: cursor.map_or(0, |r| r.revision),
            value: Some(through.into()),
        });
        self.control
            .transact(
                crate::shared_kernel::sha256_hex::sha256_hex(
                    format!("error-feed-{project}-{from}-{through}").as_bytes(),
                ),
                changes,
            )
            .await?;
        Ok(())
    }
    pub(crate) async fn status(
        &self,
        project: &str,
        id: &str,
        revision: u64,
        request_id: String,
        status: &str,
    ) -> Result<(), String> {
        let status: ErrorStatus = serde_json::from_value(status.into())
            .map_err(|_| "status must be open, resolved, or ignored")?;
        let record = self
            .control
            .list(project, RecordKind::ErrorGroup)
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("error group not found")?;
        let mut group: ErrorGroup =
            serde_json::from_value(record.value.ok_or("error group not found")?)
                .map_err(|e| e.to_string())?;
        group.status = status;
        self.control
            .transact(
                request_id,
                vec![Change {
                    project: project.into(),
                    kind: RecordKind::ErrorGroup,
                    id: id.into(),
                    expected_revision: revision,
                    value: Some(serde_json::to_value(group).map_err(|e| e.to_string())?),
                }],
            )
            .await?;
        Ok(())
    }
}
