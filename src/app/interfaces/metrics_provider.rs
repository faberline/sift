//! The metrics `/metrics` renders: the journal's metrics, then local storage
//! use and its warning and critical levels.

use metrics_prometheus::Sample;

use crate::app::service_state::ServiceState;
use crate::node::infrastructure::local_capacity::CapacityLevel;

impl service_http::MetricsProvider for ServiceState {
    fn render_metrics(&self) -> String {
        let mut text = self.journal.metrics_text();
        text.push_str(&metrics_prometheus::render(&[
            Sample::new(
                "sift_local_storage_used_bytes",
                "gauge",
                "Reserved bytes in the local Sift data root.",
                self.local_capacity.used_bytes(),
            ),
            Sample::new(
                "sift_local_storage_max_bytes",
                "gauge",
                "Configured local Sift storage safety capacity.",
                self.local_capacity.max_bytes(),
            ),
            Sample::new(
                "sift_local_storage_warning",
                "gauge",
                "One when local Sift storage is at or above the 70 percent warning threshold.",
                u64::from(matches!(
                    self.local_capacity.level(),
                    CapacityLevel::Warning | CapacityLevel::Backpressure | CapacityLevel::Critical
                )),
            ),
            Sample::new(
                "sift_local_storage_critical",
                "gauge",
                "One when local Sift storage is at or above the 90 percent readiness threshold.",
                u64::from(self.local_capacity.level() == CapacityLevel::Critical),
            ),
        ]));
        text
    }
}
