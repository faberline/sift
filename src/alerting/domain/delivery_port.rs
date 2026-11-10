//! Delivery is an effect. The caller persists its result.
use super::state::Notification;
use crate::shared_kernel::port_future::PortFuture;
pub(crate) trait WebhookPort: Send + Sync {
    fn send<'a>(&'a self, notification: &'a Notification) -> PortFuture<'a, (), String>;
}
