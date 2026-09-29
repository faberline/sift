//! `sift acceptance-grpc`, hidden: sends one valid and one invalid log through
//! OTLP/gRPC for acceptance.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct AcceptanceGrpcArgs {
    #[arg(long)]
    endpoint: String,
    #[arg(long, default_value = "sift")]
    project: String,
    #[arg(long)]
    token_file: Option<PathBuf>,
}

pub(super) async fn acceptance_grpc(args: AcceptanceGrpcArgs) -> Result<()> {
    use opentelemetry_proto::tonic::{
        collector::logs::v1::{logs_service_client::LogsServiceClient, ExportLogsServiceRequest},
        common::v1::{any_value, AnyValue, KeyValue},
        logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
        resource::v1::Resource,
    };
    use tonic::{codec::CompressionEncoding, Request};

    if args.project.trim().is_empty() {
        anyhow::bail!("--project must not be empty");
    }
    let timestamp = u64::try_from(chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default())
        .context("current time precedes Unix epoch")?;
    let valid = LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: timestamp,
        severity_text: "INFO".into(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(
                "Sift OTLP/gRPC acceptance".into(),
            )),
        }),
        attributes: vec![KeyValue {
            key: "sift.event_id".into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(format!(
                    "grpc-acceptance-{timestamp}"
                ))),
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let invalid = LogRecord {
        time_unix_nano: timestamp.saturating_add(1),
        observed_time_unix_nano: timestamp.saturating_add(1),
        ..Default::default()
    };
    let request = ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: vec![KeyValue {
                    key: "service.name".into(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::StringValue("sift-acceptance".into())),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![valid, invalid],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };
    let mut client = LogsServiceClient::connect(args.endpoint)
        .await
        .context("connect to Sift OTLP/gRPC")?
        .send_compressed(CompressionEncoding::Gzip)
        .accept_compressed(CompressionEncoding::Gzip);
    let mut request = Request::new(request);
    request.metadata_mut().insert(
        "x-sift-project",
        args.project.parse().context("encode project metadata")?,
    );
    if let Some(path) = args.token_file {
        let token = std::fs::read_to_string(&path)
            .with_context(|| format!("read projected token {}", path.display()))?;
        let authorization = format!("Bearer {}", token.trim());
        request.metadata_mut().insert(
            "authorization",
            authorization
                .parse()
                .context("encode authorization metadata")?,
        );
    }
    let response = client
        .export(request)
        .await
        .context("export OTLP/gRPC logs")?
        .into_inner();
    let rejected = response
        .partial_success
        .as_ref()
        .map_or(0, |partial| partial.rejected_log_records);
    if rejected != 1 {
        anyhow::bail!("OTLP/gRPC acceptance expected one rejected log, got {rejected}");
    }
    print_json_terminal(serde_json::json!({
        "signal":"logs",
        "accepted":1,
        "rejected":rejected,
        "compression":"gzip"
    }))
}
