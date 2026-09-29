//! The journal's Prometheus metrics.

use metrics_prometheus::Sample;

use crate::journal::infrastructure::durable_journal::DurableJournal;

impl DurableJournal {
    pub(crate) fn metrics_text(&self) -> String {
        metrics_prometheus::render(&[
            Sample::new(
                "sift_raw_events_total",
                "counter",
                "Durably accepted Sift raw events.",
                self.accepted.get(),
            ),
            Sample::new(
                "sift_duplicate_events_total",
                "counter",
                "Idempotent duplicate Sift event submissions.",
                self.duplicates.get(),
            ),
            Sample::new(
                "sift_journal_fsync_total",
                "counter",
                "Sift journal fsync operations completed before acknowledgement.",
                self.fsyncs.get(),
            ),
            Sample::new(
                "sift_journal_resident_events",
                "gauge",
                "Sift events currently retained in journal memory.",
                self.resident_event_count() as u64,
            ),
        ])
    }
}
