# ADR 0001: DDD layout for Sift

- **Status:** Accepted. P1 done on `refactor/ddd-p1`; each P2 item is approved
  on its own.
- **Date:** 2026-09-29
- **Applies to:** everything under `src/`, and the `docs/` layout.
  `external-contracts/` and `ec.lock` are out of scope.

## Context

Every faberlines Rust repo is being brought under one architecture contract,
checked by the workspace repo's `scripts/meta/rust_arch_contract.py` against a
`ddd.toml` at the repo root. Before this change Sift was 46 source files and
30,188 lines, with most of the service in two files: `src/lib.rs` (5,560
lines) and `src/bin/sift.rs` (1,702 lines). The first check, at workspace
`07654e62`, reported 42 errors and 82 warnings, among them eight files above
1000 lines (C2), a `lib.rs` full of items (B5), a thick binary entry (B6), a
root `HA.md` (A4), a missing `docs/` skeleton (A5), and the missing
`mod_module_files` lint (A3).

Sift's public paths are used by its own integration tests and by other repos,
and its persisted bytes (WAL, segments, Raft snapshots, archive manifests,
digests) must not change. A layout change could not move a public path or a
byte.

## Decision

Cut `src/` into the contexts and layers of [architecture.md](../architecture.md)
in two phases, and record every rule break not fixed yet as a `ddd.toml`
exception with a reason. The mapping behind the move numbered its decisions;
the numbers are kept so that commit messages can cite them.

### Phasing

- **Two phases.** P1 moves and splits files only. Public paths, behaviour and
  persisted bytes stay the same. P2 makes the changes the checker requires,
  each approved on its own.
- **400-line target.** P1 splits every `src/` file above 400 lines, the
  checker's warning line. There is no smaller target.
- **One crate.** P1 keeps Sift a single crate. Splitting it into crates needs
  the journal–archive cycle broken first.
- **One commit per context,** in the order shared_kernel, collector,
  projection, query, ingest, journal, archive, node, access, operations, app,
  the binary, docs, and `ddd.toml`. Each commit builds, passes clippy and the
  full test suite (223 passed, 24 ignored), and prints the same
  `sift spec`, OpenAPI document and `--help` text as the one before it.

### Where code goes

- **Ten contexts and an assembly root.** `shared_kernel`, `collector`,
  `node`, `access`, `journal`, `archive`, `projection`, `ingest`,
  `operations` and `query`. The assembly root is `src/app/`, named like
  lumen's; it is not a context and may use every context (E11).
- **N1 — `operations`, not `operator`.** Sift's context covers the operator,
  deployment artifacts, backup and restore, the gateway proxy and the admin
  endpoints; lumen's `operator` is only the Kubernetes operator. Different
  concepts get different words. The Kubernetes operator inside Sift is still
  called the operator.
- **The journal and the archive stay two contexts.** They use each other; P1
  records the archive's side as `depends_on` and the journal's side as
  exceptions, and P2 breaks the cycle with ports.
- **Governance lives in ingest.** `IncomingEvent` and `decode_event_json`
  live in the shared kernel.
- **`access` and `node` stay separate contexts.** Authentication is used by
  ingest, query and operations; the data root by the journal and the archive.
- **E12 — the CLI stays in the binary.** The entry is `src/bin/sift/main.rs`,
  which holds only `mod`, `use`, `fn main` and its test module; each
  subcommand is a module beside it. Moving the CLI into the library would make
  clap library API.
- **N2 — no one-concept-per-file rule.** A file that is already on its own
  and at most 400 lines moves as it is. Code split out of a large file is
  grouped by topic within one layer, up to about 400 lines. Where the mapping
  named a leaf per concept, P1 kept the concepts together.
- **Where the mapping disagreed with itself, §2 (the file-by-file table) won**
  over §1 and §3, and each resulting layer break is an exception.

### What moves as it is

- **Dead and duplicated code** moves unchanged. The list (X1–X10: the dead
  `ingest::batch`, three single-signal checks, five default query limits,
  four UTF-8 truncations, the sha256 helpers, two event-digest forms, three
  framed checkpoint codecs, and others) waits for individual approval in P2.
- **N3 — HANDWRITE markers.** The markers in `src/` are deleted as their
  files move; the six with a long reason become the file's `//!` doc. Markers
  under `tests/`, in non-Rust files, and the `strip_ownership_markers` filter
  in the deployment bundle stay.
- **N4 — the two long functions stay whole (E9).** Retention expiration (534
  lines in its file) and `start_lifecycle_worker` (458) cannot drop under 400
  by moving. They stay C1 warnings until P2 splits each in its own
  extract-function commit. `[[large_files]]` accepts only files above 1000
  lines, so they are not listed there.
- **N5 — test file headers are unchanged.** The checker's isolation markers
  already accept Sift's standalone test binaries.
- **Visibility.** `ServiceState`'s fields became `pub(crate)` so no caller
  changed; the accessors the mapping asked for are P2. New types the mapping
  proposed (a `BlobHash`, an `EventContentDigest` newtype) are new code, not
  moves, and wait for P2.

### Exceptions

Every structure or dependency break is an `[[exceptions]]` entry with exact
paths (no globs) and a reason that says what P2 does. The kinds, by the
mapping's numbers:

| # | Break | Why it is allowed in P1 | P2 |
|---|-------|-------------------------|----|
| E1 | serde and utoipa on persisted and wire types | One serde shape is the WAL, segment, Raft and snapshot bytes, the digest input and the OpenAPI wire; a mirror DTO would triple about 40 types and risk digest drift. | Approve as the published language, pinned with golden-bytes tests. |
| E2–E5 | chrono parsing, sha2 and hex, `serde_json::Value`, regex in domain code | Pure libraries; the hashes are persisted identity. | These crates are on the domain allowlist; no exception needed. |
| E6 | `anyhow` in domain code | Error text reaches HTTP 400 bodies and tests match it by substring. | thiserror per context, same Display text. |
| E7 | `Utc::now` in domain code | Moving is not rewriting. | `*_at(now)` forms and a `Clock` port. |
| E8 | Hot paths and atomic units not split into ports | The append lock scope, archive adoption, restore, the lifecycle order and the dedupe formats are each one unit of work. | Kept whole. |
| E9 | Two functions above 400 lines | See N4. | Extract-function commits. |
| E10 | Public fields on DTOs and configs | Tests build them with struct literals. | Kept for DTOs; new aggregates use private fields. |
| E11 | The assembly root in `src/app/` | `ServiceState`, `router` and `openapi` are public API. | — |
| E12 | CLI modules in the binary | See above. | — |
| E13 | Trait impls that cannot span files | A Rust limit. With a 400-line target every impl fits one file. | — |
| E14 | Core-crate types in domain signatures | `storage_segment::CatalogRoot`, `service_projection` descriptors, `service_observability::StructuredServiceLogV1` are part of the model's types. | Approve, or wrap in a newtype. |
| — | Context cycles (journal ↔ archive and the others) | Moving does not break a cycle. | Ports; required before a crate split. |
| — | Compatibility facades (B1) | Old public paths keep working. | Delete each facade once nothing imports it. |

## Consequences

- The last P1 commit sets `[migration] enforce = true`, so a new rule break
  fails the check unless it is recorded with a reason. The flag is never set
  back to `false`.
- Every public path, CLI flag and persisted byte from before P1 still works.
- The `ddd.toml` exceptions are the P2 backlog: each one names what removes it.
- The two E9 files remain visible as C1 warnings until P2.
- The P2 list, each item approved on its own: thiserror errors (E6), the
  `Clock` port (E7), the two extract-function splits (E9), the journal–archive
  and archive-checkpoint ports, `ServiceState` accessors, removing the
  compatibility facades, renaming `JournalState` to `ResidentWindow`, and the
  X1–X10 deletions and merges.
