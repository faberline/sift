//! Render a resolved instance and obtain platform network facts.

use crate::shared_kernel::instance_spec::InstanceSpec;
use crate::shared_kernel::port_future::PortFuture;
use serde_json::Value;

pub(crate) trait InstanceRenderer: Send + Sync {
    type Client;
    type Error;
    fn render(
        &self,
        instance: &InstanceSpec,
        api_cidrs: &[String],
        api_ports: &[u16],
    ) -> Vec<Value>;
    fn discover_api_endpoint(
        &self,
        client: Self::Client,
    ) -> PortFuture<'_, (Vec<String>, Vec<u16>), Self::Error>;
}
