# P2 completion report

- Date: 2026-10-01.
- Refactor branch: `refactor/ddd-p2`.
- Starting commit: `5ae45b9079538af8f70a5bfd4f3c0948adff83a6`.
- Validated implementation commit: `370adf3ca3655b4a1122d835e18268520d96ecc0`.
- Delivery: local commits.

## Result

All 46 starting architecture exceptions are removed.
The policy and checker are unchanged.
Strict architecture and exception audit report zero errors and warnings.
X7, X8 and X9 each have a separate verified commit.
The crate remains one crate and needs no data migration.

The full suite has 271 passing cases and 24 original ignored cases.
All 225 original passing cases remain.
There are 46 new cases, listed separately in the gate's `test-diff.log`.
Build has no warnings.
Clippy has nine existing finding groups and no new findings.
The unchanged query-enum finding follows its byte-identical type to the kernel.
Raw diagnostics and the checked relocation are kept in the evidence.

## Verified source batches

Each commit ran a locked build, Clippy for all targets, the full test suite,
strict architecture and the exception audit before the next batch.
Each document check kept the same twelve existing findings.
The first row uses `baseline/golden-test.log` and `baseline/golden-clippy.jsonl`.
Later rows use `test.log`, `test-diff.log`, `build.log`, `clippy.jsonl`,
`clippy-diff.log`, `strict.json`, `audit.log` and `docs.txt` in the named directory.

| Batch | Change | Commit | Passed | New cases | Exceptions | Evidence directory |
|-------|--------|--------|--------|-----------|------------|--------------------|
| 1 | Fixed compatibility bytes | `255586f` | 234 | 9 | 46 | `baseline` |
| 2 | Pure domain dependencies | `9e220a6` | 236 | 2 | 35 | `phase2` |
| 3 | Collector, access and operations | `1199643` | 239 | 3 | 29 | `phase3` |
| 4 | Published journal and archive data | `ba1fabb` | 239 | 0 | 24 | `phase4-values` |
| 5 | Command and batch submission | `38a5d30` | 243 | 4 | 19 | `phase5-ingress-verified` |
| 6 | Journal storage and recovery | `552d055` | 247 | 4 | 13 | `phase6-journal-storage-final` |
| 7 | Projection and cold replay | `467d82f` | 248 | 1 | 10 | `phase7-projection` |
| 8 | Query data and HTTP use cases | `77ac560` | 250 | 2 | 4 | `phase8-query-contracts-final` |
| 9 | Query jobs and forwarding | `7733005` | 253 | 3 | 2 | `phase9-query-ports` |
| 10 | Lifecycle checkpoint ports | `a275aa7` | 253 | 0 | 2 | `phase10-checkpoint-ports` |
| 11 | Archive control and codec ports | `363210c` | 255 | 2 | 2 | `phase11-archive-controls-final` |
| 12 | Archive catalogs and transactions | `6caacee` | 258 | 3 | 1 | `phase12-archive-resources-final` |
| 13 | Restore target ports | `208ddc1` | 260 | 2 | 0 | `phase13-restore-target` |
| 14 | Private state and archive ownership | `70849d2` | 262 | 2 | 0 | `phase14-service-facades` |
| 15 | X7 event digests and XOR | `f5aa1e4` | 266 | 4 | 0 | `phase15-x7-digests` |
| 16 | X8 checkpoint framing | `c02e7a6` | 266 | 0 | 0 | `phase16-x8-framing-final` |
| 17 | X9 barrier submission | `370adf3` | 271 | 5 | 0 | `phase17-x9-barriers` |

Local evidence is under `/private/tmp/sift-refactor-completion-20261001`.
The final documentation batch passed the same checks in `phase18-documentation`.
`final-receipt.json` records the refactor branch HEAD and results after that commit.

## Local main landing

Local `main` moved from `61ba693` to the verified `cff6f07` by fast-forward.
This kept the complete commit history.
The first landing check found four completed rename entries in `ddd.toml`.
The entries were needed against the earlier `main` and are now removed.
The checker and policy stay unchanged.
Rust source and all compatibility fixture bytes stay unchanged.

The follow-up batch passed the same full checks in `phase19-main-landing`.
Strict architecture and the exception audit report zero errors and warnings.
The full suite still has 271 passing cases and the same 24 ignored cases.
`landing-receipt.json` records the local `main` HEAD after the follow-up commit.

## Compatibility evidence

Thirteen fixture hashes remain equal to the baseline.
Fixtures pin three event kinds, a manifest, a query job, three projection
snapshots, three checkpoint formats, OpenAPI and 34 CLI help pages.
Fixed times and identifiers keep the bytes stable.
Old fixture data is read and encoded again to the same bytes.

New port tests check authorization, codes, retry hints, cursor text, recovery,
shared backend ownership, capacity and proposal failures, drain order and error
chains. Original restart, dedupe, batch, storage failure and archive tests remain.
Checkpoint tests cover magic, length, CRC32, truncation and trailing bytes.
They also check the 64 KiB boundary, failure order and writer and reader sources.
Barrier tests cover encoding and proposal failures, missing identity, behind
voters, uninstalled checkpoints and the no-GC quorum fallback.
The original lifecycle logs and GC permission expression remain unchanged.

## Original ignored cases

The 24 cases keep their existing reasons.
They are not counted as passing cases.

| Group | Cases | Original reason |
|-------|-------|-----------------|
| `gke_operation_scope` | 19 | `needs FABERLINE_WORKSPACE: a faberline/workspace checkout` |
| `operator_reconcile_oracle` | 3 | `needs FABERLINE_WORKSPACE: a faberline/workspace checkout` |
| `vat_lumen_observability_e2e` | 1 | `needs vat and lumen installed on PATH` |
| `vat_service_observability_e2e` | 1 | `needs vat, lumen, tape, relay and defer installed on PATH` |

## Original document findings

The product document contract still reports the same twelve findings.
No new finding is added.
These findings are separate from the architecture check.

| Rule | Cases | Existing finding |
|------|-------|------------------|
| README R7 | 8 | Source bullets at lines 300, 313, 324, 336, 350, 367, 378 and 390 do not name exactly one supported source. |
| SET P1 | 2 | Root `ROADMAP.md` and `STATUS.md` are absent. |
| SET P2 | 2 | README Supporting documents does not link those two root files. |

The root README is unchanged.
Its hash is `1da92df874803e4e8bf573373e9cbd36c1d5cb3e0b04bf36af191366cc1a15ad`.
Architecture, context, public API and ADR pages now describe the completed design.

[Architecture](architecture.md), [public API](public-api.md) and
[ADR 0002](adr/0002-ddd-p2.md) describe the source and compatibility rules.
