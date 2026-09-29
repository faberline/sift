# Glossary

The terms Sift's code and docs use. General storage and service terms (WAL,
segment, manifest, snapshot, checkpoint, projection, watermark, compaction,
retention, cursor, drain, role, port) mean what they mean in core's glossary;
this page gives Sift's specific sense where it has one. Nothing here renames
code.

## Architecture

| Term | Meaning |
|------|---------|
| **context** | A bounded context: one directory `src/<context>/` with one model and one vocabulary. See [architecture](architecture.md#context-map). |
| **layer** | One of `domain`, `application`, `infrastructure`, `interfaces` inside a context. |
| **published language** | The types and functions a context offers to other contexts: its application layer, or its whole public API when it has none. |
| **shared kernel** | `src/shared_kernel/`: the event model and the digests and ids every context shares, usable without declaring a dependency. |
| **assembly** | Code that wires contexts together and belongs to none: `src/lib.rs`, `src/app/` and `src/bin/sift/`. |
| **compat facade** | A module that only re-exports items from their new home, so a public path from before the DDD split keeps compiling. |
| **exception** | A recorded rule break in `ddd.toml`, with the exact files and a reason that says what P2 does. |
| **P1 / P2** | The two phases of the DDD split: P1 moves and splits files without changing behaviour; P2 makes each approved change. See [ADR 0001](adr/0001-ddd-layout.md). |

## Names that differ from other repos

| Term | In Sift | Elsewhere |
|------|---------|-----------|
| **operations** | The context for running Sift as a service: the Kubernetes operator, the deployment artifacts, backup and restore, the gateway proxy, the integrity report and the admin endpoints. The Kubernetes operator inside it is still called the operator. | lumen's `operator` context is only the Kubernetes operator (CRD, reconcile, render). The two are different concepts, so they have different names. |
| **app** | The assembly root, not a context. | The same in lumen. |

## Events

| Term | Meaning |
|------|---------|
| **signal** | One of the three telemetry kinds: log, span or metric (`SignalKind`). A durable batch holds events of exactly one signal. |
| **OperationalEventV2** | The versioned event every signal is ingested, journaled, archived and queried as (`EVENT_SCHEMA_VERSION = 2`). **EventEnvelope** is an alias for it, not a separate type. |
| **StoredEvent** | An event as the journal stores it: the cursor it was appended at, when Sift accepted it, and the event. |
| **acknowledged_at** | When Sift accepted the event: the leader-chosen Raft decision time. The idempotency window is measured from it. |
| **occurred_at** | The event's own time. The 180-day ingest rejection, hot eviction and metric point times use it. |
| **event content digest** | `xor-sha256-v1`: the XOR of each event's SHA-256. It is order-free, so a prefix digest and a suffix digest combine without rereading either. |
| **temporality** | How a metric value accumulates: delta, cumulative or gauge. |
| **exemplar** | A sample on a metric point linked to a trace id and span id; correlate uses it to connect metrics to traces. |
| **stale point** | A metric point Prometheus marked stale with its reserved NaN. Sift stores it as an explicit `stale` flag with value 0.0, so the JSON holds no non-finite number. |

## Journal

| Term | Meaning |
|------|---------|
| **canonical WAL** | The per-signal framed write-ahead log, fsynced before acknowledgement. It is the only source of truth; every index can be rebuilt from it. |
| **raw cursor** | The journal's monotonic append position. A checkpoint pairs it with the Raft applied index; both describe the same durable prefix. |
| **journal head** | The persisted journal identity (last cursor, retained event count, projection generation, retention generation). It survives hot-segment eviction. |
| **resident window** | The in-memory tail of recent stored events the journal keeps, 100,000 by default. The code calls it `JournalState`; P2 renames it. |
| **epoch map / virtual bucket** | An event id hashes to one of 4,096 virtual buckets (`VIRTUAL_BUCKETS`), and an epoch map assigns buckets to shards from an activation cursor on. |
| **idempotency window** | Six hours (`IDEMPOTENCY_WINDOW_SECONDS`) of hourly acknowledgement generations, during which a repeated event id is a duplicate. |
| **dedupe receipt** | What the journal keeps for each acknowledged event (project, event id, cursor, acknowledgement time) to detect duplicates. |
| **checkpoint identity** | The journal's prefix event count, content digest and retention generation at a given raw cursor. |
| **retained prefix** | The archived segment prefix a node keeps locally up to the snapshot index. Every voter replaces it with the exact, hash-verified rows before Raft compaction. |
| **snapshot index** | The raw cursor an archive manifest covers, which must equal the checkpoint's raw cursor; rows after it are the Raft suffix. |
| **all-voter / quorum compaction** | All-voter compaction snapshots and compacts the resident journal on every voter; quorum compaction needs only a quorum and skips archive GC. |

## Archive and retention

| Term | Meaning |
|------|---------|
| **hot window** | 30 days. A local segment whose events are all older is evicted once it is archived. |
| **expiration** | 180 days. Older events are removed from the committed archive and rejected at ingest. |
| **retention fence** | A replicated command naming the source manifest (URI and SHA-256), the target retention generation and the evaluation time; every voter applies it before retention continues (`RetentionFenceV1`). |
| **retention generation** | A monotonic number bumped at each retention-scan commit, so a replica applies one small delta instead of downloading the whole archive. |
| **retention scan** | A durable cursor over the archive catalog that keeps one cutoff until every segment is visited, 64 segments per batch. |
| **archive checkpoint barrier** | A Raft command with no events that carries the retention generation and the manifest's URI and SHA-256. It is proposed before a checkpoint when archive GC is pending, so its index becomes the captured applied index. |
| **lifecycle worker** | The loop that runs only on the leader and archives, evicts, expires and garbage-collects on each tick. |
| **upload intent** | A record written before an archive upload starts, so an interrupted upload can be recovered. |
| **spill catalog** | A catalog spilled to disk, where the archive builds GC plans and blob references. |
| **portable manifest** | A segment manifest rewritten to a node-independent path (`segments/{signal}/{id}.framed`). |
| **restore stage** | A resumable staging directory inside the target volume; the live data-root names are published only after every remote object and digest is verified. |
| **bootstrap archive** | A one-time restore of a fresh volume from a GCS archive manifest (`SIFT_BOOTSTRAP_ARCHIVE_MANIFEST_URI`); later restarts reuse the completed restore. |

## Projections

| Term | Meaning |
|------|---------|
| **projection generation** | A counter bumped when the retained source changes (archive adoption with retention, expiration, restore, recovery); a new generation makes the projections rebuild. |
| **semantic digest** | A hash of a projection's state. A restored or rebuilt projection must have the same one as the projection it came from. |
| **series identity** | The SHA-256 of a metric series' project, environment, name, unit, temporality, resource, attributes and overflow flag: the 64-hex `series_id`. |
| **cardinality overflow** | Once a project has 10,000 series identities, points for a new identity go to an overflow series without resource or attributes. |
| **memtable** | The in-memory budget for resident metric data (256 MiB by default), enforced by sealing chunks to disk. |
| **resident / sealed chunk** | A resident chunk holds metric points in memory; at 256 points it is sealed into a SHA-256-checked file in the chunk store. |
| **obsolete ledger** | A framed log of chunk keys that are deleted once a checkpoint commits. |
| **rollup** | A per-window metric summary (count, sum, min, max, last, histogram, exemplars) over 60-second and one-hour windows, without stale points. |
| **trace gap** | A missing parent or conflicting span found while assembling a trace; it marks the trace partial. |
| **trace cycle** | A loop in span parent links; it is reported and marks the trace partial. |
| **critical path** | The root-to-leaf span path with the largest summed duration. |

## Queries

| Term | Meaning |
|------|---------|
| **Query AST v1** | The versioned JSON query over logs, metrics or traces (`QueryRequestV1`), with a default limit of 100 and a maximum of 1,000. |
| **query mode** | `auto` (the default), `sync` or `async`. `auto` runs as an async job when the limit is above 500. |
| **query job** | A persisted async query (queued, running, succeeded or failed), read back by its `query_id`. |
| **query cursor** | An opaque page token for one signal; a cursor from another signal is rejected. |
| **cold query** | A query that starts more than 30 days ago, answered by replaying the archive. |
| **phase-one API** | The first public wire contract: the versioned query, log tail, trace lookup, correlate, service listing, query jobs, and the MCP tools. |
| **correlate** | Returns the logs, metrics (matched by exemplar) and traces that share a trace, span, service or attributes. |

## Ingest

| Term | Meaning |
|------|---------|
| **admission / permit** | The ingest gate that checks sizes, event counts, per-project quota, concurrency and draining; the permit is the concurrency lease it hands out. |
| **storage reservation** | The local disk bytes an append reserves before it is admitted: three times the encoded bytes plus 128 bytes per event. |
| **governance policy** | A per-project privacy and cardinality policy (string cap, attribute allow and deny lists, redaction text, GenAI capture) applied before an event enters Raft. |
| **GenAI content capture** | A policy flag, off by default. When off, GenAI prompt, completion and message attributes are redacted. |
| **Cloud Logging coexistence identity** | A stable event id (`gcp-log-…`) computed from project, monitored resource, timestamp and canonical payload, so the Cloud Logging normalizer and CRI collection give one line one id and dual delivery is duplicate-safe. |
| **drain** | Refusing new ingest batches and waiting until every accepted batch has a durable result; readiness reports not ready meanwhile. |

## Collector

| Term | Meaning |
|------|---------|
| **CRI frame** | One parsed container-runtime log line: timestamp, stream, full (`F`) or partial (`P`) tag, and content. Partial fragments join up to the next full one. |
| **workload identity** | The namespace, pod, pod UID, container and restart number parsed from a CRI log file path. |
| **device-inode identity** | A CRI log file's `{dev}:{ino}` key, so rotation is followed and a renamed file is drained before a same-path replacement starts at offset 0. |
| **enrichment** | The resource labels, attributes and coexistence flag the collector adds to each record. |
| **loss accounting** | When a CRI source disappears with uncommitted bytes, the checkpoint counts the lost bytes and sources and the collector writes a durable `source_lost` rejection. |
| **quarantine** | A bounded JSONL file of rejected lines (`collector.rejection.v1`), fsynced before the checkpoint advances. |

## Node and roles

| Term | Meaning |
|------|---------|
| **data root** | A process's private, versioned data directory (`/var/lib/sift` by default). |
| **layout manifest** | The data root's `layout.json`: format version, cluster, node, role and restore source. |
| **role** | What one shared binary runs as: `all`, `agent`, `gateway`, `query`, `store`, `control` or `operator` (`StorageRole`; the CLI's `RunRole` converts to it). The role decides the data layout the process opens. |
| **store role** | Owns the WAL, segments, projections and Raft group. |
| **query role** | Serves queries: sync work reads from the store, and async job state stays on its own data root. |
| **gateway role** | Proxies each request to the store or query upstream. |
| **capacity level** | The local-disk pressure level: warning, backpressure or critical. Readiness fails at critical. |
