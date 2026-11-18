//! Checks that the cluster serves the FQDN Network Policy kind before the
//! operator renders a policy of that kind.

pub(super) async fn ensure_fqdn_network_policy_served(client: kube::Client) -> anyhow::Result<()> {
    let served = client
        .list_api_group_resources("networking.gke.io/v1alpha1")
        .await
        .ok()
        .is_some_and(|list| {
            list.resources
                .iter()
                .any(|resource| resource.kind == "FQDNNetworkPolicy")
        });
    if served {
        return Ok(());
    }
    anyhow::bail!(
        "FQDN Network Policy is required but not served: enable Dataplane V2 FQDN Network Policy on the cluster (networking.gke.io/v1alpha1 FQDNNetworkPolicy), or remove the gs:// destinations and alerting allowedHosts"
    )
}
