#!/usr/bin/env bash
# Fetch and build the pinned public sources that the #[ignore] tests need.
# Replaces scripts/ci/prepare.py for the ci.yml test-ignored job.
set -euo pipefail

if [[ -z "${RUNNER_TEMP:-}" ]]; then
  echo "prepare-ignored: RUNNER_TEMP must be set" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
lock="$root/scripts/ci/online-dependencies.lock.json"
dir="$RUNNER_TEMP/sources"
bin_dir="$RUNNER_TEMP/bin"
target_dir="$RUNNER_TEMP/target-tools"

mkdir -p "$dir" "$bin_dir"

for name in workspace vat lumen tape relay defer; do
  sha="$(jq -r --arg n "$name" '.repositories[$n]' "$lock")"
  if [[ -z "$sha" || "$sha" == "null" ]]; then
    echo "prepare-ignored: no pinned commit for $name" >&2
    exit 1
  fi
  checkout="$dir/$name"
  git init -q "$checkout"
  git -C "$checkout" remote add origin "https://github.com/faberline/$name.git"
  git -C "$checkout" fetch -q --depth=1 origin "$sha"
  git -C "$checkout" checkout -q --detach "$sha"
  if [[ "$(git -C "$checkout" rev-parse HEAD)" != "$sha" ]]; then
    echo "prepare-ignored: $name checkout differs from the pinned commit" >&2
    exit 1
  fi
done

for name in vat lumen tape relay defer; do
  CARGO_TARGET_DIR="$target_dir" cargo build --locked \
    --manifest-path "$dir/$name/Cargo.toml" --bin "$name"
  cp "$target_dir/debug/$name" "$bin_dir/$name"
done

workspace="$dir/workspace"
if [[ ! -f "$workspace/acceptance/gcp/scripts/verify-operator-cell.sh" ]]; then
  echo "prepare-ignored: pinned workspace lacks the required local oracle" >&2
  exit 1
fi

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "FABERLINE_WORKSPACE=$workspace" >> "$GITHUB_ENV"
else
  echo "FABERLINE_WORKSPACE=$workspace"
fi
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "$bin_dir" >> "$GITHUB_PATH"
else
  echo "PATH+=$bin_dir"
fi
