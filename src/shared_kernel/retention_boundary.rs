//! Sift keeps 180 days of events; older ones are rejected at ingest.

use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, Utc};

use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;

pub(crate) const MAX_RETENTION_DAYS: u32 = 180;

/// The retention window in days, always in `1..=MAX_RETENTION_DAYS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetentionDays(u32);

static CONFIGURED: AtomicU32 = AtomicU32::new(MAX_RETENTION_DAYS);

impl RetentionDays {
    pub(crate) const DEFAULT: RetentionDays = RetentionDays(MAX_RETENTION_DAYS);

    pub(crate) fn new(days: u32) -> anyhow::Result<RetentionDays> {
        if (1..=MAX_RETENTION_DAYS).contains(&days) {
            Ok(RetentionDays(days))
        } else {
            Err(anyhow::anyhow!(
                "retention days {days} is outside the range 1..={MAX_RETENTION_DAYS}"
            ))
        }
    }

    pub(crate) fn days(self) -> u32 {
        self.0
    }

    /// The value the server was started with; 180 until `set_configured` runs.
    pub(crate) fn configured() -> RetentionDays {
        RetentionDays(CONFIGURED.load(Ordering::Relaxed))
    }
}

impl Default for RetentionDays {
    fn default() -> Self {
        RetentionDays::DEFAULT
    }
}

/// Written once at server start; only `RetentionDays` values reach it.
pub(crate) fn set_configured(retention: RetentionDays) {
    CONFIGURED.store(retention.days(), Ordering::Relaxed);
}

pub(crate) fn retention_rejection_at(
    event: &EventEnvelope,
    decision_time: DateTime<Utc>,
    retention: RetentionDays,
) -> Option<String> {
    let occurred = DateTime::parse_from_rfc3339(&event.occurred_at).ok()?;
    let cutoff = decision_time - chrono::Duration::days(i64::from(retention.days()));
    (occurred.with_timezone(&Utc) < cutoff).then(|| {
        format!(
            "event `{}` occurred before Sift's {}-day retention boundary",
            event.event_id,
            retention.days()
        )
    })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{retention_rejection_at, RetentionDays};
    use crate::shared_kernel::event::{EventEnvelope, SignalKind};

    fn event_aged(days: i64, now: DateTime<Utc>) -> EventEnvelope {
        EventEnvelope::for_project_at(
            "project-a",
            "prod",
            "old-1",
            SignalKind::Log,
            serde_json::json!({"message": "x", "level": "INFO"}),
            now - chrono::Duration::days(days),
        )
    }

    #[test]
    fn retention_days_accepts_one_to_180() {
        assert_eq!(RetentionDays::new(1).unwrap().days(), 1);
        assert_eq!(RetentionDays::new(30).unwrap().days(), 30);
        assert_eq!(RetentionDays::new(180).unwrap().days(), 180);
    }

    #[test]
    fn retention_days_refuses_zero_and_above_180() {
        assert!(RetentionDays::new(0).is_err());
        let error = RetentionDays::new(181).unwrap_err().to_string();
        assert!(error.contains("181"), "{error}");
        assert!(error.contains("1..=180"), "{error}");
    }

    #[test]
    fn retention_days_default_is_180() {
        assert_eq!(RetentionDays::default(), RetentionDays::DEFAULT);
        assert_eq!(RetentionDays::default().days(), 180);
    }

    #[test]
    fn thirty_one_day_old_event_is_rejected_with_30_days_and_accepted_with_default() {
        let now = Utc::now();
        let event = event_aged(31, now);
        let message = retention_rejection_at(&event, now, RetentionDays::new(30).unwrap())
            .expect("31 days old is outside a 30-day retention");
        assert!(message.contains("30-day retention boundary"), "{message}");
        assert_eq!(
            retention_rejection_at(&event, now, RetentionDays::DEFAULT),
            None
        );
    }

    #[test]
    fn hundred_eighty_one_day_old_event_is_rejected_with_default() {
        let now = Utc::now();
        let event = event_aged(181, now);
        let message = retention_rejection_at(&event, now, RetentionDays::DEFAULT)
            .expect("181 days old is outside the default retention");
        assert!(message.contains("180-day retention boundary"), "{message}");
    }
}
