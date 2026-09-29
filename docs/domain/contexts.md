# Contexts

One section per bounded context: what it owns, what each layer holds, what it
depends on, and which exceptions it carries. The rules the layers follow are in
[architecture.md](../architecture.md); the exact exceptions and their reasons
are in [`ddd.toml`](../../ddd.toml). Module names below are file names under
`src/<context>/<layer>/`.

## shared_kernel

The published language every context shares: the versioned operational event,
the stored event a cursor points at, and the digests, watermarks and ids that
the journal, the archive and the queries agree on. It has no layers and uses no
context.

- **Modules:** `event` (`OperationalEventV2`, `EventEnvelope`, `IncomingEvent`,
  `decode_event_json`, `SignalKind`), `stored_event`, `event_content_digest`,
  `single_signal_batch`, `archive_watermarks`, `retention_boundary`,
  `cloud_logging_event_id`.
- **Exceptions:** `utoipa` on the event and `StoredEvent` (E1), `anyhow` (E6),
  `clap::ValueEnum` on `SignalKind`, and `Utc::now` in the event constructors
  (E7).

## collector

The agent that reads service logs from a file, stdin or a node's CRI log root,
maps them to Sift events and delivers them over OTLP. It reads nothing of
Sift's but the shared kernel.

| Layer | Holds |
|-------|-------|
| domain | `config`, `cri` (CRI frames and workload identity), `record`, `service_log`, `quarantine` |
| application | `run_collector`, `runtime` |
| infrastructure | `source`, `cri_source`, `cri_discovery`, `checkpoint`, `cri_checkpoint`, `client`, `service_log_mapper` |

- **Depends on:** nothing. The CLI that starts it is `src/bin/sift/collect.rs`.
- **Exceptions:** `anyhow` (E6), `service_observability::StructuredServiceLogV1`
  in the domain (E14), and the runtime building its own source and client (B3).

## node

One Sift process's own data root: the private, versioned data directory with
its layout manifest, the role the process runs as, and the local disk-capacity
guard.

| Layer | Holds |
|-------|-------|
| domain | `storage_role` (`StorageRole`) |
| infrastructure | `data_layout` (`DataLayout`, `LayoutManifest`, `SiftDataRootPolicy`), `local_capacity` |

- **Depends on:** nothing. It has no application layer, so the journal and the
  archive use its domain and infrastructure directly.
- **Exceptions:** none.

## access

Who may call Sift, and for which project.

| Layer | Holds |
|-------|-------|
| domain | `route_role` (`http_role`, `role_verb`) |
| infrastructure | `auth_config` (`SiftAuthConfig`), `sift_verifier` (`SiftVerifier`: a static role map or a Kubernetes token review) |
| interfaces | `http/scoped_authorization` (the scoped bearer middleware), `http/project_authorization` (the project, read and global-admin checks handlers run) |

- **Depends on:** nothing. It has no application layer, so ingest, query and
  operations use its interfaces directly.
- **Exceptions:** `axum` and `service_auth` types in `route_role` (B2), and the
  middleware reaching the domain and the verifier (B3).

## journal

Sift's durable, replicated event log. It accepts governed batches through
Raft, deduplicates them inside the idempotency window, recovers from the WAL
and sealed segments, adopts archive checkpoints and retention, and serves reads
to the projections and queries.

| Layer | Holds |
|-------|-------|
| domain | `sift_command` (`SiftCommandV1`), `retention_fence`, `journal_head`, `segment_manifest`, `shard_route`, `idempotency_window`, `bloom_filter`, `dedupe_receipt`, `journal_state`, `recent_cursor`, `event_query`, `append_result`, `storage_config`, `raft_batch_limits` and the other limits and stats |
| application | `recover_journal`, `append_durable_batch`, `commit_append_batch`, `dedupe_window`, `adopt_archive`, `apply_retention_expiration`, `restore_journal`, `query_events`, `checkpoint_identity` |
| infrastructure | `raft/` (the Sift state machine, command codec, snapshots, checkpoints, membership policy), `storage/` (WAL, segments, dedupe index and shards, blob store, raw storage, journal head, local eviction), `durable_journal` (`DurableJournal`), `journal_projection_read_session`, `canonical_recovery_reader`, `snapshot_codec` |
| interfaces | `peer_server` (the Raft peer server), `journal_metrics` |

- **Depends on:** node.
- **Exceptions:** `anyhow`, `utoipa` on `AppendResult`, and `Utc::now` in
  `SiftCommandV1` (B2); use cases that are `impl DurableJournal` blocks over
  the storage, and the command committer and metrics that sit in other layers
  (B3); and its uses of the archive's committed state, control records and
  manifest, and of ingest's governance and storage reservation (B4). Declaring
  the archive would close a journal–archive cycle; P2 breaks it with a port.

## archive

Sift's cold tier. It uploads the journal's sealed segments, blobs and dedupe
receipts to GCS or a local archive under a verified manifest, evicts archived
segments from the hot tier, expires events past retention, garbage-collects
blobs once every voter has checkpointed, restores an empty node from the
archive, and replays archived events to cold queries.

| Layer | Holds |
|-------|-------|
| domain | `archive_manifest`, `portable_manifest`, `archive_manifest_validator`, `archive_catalog_item`, `event_set_digest`, `blob_hash_set`, `retention_policy`, `lifecycle_settings`, `archive_event_time` |
| application | one module per use case: `archive_journal_local`, `archive_journal_to_gcs`, `upload_archive_snapshot`, `evict_cold_segments`, `expire_committed_events`, `compact_journal`, `finalize_archive_gc`, `restore_into_empty`, `restore_gcs`, `replay_cold_query`, `run_lifecycle_attempt` and the rest |
| infrastructure | `archive_catalog`, `spill_catalog`, `archive_upload_intent`, `gc_plan_builder`, `parquet_event_codec`, `archive_segment_cache`, `verified_manifest_fetcher`, `gcs_uri`, `ephemeral_file_object_store`, the local GC and commit-state stores |
| interfaces | `archive_worker`, `lifecycle_worker` |

- **Depends on:** journal, node.
- **Exceptions:** `anyhow`, `storage_segment::CatalogRoot` and `service_backup`
  in the manifest (B2, E14); layer breaks in the lifecycle units of work (B3);
  its uses of the journal's domain and `DurableJournal`, and the cold-query
  replay's use of query and projection types (B4).
- **File size:** `expire_committed_events.rs` (534 lines) and
  `lifecycle_worker.rs` (458 lines) each hold one function that P2 splits
  (E9); until then they are C1 warnings.

## projection

The logging, metric and trace read models Sift builds from the journal and
answers queries from. They can always be rebuilt from the journal.

| Layer | Holds |
|-------|-------|
| domain | `log`, `metric`, `metric_series`, `metric_aggregation`, `metric_memory_size`, `trace`: records, queries, pages and the metric and trace models |
| application | `projection_runtime` (`ProjectionRuntime`), `log_record_normalizer` |
| infrastructure | `logging_projection`, `metric_projection` with its apply, query, scan and maintenance modules, `metric_chunk_store`, `trace_projection`, `service_projection_adapter` |
| interfaces | `projection_worker` (`ProjectionWorker`) |

- **Depends on:** journal.
- **Exceptions:** `anyhow` and `utoipa` on the `*V1` records (B2, E1); the
  runtime building the three projections, and the projections implementing
  the application's `Projection` trait (B3); and the runtime holding a
  `DurableJournal` (B4).

## ingest

Telemetry admitted within bounded limits, decoded from OTLP, Cloud Logging
structured JSON and Prometheus remote write into operational events, governed,
and appended through the group-commit queue to the journal.

| Layer | Holds |
|-------|-------|
| domain | `ingest_limits`, `admission_error`, `governance_policy`, `otlp_event_id`, `storage_reservation`, `raft_batch_splitter` |
| application | `admission_controller`, `append_events`, `drain_ingest`, `ensure_local_capacity`, `retention_admission` |
| infrastructure | `ingest_limits_env`, `governance_policy_file`, `gcp` (the Cloud Logging normalizer), `ingest_batch_coordinator` |
| interfaces | `http/` (OTLP handlers, remote write, request-body decoding), `grpc/otlp_grpc_server`, `otlp/` (the JSON and protobuf decoders), `remote_write/` |

- **Depends on:** journal, access.
- **Exceptions:** the compatibility facades `ingest::{batch,gcp,limits,otlp}`
  (B1); `anyhow` and `axum` in `admission_error` (B2); layer breaks in the
  append path and handlers (B3); and sizing batches by the journal's Raft
  limits (B4).

## operations

Running Sift as a service: the `Sift` custom resource and the operator that
reconciles it, the deployment artifacts rendered offline from checked-in
templates, off-node snapshot backup and restore, the gateway's reverse proxy,
the OTLP/gRPC proxy, and the admin backup and integrity endpoints.

| Layer | Holds |
|-------|-------|
| domain | `namespace_name`, `image_reference`, `route_class` |
| application | `journal_backup` (`backup_journal`, `restore_journal`) |
| infrastructure | `k8s/` (the renderers for role and agent workloads, the backup CronJob, network policy, API endpoint discovery, `impl ManagedService for Sift`), `manifest_bundle`, `live_snapshot_client` |
| interfaces | `crd/sift_spec` (`Sift`, `SiftSpec`, `SiftStatus`), `operator` (`run`), `http/gateway_proxy`, `http/admin_backup`, `http/admin_integrity`, `http/integrity_report_v1`, `grpc/otlp_proxy` |

- **Depends on:** journal, archive, node, access, ingest.
- **Exceptions:** `anyhow` in the domain (B2); the renderers reading the CRD
  types and the proxy reaching the domain (B3); and its uses of
  `DurableJournal` and ingest's gRPC project check (B4).

## query

The versioned query API over logs, metrics and traces, its async jobs, the
Prometheus-compatible endpoints, the query role, and the read-only MCP tools.

| Layer | Holds |
|-------|-------|
| domain | `filter_expression`, `filter_evaluator`, `promql`, `promql_evaluator`, `query_job`, `query_cursor`, `query_time` |
| application | `execute_query`, `get_query_job`, `prom_query`, `archive_query_status` |
| infrastructure | `file_query_job_store`, `store_query_client`, `sift_api_client` |
| interfaces | `http/` (query, correlate, services, trace, phase-one and Prometheus handlers, the query role router), `mcp/` (the MCP server and its transport) |

- **Depends on:** projection, journal, archive, access, operations.
- **Exceptions:** `anyhow`, `base64` in the query cursor and `Utc::now` in
  `query_time` (B2); layer breaks between the handlers, DTOs and use cases
  (B3); and its uses of projection types, the journal's domain and the
  gateway's store-forwarding router (B4).
