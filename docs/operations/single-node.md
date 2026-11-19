# Run Sift as one Pod (single-node topology)

This page explains how to run Sift on Kubernetes as one `all`-role Pod with one
PVC. It is the `singleNode` topology of the `Sift` resource. The three-voter
topology is the default and is described in [ha.md](ha.md).

## When to use it

Use `spec.topology: singleNode` when one copy of the data on one volume is
acceptable for the installation. `--role all` with one PVC is a supported production mode, not only a trial mode. Read the profile contract in
[single-node-v1.md](../single-node-v1.md) for its promises, limits and gates.

The profile has no live GKE evidence yet: every gate in
[single-node-v1.md](../single-node-v1.md) is not run. Do not read this page as
a capacity or cost statement.

## Install

Copy the field layout of `k8s/instances/dev.yaml` and set the values below.
Replace each `REPLACE_ME__` placeholder with your own value.

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
```

`20Gi` is an example value, not a recommendation. The fields of the `ha`
topology are not set here.

### Run on a tainted node pool

When the Pod must run on a node pool with a taint, set the pool's label in
`placement.nodeSelector`, a matching toleration in `placement.tolerations` and
the class of the volume in `storage.storageClass`:

```yaml
spec:
  topology: singleNode
  placement:
    nodeSelector:
      faberline.dev/pool: sift-server
    tolerations:
      - key: faberline.dev/pool
        operator: Equal
        value: sift-server
        effect: NoSchedule
  storage:
    storeSize: 20Gi
    storageClass: dynamic-rwo
```

The same `placement` and `storage` fields also apply to the `ha` topology. An
empty `storageClass` uses the cluster default class.

## What the operator renders

For `spec.topology: singleNode` the operator renders:

- one ServiceAccount named `<name>`;
- one Service named `<name>` with the ports `http` and `otlp-grpc`;
- one StatefulSet named `<name>-all` with 1 replica and one PVC template named
  `data`, mounted at `/var/lib/sift`, which gets `storageClassName` when
  `spec.storage.storageClass` is set; the container args are
  `serve --role all --data-dir /var/lib/sift`;
- two NetworkPolicies, `<name>-network` and `<name>-all-network`.

It renders no Deployment, no DaemonSet (so no log agent and no metric agent),
no PodDisruptionBudget and no CronJob (so no backup job). It renders no Role or
RoleBinding. It renders an FQDNNetworkPolicy only when a `gs://` destination or
`alerting.allowedHosts` is set. It renders a ClusterRoleBinding only when
`auth: kubernetes` is set.

The Pod mounts the `peerTlsSecret` volume, but the single Pod has no peer to
talk to, so the peer endpoints are not set for the `all` role.

## Size the PVC

`spec.storage.storeSize` (default `50Gi`) sets the size of the one PVC. The
deprecated `spec.dataSize` wins over it when it is set. The other three
`spec.storage` fields (`controlSize`, `gatewaySize`, `querySize`) are not used
by `singleNode`.

A volume cannot be shrunk. This page gives no sizing formula: the sizing
formula comes with the capacity numbers (issue #246).

## What is lost against the three-voter topology

- **No quorum** - there is one copy of the data, and no second voter confirms a write.
- **No failover** - no other Pod takes over.
- **Unavailable while it restarts** - while the Pod restarts, the API, ingest and query do not answer.
- **Recovery only from the PVC or a GCS archive or backup** - a lost volume loses the local copy. The recovery point survives only when a `gs://` destination is set (`spec.archive.destination`, see the "Small installation" section of [ha.md](ha.md)).

## Related

- [single-node-v1.md](../single-node-v1.md) - the profile contract: promises, limits and gates.
- [ha.md](ha.md) - the three-voter topology and the small installation.
- [production-v1.md](../production-v1.md) - the production contract.
