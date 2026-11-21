#!/usr/bin/env bash
# Release-notes lines for a release profile. Prints nothing for production,
# so the production notes stay byte for byte the same.
#
# Usage: scripts/release/profile-notes.sh <production|single-node>
set -euo pipefail

case "${1:-}" in
  production)
    exit 0
    ;;
  single-node)
    printf -- '- Profile: profile=single-node\n'
    printf -- '- Claim: this release makes no claim for the three-voter topology.\n'
    exit 0
    ;;
  *)
    printf 'invalid profile: %s\n' "${1:-<none>}" >&2
    exit 2
    ;;
esac
