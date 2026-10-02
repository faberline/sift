#!/usr/bin/env bash
set -euo pipefail
sift_root="${SIFT_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)}"
export UV_CACHE_DIR="${UV_CACHE_DIR:-${TMPDIR:-/tmp}/sift-uv-cache}"
uv run "$sift_root/scripts/meta/rust_arch_contract.py" \
  check --repo "$sift_root" --strict --base main
