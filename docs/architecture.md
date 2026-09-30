# Architecture

Sift is one Rust crate, the `sift` library plus the `sift` binary. Its source is
cut into bounded contexts, and each context into layers. This page says what
the contexts are, what may depend on what, and which rule breaks are allowed
and why. The machine-checked form of everything here is
[`ddd.toml`](../ddd.toml); the decisions behind it are in
[ADR 0001](adr/0001-ddd-layout.md) and [ADR 0002](adr/0002-ddd-p2.md).

> **Migration status.** The layout below was applied in phase P1 on the
> `refactor/ddd-p1` branch, which moved and split files without changing
> behaviour, public paths or persisted bytes. Phase P2 removes the exceptions
> that have a planned fix; [ADR 0002](adr/0002-ddd-p2.md) records its first
> batch. See [Phases](#phases).

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
| `ingest` | all four | journal, access | Admission within bounded limits, OTLP and Prometheus remote-write decoding, governance, and the group-commit append to the journal. |
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
exception: `Utc::now` in the event constructors, query time and the Raft
command (E7), `utoipa` schemas on the published event and the projection
records (E1), and core-crate types that are part of a domain signature (E14).
ADR 0001 lists them. Domain errors are thiserror types whose Display text is
the text an HTTP 400 body or a log line shows; [ADR 0002](adr/0002-ddd-p2.md)
records the rules.

## Public API

Sift's module paths are used by its own binary and integration tests. No
other repo imports Sift by a module path. P1 kept every path from before the
split working through compatibility facades. A facade was a file that only
re-exported items from their new home. P2 deleted the facades and moved
their callers.

The rules now:

- **The crate root.** It re-exports the assembly and the event model:
  `ServiceState`, `DurableJournal`, `router`, `openapi`, `OperationalEventV2`
  and the others listed in `src/lib.rs`.
- **Contexts.** Every context is a `pub mod`. Its module file,
  `src/<context>.rs`, re-exports the context's public items, so a caller
  writes `sift::<context>::Item`.
  - The layers stay `pub(crate)`, so `sift::<context>::<layer>::…` is not a
    public path.
  - `shared_kernel` stays private. The crate root re-exports its public
    items.
- **Prefixed names.** A name that would not say what it is at the context
  root gets a prefix when it is re-exported, such as `run_operator` or
  `decode_otlp`.
- **Protobuf types.** The OTLP and remote-write protobuf types are not
  re-exported. Callers take them from `transport_otlp::proto` and
  `metrics_remote_write::proto`.
- **Imports inside the crate.** Code inside the crate imports an item from
  the module that defines it, not from a context root.
- **The binary.** Its name, subcommands, flags, environment variables and
  output did not change.

What replaced each facade path:

| Old path | New path |
|----------|----------|
| `sift::auth::*` | `sift::access::*` |
| `sift::api::*` | `sift::query::*` |
| `sift::mcp::SiftApiClient` | `sift::query::SiftApiClient` |
| `sift::mcp::{http_router, serve_stdio, tool_names}` | `sift::query::{mcp_http_router, serve_mcp_stdio, mcp_tool_names}` |
| `sift::prometheus::{parse_promql, ParsedPromQuery, PromFunction, nanos_rfc3339, parse_prom_duration_nanos, parse_prom_time_nanos, InstantQueryParams, RangeQueryParams}` | `sift::query::*` |
| `sift::prometheus::{decode_remote_write, DecodedWrite}` | `sift::ingest::*` |
| `sift::prometheus::{remote, PROMETHEUS_STALE_NAN_BITS}` | `metrics_remote_write::{proto, PROMETHEUS_STALE_NAN_BITS}` |
| `sift::{backup, deploy, proxy}::*` and `sift::operator::*` | `sift::operations::*` |
| `sift::operator::run` | `sift::operations::run_operator` |
| `sift::grpc::serve_proxy` | `sift::operations::serve_otlp_grpc_proxy` |
| `sift::grpc::serve` | `sift::ingest::serve_otlp_grpc` |
| `sift::durability::*`, and the journal items of `sift::storage` | `sift::journal::*` |
| The node items of `sift::storage`: `StorageRole`, `DEFAULT_DATA_DIR`, `DataLayout`, `LayoutManifest` and the capacity types | `sift::node::*` |
| `sift::storage::archive::*` | `sift::archive::*` |
| `sift::event::*` | `sift::*` |
| `sift::ingest::limits::*` | `sift::ingest::*` |
| `sift::ingest::otlp::{decode, encode_response, normalize_payload}` | `sift::ingest::{decode_otlp, encode_otlp_response, normalize_otlp_payload}` |
| `sift::ingest::otlp::{DecodedOtlp, EncodedOtlpResponse, OtlpItemError, OtlpMediaType, OtlpSignal}` | `sift::ingest::*` |
| `sift::ingest::otlp::wire` | `transport_otlp::proto` |

## File size

The checker measures non-test source files under `src/`: above 400 lines is a
warning (C1), above 1000 lines is an error (C2). No file is above 400 lines.
P1 left two, each holding one function that a move could not shorten (E9).
P2 split both by extracting functions: retention expiration (534 lines) into
`retention_expiration.rs`, `retention_pass.rs` and
`rewrite_retained_segment.rs`, and the lifecycle worker (458 lines) into
`lifecycle_tick.rs` and `lifecycle_checkpoint.rs`, with its loop in
`archive_worker.rs`. Integration tests under `tests/` are not measured.

## Exceptions

`ddd.toml` lists every structure or dependency break (rules B1–B6) as an
`[[exceptions]]` entry: the rule, the subject, the exact files, and a reason.
Each reason says what P2 does about it; a B3 reason names the types involved.

| Category | Rule | Examples | Fate |
|----------|------|----------|------|
| Domain uses a non-allowlisted crate | B2 | `utoipa` on the published event, the projection records and `AppendResult` (E1), `clap::ValueEnum` on `SignalKind`, core types such as `storage_segment::CatalogRoot` (E14), `axum` and `service_auth` in access's route roles | P2 approves the crate as part of the model, or moves its use out of the domain. |
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
  and duplicate code list (X1–X10). [ADR 0002](adr/0002-ddd-p2.md) records
  the first batch.
