//! The node: one Sift process's own data root. It owns the private, versioned
//! data directory and its layout manifest, the role the process runs as, and
//! the local disk-capacity guard that refuses writes before the volume fills.

pub(crate) mod domain;
pub(crate) mod infrastructure;

/// Sets the retention window (1 to 180 days) that ingest and the journal use.
pub fn configure_retention_days(days: u32) -> anyhow::Result<()> {
    let retention = crate::shared_kernel::retention_boundary::RetentionDays::new(days)?;
    crate::shared_kernel::retention_boundary::set_configured(retention);
    Ok(())
}

pub use crate::node::{
    domain::storage_role::StorageRole,
    infrastructure::{
        data_layout::{DataLayout, LayoutManifest, DEFAULT_DATA_DIR},
        local_capacity::{CapacityLevel, LocalCapacity, LocalCapacityError},
    },
};
