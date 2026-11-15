# Install Sift

Sift has four install paths: `install.sh`, the container image, Docker Compose and the Kubernetes manifests under `k8s/`. The marker line above each command block says how its commands are tested: `tested` names the repository file that runs them, and `manual` gives the reason no test runs them. `scripts/verify-install-doc.py` checks that every block has a marker.

## install.sh

`install.sh` installs the newest `sift@X.Y.Z` release into `~/.local/bin`. Set `SIFT_VERSION=sift@X.Y.Z` to pin one release, and `SIFT_INSTALL` to choose another directory.

<!-- manual: downloads a published sift@X.Y.Z release from GitHub; no release is published yet and CI has no network release to install -->
```sh
sh ./install.sh
```

Then start a server with a data directory. It listens on port 7380 by default.

<!-- manual: runs a long-lived server; CI does not start it from an installed binary -->
```sh
sift serve --role all --data-dir ./sift-data
```

Check that it is ready:

<!-- manual: needs the server from the previous block to be running -->
```sh
curl -fsS http://127.0.0.1:7380/readyz
```

To build from source instead, use a release build:

<!-- manual: CI builds the debug profile (cargo build --locked -p sift --bins), not --release -->
```sh
cargo build --locked --release -p sift
```

## Container image

`Dockerfile` builds Sift from source; it is the image for development and CI. `Dockerfile.release` is the production image: a static release binary in a distroless runtime with no shell.

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker build --file Dockerfile --tag sift:dev .
```

Run it with a named volume at `/var/lib/sift`, so the data survives a restart of the container:

<!-- tested: tests/docker_named_volume.sh -->
```sh
docker run --detach --name sift --mount type=volume,source=sift-data,target=/var/lib/sift --publish 127.0.0.1:7380:7380 sift:dev serve --host 0.0.0.0 --port 7380 --grpc-port 4317
```

## Docker Compose

`compose.yaml` builds the same image and keeps the data in the `sift-data` named volume.

<!-- manual: CI builds the same Dockerfile in tests/docker_named_volume.sh but does not run docker compose -->
```sh
docker compose -f compose.yaml up --build
```

Check that it is ready:

<!-- manual: needs the compose service to be running -->
```sh
curl -fsS http://127.0.0.1:7380/readyz
```

## Kubernetes

Copy `k8s/overlays/template` to a new directory under `k8s/overlays/`. In its `sift.yaml`, replace each `REPLACE_ME__` value. Create the peer TLS Secret as [ha.md](ha.md) describes, and set `spec.peerTlsSecret` to its name.

Render the manifests to check them. CI renders the `template` overlay the same way.

<!-- tested: scripts/verify-deployment-artifacts.py -->
```sh
kubectl kustomize k8s/overlays/template
```

Apply your copy. The apply installs the CRD (`k8s/crd`), the operator (`k8s/operator`) and the Sift instance.

<!-- manual: needs a Kubernetes cluster and credentials; CI only renders the manifests -->
```sh
kubectl apply -k k8s/overlays/<your-copy>
```

Check the instance:

<!-- manual: needs a running cluster -->
```sh
kubectl -n <namespace> get sift
```
