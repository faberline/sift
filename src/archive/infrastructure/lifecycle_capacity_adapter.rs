//! Capacity reconciliation retains the original node storage guard.

use crate::archive::domain::lifecycle_capacity::LifecycleCapacity;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use std::sync::Arc;

pub(crate) struct NativeLifecycleCapacity {
    pub(crate) capacity: Arc<LocalCapacity>,
}

impl LifecycleCapacity for NativeLifecycleCapacity {
    type Error = anyhow::Error;
    fn reconcile(&self) -> anyhow::Result<()> {
        self.capacity.reconcile()
    }
}
