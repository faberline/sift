//! Alert state and notification intent commit in the same transaction.
pub(crate) use crate::alerting::domain::state::Observation;
pub use crate::alerting::domain::{
    rule::{AlertRule, RuleSource},
    state::{AlertState, Notification},
};
use crate::control::application::service::{Change, ControlService, RecordKind};
use std::{collections::BTreeMap, sync::Arc};

pub struct AlertingService {
    control: Arc<ControlService>,
}
impl AlertingService {
    pub fn new(control: Arc<ControlService>) -> Self {
        Self { control }
    }
    pub(crate) fn validate_rule(value: &serde_json::Value, project: &str) -> Result<(), String> {
        let rule: AlertRule = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        rule.validate()?;
        if rule.project != project {
            return Err("rule project must match the request".into());
        }
        Ok(())
    }
    pub(crate) async fn mute(
        &self,
        project: &str,
        id: String,
        revision: u64,
        request_id: String,
        until: Option<i64>,
    ) -> Result<(), String> {
        let record = self
            .control
            .list(project, RecordKind::Alert)
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("alert not found")?;
        let mut state: AlertState = serde_json::from_value(record.value.ok_or("alert not found")?)
            .map_err(|e| e.to_string())?;
        state.mute_until = until;
        self.control
            .transact(
                request_id,
                vec![Change {
                    project: project.into(),
                    kind: RecordKind::Alert,
                    id,
                    expected_revision: revision,
                    value: Some(serde_json::to_value(state).map_err(|e| e.to_string())?),
                }],
            )
            .await?;
        Ok(())
    }
    pub(crate) async fn evaluate(
        &self,
        rule_id: &str,
        rule: &AlertRule,
        mut values: BTreeMap<BTreeMap<String, String>, Observation>,
        event_cursor: Option<u64>,
        now: i64,
    ) -> Result<(), String> {
        rule.validate()?;
        if !rule.enabled || !self.control.is_leader().await? {
            return Ok(());
        }
        let previous = self.control.list(&rule.project, RecordKind::Alert).await?;
        let failure = values
            .get(&BTreeMap::new())
            .filter(|v| matches!(v, Observation::Unknown(_)))
            .cloned();
        for record in &previous {
            if let Some(value) = record.value.clone() {
                let state: AlertState = serde_json::from_value(value).map_err(|e| e.to_string())?;
                if state.rule_id == rule_id {
                    values
                        .entry(state.labels)
                        .or_insert_with(|| failure.clone().unwrap_or(Observation::NoData));
                }
            }
        }
        if values.is_empty() {
            values.insert(BTreeMap::new(), Observation::NoData);
        }
        if values.len() > 1000 {
            return Err("alert evaluation exceeds 1000 series".into());
        }
        for batch in values.into_iter().collect::<Vec<_>>().chunks(250) {
            let mut changes = Vec::new();
            for (labels, observation) in batch {
                let fresh = AlertState::new(rule_id, labels.clone());
                let record = previous.iter().find(|r| r.id == fresh.id);
                let revision = record.map_or(0, |r| r.revision);
                let mut state = match record.and_then(|r| r.value.clone()) {
                    Some(value) => {
                        serde_json::from_value::<AlertState>(value).map_err(|e| e.to_string())?
                    }
                    None => fresh,
                };
                let observation =
                    if event_cursor.is_some_and(|cursor| cursor <= state.last_event_cursor) {
                        Observation::Condition(false)
                    } else {
                        observation.clone()
                    };
                let notification = state.observe(rule, observation, now)?;
                if let Some(cursor) = event_cursor {
                    state.last_event_cursor = state.last_event_cursor.max(cursor);
                }
                changes.push(Change {
                    project: rule.project.clone(),
                    kind: RecordKind::Alert,
                    id: state.id.clone(),
                    expected_revision: revision,
                    value: Some(serde_json::to_value(state).map_err(|e| e.to_string())?),
                });
                if let Some(notification) = notification {
                    changes.push(Change {
                        project: rule.project.clone(),
                        kind: RecordKind::Notification,
                        id: notification.id.clone(),
                        expected_revision: 0,
                        value: Some(serde_json::to_value(notification).map_err(|e| e.to_string())?),
                    });
                }
            }
            let digest = crate::shared_kernel::sha256_hex::sha256_hex(
                &serde_json::to_vec(&changes).map_err(|e| e.to_string())?,
            );
            if !self.control.is_leader().await? {
                return Ok(());
            }
            self.control
                .transact(format!("alert-{digest}"), changes)
                .await?;
        }
        Ok(())
    }
}
