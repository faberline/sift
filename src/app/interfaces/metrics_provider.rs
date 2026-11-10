//! The metrics `/metrics` renders: the journal's metrics, then local storage
//! use and its warning and critical levels.

use metrics_prometheus::Sample;

use crate::app::service_state::ServiceState;
use crate::node::infrastructure::local_capacity::CapacityLevel;

impl service_http::MetricsProvider for ServiceState {
    fn render_metrics(&self) -> String {
        let mut text = self.journal.metrics_text();
        let (pending, failures, leader) = self
            .control
            .as_ref()
            .map(|control| control.notification_metrics())
            .unwrap_or_default();
        text.push_str(&metrics_prometheus::render(&[
            Sample::new(
                "sift_query_failures_total",
                "counter",
                "Failed or incomplete query executions.",
                self.query_failures
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            Sample::new(
                "sift_notification_failures_total",
                "counter",
                "Failed Webhook delivery attempts on this process.",
                failures,
            ),
            Sample::new(
                "sift_pending_notifications",
                "gauge",
                "Pending outbox records observed by the current control worker.",
                pending,
            ),
            Sample::new(
                "sift_control_leader",
                "gauge",
                "One when this control worker currently leads its Raft group.",
                u64::from(leader),
            ),
            Sample::new(
                "sift_remote_protection_lag_seconds",
                "gauge",
                "Age of the complete remote data and control recovery record.",
                if self.recovery_guard.enabled {
                    self.recovery_guard.lag(chrono::Utc::now().timestamp())
                } else {
                    0
                },
            ),
            Sample::new(
                "sift_remote_protection_backpressure",
                "gauge",
                "One when the remote recovery point prevents new acknowledgements.",
                u64::from(
                    self.recovery_guard
                        .check(chrono::Utc::now().timestamp())
                        .is_err(),
                ),
            ),
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
