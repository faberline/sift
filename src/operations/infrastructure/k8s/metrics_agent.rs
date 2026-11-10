//! A second process keeps log collection independent of metric scrape delays.
use crate::shared_kernel::instance_spec::InstanceSettings;
use serde_json::{json, Value};
use service_k8s::render::RenderCtx;
pub(super) fn append_metrics_agent(
    children: &mut Vec<Value>,
    cx: &RenderCtx<'_>,
    spec: &InstanceSettings,
) {
    let Some(mut daemon) = children
        .iter()
        .find(|v| v["kind"] == "DaemonSet" && v["metadata"]["name"] == format!("{}-agent", cx.name))
        .cloned()
    else {
        return;
    };
    daemon["spec"]["selector"]["matchLabels"]["sift.axiom.dev/collector"] = json!("metrics");
    daemon["spec"]["template"]["metadata"]["labels"]["sift.axiom.dev/collector"] = json!("metrics");
    daemon["metadata"]["labels"]["app.kubernetes.io/component"] = json!("agent-metrics");
    daemon["spec"]["selector"]["matchLabels"]["app.kubernetes.io/component"] =
        json!("agent-metrics");
    daemon["spec"]["template"]["metadata"]["labels"]["app.kubernetes.io/component"] =
        json!("agent-metrics");
    let pod = &mut daemon["spec"]["template"]["spec"];
    pod["automountServiceAccountToken"] = json!(true);
    pod["containers"][0]["args"] = json!([
        "scrape",
        "--project",
        cx.name,
        "--environment",
        cx.ns,
        "--namespace",
        cx.ns,
        "--node",
        "$(NODE_NAME)",
        "--interval-seconds",
        "30",
        "--state",
        "/var/lib/sift/agent/metrics"
    ]);
    pod["containers"][0]["volumeMounts"]
        .as_array_mut()
        .expect("agent mounts")
        .retain(|v| v["name"] != "pod-logs");
    pod["volumes"]
        .as_array_mut()
        .expect("agent volumes")
        .retain(|v| v["name"] != "pod-logs");
    pod["containers"][0]["securityContext"] = super::role_workloads::container_security_context();
    pod["containers"][0]["image"] = json!(spec.image);
    daemon["metadata"]["name"] = json!(format!("{}-agent-metrics", cx.name));
    if let Some(logs) = children
        .iter_mut()
        .find(|v| v["kind"] == "DaemonSet" && v["metadata"]["name"] == format!("{}-agent", cx.name))
    {
        logs["spec"]["template"]["metadata"]["labels"]["sift.axiom.dev/scrape"] = json!("true");
        logs["spec"]["template"]["metadata"]["annotations"] = json!({
            "prometheus.io/scrape":"true", "prometheus.io/port":"9318", "prometheus.io/path":"/metrics"
        });
        logs["spec"]["template"]["spec"]["containers"][0]["args"]
            .as_array_mut()
            .expect("log args")
            .extend([
                json!("--metrics-host"),
                json!("0.0.0.0"),
                json!("--metrics-port"),
                json!("9318"),
            ]);
        logs["spec"]["template"]["spec"]["containers"][0]["ports"] = json!([
            {"name":"metrics","containerPort":9318}
        ]);
    }
    children.push(daemon);
}
