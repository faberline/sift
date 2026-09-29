//! Readiness: the service reports not ready while it drains, while the journal
//! needs recovery, or while local storage is critical.

use std::sync::atomic::Ordering;

use crate::app::service_state::ServiceState;
use crate::node::infrastructure::local_capacity::CapacityLevel;

impl service_http::ReadinessHook for ServiceState {
    fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
            || self.journal.recovery_required()
            || self.local_capacity.level() == CapacityLevel::Critical
    }
}
