//! Whether an incoming event falls outside its project's retention window, and
//! the rejection message when it does.

use chrono::Utc;

use crate::event::EventEnvelope;
use crate::shared_kernel::retention_boundary::retention_rejection_at;

pub(crate) fn retention_rejection(event: &EventEnvelope) -> Option<String> {
    retention_rejection_at(event, Utc::now())
}
