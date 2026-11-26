# Public API over TLS

This page sets up TLS for the public Sift API in production.

## What it does

A GKE Ingress terminates TLS on `https://<host>` with a Google-managed
certificate. Traffic then goes to the `sift-gateway` Service on port 7380
inside the cluster. Plain `http://` is refused at the load balancer.

Raft peer mTLS on port 7381 is separate. See [ha.md](ha.md) for it.

The manifest is `k8s/overlays/prod/public-tls.yaml`. It holds the
`ManagedCertificate` `sift-public` and the Ingress `sift-public`, both in
namespace `production`. Its values are `REPLACE_ME__` placeholders.

## Set up

1. Reserve a global static IP address:

   ```sh
   gcloud compute addresses create <name> --global
   ```

2. Point a DNS A record for the public host at that address.
3. In `k8s/overlays/prod/public-tls.yaml`, replace
   `REPLACE_ME__SIFT_PUBLIC_HOST` with the host and
   `REPLACE_ME__SIFT_PUBLIC_STATIC_IP_NAME` with the address name from step 1.
4. Apply the overlay:

   ```sh
   kubectl apply -k k8s/overlays/prod
   ```

5. Wait until the certificate is `Active`:

   ```sh
   kubectl -n production get managedcertificate sift-public
   ```

## Verify

Use normal certificate checks. Never use `curl -k`.

```sh
curl --fail https://<host>/readyz
```

A request to `http://<host>` must not be served.

## Notes

- The Ingress needs a VPC-native GKE cluster, so the load balancer uses
  container-native load balancing.
- The certificate can take up to an hour to become `Active`. Until then,
  HTTPS requests fail.
- The production topology is in [production-v1.md](../production-v1.md).
  The check that `scripts/verify-deployment-artifacts.py` runs against this
  overlay fails when the public API has no TLS.

## Gateway API variant

Use this variant when the cluster exposes services through a shared Gateway.
The certificate is attached to the Gateway. It needs no `ManagedCertificate`
and no Ingress controller.

The component is in `k8s/components/gateway-api-public`. It has an
`HTTPRoute` for the HTTP API on port 7380 and a `GRPCRoute` for OTLP/gRPC on
port 4317.

Steps:

1. Replace the four `REPLACE_ME__` placeholders in `httproute.yaml` and
   `grpcroute.yaml`: the Gateway name, the Gateway namespace, the listener
   name and the public host.
2. In an overlay `kustomization.yaml`, add `components:` with
   `../../components/gateway-api-public`. Do not list `public-tls.yaml` in
   `resources:`. The prod overlay in this repository keeps the Ingress. Do not
   edit it.
3. Apply the overlay. Then check the routes: `kubectl -n production get httproute,grpcroute`.
4. Verify the public API: `curl --fail https://<host>/readyz`.

The Gateway API CRDs must already be installed in the cluster.

### Which paths are safe to expose

| Path | Exposed | Why |
|---|---|---|
| `/v1/logs`, `/v1/metrics`, `/v1/traces` (POST), `/prometheus/api/v1/write` | yes | Ingest. Needs a token when `spec.auth` is `required` or `kubernetes`. |
| `/api/v1/query`, `/api/v2/query`, `/api/v1/logs/tail`, `/api/v1/traces/{id}`, `/v1/traces/{id}`, `/api/v1/correlate`, `/api/v1/services`, `/api/v1/queries/{id}`, `/prometheus/api/v1/query`, `/prometheus/api/v1/query_range` | yes, with auth on | Query. |
| `/api/v1/saved-queries`, `/api/v1/error-groups`, `/api/v1/alert-rules`, `/api/v1/alerts` | yes, with auth on | Rule and saved-query management. |
| `/readyz` | yes | Readiness check for the verify command. |
| OTLP/gRPC, port 4317 (`GRPCRoute`) | yes | Same token rule as ingest. The Gateway listener must be HTTPS with HTTP/2. |
| `/mcp` | no | In `kubernetes` auth mode, the path-level authorization skips `/mcp`. The transport checks only Host and Origin. Use `kubectl port-forward` or an internal route. |
| `/admin/backup`, `/admin/integrity` | no | Operator routes that need the Admin role. Internal only. |
| `/api/v1/recovery/data` | no | Admin recovery capture. Internal only. |

No live GKE run proves this component yet. A local test is not evidence for a
production claim (see `README.md`).
