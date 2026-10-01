//! Capacity reconciliation after a lifecycle state change.

pub(crate) trait LifecycleCapacity: Send + Sync {
    type Error;
    fn reconcile(&self) -> Result<(), Self::Error>;
}
