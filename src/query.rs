//! Queries: the versioned query API over logs, metrics and traces, its async
//! jobs, the log tail, service listing and correlation, trace lookup, the
//! Prometheus-compatible query endpoints, the query role that forwards to the
//! store, and the read-only MCP tools over the same API.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;

pub use crate::query::{
    domain::{
        filter_evaluator::evaluate_filter,
        filter_expression::{FilterError, QueryExpressionV1},
        promql::{parse_promql, ParsedPromQuery, PromFunction, PromQlError},
        query_job::{QueryJobStatusV1, QueryJobV1},
    },
    infrastructure::sift_api_client::SiftApiClient,
    interfaces::{
        http::{
            phase_one::{
                CorrelationRequestV1, CorrelationResponseV1, LogTailRequestV1,
                ServiceListResponseV1, ServiceQueryV1, ServiceSummaryV1,
            },
            prom_query_params::{
                nanos_rfc3339, parse_prom_duration_nanos, parse_prom_time_nanos,
                InstantQueryParams, RangeQueryParams,
            },
            query_request_v1::{
                MetricFunctionV1, QueryModeV1, QueryRequestV1, QuerySignalV1, TimeRangeV1,
            },
            query_response_v1::{QueryResponseV1, QueryStatsV1},
        },
        mcp::{
            mcp_transport::{http_router as mcp_http_router, serve_stdio as serve_mcp_stdio},
            sift_mcp_server::tool_names as mcp_tool_names,
        },
    },
};
