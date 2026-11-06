# Public API after P2

P2 keeps the CLI, HTTP, gRPC and persisted data contracts from `5ae45b9`.
Internal Rust types, constructors and traits can change.
The crate remains one crate.

## Public Rust paths

`src/lib.rs` is the list of crate-root re-exports.
Each public context module also lists its own re-exports.
Layer modules and the shared kernel remain private to the crate.

| Path | Use |
|------|-----|
| `sift::ServiceState` | Open the assembled service, submit events and control draining. |
| `sift::DurableJournal` | Use the journal application facade for append, reads, recovery and snapshots. |
| `sift::{OperationalEventV2, EventEnvelope, IncomingEvent, StoredEvent}` | Read and write the same versioned event data. |
| `sift::{router, protected_router, protected_router_with_mcp, query_role_router}` | Build the existing HTTP routers. |
| `sift::{openapi, openapi_json}` | Read the existing OpenAPI document. |
| `sift::archive::*` | Use archive data and the assembly-backed archive operations. |
| `sift::projection::*` | Use projection models, implementations and the application runtime. |
| `sift::query::*` | Use query models, the API client and MCP entry points. |
| `sift::ingest::*` | Use ingest limits, decoding and gRPC entry points. |
| `sift::{access, collector, journal, node, operations}` | Use each context's published Rust API. |

## Rust changes

`ServiceState` fields are private.
Use `journal()` and `projections()` for public reads.
Use `append_batch()`, `append_events()`, `start_drain()` and `finish_drain()`
for ingest and drain operations.
Crate-internal handlers obtain the other application services through methods.

`DurableJournal` is the owned journal application facade.
`RawStorage` is the raw event application service.
Their native files and stores are supplied by assembly.
The public open methods still build the standard adapters.

`sift::projection::Projection` is now Sift's pure trait.
It has an associated `Error` type.
The native projections use `anyhow::Error`.
A trait object for them is `dyn Projection<Error = anyhow::Error>`.
Projection descriptors, lag errors and rebuild results are Sift-owned data.

Archive `CatalogRoot` and `CatalogPageRef` are Sift-owned data.
Adapters convert them to Core catalog values.
Manifest field order and serialization rules remain unchanged.
The query client, projection runtime and archive operations use injected ports.
Their public constructors build the original native backends.

OpenAPI trait implementations live in assembly.
The CLI converts its signal enum to the event enum.
These moves do not alter wire values or help text.

## Earlier P2 path changes

The first P2 batch removed compatibility facades.
These changes happened before the completion baseline at `5ae45b9`.

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

## Byte contracts

Golden tests compare fixed output with bytes saved on `5ae45b9`.
They use fixed times and identifiers.
The saved files are under `tests/fixtures/refactor_compatibility/`.
They cover three event kinds, a manifest, a query job, three projection
snapshots, three checkpoints, OpenAPI and 34 CLI help pages.
Old stored data is decoded and encoded again to the same bytes.

Query cursors retain their prefixes and text encoding.
Query jobs retain their JSON fields, status values and field order.
HTTP rejection codes, retry hints and error messages stay the same.
Native errors keep their original source chains through the ports.

Event content digests hash the encoded event without receipt metadata.
Event sets combine SHA-256 digests with XOR.
Dedupe event digests keep `sift-dedupe-v6\0`, project bytes and lengths.
Dedupe record digests keep `sift-dedupe-record-v1\0` and little-endian fields.

| Checkpoint | Magic | Header and payload |
|------------|-------|--------------------|
| Archive | `SIFTRCP1` | 8-byte magic, 4-byte little-endian length, 4-byte CRC32, then JSON. |
| Local | `SIFTLCP1` | The same framing with the local error messages. |
| Resident | `SIFTRSD1` | The same framing with the resident error messages. |

The checkpoint payload limit remains 64 KiB.
Each codec validates before writing and after decoding.
Readers reject bad magic, invalid length, checksum mismatch, truncated data
and trailing bytes with the original errors.
Full snapshot and Raft command formats retain their existing separate framing.
No format version or migration step is added.

[The completion report](refactor-completion.md) records validation and limits.
