# Sift limits

This page lists each limit with its value and the file and item where the code enforces it. `scripts/verify-limits-doc.py` checks that each named file and item exists in the source, and CI runs it.

## Limits

| Limit | Value | Enforced at |
|---|---|---|
| Compressed ingest body | 1,048,576 bytes | `src/ingest/domain/ingest_limits.rs` `max_compressed_body_bytes` |
| Decoded ingest body | 8,388,608 bytes | `src/ingest/domain/ingest_limits.rs` `max_decoded_body_bytes` |
| One event | 262,144 bytes | `src/ingest/domain/ingest_limits.rs` `max_event_bytes` |
| Events per ingest batch | 1,000 | `src/ingest/domain/ingest_limits.rs` `max_events_per_batch` |
| Concurrent requests per project | 32 | `src/ingest/domain/ingest_limits.rs` `max_concurrent_requests_per_project` |
| Items per project per window | 720,000 per 60 s | `src/ingest/domain/ingest_limits.rs` `max_items_per_project_window` `quota_window_secs` |
| Local storage cap | 50 GiB | `src/ingest/domain/ingest_limits.rs` `max_local_storage_bytes` |
| Minimum free local disk | 1 GiB | `src/ingest/domain/ingest_limits.rs` `min_local_free_bytes` |
| Governed string length | 4,096 bytes | `src/ingest/domain/governance_policy.rs` `DEFAULT_MAX_STRING_BYTES` |
| Default query result limit | 100 | `src/shared_kernel/query_limit.rs` `DEFAULT_QUERY_LIMIT` |
| Maximum query result limit | 1,000 | `src/shared_kernel/query_request.rs` `MAX_QUERY_LIMIT` |
| Maximum log query limit | 1,000 | `src/shared_kernel/projection_log.rs` `MAX_LOG_QUERY_LIMIT` |
| Maximum metric query limit | 1,000 | `src/shared_kernel/projection_metric.rs` `MAX_METRIC_QUERY_LIMIT` |
| Maximum trace query limit | 1,000 | `src/shared_kernel/projection_trace.rs` `MAX_TRACE_QUERY_LIMIT` |
| Metric query source points | 250,000 | `src/projection/domain/metric.rs` `MAX_METRIC_QUERY_SOURCE_POINTS` |
| Metric query source bytes | 64 MiB | `src/projection/domain/metric.rs` `MAX_METRIC_QUERY_SOURCE_BYTES` |
| Prometheus range query work | 1,000,000 samples | `src/query/domain/promql_evaluator.rs` `PROM_MAX_RANGE_WORK_SAMPLES` |
| Filter nesting depth | 32 | `src/query/domain/filter_expression.rs` `MAX_FILTER_DEPTH` |
| Regular expression length | 1,024 bytes | `src/query/domain/filter_expression.rs` `MAX_REGEX_BYTES` |
| Metric series cardinality | 10,000 | `src/projection/domain/metric.rs` `DEFAULT_METRIC_CARDINALITY_LIMIT` |
| Retained points per series | 100,000 | `src/projection/domain/metric.rs` `DEFAULT_RETAINED_POINTS_PER_SERIES` |
| Metric memtable | 256 MiB | `src/projection/domain/metric.rs` `DEFAULT_METRIC_MEMTABLE_BYTES` |
| Retained log records | 100,000 | `src/projection/domain/log.rs` `DEFAULT_RETAINED_LOG_RECORDS` |
| Retained trace spans | 100,000 | `src/projection/domain/trace.rs` `DEFAULT_RETAINED_TRACE_SPANS` |
| Resident journal events | 100,000 | `src/journal/domain/journal_limits.rs` `DEFAULT_RESIDENT_JOURNAL_EVENTS` |
| Event-ID idempotency window | 6 hours (21,600 s) | `src/shared_kernel/idempotency_window.rs` `IDEMPOTENCY_WINDOW_SECONDS` |
| Hot retention | 30 days | `src/archive/application/evict_cold_segments.rs` `Duration::days(30)` |
| Total retention | default and maximum 180 days; `--retention-days` sets 1 to 180 | `src/shared_kernel/retention_boundary.rs` `MAX_RETENTION_DAYS` |
| Collector batch size | default 100, maximum 1,000 | `src/collector/domain/config.rs` `DEFAULT_BATCH_SIZE` `MAX_BATCH_SIZE` |
| Collector line size | default 512 KiB, maximum 1 MiB | `src/collector/domain/config.rs` `DEFAULT_MAX_LINE_BYTES` `MAX_LINE_BYTES` |
| Collector retries | 3 | `src/collector/domain/config.rs` `DEFAULT_MAX_RETRIES` |
| Items per request of the workload tool | 1,000 | `src/bin/sift/acceptance_payload.rs` `args.items > 1_000` |
| Raft batch size | 1,048,576 bytes, 1,000 items, 10 ms delay | `src/shared_kernel/raft_batch_limits.rs` `RAFT_BATCH_MAX_BYTES` `RAFT_BATCH_MAX_ITEMS` `RAFT_BATCH_MAX_DELAY` |
| Checkpoint payload | 64 KiB | `src/journal/infrastructure/raft/checkpoint_framing.rs` `MAX_PAYLOAD_BYTES` |
| Control state | 64 MiB | `src/control/domain/state.rs` `MAX_STATE_BYTES` |
| Alert hold time | 604,800 s (7 days) | `src/alerting/domain/rule.rs` `hold_seconds > 604_800` |
| MCP request timeout | 30 s | `src/query/interfaces/mcp/mcp_transport.rs` `MCP_TIMEOUT` |
| Control request file | 1 MiB | `src/bin/sift/control.rs` `control request exceeds 1 MiB` |

## Known limits

Production acceptance for 0.2.0 is pending. These limits remain:

- V2 supports metric selectors, grouped aggregation, `rate`, `increase`, arithmetic, comparisons, one-to-one label matching, and `histogram_quantile`. Unsupported functions, many-to-many joins, and subqueries return errors.
- Logs support structured search, bounded text search, facets, context, and tail. They do not yet have production-scale load evidence.
- Traces support trace reads, search filters, gaps, and critical paths. Tail sampling and a complete service dependency engine are not complete.
- Local and emulated GCS archive, outage, lifecycle, and fresh-volume restore tests pass. One live GCS backup and restore drill passed on the single-voter path; see [restore-drill-2026-11-13.md](restore-drill-2026-11-13.md).
- Local store and control voters pass mutual-TLS leader-loss tests. GKE fault and recovery drills remain required.
- The fixed 30-day hot and 180-day total retention worker is implemented. It keeps WAL during a GCS outage, rewrites mixed-age segments, removes expired blobs, and resumes safe cleanup after restart. Exact event-ID idempotency is a separate six-hour window.
- The 10,000-item-per-second target needs both 30-minute and 24-hour GKE evidence. Neither gate has run. The later 100,000-item-per-second target remains outside this release.
- The 128-event drain and reopen test covers `baseline-ingest-and-restart-resilience`. The sustained load claim `ingest-query-replay-soak` stays pending.
- Error groups, alerts, and saved queries are implemented. Live Webhook delivery and a complete source comparison remain required. A fresh installation can compare generated input with Sift output. Seven-day dual-write is optional for an existing-service migration. SLOs, incidents, on-call, synthetics, RUM, profiles, and cost analysis are later work. The data-plane runbook is [runbook.md](runbook.md).

Do not use a green unit or local integration test as evidence for a live GKE or production performance claim.
