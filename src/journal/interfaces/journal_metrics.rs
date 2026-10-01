//! The journal's Prometheus metrics.

use metrics_prometheus::Sample;

use crate::journal::application::journal_service::DurableJournal;

impl DurableJournal {
    pub(crate) fn metrics_text(&self) -> String {
        let values = self.metrics_snapshot();
        metrics_prometheus::render(&[
            Sample::new(
                "sift_raw_events_total",
                "counter",
                "Durably accepted Sift raw events.",
                values[0],
            ),
            Sample::new(
                "sift_duplicate_events_total",
                "counter",
                "Idempotent duplicate Sift event submissions.",
                values[1],
            ),
            Sample::new(
                "sift_journal_fsync_total",
                "counter",
                "Sift journal fsync operations completed before acknowledgement.",
                values[2],
            ),
            Sample::new(
                "sift_journal_resident_events",
                "gauge",
                "Sift events currently retained in journal memory.",
                values[3],
            ),
        ])
    }
}
