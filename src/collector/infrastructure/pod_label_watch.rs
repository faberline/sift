//! Kubernetes discovery of the Pods that carry the `--pod-label` selector. The
//! first list must succeed before collection starts; later refreshes keep the
//! last set when the API cannot be reached.

use std::time::Duration;

use anyhow::{Context, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api};

use crate::collector::domain::pod_labels::{PodLabelSelector, PodLabelSet};

const POD_LABEL_REFRESH: Duration = Duration::from_secs(5);

/// Lists the Pods that carry the selector, once, and then keeps the set current
/// in the background. Empty `namespaces` means every namespace.
pub async fn watch_pod_labels(
    namespaces: &[String],
    selector: PodLabelSelector,
    node: Option<String>,
) -> Result<PodLabelSet> {
    let client = kube::Client::try_default()
        .await
        .context("create Kubernetes client for --pod-label")?;
    let apis: Vec<Api<Pod>> = if namespaces.is_empty() {
        vec![Api::all(client)]
    } else {
        namespaces
            .iter()
            .map(|namespace| Api::namespaced(client.clone(), namespace))
            .collect()
    };
    let params = list_params(&selector, node.as_deref());
    let set = PodLabelSet::default();
    set.replace(list_uids(&apis, &params, &selector).await?);

    let refreshed = set.clone();
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(POD_LABEL_REFRESH);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick is immediate and the first list has already run.
        ticks.tick().await;
        loop {
            ticks.tick().await;
            match list_uids(&apis, &params, &selector).await {
                Ok(uids) => refreshed.replace(uids),
                Err(error) => tracing::warn!(%error, "pod label discovery failed"),
            }
        }
    });
    Ok(set)
}

fn list_params(selector: &PodLabelSelector, node: Option<&str>) -> ListParams {
    let params = ListParams::default().labels(&format!("{}={}", selector.key, selector.value));
    match node {
        Some(node) => params.fields(&format!("spec.nodeName={node}")),
        None => params,
    }
}

async fn list_uids(
    apis: &[Api<Pod>],
    params: &ListParams,
    selector: &PodLabelSelector,
) -> Result<Vec<String>> {
    let mut uids = Vec::new();
    for api in apis {
        let pods = api
            .list(params)
            .await
            .context("list Pods for --pod-label")?;
        uids.extend(matching_uids(&pods.items, selector));
    }
    Ok(uids)
}

/// The UIDs of the Pods whose labels hold the selector's key with exactly its value.
pub(crate) fn matching_uids(pods: &[Pod], selector: &PodLabelSelector) -> Vec<String> {
    pods.iter()
        .filter(|pod| {
            pod.metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get(&selector.key))
                .is_some_and(|value| value == &selector.value)
        })
        .filter_map(|pod| pod.metadata.uid.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn pod(value: Value) -> Pod {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn matching_uids_keeps_only_the_labelled_pod() {
        let selector = PodLabelSelector::parse("sift.axiom.dev/logs=true").unwrap();
        let pods = [
            pod(
                json!({"metadata":{"name":"a","uid":"u1","namespace":"prod","labels":{"sift.axiom.dev/logs":"true"}}}),
            ),
            pod(json!({"metadata":{"name":"b","uid":"u2","namespace":"prod","labels":{}}})),
            pod(
                json!({"metadata":{"name":"c","uid":"u3","namespace":"prod","labels":{"sift.axiom.dev/logs":"false"}}}),
            ),
        ];
        assert_eq!(matching_uids(&pods, &selector), vec!["u1".to_string()]);
    }
}
