//! Finds the Kubernetes API server's addresses and ports, which the network
//! policies must let the store and agent roles reach.

use std::collections::BTreeSet;
use std::net::IpAddr;

use k8s_openapi::api::core::v1::Endpoints;
use kube::Api;

pub(super) async fn discover_kubernetes_api_endpoint(
    client: kube::Client,
) -> anyhow::Result<(Vec<String>, Vec<u16>)> {
    let endpoints = Api::<Endpoints>::namespaced(client, "default")
        .get("kubernetes")
        .await
        .map_err(|error| anyhow::anyhow!("discover Kubernetes API endpoints: {error}"))?;
    let mut cidrs = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for subset in endpoints.subsets.into_iter().flatten() {
        for address in subset.addresses.into_iter().flatten() {
            let address = address.ip.parse::<IpAddr>().map_err(|error| {
                anyhow::anyhow!(
                    "Kubernetes API endpoint {} is not an IP address: {error}",
                    address.ip
                )
            })?;
            cidrs.insert(match address {
                IpAddr::V4(address) => format!("{address}/32"),
                IpAddr::V6(address) => format!("{address}/128"),
            });
        }
        for port in subset.ports.into_iter().flatten() {
            if port.protocol.as_deref().unwrap_or("TCP") == "TCP" {
                ports.insert(u16::try_from(port.port).map_err(|_| {
                    anyhow::anyhow!("Kubernetes API endpoint port {} is invalid", port.port)
                })?);
            }
        }
    }
    if cidrs.is_empty() {
        anyhow::bail!("Kubernetes API Endpoints/default/kubernetes has no ready addresses");
    }
    if ports.is_empty() {
        anyhow::bail!("Kubernetes API Endpoints/default/kubernetes has no TCP ports");
    }
    Ok((cidrs.into_iter().collect(), ports.into_iter().collect()))
}
