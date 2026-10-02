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
  if git -C "$repo_root" rev-parse --git-dir >/dev/null 2>&1; then
    [[ "$(git -C "$repo_root" rev-parse HEAD)" == "$candidate_revision" ]] || { echo 'candidate source does not match HEAD' >&2; exit 1; }
    [[ -z "$(git -C "$repo_root" status --porcelain)" ]] || { echo 'candidate mode requires committed source bytes' >&2; exit 1; }
  fi
  candidate_report="${CARGO_TARGET_DIR:-$repo_root/target}/sift-candidate-check.json"
  rm -f -- "$candidate_report"
  cargo fmt --all -- --check
  cargo build --locked -p sift --bins
  source scripts/prepare-test-environment.sh
  prepare_sift_test_environment
  bash scripts/faberline-core-test.sh build-stamp
  cargo test --locked -p sift "$@"
  cargo test --locked -p sift --test it -- --ignored --test-threads=1
  cargo clippy --locked -p sift --all-targets -- -D warnings
  bash scripts/check-architecture.sh
  python3 -B scripts/release/verify-release-contract.py --self-test
  python3 -B scripts/release/production_receipt.py
  python3 -B scripts/production/test_prepare.py
  python3 -B scripts/production/test_runtime.py
  python3 -B scripts/production/test_exports.py
  SIFT_PRODUCTION_TEST_BIN="${CARGO_TARGET_DIR:-$repo_root/target}/debug/sift" \
    python3 -B scripts/production/test_load.py
  python3 -B scripts/release/make-gke-release-receipt.py --self-test
  source scripts/prepare-prometheus-oracle.sh
  verify_official_promql_oracle
  candidate_bin="${CARGO_TARGET_DIR:-$repo_root/target}/debug/sift"
  [[ -x "$candidate_bin" ]] || {
    echo "candidate Sift binary was not produced at $candidate_bin" >&2
    exit 1
  }
  SIFT_EXPECTED_SOURCE_REVISION="$candidate_revision" \
    bash tests/prometheus_compliance.sh
  bash tests/docker_named_volume.sh
  python3 -B - "$candidate_revision" "$repo_root" "$candidate_report" <<'PY'
import datetime,json,os,subprocess,sys,tempfile
from pathlib import Path
revision,root,output=sys.argv[1:]
def git(*args):return subprocess.check_output(['git','-C',root,*args],text=True).strip()
if git('rev-parse','HEAD')!=revision or git('status','--porcelain'):
    raise SystemExit('source changed while the candidate gate was running')
report={'schema':'sift.local-candidate-check/v1','result':'passed','production_accepted':False,
    'source_commit':revision,'completed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'checks':{name:'passed' for name in ['format','locked_build','pinned_core_build_stamp','full_rust_suite',
        'required_environment_tests','clippy','strict_architecture','release_contract',
        'production_receipt_controls','production_preparation_controls','production_runtime_controls',
        'production_comparison_controls','production_load_driver','official_prometheus_oracle',
        'remote_write_compliance','docker_named_volume']}}
path=Path(output)
fd,temporary=tempfile.mkstemp(prefix='.sift-candidate-check-',dir=path.parent)
try:
    with os.fdopen(fd,'w') as out:
        json.dump(report,out,indent=2);out.write('\n');out.flush();os.fsync(out.fileno())
    os.replace(temporary,path)
finally:
    if os.path.exists(temporary):os.unlink(temporary)
print('PASS: exact source candidate checks; GKE production acceptance remains pending')
PY
  exit 0
fi

exec cargo test --locked -p sift "$@"
