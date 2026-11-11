//! Unknown results retain the episode. They never create a recovery.
use super::rule::{AlertRule, NoData};
use crate::shared_kernel::sha256_hex::sha256_hex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Inactive,
    Pending,
    Firing,
    Unknown,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertState {
    pub id: String,
    pub rule_id: String,
    pub labels: BTreeMap<String, String>,
    pub phase: Phase,
    pub active_episode: bool,
    pub episode: u64,
    pub pending_since: Option<i64>,
    pub last_evaluated: i64,
    pub last_event_cursor: u64,
    pub mute_until: Option<i64>,
    pub reason: Option<String>,
    #[serde(default)]
    pub notified_episode: u64,
}
#[derive(Clone, Debug)]
pub(crate) enum Observation {
    Condition(bool),
    Event(u64),
    NoData,
    Unknown(String),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Notification {
    pub id: String,
    pub project: String,
    pub rule_id: String,
    pub alert_id: String,
    pub webhook: String,
    pub transition: String,
    pub created_at: i64,
    pub payload: Value,
    pub attempts: u32,
    pub next_attempt: i64,
    pub delivered_at: Option<i64>,
    pub last_error: Option<String>,
}
impl AlertState {
    pub(crate) fn new(rule_id: &str, labels: BTreeMap<String, String>) -> Self {
        let id = sha256_hex(
            serde_json::to_string(&(rule_id, &labels))
                .expect("alert key")
                .as_bytes(),
        );
        Self {
            id,
            rule_id: rule_id.into(),
            labels,
            phase: Phase::Inactive,
            active_episode: false,
            episode: 0,
            pending_since: None,
            last_evaluated: 0,
            last_event_cursor: 0,
            mute_until: None,
            reason: None,
            notified_episode: 0,
        }
    }
    pub(crate) fn observe(
        &mut self,
        rule: &AlertRule,
        observation: Observation,
        now: i64,
    ) -> Result<Option<Notification>, String> {
        if now < self.last_evaluated {
            return Err("alert clock moved backwards".into());
        }
        self.last_evaluated = now;
        let event_only = matches!(observation, Observation::Event(_));
        let condition = match observation {
            Observation::Unknown(reason) => {
                self.phase = Phase::Unknown;
                self.reason = Some(reason);
                self.pending_since = None;
                return Ok(None);
            }
            Observation::NoData => match rule.no_data {
                NoData::Unknown => {
                    self.phase = Phase::Unknown;
                    self.reason = Some("no data".into());
                    self.pending_since = None;
                    return Ok(None);
                }
                NoData::Firing => true,
                NoData::Inactive => false,
            },
            Observation::Condition(value) => value,
            Observation::Event(cursor) => {
                let fresh = cursor > self.last_event_cursor;
                self.last_event_cursor = self.last_event_cursor.max(cursor);
                if fresh {
                    self.active_episode = false;
                    self.pending_since = None;
                }
                fresh
            }
        };
        self.reason = None;
        let transition = if condition {
            if self.active_episode {
                self.phase = Phase::Firing;
                (self.notified_episode < self.episode).then_some("firing")
            } else {
                let since = *self.pending_since.get_or_insert(now);
                if now.saturating_sub(since) >= rule.hold_seconds as i64 {
                    self.phase = Phase::Firing;
                    self.active_episode = true;
                    self.episode = self
                        .episode
                        .checked_add(1)
                        .ok_or("alert episode overflow")?;
                    Some("firing")
                } else {
                    self.phase = Phase::Pending;
                    None
                }
            }
        } else {
            self.phase = Phase::Inactive;
            self.pending_since = None;
            if self.active_episode {
                self.active_episode = false;
                (!event_only && self.notified_episode == self.episode).then_some("resolved")
            } else {
                None
            }
        };
        let Some(transition) = transition else {
            return Ok(None);
        };
        if rule.mute_until.is_some_and(|until| until > now)
            || self.mute_until.is_some_and(|until| until > now)
        {
            return Ok(None);
        }
        if transition == "firing" {
            self.notified_episode = self.episode;
        }
        let id = sha256_hex(
            format!(
                "{}:{}:{}:{}",
                rule.project, self.id, self.episode, transition
            )
            .as_bytes(),
        );
        let payload = serde_json::json!({"notification_id":id,"project":rule.project,"rule_id":self.rule_id,
            "alert_id":self.id,"transition":transition,"episode":self.episode,"labels":self.labels,"evaluated_at":now});
        Ok(Some(Notification {
            id,
            project: rule.project.clone(),
            rule_id: self.rule_id.clone(),
            alert_id: self.id.clone(),
            webhook: rule.webhook.clone(),
            transition: transition.into(),
            created_at: now,
            payload,
            attempts: 0,
            next_attempt: now,
            delivered_at: None,
            last_error: None,
        }))
    }
}

#[cfg(test)]
mod tests;
