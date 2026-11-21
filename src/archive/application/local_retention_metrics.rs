//! Counters of the local retention expiry, read by the metrics page: how many
//! events it removed, how many passes finished, and the occurred time of the
//! oldest event it kept.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::shared_kernel::archive_receipts::LocalExpiryReceipt;

#[derive(Default)]
pub(crate) struct LocalRetentionMetrics {
    expired_events: AtomicU64,
    runs: AtomicU64,
    oldest_retained_unix_nano: AtomicU64,
}

impl LocalRetentionMetrics {
    /// Records one successful expiry call.
    pub(crate) fn record(&self, receipt: &LocalExpiryReceipt) {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.expired_events
            .fetch_add(receipt.removed_events, Ordering::Relaxed);
        let oldest = receipt
            .oldest_retained_unix_nano
            .and_then(|nanos| u64::try_from(nanos).ok())
            .unwrap_or(0);
        self.oldest_retained_unix_nano
            .store(oldest, Ordering::Relaxed);
    }

    pub(crate) fn expired_events(&self) -> u64 {
        self.expired_events.load(Ordering::Relaxed)
    }

    pub(crate) fn runs(&self) -> u64 {
        self.runs.load(Ordering::Relaxed)
    }

    pub(crate) fn oldest_retained_unix_nano(&self) -> u64 {
        self.oldest_retained_unix_nano.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::LocalRetentionMetrics;
    use crate::shared_kernel::archive_receipts::LocalExpiryReceipt;

    #[test]
    fn record_adds_removed_events_and_counts_the_run() {
        let metrics = LocalRetentionMetrics::default();
        metrics.record(&LocalExpiryReceipt {
            removed_events: 2,
            retained_events: 3,
            oldest_retained_unix_nano: Some(500),
        });
        metrics.record(&LocalExpiryReceipt {
            removed_events: 0,
            retained_events: 3,
            oldest_retained_unix_nano: Some(700),
        });

        assert_eq!(metrics.expired_events(), 2);
        assert_eq!(metrics.runs(), 2);
        assert_eq!(metrics.oldest_retained_unix_nano(), 700);
    }

    #[test]
    fn record_stores_zero_when_nothing_is_retained() {
        let metrics = LocalRetentionMetrics::default();
        metrics.record(&LocalExpiryReceipt {
            removed_events: 1,
            retained_events: 1,
            oldest_retained_unix_nano: Some(9),
        });
        metrics.record(&LocalExpiryReceipt {
            removed_events: 1,
            retained_events: 0,
            oldest_retained_unix_nano: None,
        });

        assert_eq!(metrics.oldest_retained_unix_nano(), 0);
    }

    #[test]
    fn record_stores_zero_for_a_negative_time() {
        let metrics = LocalRetentionMetrics::default();
        metrics.record(&LocalExpiryReceipt {
            removed_events: 0,
            retained_events: 1,
            oldest_retained_unix_nano: Some(-5),
        });

        assert_eq!(metrics.oldest_retained_unix_nano(), 0);
    }
}
