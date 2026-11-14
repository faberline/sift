#!/usr/bin/env bash
# Build the release image twice from one binary and prove that both builds
# give the same image digest. No cache is reused, so the second build is a
# real rebuild. Prints one evidence line on success and exits 0.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
  cat >&2 <<'USAGE'
Usage: verify-reproducible-image.sh [--binary <path>]

Builds Dockerfile.release twice from the same linux/amd64 binary with
--no-cache and fixed SOURCE_DATE_EPOCH, and fails when the image digests differ.
Without --binary, builds target/release/sift with cargo first.
USAGE
  exit 2
}

binary=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)
      [[ $# -ge 2 ]] || usage
      binary="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      usage
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
declare -a digests=()
builder="$(docker buildx create --driver docker-container)"
trap 'docker buildx rm "$builder" >/dev/null 2>&1 || true; rm -rf "$tmp"' EXIT

for run in 1 2; do
  ctx="$tmp/ctx$run"
  install -D -m 0755 "$binary" "$ctx/dist/linux/amd64/sift"
  find "$ctx" -exec touch -h -d "@$epoch" {} +

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
  digests[$run]="$digest"
done

if [[ "${digests[1]}" != "${digests[2]}" ]]; then
  {
    echo "image digests differ"
    echo "build 1: ${digests[1]}"
    echo "build 2: ${digests[2]}"
  } >&2
  exit 1
fi

echo "reproducible: ${digests[1]}"
