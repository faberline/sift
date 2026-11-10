//! Retrying an outbox record preserves its notification ID and payload.
use crate::alerting::domain::{delivery_port::WebhookPort, state::Notification};
use crate::control::application::service::{Change, ControlService, RecordKind};
use std::sync::Arc;

pub(crate) struct DeliveryService {
    pub control: Arc<ControlService>,
    pub webhook: Arc<dyn WebhookPort>,
}
impl DeliveryService {
    pub async fn deliver_due(&self, now: i64) -> Result<(), String> {
        if !self.control.is_leader().await? {
            return Ok(());
        }
        let records = self.control.all(RecordKind::Notification).await?;
        let mut pending = records
            .iter()
            .filter(|record| {
                record.value.as_ref().is_some_and(|value| {
                    value
                        .get("delivered_at")
                        .is_none_or(serde_json::Value::is_null)
                })
            })
            .count() as u64;
        self.control.set_pending_notifications(pending);
        let mut sent = 0;
        for record in records {
            let Some(value) = record.value else {
                continue;
            };
            let mut notification: Notification =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            if notification.delivered_at.is_some() || notification.next_attempt > now {
                continue;
            }
            if sent >= 100 || !self.control.is_leader().await? {
                break;
            }
            sent += 1;
            notification.attempts = notification.attempts.saturating_add(1);
            match self.webhook.send(&notification).await {
                Ok(()) => {
                    notification.delivered_at = Some(now);
                    notification.last_error = None;
                }
                Err(error) => {
                    self.control.record_notification_failure();
                    notification.last_error = Some(error.chars().take(512).collect());
                    notification.next_attempt =
                        now.saturating_add(2_i64.pow(notification.attempts.min(8)).min(300));
                }
            }
            let request_id = format!("delivery-{}-{}", record.id, record.revision);
            let delivered = notification.delivered_at.is_some();
            self.control
                .transact(
                    request_id,
                    vec![Change {
                        project: record.project,
                        kind: RecordKind::Notification,
                        id: record.id,
                        expected_revision: record.revision,
                        value: Some(serde_json::to_value(notification).map_err(|e| e.to_string())?),
                    }],
                )
                .await?;
            if delivered {
                pending = pending.saturating_sub(1);
                self.control.set_pending_notifications(pending);
            }
        }
        Ok(())
    }
}
