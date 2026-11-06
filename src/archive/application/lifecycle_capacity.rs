//! Reconcile the shared local capacity instance after lifecycle work.

use crate::archive::domain::lifecycle_capacity::LifecycleCapacity;
use std::sync::Arc;

pub(crate) struct LifecycleCapacityService {
    port: Arc<dyn LifecycleCapacity<Error = anyhow::Error>>,
}

impl LifecycleCapacityService {
    pub(crate) fn new(port: Arc<dyn LifecycleCapacity<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(crate) fn reconcile(&self) -> anyhow::Result<()> {
        self.port.reconcile()
    }
}
