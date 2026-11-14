# Reproducible release image

## What is proven

Two builds of `Dockerfile.release` from the same binary give the same image digest.

## Command

```sh
bash scripts/release/verify-reproducible-image.sh [--binary <path>]
```

The script needs Docker with buildx and `jq`. It needs `cargo` only when `--binary` is not given; then it builds `sift` with `cargo build --locked --release -p sift --bin sift`. CI runs it as the job `test-reproducible-image`.

## Expected result

- Exit code 0 and the single line `reproducible: sha256:<64 hex>` on stdout.
- Exit code 1 when the digests differ: the script prints `image digests differ` with `build 1: <digest>` and `build 2: <digest>` on stderr.
- Exit code 2 for a wrong argument, a non-numeric `SOURCE_DATE_EPOCH`, or a missing binary.

## How it works

- `SOURCE=staged` copies the binary from the build context at `dist/linux/amd64/sift`.
- Base images are pinned by digest in `Dockerfile.release`.
- `SOURCE_DATE_EPOCH` is set to the commit time, or to the value given in the environment.
- File times in the build context are set to that same time.
- `rewrite-timestamp=true` clamps layer file times and the image creation time.
- `--no-cache` is set on both builds, so the second build cannot reuse the layers of the first.
- `--provenance=false --sbom=false` leaves out the attestations, which carry the build time.

## Limits

- The script builds the image twice from one binary. It does not prove that `cargo build` gives identical binaries on two machines.
- It covers `linux/amd64` only.
- The `SOURCE=fetch` path, which downloads a release, is not covered.
