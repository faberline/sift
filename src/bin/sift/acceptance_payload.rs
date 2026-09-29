//! `sift acceptance-payload`, hidden: emits deterministic protocol bytes for the
//! isolated acceptance runner.

use std::io::Write;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use prost::Message as _;
use prost14::Message as _;

#[derive(Args)]
pub(super) struct AcceptancePayloadArgs {
    #[arg(long, value_enum)]
    kind: AcceptancePayloadKind,
    #[arg(long, default_value_t = 1)]
    items: usize,
    #[arg(long, default_value = "sift")]
    project: String,
    #[arg(long, default_value = "acceptance")]
    event_prefix: String,
    #[arg(long)]
    timestamp_unix_nano: u64,
}

#[derive(Clone, Copy, ValueEnum)]
enum AcceptancePayloadKind {
    OtlpLogsProtobuf,
    PrometheusRemoteWriteV1,
}

pub(super) fn acceptance_payload(args: AcceptancePayloadArgs) -> Result<()> {
    if args.items == 0 || args.items > 1_000 {
        anyhow::bail!("--items must be between 1 and 1000");
    }
    if args.project.trim().is_empty() {
        anyhow::bail!("--project must not be empty");
    }
    if args.event_prefix.trim().is_empty() {
        anyhow::bail!("--event-prefix must not be empty");
    }
    let bytes = match args.kind {
        AcceptancePayloadKind::OtlpLogsProtobuf => {
            use sift::ingest::otlp::wire::{
                any_value, AnyValue, ExportLogsServiceRequest, KeyValue, LogRecord, Resource,
                ResourceLogs, ScopeLogs,
            };

            let mut log_records = Vec::with_capacity(args.items);
            for index in 0..args.items {
                let timestamp = args
                    .timestamp_unix_nano
                    .checked_add(index as u64)
                    .context("OTLP fixture timestamp overflow")?;
                log_records.push(LogRecord {
                    time_unix_nano: timestamp,
                    observed_time_unix_nano: timestamp,
                    severity_number: 9,
                    severity_text: "INFO".into(),
                    body: Some(AnyValue {
                        value: Some(any_value::Value::StringValue(format!(
                            "Sift acceptance log {index}"
                        ))),
                    }),
                    attributes: vec![KeyValue {
                        key: "sift.event_id".into(),
                        value: Some(AnyValue {
                            value: Some(any_value::Value::StringValue(format!(
                                "{}-{index}",
                                args.event_prefix
                            ))),
                        }),
                        ..Default::default()
                    }],
                    dropped_attributes_count: 0,
                    flags: 0,
                    trace_id: Vec::new(),
                    span_id: Vec::new(),
                    event_name: String::new(),
                });
            }
            ExportLogsServiceRequest {
                resource_logs: vec![ResourceLogs {
                    resource: Some(Resource {
                        attributes: vec![KeyValue {
                            key: "service.name".into(),
                            value: Some(AnyValue {
                                value: Some(any_value::Value::StringValue(
                                    "sift-acceptance".into(),
                                )),
                            }),
                            ..Default::default()
                        }],
                        dropped_attributes_count: 0,
                        entity_refs: Vec::new(),
                    }),
                    scope_logs: vec![ScopeLogs {
                        scope: None,
                        log_records,
                        schema_url: String::new(),
                    }],
                    schema_url: String::new(),
                }],
            }
            .encode_to_vec()
        }
        AcceptancePayloadKind::PrometheusRemoteWriteV1 => {
            use sift::prometheus::remote::{Label, Sample, TimeSeries, WriteRequest};

            let base_millis = i64::try_from(args.timestamp_unix_nano / 1_000_000)
                .context("Prometheus fixture timestamp exceeds i64 milliseconds")?;
            let samples = (0..args.items)
                .map(|index| {
                    let timestamp = base_millis
                        .checked_add(index as i64)
                        .context("Prometheus fixture timestamp overflow")?;
                    Ok(Sample {
                        value: index as f64,
                        timestamp,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let request = WriteRequest {
                timeseries: vec![TimeSeries {
                    labels: vec![
                        Label {
                            name: "__name__".into(),
                            value: "sift_acceptance_total".into(),
                        },
                        Label {
                            name: "environment".into(),
                            value: "acceptance".into(),
                        },
                        Label {
                            name: "fixture".into(),
                            value: args.event_prefix,
                        },
                        Label {
                            name: "project".into(),
                            value: args.project,
                        },
                    ],
                    samples,
                    exemplars: Vec::new(),
                }],
                metadata: Vec::new(),
            };
            metrics_remote_write::encode_snappy(&request.encode_to_vec())
                .context("compress Prometheus Remote Write fixture")?
        }
    };
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .context("write acceptance protocol bytes")
}
