# sift

Sift is one Rust binary for logs, metrics and traces. Read `README.md` and `STATUS.md` first.

## Output style

- Write in the user's language. Keep paths, commands and exact output intact.
- Use short, active sentences. Use one idea in each sentence.
- Explain a technical term the first time you use it.
- Report what changed, what passed, what is unknown and the next action.
- Offer at most two choices, and recommend one.

## Scope

- The product is one Rust binary for logs, metrics and traces.
- A green unit test or local integration test is not evidence for a live GKE or production performance claim (`README.md`).
- Do not move a capability to a better state in `STATUS.md` without the evidence that file names.
- This repository is public. Do not write a secret, a token, a private host name, an e-mail address or a personal path in it.

## Issue work

- Work on a claimed issue only through the `aw` CLI: `aw claim`, the work, then `aw report`. `aw` does the branch, push, pull request, merge and signal comment.
- Edit labels only in the plan step of the `aw-worker` skill.

## Verify

- Quick check: `cargo test -p sift` (the `test_cmd` in `aw.toml`).
- CI runs these commands. Run them before you report ready:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo test --locked -p sift`
- `./test.sh --candidate` is the exact-source candidate check. It needs a clean, committed tree and pinned oracle binaries. Do not use it as a quick check.
- Do not delete or weaken a test, and do not lower the coverage value in `aw.toml`.
