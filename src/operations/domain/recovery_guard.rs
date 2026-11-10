//! New writes stop after the remote recovery point becomes 15 minutes old.
use crate::shared_kernel::recovery_record::RecoveryRecord;
use std::sync::atomic::{AtomicI64, Ordering};
pub(crate) struct RecoveryGuard {
    pub enabled: bool,
    protected_at: AtomicI64,
}

impl RecoveryGuard {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            protected_at: AtomicI64::new(0),
        }
    }
    pub fn accept_record(&self, record: &RecoveryRecord, now: i64) -> Result<(), String> {
        record.validate()?;
        if record.created_at > now.saturating_add(30) {
            return Err("recovery record is from the future".into());
        }
        self.protected_at
            .fetch_max(record.protected_at, Ordering::SeqCst);
        Ok(())
    }
    pub fn lag(&self, now: i64) -> u64 {
        now.saturating_sub(self.protected_at.load(Ordering::SeqCst))
            .max(0) as u64
    }
    pub fn check(&self, now: i64) -> Result<(), String> {
        if self.enabled && self.lag(now) > 900 {
            Err(
                "remote protection is more than 15 minutes behind; retain and retry the write"
                    .into(),
            )
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_kernel::recovery_record::DataRecovery;
    #[test]
    fn remote_protection_fails_closed_at_startup_and_after_fifteen_minutes() {
        let guard = RecoveryGuard::new(true);
        assert!(guard.check(10_000).is_err());
        let record = RecoveryRecord {
            version: 1,
            created_at: 10_000,
            protected_at: 9_999,
            data: DataRecovery {
                manifest_uri: "gs://b/data/manifest".into(),
                manifest_sha256: "a".repeat(64),
                protected_at: 9_999,
                through_cursor: 10,
            },
            control_uri: "gs://b/recovery/control".into(),
            control_sha256: "b".repeat(64),
            control_index: 12,
        };
        guard.accept_record(&record, 10_000).unwrap();
        assert!(guard.check(10_899).is_ok());
        assert!(guard.check(10_900).is_err());
        let mut older = record.clone();
        older.protected_at = 9_990;
        older.data.protected_at = 9_990;
        guard.accept_record(&older, 10_000).unwrap();
        assert_eq!(guard.lag(10_000), 1);
        let mut invalid = record;
        invalid.control_sha256 = "tampered".into();
        assert!(guard.accept_record(&invalid, 10_000).is_err());
        assert!(RecoveryGuard::new(false).check(10_000).is_ok());
    }
}
