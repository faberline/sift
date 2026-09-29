//! The OpenAPI document: the annotated ingest and admin operations and their
//! schemas, plus a summary entry for each query and Prometheus route.

use anyhow::{Context, Result};
use service_http::DetailedErrorEnvelope as ErrorEnvelope;
use utoipa::OpenApi;

use crate::ingest::interfaces::http::otlp_handlers::{
    __path_ingest_logs, __path_ingest_metrics, __path_ingest_traces,
};
use crate::operations::interfaces::http::admin_backup::__path_admin_backup;
use crate::operations::interfaces::http::admin_integrity::__path_admin_integrity;
use crate::operations::interfaces::http::integrity_report_v1::{
    IntegrityArchiveV1, IntegrityReportV1, IntegritySignalV1, IntegritySignalsV1,
    IntegrityStorageV1, IntegrityWalBytesV1, IntegrityWatermarksV1,
};
use crate::projection;
use crate::shared_kernel::event::{
    AttributeValue, InstrumentationScope, MetricExemplar, MetricPoint, MetricTemporality,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        ingest_logs,
        ingest_traces,
        ingest_metrics,
        admin_backup,
        admin_integrity
    ),
    components(schemas(
        AttributeValue,
        InstrumentationScope,
        MetricPoint,
        MetricTemporality,
        MetricExemplar,
        projection::LogRecordV1,
        projection::SpanLinkV1,
        projection::SpanEventV1,
        projection::SpanRecordV1,
        projection::TraceResultV1,
        projection::HistogramKind,
        projection::MetricHistogramV1,
        projection::MetricPointV1,
        projection::MetricChunkV1,
        projection::MetricRollupV1,
        projection::MetricAggregation,
        projection::MetricSeriesResultV1,
        IntegritySignalV1,
        IntegritySignalsV1,
        IntegrityWatermarksV1,
        IntegrityWalBytesV1,
        IntegrityArchiveV1,
        IntegrityStorageV1,
        IntegrityReportV1,
        ErrorEnvelope
    )),
    tags((name = "telemetry", description = "Sift logs, metrics, and traces"))
)]
struct SiftApi;

pub fn openapi() -> utoipa::openapi::OpenApi {
    use utoipa::openapi::{
        path::{OperationBuilder, PathItem, PathItemType},
        response::ResponseBuilder,
    };

    let mut document = SiftApi::openapi();
    for (path, method, summary) in [
        (
            "/api/v1/query",
            "post",
            "Run one versioned logs, metrics, or traces query",
        ),
        (
            "/api/v1/logs/tail",
            "post",
            "Read a bounded resumable log tail",
        ),
        ("/api/v1/traces/{trace_id}", "get", "Read one trace"),
        ("/api/v1/correlate", "post", "Find related telemetry"),
        ("/api/v1/services", "get", "List observed services"),
        (
            "/api/v1/queries/{query_id}",
            "get",
            "Read a persistent asynchronous query job",
        ),
        (
            "/prometheus/api/v1/write",
            "post",
            "Receive Prometheus Remote Write 1.0",
        ),
        (
            "/prometheus/api/v1/query",
            "get",
            "Run an instant PromQL query",
        ),
        (
            "/prometheus/api/v1/query_range",
            "get",
            "Run a range PromQL query",
        ),
    ] {
        let method = match method {
            "get" => PathItemType::Get,
            "post" => PathItemType::Post,
            _ => unreachable!("phase-one OpenAPI method is fixed"),
        };
        let operation = OperationBuilder::new()
            .summary(Some(summary))
            .response("200", ResponseBuilder::new().description("Success").build())
            .build();
        document
            .paths
            .paths
            .insert(path.to_string(), PathItem::new(method, operation));
    }
    document
}

pub fn openapi_json() -> Result<String> {
    serde_json::to_string_pretty(&openapi()).context("serialize OpenAPI contract")
}
