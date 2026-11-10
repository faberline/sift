#!/usr/bin/env bash
# Verify the official archive before using its promtool binary.
verify_official_promql_oracle() {
  local oracle_os oracle_arch archive_name expected cache archive work_root result
  case "$(uname -s)" in Darwin) oracle_os=darwin;; Linux) oracle_os=linux;; *) echo 'unsupported oracle OS' >&2; return 1;; esac
  case "$(uname -m)" in arm64|aarch64) oracle_arch=arm64;; x86_64|amd64) oracle_arch=amd64;; *) echo 'unsupported oracle CPU' >&2; return 1;; esac
  case "$oracle_os-$oracle_arch" in
    darwin-arm64) expected=c17bc2c992f0b5515eebe2b4ccd626214f8e309a0fe6ec40ed13c42ec13d05fb;;
    darwin-amd64) expected=5856d0c306e7d852d9a79788b20acb630501b3fab120b69f5317d6b0732dea57;;
    linux-amd64) expected=e811827af26d822afb09a4f28314f61b618b12cff5369835a67f674d8b46f39a;;
    linux-arm64) expected=173389cc42bf09c4e6e54cb53fa07a5a835d7c261e14775d2183181d6e385d1c;;
  esac
  archive_name="prometheus-3.5.0.$oracle_os-$oracle_arch"
  cache="${SIFT_ORACLE_CACHE:-${TMPDIR:-/tmp}/sift-prometheus-oracle-cache}"
  mkdir -p "$cache" || return 1
  archive="$cache/$archive_name.tar.gz"
  if [[ ! -f "$archive" ]]; then
    curl --fail --silent --show-error --location --connect-timeout 20 --max-time 300 \
      "https://github.com/prometheus/prometheus/releases/download/v3.5.0/$archive_name.tar.gz" \
      --output "$archive" || return 1
  fi
  python3 - "$archive" "$expected" <<'PY' || return 1
import hashlib,pathlib,sys
path=pathlib.Path(sys.argv[1])
if path.is_symlink() or not path.is_file(): raise SystemExit('oracle archive must be a regular file')
with path.open('rb') as stream: actual=hashlib.file_digest(stream,'sha256').hexdigest()
if actual!=sys.argv[2]: raise SystemExit('official Prometheus archive digest mismatch')
PY
  work_root="$(mktemp -d "${TMPDIR:-/tmp}/sift-promql-oracle.XXXXXX")" || return 1
  result=0
  tar -xzf "$archive" -C "$work_root" || result=$?
  if [[ "$result" == 0 ]]; then
    python3 -B scripts/verify-prometheus-oracle.py --promtool "$work_root/$archive_name/promtool" || result=$?
  fi
  python3 - "$work_root" <<'PY'
import pathlib,shutil,sys
path=pathlib.Path(sys.argv[1])
if not path.name.startswith('sift-promql-oracle.'): raise SystemExit('unexpected oracle directory')
shutil.rmtree(path)
PY
  return "$result"
}
