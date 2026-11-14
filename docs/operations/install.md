# Install Sift

Sift has four install paths: `install.sh`, the container image, Docker Compose and the Kubernetes manifests. Each command block has a marker line directly above it. A `tested` marker names the file that runs the block in CI. A `manual` marker gives the reason that no test runs the block.

## install.sh

`install.sh` downloads the newest `sift@X.Y.Z` release for your system. It checks the SHA-256 sum when the release publishes one, and it installs `sift` into `~/.local/bin`. Set `SIFT_VERSION=sift@X.Y.Z` to pin one release. Set `SIFT_INSTALL` to install into another directory.

<!-- manual: downloads a published sift@X.Y.Z release from GitHub; no release is published yet and CI has no network release to install -->
```sh
sh ./install.sh
```

Start a server from the installed binary. It keeps its data in `./sift-data`.

<!-- manual: runs a long-lived server; CI does not start it from an installed binary -->
```sh
sift serve --role all --data-dir ./sift-data
```

From another shell, check readiness:

<!-- manual: needs the server from the previous block to be running -->
```sh
curl -fsS http://127.0.0.1:7380/readyz
```

To build from source instead, run:

<!-- manual: CI builds the debug profile (cargo build --locked -p sift --bins), not --release -->
```sh
cargo build --locked --release -p sift
```

## Container image

`Dockerfile` builds Sift from source. Use it for development and CI. `Dockerfile.release` builds the production image: a fully static release binary in a digest-pinned distroless runtime with no shell.

Build the development image:

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker build --file Dockerfile --tag sift:dev .
```

Run the image with a named volume at `/var/lib/sift`. The data stays in the volume when the container is removed:

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker run --detach --name sift --mount type=volume,source=sift-data,target=/var/lib/sift --publish 127.0.0.1:7380:7380 sift:dev serve --host 0.0.0.0 --port 7380 --grpc-port 4317
```

## Docker Compose

`compose.yaml` builds the same `Dockerfile`. It publishes port 7380 and keeps the data in the `sift-data` named volume.

Start the service:

<!-- manual: CI builds the same Dockerfile in tests/docker_named_volume.sh but does not run docker compose -->
```sh
docker compose -f compose.yaml up --build
```

Check readiness:

<!-- manual: needs the compose service to be running -->
```sh
curl -fsS http://127.0.0.1:7380/readyz
```

## Kubernetes

The Kubernetes path has two parts. `k8s/base` installs the cluster CRD from `k8s/crd` and the operator from `k8s/operator`. An overlay adds one Sift instance. `k8s/overlays/template` is the template for an instance. To make your own instance:

1. Copy `k8s/overlays/template` to a new directory under `k8s/overlays/`.
2. In its `sift.yaml`, replace each `REPLACE_ME__` value with your own value.
3. Create the peer TLS Secret. `docs/operations/ha.md` lists the keys it needs, and `spec.peerTlsSecret` names it.

Render the template overlay. CI renders every overlay the same way:

<!-- tested: scripts/verify-deployment-artifacts.py -->
```sh
kubectl kustomize k8s/overlays/template
```

Apply your copy. This needs a cluster and credentials. The apply installs the CRD, the operator and the instance:

<!-- manual: needs a Kubernetes cluster and credentials; CI only renders the manifests -->
```sh
kubectl apply -k k8s/overlays/<your-copy>
```

Check the instance:

<!-- manual: needs a running cluster -->
```sh
kubectl -n <namespace> get sift
```
