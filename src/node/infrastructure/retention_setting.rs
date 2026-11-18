//! The retention window that `sift serve` sets once at startup.

use crate::shared_kernel::retention_boundary::{set_configured, RetentionDays};

/// Sets the retention window (1 to 180 days) that ingest and the journal use.
pub fn configure_retention_days(days: u32) -> anyhow::Result<()> {
    let retention = RetentionDays::new(days).map_err(|error| anyhow::anyhow!(error))?;
    set_configured(retention);
    Ok(())
}
