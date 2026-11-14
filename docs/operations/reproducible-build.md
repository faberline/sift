# Reproducible release image

## What is proven

Two builds of `Dockerfile.release` from the same binary give the same image
digest.

## Command

```sh
bash scripts/release/verify-reproducible-image.sh [--binary <path>]
```

It needs Docker with buildx and `jq`. Without `--binary` it also needs `cargo`
and builds `target/release/sift` first. CI runs it as the job
`test-reproducible-image`.

## Expected result

- The single line `reproducible: sha256:<64 hex>` on stdout, exit code 0.
- When the digests differ, the script prints `image digests differ` with
  `build 1:` and `build 2:` lines on stderr, and exits 1.
- Exit code 2 means a wrong argument, a bad `SOURCE_DATE_EPOCH` or a missing
  binary.

## How it works

- `SOURCE=staged` builds from the candidate binary placed at
  `dist/linux/amd64/sift` in the build context.
- The base images are pinned by digest in `Dockerfile.release`.
- `SOURCE_DATE_EPOCH` is the commit time, unless it is set in the environment.
- The file times in the build context are set to that time.
- `rewrite-timestamp=true` clamps the layer file times and the image creation time.
- `--no-cache` is set on both builds, so the second build is a real rebuild.
- `--provenance=false --sbom=false` leave out the attestations, which carry the build time.

## Limits

- The script builds the image twice from one binary.
- It does not prove that `cargo build` gives identical binaries on two machines.
- It covers `linux/amd64` only.
- The `SOURCE=fetch` path downloads a published release and is not covered.
