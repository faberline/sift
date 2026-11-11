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
