//! Prometheus's public paths. The query context owns PromQL and the query
//! parameters, and the ingest context owns remote write; these re-exports
//! keep `sift::prometheus::*` compiling for callers outside the crate.

pub use crate::ingest::interfaces::remote_write::remote_write_consumer::{
    decode_remote_write, DecodedWrite,
};
pub use crate::query::domain::promql::{parse_promql, ParsedPromQuery, PromFunction};
pub use crate::query::interfaces::http::prom_query_params::{
    nanos_rfc3339, parse_prom_duration_nanos, parse_prom_time_nanos, InstantQueryParams,
    RangeQueryParams,
};
pub use metrics_remote_write::{proto as remote, PROMETHEUS_STALE_NAN_BITS};
