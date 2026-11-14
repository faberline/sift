# sift

Sift is one Rust binary for logs, metrics and traces. Read `README.md` and
`STATUS.md` first.

## Verify

- Quick check: `cargo test -p sift` (the `test_cmd` in `aw.toml`).
- `./test.sh --candidate` is the exact-source candidate check. It needs a clean,
  committed tree and pinned oracle binaries. Do not use it as a quick check.
- Do not delete or weaken a test, and do not lower the coverage value in `aw.toml`.

## Evidence

A green unit test or local integration test is not evidence for a live GKE or
production performance claim (`README.md`). Do not move a capability to a better
state in `STATUS.md` without the evidence that file names.

## Public repo

Do not write a secret, a token, a private host name, an e-mail address or a
personal path in this repo.
