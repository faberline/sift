#!/usr/bin/env bash
# Source this file. Test tools have source identities separate from Sift.
prepare_sift_test_environment() {
  local task_root="${SIFT_TEST_ENV_ROOT:-${TMPDIR:-/tmp}/sift-test-environment}"
  local sift_root="${SIFT_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)}"
  local name revision checkout
  mkdir -p "$task_root/bin" "$task_root/sources" || return 1
  while read -r name revision; do
    checkout="$task_root/sources/$name-$revision"
    if [[ ! -d "$checkout/.git" ]]; then
      git clone --quiet --no-checkout "https://github.com/faberline/$name.git" "$checkout" || return 1
      git -C "$checkout" checkout --quiet --detach "$revision" || return 1
    fi
    [[ "$(git -C "$checkout" rev-parse HEAD)" == "$revision" ]] || {
      echo "test dependency source mismatch: $name" >&2; return 1;
    }
    [[ -z "$(git -C "$checkout" status --porcelain)" ]] || {
      echo "test dependency has local changes: $name" >&2; return 1;
    }
    if [[ "$name" == workspace ]]; then
      export FABERLINE_WORKSPACE="$checkout"
    else
      cargo build --locked --manifest-path "$checkout/Cargo.toml" \
        --target-dir "$task_root/target" --bin "$name" || return 1
      cp "$task_root/target/debug/$name" "$task_root/bin/$name" || return 1
      [[ -x "$task_root/bin/$name" ]] || return 1
    fi
  done < <(python3 - "$sift_root/tests/environment.lock.json" <<'PY'
import json, sys
for name, revision in json.load(open(sys.argv[1])).items():
    print(name, revision)
PY
  )
  export PATH="$task_root/bin:$PATH"
}
