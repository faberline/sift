# Architecture

Sift is one Rust crate, the `sift` library plus the `sift` binary. Its source is
cut into bounded contexts, and each context into layers. This page says what
the contexts are, what may depend on what, and which rule breaks are allowed
and why. The machine-checked form of everything here is
[`ddd.toml`](../ddd.toml); the decisions behind it are in
[ADR 0001](adr/0001-ddd-layout.md).

> **Migration status.** The layout below was applied in phase P1 on the
> `refactor/ddd-p1` branch, which moved and split files without changing
> behaviour, public paths or persisted bytes. Phase P2 removes the exceptions
> that have a planned fix. See [Phases](#phases).

## Context map

A context lives in `src/<context>/`, with its module file at
`src/<context>.rs`. The shared kernel is usable by every context without
declaring it. The graph of `depends_on` has no cycles; the uses that would
close one are recorded as exceptions (see [Exceptions](#exceptions)).

| Context | Layers | Depends on | What it owns |
|---------|--------|------------|--------------|
| `shared_kernel` | kernel | — | The published language: the versioned operational event, `IncomingEvent` and `decode_event_json`, `StoredEvent`, the event-content digest, the archive watermarks, the 180-day retention boundary and the Cloud Logging event id. |
| `collector` | domain, application, infrastructure | — | The agent that reads service logs from a file, stdin or a node's CRI log root and delivers them to Sift over OTLP. |
| `node` | domain, infrastructure | — | One process's data root: the versioned layout and its manifest, the role the process runs as (`StorageRole`), and the local disk-capacity guard. |
| `access` | domain, infrastructure, interfaces | — | Who may call Sift, and for which project: the auth configuration, the verifier that checks a bearer token against a static role map or the Kubernetes API server, the scoped bearer middleware, and the project and admin checks. |
| `journal` | all four | node | The durable, replicated event log: Raft, the WAL, sealed segments, the idempotency window, blobs, recovery, archive-checkpoint adoption and the reads the projections and queries run. |
| `archive` | all four | journal, node | The cold tier: upload to GCS or a local archive under a verified manifest, hot-tier eviction, retention expiration, blob GC, restore of an empty node, and cold-query replay. |
| `projection` | all four | journal | The rebuildable logging, metric and trace read models built from the journal. |
| `ingest` | all four | journal, access | Admission within bounded limits, OTLP, Cloud Logging and Prometheus remote-write decoding, governance, and the group-commit append to the journal. |
| `operations` | all four | journal, archive, node, access, ingest | Running Sift as a service: the `Sift` custom resource and its operator, the deployment artifacts, snapshot backup and restore, the gateway's reverse proxy, the OTLP/gRPC proxy, and the admin backup and integrity endpoints. |
| `query` | all four | projection, journal, archive, access, operations | The versioned query API over logs, metrics and traces, async query jobs, the Prometheus-compatible endpoints, the query role, and the read-only MCP tools. |

[domain/contexts.md](domain/contexts.md) describes each context by layer.
`operations` is not lumen's `operator`; the [glossary](glossary.md#names-that-differ-from-other-repos)
says why.

## Assembly code

Some code wires the contexts together and belongs to no context. The layer
and context rules do not apply to it.

| Where | What it holds |
|-------|---------------|
| `src/lib.rs` | `mod` and `pub mod` declarations and `pub use` re-exports, nothing else. |
| `src/app/` | The assembly root, declared under `[assembly] modules`: `ServiceState`, the readiness and metrics hooks `service_http` reads, the HTTP router, `ApiError`, and the OpenAPI document. |
| `src/bin/sift/` | The binary. `main.rs` holds only `mod`, `use`, `fn main` and its test module; each subcommand lives in its own module beside it, so clap never becomes library API. |
| compatibility facades | The old public module paths; see [Public API](#public-api-and-compatibility-facades). |

## Layers and dependency direction

Inside one context, a layer may use itself and the layers below it:

| Layer | May use |
|-------|---------|
| domain | nothing else in the context |
| application | domain |
| infrastructure | domain |
| interfaces | application |

- **domain** holds the model: value objects, state machines, validation, and
  the pure rules the rest of the context applies.
- **application** holds the use cases, and the types other contexts consume.
- **infrastructure** talks to real technology: files, Raft, codecs, GCS,
  Kubernetes, HTTP clients.
- **interfaces** is the inbound edge: HTTP and gRPC handlers, the MCP tools,
  the workers and the metrics a context exports.

Across contexts:

- A context may use another only if its `depends_on` lists it.
- A domain layer uses no other context. The shared kernel is the only outside
  Sift code it may name.
- Another context may use a context that has an application layer only through
  that layer. The application layer is the context's published language.
- `node` and `access` have no application layer, so their whole public API is
  open to the contexts that depend on them.

The checker resolves `pub use` chains to the file that defines an item, so a
re-export does not change where a type counts as living.

Visibility follows the same lines: an item used across layers of one context is
`pub(in crate::<context>)`, one used only inside its layer is `pub(super)`, and
one used by another context is `pub(crate)`. Only the public API is `pub`.

## Domain purity

Domain layers and the shared kernel are held to the `[policy.domain]` rules in
`ddd.toml`: a short list of allowed outside crates (`serde`, `serde_json`,
`chrono`, `sha2`, `hex`, `regex` and a few more), no file system, network,
process, environment or standard-stream access, no clock reads, and no print
macros. The policy table is copied from the workspace's canonical
`policy.toml` and is not edited here.

Sift's domain layers break these rules in known places, each recorded as an
exception: `anyhow` errors whose text reaches HTTP 400 bodies (E6), `Utc::now`
in the event constructors, query time and the Raft command (E7), `utoipa`
schemas on the published event and the projection records (E1), and
core-crate types that are part of a domain signature (E14). ADR 0001 lists
them.

## Public API and compatibility facades

Tests and other repos import Sift by module paths such as
`sift::storage::archive::…`, `sift::durability::…` or `sift::auth::SiftAuthConfig`.
P1 keeps every one of them:

- Every `pub use` at the crate root stays: `ServiceState`, `DurableJournal`,
  `router`, `openapi` and the others listed in `src/lib.rs`.
- Every old `pub mod` that is not itself a context became a **compatibility
  facade**: a file that holds only `pub use` lines pointing at the item's new
  home. They are `src/{api,auth,backup,deploy,durability,event,grpc,mcp,operator,prometheus,proxy,storage}.rs`,
  `src/storage/archive.rs`, and `src/ingest/{batch,gcp,limits,otlp}.rs`.
- `collector`, `ingest` and `projection` kept their names as contexts, so their
  module files serve as their own facades.
- The binary's name, subcommands, flags, environment variables and output are
  unchanged.

The facades do not follow the layer naming, so each has a B1 exception. P2
deletes a facade once nothing imports it by its old path.

## File size

The checker measures non-test source files under `src/`: above 400 lines is a
warning (C1), above 1000 lines is an error (C2). P1 split every file above 400
lines except two, each holding one function that a move cannot shorten (E9):

| File | Lines | Function |
|------|-------|----------|
| `src/archive/application/expire_committed_events.rs` | 534 | retention expiration |
| `src/archive/interfaces/lifecycle_worker.rs` | 458 | `start_lifecycle_worker` |

They stay C1 warnings, which cannot be suppressed, until P2 splits each
function in its own extract-function commit. Integration tests under `tests/`
are not measured.

## Exceptions

`ddd.toml` lists every structure or dependency break (rules B1–B6) as an
`[[exceptions]]` entry: the rule, the subject, the exact files, and a reason.
Each reason says what P2 does about it; a B3 reason names the types involved.

| Category | Rule | Examples | Fate |
|----------|------|----------|------|
| Compatibility facade | B1 | the old top-level modules and `ingest::{batch,gcp,limits,otlp}` | P2 deletes the facade. |
| Domain uses a non-allowlisted crate | B2 | `anyhow` (E6), `utoipa` on the published event, the projection records and `AppendResult` (E1), `clap::ValueEnum` on `SignalKind`, core types such as `storage_segment::CatalogRoot` (E14), `axum` and `service_auth` in access's route roles | P2 converts to thiserror with the same text, or approves the crate as part of the model. |
| Domain reads the clock | B2 | `Utc::now` in the event constructors, `query_time.rs` and `sift_command.rs` (E7) | P2 adds `*_at(now)` forms and a `Clock` port. |
| Same-context layer direction | B3 | the collector runtime building its source and client, the operator's renderers reading the CRD types | P2 moves the wiring to the assembly root or splits the type. |
| Cross-context use outside the published language | B4 | archive and projections holding `DurableJournal`, the journal validating an `ArchiveManifest`, ingest sizing batches by the journal's Raft limits | P2 publishes the use through the owner's application layer or a port. The journal–archive pair needs a port before Sift can split into crates. |

## Checking

Run the checker from the workspace repo:

```sh
scripts/meta/rust_arch_contract.py check --repo sift --strict
```

`--strict` fails on any error that no exception suppresses; warnings, such as
the two C1 files above, are reported but do not fail. With
`[migration] enforce = true` in `ddd.toml`, the check fails the same way
without `--strict`.
`Cargo.toml` denies `clippy::mod_module_files`, so module files are
`foo.rs` beside `foo/`, never `foo/mod.rs`.

## Phases

- **P1 — move, do not change.** Files move into the layout above and files
  above 400 lines are split. Public paths, behaviour and persisted formats are
  unchanged. Dead and duplicated code moves as it is. Every rule break is an
  exception with a reason.
- **P2 — fix what the checker requires.** Each item is approved on its own:
  thiserror errors (E6), the `Clock` port (E7), the two extract-function splits
  (E9), the journal–archive ports, removing the compatibility facades,
  `ServiceState` accessors in place of its `pub(crate)` fields, and the dead
  and duplicate code list (X1–X10).
