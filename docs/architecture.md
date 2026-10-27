# Architecture

Sift has one Rust crate. The crate builds the `sift` library and binary.
The P2 refactor removes all 46 exceptions present at `5ae45b9`.
[`ddd.toml`](../ddd.toml) has no exception entries.
The checker and its `[policy]` rules stay unchanged.

## Context map

A context owns one business area. Its source is under `src/<context>/`.
The shared kernel holds pure data and rules used by several contexts.
The dependency graph has no cycles.

| Context | Depends on | Responsibility |
|---------|------------|----------------|
| `shared_kernel` | None | Events, manifests, segments, epochs, receipts, query data, projection data and shared pure rules. |
| `collector` | None | Read service logs, decode records and deliver events. |
| `node` | None | Data-root ownership, storage roles and disk capacity. |
| `access` | None | Authenticate credentials and authorize project and admin operations. |
| `journal` | `node` | Durable append, dedupe, recovery, reads, snapshots and checkpoint coordination. |
| `archive` | `journal`, `node` | Archive commits, retention, restore, cold reads and safe garbage collection. |
| `projection` | `journal` | Rebuildable log, metric and trace read models. |
| `ingest` | `journal`, `access` | Decode requests, enforce admission and governance, and submit batches. |
| `operations` | `journal`, `archive`, `node`, `access` | Deployment, the operator, proxies, backup and integrity checks. |
| `query` | `projection`, `journal`, `archive`, `access` | Queries, jobs, Prometheus evaluation, forwarding and MCP reads. |

[domain/contexts.md](domain/contexts.md) describes the service boundaries.

## Layers

A port is a trait that separates a use case from a backend.
An adapter implements that trait with a real backend.
Sift defines its storage ports in domain and builds adapters in infrastructure.
Application uses the ports supplied by assembly.

| Layer | May use in its own context | Holds |
|-------|---------------------------|-------|
| domain | Domain only | Pure rules, value types and ports. |
| application | Domain | Use cases and services published to other contexts. |
| infrastructure | Domain | Files, Core libraries, Raft, GCS and transport adapters. |
| interfaces | Application | HTTP, gRPC, MCP and worker entry points. |

A context must declare each other context it uses.
It uses that context through its application layer.
`node` has no application layer, so its public API is available directly.
Domain uses only its own context and the shared kernel.
The checker follows re-exports to the file that defines each item.

## Pure domain

Domain and the shared kernel follow `[policy.domain]` in `ddd.toml`.
They do not read files, the network, the environment or the system clock.
They receive time as data when a rule needs it.
OpenAPI implementations and clock-reading constructors belong to assembly.
The CLI has its own signal enum.

Ports use local or shared data.
Associated error types keep the backend's original error chain.
Async ports return boxed futures, so domain does not need Tokio.
Catalog and instance adapters convert pure Sift data at the backend boundary.

## Assembly and ownership

`src/lib.rs` declares modules and re-exports public items.
`src/app.rs` and `src/app/` build and connect the services.
`src/bin/sift/` owns the CLI and its command modules.

`ServiceState` has private fields.
Handlers obtain application services through methods.
Ingest, journal checkpoint services and lifecycle services share the original
journal, state machine, Raft host, locks and capacity guard.
Projection services share the original registry and projection handles.

Archive reads and archive lifecycle are separate services.
They share `ArchiveServices` and do not own a journal backpointer.
Journal recovery uses injected archive read and maintenance ports.
Archive maintenance borrows the journal for each operation.
This avoids two services owning each other through `Arc`.

Query receives its store fallback from assembly.
It does not depend on operations.
HTTP and gRPC share the access authorization use case.
Kubernetes rendering receives pure `InstanceSpec` data.
The interface layer converts the CRD, which is the Kubernetes resource type.

## Durable order and compatibility

The refactor keeps the original lock scopes and operation order.
Capacity reservation, WAL writes, fsync, state changes and replies retain their
positions in each use case.
Governance runs before a Raft proposal and during direct journal append.
Archive objects and catalogs are uploaded before the final manifest commit.

Event content hashing and XOR have one shared implementation.
Dedupe hashing keeps its project field and format prefixes.
The three checkpoint codecs share framing and keep their own error text.
Barrier encoding and submission share one journal application use case.
The two lifecycle callers keep their own flags and failure handling.

Replicated archive garbage collection needs an installed checkpoint on every
voter, which is a node that votes in Raft.
A quorum checkpoint without GC does not grant that permission.
Lost leadership or an unfinished retention scan also blocks archive GC.

CLI, HTTP, gRPC, OpenAPI, errors and persisted formats stay compatible.
No data migration is required.
[public-api.md](public-api.md) describes Rust API changes and byte checks.

## Checking

Each local commit runs these checks in Sift:

```sh
cargo build --locked
cargo clippy --locked --all-targets
cargo test --locked --no-fail-fast
```

Run the architecture check from the workspace repository:

```sh
scripts/meta/rust_arch_contract.py check --repo sift --strict --base main
```

The completion check has zero architecture errors and warnings.
There are no exceptions to suppress a finding.
The audit also checks missing paths and unused exceptions.
The unchanged policy warns above 400 source lines and fails above 1000.
No measured source file is above 400 lines.

[ADR 0001](adr/0001-ddd-layout.md) records the original layout.
[ADR 0002](adr/0002-ddd-p2.md) records the P2 batches.
[The completion report](refactor-completion.md) lists local commits, test counts
and the existing test and document limits.
