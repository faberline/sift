# Collect logs and scrape metrics for one Deployment

This page shows how one Deployment opts in to log collection and metric
scraping with two Pod labels. The labels are independent: a Pod can carry one
or both.

## The two labels

| Label | What it turns on | Set in the Sift resource |
|---|---|---|
| `sift.axiom.dev/logs: "true"` | Collect the container logs of the Pod. | `spec.logs.podLabel: sift.axiom.dev/logs=true` |
| `sift.axiom.dev/scrape: "true"` | Scrape the metrics of the Pod. The Pod also needs the annotation `prometheus.io/scrape: "true"`, see [metrics-scrape.md](metrics-scrape.md). | Nothing. The metric agent always reads this label. |

## Example Deployment

The labels go on the Pod template (`spec.template.metadata`), not on the
Deployment object, because the agents read the labels of Pods.

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
  namespace: team-a
spec:
  selector:
    matchLabels:
      app: app
  template:
    metadata:
      labels:
        app: app
        sift.axiom.dev/logs: "true"
        sift.axiom.dev/scrape: "true"
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "8080"
        prometheus.io/path: "/metrics"
    spec:
      containers:
        - name: app
          image: REPLACE_ME__APP_IMAGE
```

## Example Sift resource

Set the namespaces and the label in the `Sift` object:

```yaml
apiVersion: sift.axiom.dev/v1alpha1
kind: Sift
metadata:
  name: sift
  namespace: default
spec:
  image: REPLACE_ME__SIFT_IMAGE_DIGEST
  peerTlsSecret: REPLACE_ME__SIFT_PEER_TLS_SECRET
  topology: singleNode
  storage:
    storeSize: 20Gi
  auth: "off"
  gcpProjectId: REPLACE_ME__GCP_PROJECT_ID
  gkeClusterName: REPLACE_ME__GKE_CLUSTER_NAME
  gkeLocation: REPLACE_ME__GKE_LOCATION
  logs:
    namespaces: [team-a]
    podLabel: sift.axiom.dev/logs=true
  metrics:
    namespaces: [team-a]
```

The setup of one Sift object for one Deployment is described in
[single-node.md](single-node.md).

## Namespaces and the label work together

A Pod is collected when it is in a listed namespace and carries the label. Both
must hold:

- A Pod with the label in a namespace that is not listed is not collected.
- A Pod in a listed namespace without the label is not collected.
- An empty `logs.namespaces` means the namespace of the `Sift` object.
- Without `logs.podLabel`, every Pod of the listed namespaces is collected. This
  is the default, as before.

The `--pod-label` option of `sift collect` needs `--cri-root` and `--follow`.
The operator always sets both.

## When the label is added or removed

- The agent lists the labelled Pods every 5 seconds, so a change takes effect
  within about 5 seconds.
- A Pod that gets the label is collected from the start of its current log file.
  The lines written before the label are collected too. Lines in log files that
  were already rotated away are not.
- A Pod that loses the label stops being collected. This loses no data that was
  already read.

Before the first successful Pod list, nothing is collected. When a later list
fails, the agent keeps the last set of Pods and logs a warning.

## Permissions

With `logs.podLabel` set, the log agent Pod mounts a Kubernetes API token and
lists Pods with the label selector in each listed namespace. The operator grants
`get` and `list` on Pods only in the namespace of the `Sift` object, in the Role
`<name>-agent-project`. A cluster admin grants `get` and `list` on `pods` in each
other listed namespace. The operator creates no cluster-wide role for this.

See [configuration.md](configuration.md) for the settings.

## Check it

Confirm both labels are on the Pod:

```sh
kubectl get pod -n team-a --show-labels
```

List the Pods that the log agent collects:

```sh
kubectl get pod -n team-a -l sift.axiom.dev/logs=true
```

For the query endpoint and how to check that data arrives, see
[install.md](install.md).
