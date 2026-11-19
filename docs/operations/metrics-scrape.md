# Scrape metrics from Pods (replacing a PodMonitoring)

## What Sift reads

The metric agent scrapes a Pod only when the Pod carries both the label `sift.axiom.dev/scrape: "true"` and the annotation `prometheus.io/scrape: "true"`. Add the port and path annotations when the defaults do not fit:

```yaml
metadata:
  labels:
    sift.axiom.dev/scrape: "true"
  annotations:
    prometheus.io/scrape: "true"
    prometheus.io/port: "8080"
    prometheus.io/path: "/metrics"
```

When an annotation is missing, the default port is `9090` and the default path is `/metrics`. A response larger than 8 MiB is refused.

## PodMonitoring to Sift

| PodMonitoring field | Sift |
|---|---|
| `selector.matchLabels` | The Pod label `sift.axiom.dev/scrape: "true"`. Sift has no label selector: the Pod carries the one fixed label. |
| `endpoints[].port` | The annotation `prometheus.io/port` (a number; a port name is not supported). |
| `endpoints[].path` | The annotation `prometheus.io/path` (must start with `/`, not `//`). |
| `endpoints[].interval` | One value for the whole agent: `--interval-seconds`, default 30. There is no interval for each Pod. |
| `targetLabels.metadata: [pod]` | The resource attribute `k8s.pod.name` (always set). |
| `targetLabels.metadata: [container]` | The resource attribute `k8s.container.name`: the first container of the Pod that declares a `containerPort` equal to the scraped port. Set only when such a container exists; a port that no container declares gives no attribute. |
| namespaces | `--namespace` of `sift scrape` takes a comma-separated list. Today the operator passes only the namespace of the `Sift` object. |

## What Sift does not support

- A label selector other than the fixed label `sift.axiom.dev/scrape: "true"`.
- A port by name.
- A per-Pod interval.
- `scheme: https`, `params`, `authorization`, `metricRelabeling` and `targetLabels.fromPod`.
- Any format other than Prometheus text format 0.0.4.
- Pods on any node other than the agent's own node: the agent scrapes only Pods on its own node.
- Parallel scrapes: the agent scrapes one Pod after another.

## Check it

Confirm the label is on the Pod:

```sh
kubectl get pod <pod> --show-labels
```

For the query endpoint and how to check that metrics arrive, see [install.md](install.md).
