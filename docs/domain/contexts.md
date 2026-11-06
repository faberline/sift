# Contexts

Each context owns one business area.
[The architecture](../architecture.md) defines the layer rules.
[`ddd.toml`](../../ddd.toml) defines the checked dependency graph.
All contexts have zero architecture exceptions.

## Shared kernel

The shared kernel has no context dependencies.
It holds pure data used across module boundaries.
This includes events, stored events, append results, manifests, catalog roots,
segments, epochs, dedupe receipts, watermarks and retention fences.
It also holds query requests, responses, jobs and projection read data.
Layout manifests, checkpoint results and forwarding frames are pure data too.

Shared functions handle event content digests, XOR, SHA-256 text, retention
boundaries, limits and UTF-8 truncation.
Time is passed into domain rules.
Clock-reading event constructors and OpenAPI implementations live in assembly.

## Collector

Collector reads file, stdin and CRI logs and delivers operational events.
It depends only on the shared kernel.
CRI is the container log format used by Kubernetes nodes.

Domain owns configuration, record data, CRI rules, borrowed service-log
validation, quarantine data and the record decoder port.
Application receives an already built source, decoder and sink.
The Core collector runtime keeps its bounded delivery and checkpoint loop.
Infrastructure owns source I/O, checkpoint files, OTLP delivery and decoding.
Assembly selects and creates the source and sink.

## Node

Node owns the process data root, storage roles and local disk capacity.
It has no context dependencies and no application layer.
Its public domain and infrastructure API is available to declared dependents.
The layout manifest is shared pure data.
The native data-layout implementation keeps its lease and private file modes.

## Access

Access authenticates callers and checks project and admin permissions.
It has no context dependencies.
Domain owns pure route roles and the verifier port.
Application owns the shared authorization use cases.
Infrastructure supplies static role maps and Kubernetes token review.
HTTP middleware and the gRPC adapter use the same authorization service.
Transport error conversion remains at the inbound boundary.

## Journal

Journal owns durable events, dedupe, recovery, snapshots and checkpoint control.
Its only context dependency is node.
Domain owns append rules, resident state, digest rules and storage ports.
Ports cover raw events, WAL, dedupe, recovery, archive reads, archive
maintenance, snapshots, staging, restore targets and command submission.
Governance is supplied through a journal-owned port.

Application owns `JournalRuntime`, the `DurableJournal` facade, raw event
services, batch sizing, reads, recovery and checkpoint services.
Infrastructure supplies the original WAL, segment, blob and dedupe stores.
It also owns Raft commands, snapshots, checkpoint framing and the state machine.
The interface layer exposes peer serving and metrics through application.
Assembly supplies archive bridges without a journal-owned archive service.

## Archive

Archive owns cold storage, restore, retention and archive garbage collection.
It depends on journal and node.
Domain owns archive validation, control records, catalog rows and settings.
Its ports cover commits, upload intents, catalogs, spills, object transactions,
manifest reads, codecs, restore, file modes and GC storage.
GC means deleting objects that the archive no longer needs.

Application receives `ArchiveServices` from assembly.
It owns archive commits, retention, recovery, cold reads and lifecycle work.
Infrastructure implements the ports with the original Core and file backends.
The worker receives a complete lifecycle context before it starts.

`ArchiveReadService` and `ArchiveLifecycleService` share the same services.
Neither owns a journal backpointer.
Lifecycle operations borrow journal application services for each call.
Replicated GC waits for an installed checkpoint on every voter.
The quorum-only fallback preserves old archive objects.

## Projection

Projection owns rebuildable log, metric and trace read models.
It depends on journal.
Domain owns the pure `Projection` trait, normalizers, metadata and runtime ports.
The trait uses an associated error type.
Application provides projection reads, catch-up, waiting and rebuild use cases.
Infrastructure adapts the original Core registry and projection instances.
The projection worker calls application services.
Projection reads and snapshot bytes retain their existing formats.

## Ingest

Ingest owns request decoding, admission, governance and batch submission.
It depends on journal and access.
Domain owns limits, admission failures, governance and submission ports.
Application owns ingest services and their command and capacity use cases.
Infrastructure implements the queue and capacity ports.
Interfaces decode and reply to HTTP, gRPC and Prometheus remote-write requests.

Assembly shares the original command lock, state machine and capacity guard.
The coordinator starts lazily and retains the original drain order.
Governance checks remain before Raft submission and direct journal writes.

## Operations

Operations owns deployment, the operator, proxies, backup and integrity checks.
It depends on journal, archive, node and access.
Domain owns names, image rules, route classification and the renderer port.
The renderer takes pure `InstanceSpec` data.
Application owns instance rendering, gateway policy and backup use cases.
Infrastructure renders Kubernetes objects and accesses live endpoints.
Interfaces own the CRD, operator, conversions and transport handlers.
Operations does not depend on the ingest interface layer.

## Query

Query owns execution, jobs, cold replay, Prometheus queries and MCP reads.
It depends on projection, journal, archive and access.
Domain owns query rules, pure Prometheus models and API, job and forwarding ports.
Application owns validation, cursor encoding, execution and job running.
It calls projection and archive application services for reads.
Infrastructure supplies HTTP clients, job files and the forwarding adapter.
Interfaces render HTTP and MCP results.

Assembly creates the fallback router and injects it into query.
Query does not depend on operations.
Job JSON, cursor text, retry handling and forwarded body errors stay unchanged.

[public-api.md](../public-api.md) describes public Rust paths and compatibility.
