#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script_root="$(cd "$script_dir" && pwd -P)"
if [[ -n "${SIFT_REPO_ROOT:-}" ]]; then
  if ! supplied_repo_root="$(cd "$SIFT_REPO_ROOT" 2>/dev/null && pwd -P)"; then
    echo "SIFT_REPO_ROOT does not name a readable directory" >&2
    exit 1
  fi
  if [[ "$supplied_repo_root" != "$script_root" ]]; then
    echo "SIFT_REPO_ROOT must match the repository that contains ./test.sh" >&2
    exit 1
  fi
fi
repo_root="$script_root"
export SIFT_REPO_ROOT="$repo_root"
cd "$repo_root"

if [[ "${1:-}" == "--print-repo-root" ]]; then
  printf '%s\n' "$repo_root"
  exit 0
fi

if [[ "${1:-}" == "--candidate" ]]; then
  shift
  [[ $# == 0 ]] || { echo 'candidate mode does not accept test filters' >&2; exit 1; }
  if [[ -n "${VAT_OBSERVABILITY_PROBE+x}" || -n "${VAT_SERVICE_OBSERVABILITY_PROBE+x}" ]]; then
    echo 'candidate mode refuses inherited VAT_OBSERVABILITY_PROBE or VAT_SERVICE_OBSERVABILITY_PROBE' >&2
    exit 1
  fi
  if [[ -n "${SIFT_BIN:-}" ]]; then
    echo "Sift candidate mode does not accept a caller-supplied SIFT_BIN" >&2
    exit 1
  fi
  candidate_revision="${SIFT_SOURCE_REVISION:-}"
  if [[ -z "$candidate_revision" ]]; then
    candidate_revision="$(
      git -c core.fsmonitor=false -C "$repo_root" rev-parse HEAD 2>/dev/null
    )" || {
      echo "SIFT_SOURCE_REVISION is required outside a Git checkout" >&2
      exit 1
    }
  fi
  candidate_revision="$(printf '%s' "$candidate_revision" | tr '[:upper:]' '[:lower:]')"
  [[ "$candidate_revision" =~ ^[0-9a-f]{40}$ ]] || {
    echo "SIFT_SOURCE_REVISION must be a full 40-character Git SHA" >&2
    exit 1
  }
  export SIFT_SOURCE_REVISION="$candidate_revision"
  bash tests/candidate_root.sh
  # These are real-service oracles used by the two VAT stdout journeys.
  # They belong to independent repositories, not this Cargo workspace.
  missing_binary=0
  for binary in vat lumen tape relay defer; do
    if ! binary_path="$(command -v "$binary")" || [[ ! -f "$binary_path" || ! -x "$binary_path" ]]; then
      echo "candidate requires $binary from its faberline repository on PATH" >&2
      missing_binary=1
      continue
    fi
    python3 - "$binary" "$binary_path" <<'PY'
import hashlib, pathlib, sys
path = pathlib.Path(sys.argv[2]).resolve(strict=True)
digest = hashlib.sha256()
with path.open('rb') as stream:
    for chunk in iter(lambda: stream.read(1024 * 1024), b''):
        digest.update(chunk)
print(f'candidate oracle {sys.argv[1]}: {path} sha256={digest.hexdigest()}')
PY
  done
  [[ "$missing_binary" == 0 ]] || exit 1
  cargo build --locked -p sift --bins
  bash scripts/faberline-core-test.sh build-stamp
  cargo test --locked -p sift "$@"
  cargo test --locked -p sift --test it -- vat_lumen_observability_e2e:: --ignored
  cargo test --locked -p sift --test it -- vat_service_observability_e2e:: --ignored
  candidate_bin="${CARGO_TARGET_DIR:-$repo_root/target}/debug/sift"
  [[ -x "$candidate_bin" ]] || {
    echo "candidate Sift binary was not produced at $candidate_bin" >&2
    exit 1
  }
  SIFT_EXPECTED_SOURCE_REVISION="$candidate_revision" \
    bash tests/prometheus_compliance.sh
  bash tests/docker_named_volume.sh
  exit 0
fi

exec cargo test --locked -p sift "$@"
