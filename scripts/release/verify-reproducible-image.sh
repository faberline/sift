#!/usr/bin/env bash
# Builds Dockerfile.release twice from one binary and fails when the two image digests differ.
# Usage: verify-reproducible-image.sh [--binary <path>]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

usage() {
  echo "usage: verify-reproducible-image.sh [--binary <path>]" >&2
}

binary=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      if [[ $# -lt 2 ]]; then
        usage
        exit 2
      fi
      binary="$2"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

epoch="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct)}"
if [[ ! "$epoch" =~ ^[0-9]+$ ]]; then
  echo "SOURCE_DATE_EPOCH must be a number" >&2
  exit 2
fi

if [[ -z "$binary" ]]; then
  (cd "$ROOT" && cargo build --locked --release -p sift --bin sift)
  binary="$ROOT/target/release/sift"
fi
if [[ ! -f "$binary" || ! -x "$binary" ]]; then
  echo "binary not found: $binary" >&2
  exit 2
fi

tmp="$(mktemp -d)"
# The OCI exporter with rewrite-timestamp needs the docker-container driver.
builder="$(docker buildx create --driver docker-container)"
trap 'docker buildx rm "$builder" >/dev/null 2>&1 || true; rm -rf "$tmp"' EXIT

digests=()
for run in 1 2; do
  ctx="$tmp/ctx$run"
  install -D -m 0755 "$binary" "$ctx/dist/linux/amd64/sift"
  find "$ctx" -exec touch -h -d "@$epoch" {} +

  # --no-cache on both builds: a cached second build would prove nothing.
  docker buildx build \
    --builder "$builder" \
    --platform linux/amd64 \
    --no-cache \
    --provenance=false \
    --sbom=false \
    --build-arg SOURCE=staged \
    --build-arg "SOURCE_DATE_EPOCH=$epoch" \
    --file "$ROOT/Dockerfile.release" \
    --output "type=oci,dest=$tmp/image$run.tar,rewrite-timestamp=true" \
    --metadata-file "$tmp/meta$run.json" \
    "$ctx"

  digest="$(jq -r '."containerimage.digest"' "$tmp/meta$run.json")"
  if [[ -z "$digest" || "$digest" == "null" ]]; then
    echo "no image digest in build metadata" >&2
    exit 1
  fi
  digests+=("$digest")
done

if [[ "${digests[0]}" == "${digests[1]}" ]]; then
  echo "reproducible: ${digests[0]}"
  exit 0
fi

echo "image digests differ" >&2
echo "build 1: ${digests[0]}" >&2
echo "build 2: ${digests[1]}" >&2
exit 1
