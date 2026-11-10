#!/usr/bin/env bash
set -euo pipefail
sift_root="${SIFT_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)}"
export UV_CACHE_DIR="${UV_CACHE_DIR:-${TMPDIR:-/tmp}/sift-uv-cache}"
if ! base_sha="$(git -C "$sift_root" rev-parse --verify --quiet 'refs/heads/main^{commit}')"; then
  base_sha="$(git -C "$sift_root" rev-parse --verify --quiet 'refs/remotes/origin/main^{commit}')" || {
    echo "The architecture check requires a local or fetched main commit" >&2
    exit 1
  }
fi
uv run "$sift_root/scripts/meta/rust_arch_contract.py" \
  check --repo "$sift_root" --strict --base "$base_sha"
