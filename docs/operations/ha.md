# Sift high availability operations

Sift has one product boundary and several runtime roles. Every role uses the
same Rust binary and the same `/var/lib/sift` data-root rules.

## Small installation

Run `sift serve --role all` for local use, Docker, or a small installation.
This role is persistent by default. Mount a durable volume at `/var/lib/sift`.

The `all` role is one failure domain. A process restart recovers its WAL and
projections, but one lost volume loses the local copy. Use GCS archives and
backups when the recovery point must survive a lost node or disk.

On Kubernetes, `spec.topology: singleNode` runs the `all` role as one Pod on one PVC. See [Single-node operations](single-node.md) for the install, the PVC size and what is lost against the three-voter topology.

## GKE role topology

One `Sift` resource renders these workloads:

- gateway Deployment
- query Deployment
- three-replica store StatefulSet
- three-replica control StatefulSet
- agent DaemonSet
- optional backup CronJob

The client Service selects the gateway. The gateway forwards HTTP and
OTLP/gRPC ingest to the store. It forwards queries to the query role. The query
role reads from the store source of truth.

Store and control each use one fixed three-voter Raft group. Two durable voters
must commit a write before Sift reports success. Live membership changes and a
different voter count are refused.

Each pod that owns local state has its own PVC at `/var/lib/sift`. Do not use an
`emptyDir` for Sift data. The pod has no init container. Kubernetes makes the volume group-writable for `fsGroup: 65532`, and Sift creates its directories below `/var/lib/sift` with mode `0700`.
The workload runs as a non-root user. The pod security context uses
`fsGroupChangePolicy: OnRootMismatch`.

## Peer TLS

Raft traffic uses mutual TLS on port `7381`. It does not share the public API
port.

Set `spec.peerTlsSecret` to a Secret with these keys:

- `tls.crt`
- `tls.key`
- `ca.crt`

The certificate must permit both TLS client and server use. Its server names
must cover every pod name below both headless Services:

```text
<instance>-store-{0,1,2}.<instance>-store-headless.<namespace>.svc
<instance>-control-{0,1,2}.<instance>-control-headless.<namespace>.svc
```

Sift refuses replicated startup when mutual TLS is off or any file is absent.

### cert-manager Certificate

This example issues the three keys into the Secret that `spec.peerTlsSecret`
names. Set `spec.secretName` to the same name. Its `dnsNames` are the six pod
names from the block above, in the namespace of the instance. `usages` holds
both `server auth` and `client auth`, because the peer link uses mutual TLS.
A CA issuer (an `Issuer` of type `ca`) makes the Secret hold `ca.crt` next to
`tls.crt` and `tls.key`. The `Issuer` is not part of this example; create it
in the same namespace.

```yaml
apiVersion: cert-manager.io/v1
kind: Certificate
metadata:
  name: sift-peer-tls
  namespace: observability
spec:
  secretName: sift-peer-tls
  duration: 2160h
  renewBefore: 720h
  privateKey:
    algorithm: ECDSA
    size: 256
  usages:
    - server auth
    - client auth
  dnsNames:
    - sift-store-0.sift-store-headless.observability.svc
    - sift-store-1.sift-store-headless.observability.svc
    - sift-store-2.sift-store-headless.observability.svc
    - sift-control-0.sift-control-headless.observability.svc
    - sift-control-1.sift-control-headless.observability.svc
    - sift-control-2.sift-control-headless.observability.svc
  issuerRef:
    name: sift-peer-ca
    kind: Issuer
```

### Rotation

A running pod reloads the peer TLS files without a restart. The reload is
`spawn_peer_tls_reload` in `src/app/peer_tls_reload.rs`. A replicated process
starts it from `src/app/service_state.rs`. The process reads `tls.crt`,
`tls.key` and `ca.crt` every 30 seconds (`PEER_TLS_RELOAD_INTERVAL`). It
reloads the transport only when the content of one of the files changed.
A file that cannot be read, or a reload that fails, keeps the previous
certificate and logs a warning. The process tries again only when the content
changes. The test `peer_tls_rotation_keeps_requests_ok_and_uses_the_new_certificate`
in `src/app/peer_tls_reload/tests.rs` proves that requests keep working and
that the new certificate is used.

The kubelet refreshes a mounted Secret on its own schedule. A new certificate
therefore reaches a pod some time after cert-manager renews it.

Sift does not prove that a `SecretProviderClass` (CSI) volume rotates. The
reload reads the same three files from the mount. The CSI driver must refresh
the mount. For the Secrets Store CSI driver, the cluster admin turns on its
rotation feature.

## Kubernetes delegated authentication

Set `spec.auth: kubernetes` to use TokenReview and SubjectAccessReview. The
runtime ServiceAccount is named after the Sift instance. The operator creates
an instance-scoped ClusterRoleBinding to the built-in
`system:auth-delegator` role. The operator can bind only that role. It installs
a finalizer before creating the binding. It deletes the binding before auth is
disabled or the Sift resource is deleted. It also reads the ready
`default/kubernetes` Endpoints object and limits review traffic to those IPs.
Reconcile fails before workloads start if no ready API endpoint exists. Sift
checks the delegated grant at process startup and fails closed when it is
absent.

The agent and backup job use a projected ServiceAccount token with audience
`sift.axiom.dev`. Sift rereads the token file for every request. This supports
normal Kubernetes token rotation.

## Backup and archive

Use the protected live snapshot boundary for online backup:

```sh
sift backup \
  --url http://<instance>.<namespace>.svc.cluster.local:7380 \
  --token-file /var/run/secrets/sift/client/token \
  --token-audience sift.axiom.dev \
  --project <instance> \
  --dest gs://example-sift-backups/sift \
  --retention-secs 604800
```

The operator can render the same command in a CronJob. The job does not mount a
serving PVC.

`sift backup --data-dir` is offline-only. Stop the process that owns the data
root before using it:

```sh
sift backup \
  --data-dir /var/lib/sift \
  --dest file:///recovery/sift \
  --retention-secs 604800
```

Restore an exact backup while the replacement Sift process is stopped:

```sh
sift restore \
  --data-dir /var/lib/sift \
  --source file:///recovery/sift-backup.json
```

Signal archives use immutable Snappy-compressed Parquet objects. Sift writes
the archive manifest last. It compacts the corresponding WAL only after that
manifest is committed. If GCS is unavailable, Sift keeps the local WAL and
returns backpressure before the configured local capacity limit.

## Verification boundary

The local `raft_failover` test starts three durable voters with mutual TLS. It
commits data, stops the leader, elects another leader, and commits more data.
The surviving quorum retains both writes.

This is not a live GKE recovery result. An MVP candidate still needs the
dedicated 30-minute, 10,000-item-per-second GKE run. That run includes PVC
pod recreation, voter failover, a GCS outage, and fresh-PVC restore. Production
high availability still requires the later 24-hour and 100,000-item-per-second
gates.
