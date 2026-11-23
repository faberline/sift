# Install Sift

Sift has four install paths: `install.sh`, the container image, Docker Compose
and the Kubernetes manifests. Each command block carries a marker that says
which script or CI step tests it, or why it is `manual`.

## install.sh

`install.sh` installs the newest `sift@X.Y.Z` release into `~/.local/bin`.
Set `SIFT_VERSION=sift@X.Y.Z` to pin one release, and `SIFT_INSTALL` to use
another directory.

`install.sh` checks the archive against its `.sha256` file from the same release.
A missing `.sha256` file is an error (exit code 4). Set `SIFT_INSTALL_SKIP_VERIFY=1`
to install without the check; the script then prints a warning. The `.sha256` file
comes from the same release as the archive, so it detects a damaged download but
does not prove who published the release.

<!-- manual: downloads a published sift@X.Y.Z release from GitHub; no release is published yet and CI has no network release to install -->
```sh
sh ./install.sh
```

Then run the server from the installed binary:

<!-- manual: runs a long-lived server; CI does not start it from an installed binary -->
```sh
sift serve --role all --data-dir ./sift-data
```

From a second shell, check readiness:

<!-- manual: needs the server from the previous block to be running -->
```sh
curl -fsS http://127.0.0.1:7380/readyz
```

To build from source instead, use the release profile:

<!-- manual: CI builds the debug profile (cargo build --locked -p sift --bins), not --release -->
```sh
cargo build --locked --release -p sift
```

## Container image

`Dockerfile` builds Sift from source; it is the image for development and CI.
`Dockerfile.release` is the production image: a static release binary in a
distroless runtime.

Build the source image:

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker build --file Dockerfile --tag sift:dev .
```

Run it with a named volume for its data at `/var/lib/sift`:

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker run --detach --name sift --mount type=volume,source=sift-data,target=/var/lib/sift --publish 127.0.0.1:7380:7380 sift:dev serve --host 0.0.0.0 --port 7380 --grpc-port 4317
```

## Docker Compose

`compose.yaml` builds the source image and keeps the data in the `sift-data`
named volume, so the data survives a restart of the container.

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

The manifests are under `k8s/`: `k8s/crd` holds the CRD, `k8s/operator` the
operator, and `k8s/overlays/template` the template for one instance. The GKE
steps are in the Quickstart of `README.md`. In short:

1. Copy `k8s/overlays/template` to a new directory under `k8s/overlays/`.
2. In the copy's `sift.yaml`, replace each `REPLACE_ME__` value.
3. Create the peer TLS Secret as `docs/operations/ha.md` describes (section "Peer TLS").

Render the template overlay:

<!-- tested: scripts/verify-deployment-artifacts.py -->
```sh
kubectl kustomize k8s/overlays/template
```

Apply your copy. The apply installs the CRD (`k8s/crd`), the operator
(`k8s/operator`) and the instance:

<!-- manual: needs a Kubernetes cluster and credentials; CI only renders the manifests -->
```sh
kubectl apply -k k8s/overlays/<your-copy>
```

Check the instance:

<!-- manual: needs a running cluster -->
```sh
kubectl -n <namespace> get sift
```
