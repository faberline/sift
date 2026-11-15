//! Golden tests for the OTLP and Prometheus Remote Write 1.0 wire formats.
//!
//! The fixtures in `tests/fixtures/wire_formats` are hex text of the bytes that
//! Sift accepts and emits. They must not be regenerated to make a test pass.
//! Set `SIFT_CAPTURE_WIRE_FIXTURES` only once, to write them the first time.

use metrics_remote_write::proto::{Label, Sample, TimeSeries, WriteRequest};
use opentelemetry_proto::tonic::{
    collector::{
        logs::v1::ExportLogsServiceRequest, metrics::v1::ExportMetricsServiceRequest,
        trace::v1::ExportTraceServiceRequest,
    },
    common::v1::{any_value, AnyValue, KeyValue},
    logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    metrics::v1::{
        metric, number_data_point, AggregationTemporality, Gauge, Metric, NumberDataPoint,
        ResourceMetrics, ScopeMetrics, Sum,
    },
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span, Status},
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/wire_formats");
const CAPTURE_VARIABLE: &str = "SIFT_CAPTURE_WIRE_FIXTURES";
const PROTOBUF_FIELD_ONE: u8 = 0x0a;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(FIXTURES).join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read wire fixture {name}: {error}"));
    hex::decode(text.trim())
        .unwrap_or_else(|error| panic!("wire fixture {name} is not hex: {error}"))
}

fn golden(name: &str, bytes: &[u8]) -> Vec<u8> {
    if std::env::var_os(CAPTURE_VARIABLE).is_some() {
        std::fs::create_dir_all(FIXTURES).unwrap();
        let path = std::path::Path::new(FIXTURES).join(name);
        std::fs::write(&path, format!("{}\n", hex::encode(bytes))).unwrap();
    }
    fixture(name)
}

fn string_value(value: &str) -> AnyValue {
    AnyValue {
        value: Some(any_value::Value::StringValue(value.into())),
    }
}

fn key_value(key: &str, value: AnyValue) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(value),
        ..Default::default()
    }
}

fn resource() -> Resource {
    Resource {
        attributes: vec![key_value("service.name", string_value("checkout"))],
        ..Default::default()
    }
}

fn logs_request() -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(resource()),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: vec![
                    LogRecord {
                        time_unix_nano: 1_700_000_000_000_000_000,
                        observed_time_unix_nano: 1_700_000_000_000_000_001,
                        severity_number: 17,
                        severity_text: "ERROR".into(),
                        body: Some(string_value("failed")),
                        attributes: vec![key_value(
                            "http.status_code",
                            AnyValue {
                                value: Some(any_value::Value::IntValue(500)),
                            },
                        )],
                        trace_id: (1..=16u8).collect(),
                        span_id: (0x11..=0x18u8).collect(),
                        ..Default::default()
                    },
                    LogRecord {
                        time_unix_nano: 1_700_000_000_000_000_002,
                        severity_text: "INFO".into(),
                        body: Some(string_value("ok")),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn metrics_request() -> ExportMetricsServiceRequest {
    ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: Some(resource()),
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics: vec![
                    Metric {
                        name: "queue.depth".into(),
                        data: Some(metric::Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                time_unix_nano: 1_700_000_000_000_000_000,
                                value: Some(number_data_point::Value::AsDouble(3.5)),
                                ..Default::default()
                            }],
                        })),
                        ..Default::default()
                    },
                    Metric {
                        name: "requests.total".into(),
                        data: Some(metric::Data::Sum(Sum {
                            data_points: vec![NumberDataPoint {
                                start_time_unix_nano: 1_699_999_990_000_000_000,
                                time_unix_nano: 1_700_000_000_000_000_000,
                                value: Some(number_data_point::Value::AsInt(42)),
                                attributes: vec![key_value("method", string_value("POST"))],
                                ..Default::default()
                            }],
                            aggregation_temporality: AggregationTemporality::Cumulative as i32,
                            is_monotonic: true,
                        })),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn traces_request() -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(resource()),
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: vec![Span {
                    trace_id: (1..=16u8).collect(),
                    span_id: (0x11..=0x18u8).collect(),
                    name: "GET /cart".into(),
                    kind: 2,
                    start_time_unix_nano: 1_700_000_000_000_000_000,
                    end_time_unix_nano: 1_700_000_000_250_000_000,
                    attributes: vec![key_value("http.method", string_value("GET"))],
                    status: Some(Status {
                        code: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

fn label(name: &str, value: &str) -> Label {
    Label {
        name: name.into(),
        value: value.into(),
    }
}

fn remote_write_request() -> WriteRequest {
    WriteRequest {
        timeseries: vec![
            TimeSeries {
                labels: vec![
                    label("__name__", "http_requests_total"),
                    label("method", "POST"),
                    label("service.name", "checkout"),
                ],
                samples: vec![
                    Sample {
                        value: 2.0,
                        timestamp: 1_700_000_000_000,
                    },
                    Sample {
                        value: 5.0,
                        timestamp: 1_700_000_001_000,
                    },
                ],
                exemplars: Vec::new(),
            },
            TimeSeries {
                labels: vec![
                    label("__name__", "queue_depth"),
                    label("service.name", "checkout"),
                ],
                samples: vec![Sample {
                    value: 3.5,
                    timestamp: 1_700_000_000_000,
                }],
                exemplars: Vec::new(),
            },
        ],
        metadata: Vec::new(),
    }
}

/// Checks one OTLP gRPC fixture: it is the builder's encoding, it decodes to
/// the builder value, and it re-encodes to the same bytes.
fn check_otlp<M>(name: &str, builder: M)
where
    M: prost14::Message + Default + PartialEq + std::fmt::Debug,
{
    let encoded = prost14::Message::encode_to_vec(&builder);
    let bytes = golden(name, &encoded);
    assert_eq!(
        bytes, encoded,
        "fixture {name} is not the encoding of the builder value"
    );
    assert_eq!(
        bytes.first(),
        Some(&PROTOBUF_FIELD_ONE),
        "{name}: field 1 expected first"
    );
    let decoded: M = prost14::Message::decode(bytes.as_slice())
        .unwrap_or_else(|error| panic!("decode {name}: {error}"));
    assert_eq!(decoded, builder, "{name} decodes to another value");
    assert_eq!(
        prost14::Message::encode_to_vec(&decoded),
        bytes,
        "{name} re-encodes differently"
    );
}

#[test]
fn otlp_wire_bytes_do_not_change() {
    check_otlp("otlp-logs-request.hex", logs_request());
    check_otlp("otlp-metrics-request.hex", metrics_request());
    check_otlp("otlp-traces-request.hex", traces_request());
}

#[test]
fn remote_write_wire_bytes_do_not_change() {
    let encoded = prost::Message::encode_to_vec(&remote_write_request());
    let bytes = golden("remote-write-1.0-request.hex", &encoded);
    assert_eq!(
        bytes, encoded,
        "fixture is not the encoding of the builder value"
    );
    assert_eq!(
        bytes.first(),
        Some(&PROTOBUF_FIELD_ONE),
        "timeseries is field 1"
    );
    let decoded: WriteRequest = prost::Message::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded, remote_write_request());
    assert_eq!(prost::Message::encode_to_vec(&decoded), bytes);
    // Remote Write 1.0 sends the protobuf body snappy-compressed.
    let compressed = metrics_remote_write::encode_snappy(&bytes).unwrap();
    let body = metrics_remote_write::decode_snappy(&compressed, 1024 * 1024).unwrap();
    assert_eq!(body, bytes);
}

#[test]
fn changed_field_number_in_a_fixture_is_detected() {
    let original = fixture("otlp-logs-request.hex");
    let mut changed = original.clone();
    assert_eq!(changed[0], PROTOBUF_FIELD_ONE);
    changed[0] = 0x12; // field 2 instead of field 1
    match <ExportLogsServiceRequest as prost14::Message>::decode(changed.as_slice()) {
        Err(_) => {}
        Ok(decoded) => {
            assert_ne!(
                decoded,
                logs_request(),
                "changed field number must not decode the same"
            );
            assert_ne!(
                prost14::Message::encode_to_vec(&decoded),
                original,
                "changed field number must not re-encode to the original bytes"
            );
        }
    }
    let original_remote_write = fixture("remote-write-1.0-request.hex");
    let mut changed_remote_write = original_remote_write.clone();
    assert_eq!(changed_remote_write[0], PROTOBUF_FIELD_ONE);
    changed_remote_write[0] = 0x12;
    match <WriteRequest as prost::Message>::decode(changed_remote_write.as_slice()) {
        Err(_) => {}
        Ok(decoded) => {
            assert_ne!(decoded, remote_write_request());
            assert_ne!(
                prost::Message::encode_to_vec(&decoded),
                original_remote_write
            );
        }
    }
}

#[test]
fn wire_fixtures_are_hex_and_not_empty() {
    for name in [
        "otlp-logs-request.hex",
        "otlp-metrics-request.hex",
        "otlp-traces-request.hex",
        "remote-write-1.0-request.hex",
    ] {
        let bytes = fixture(name);
        assert!(bytes.len() > 20, "{name} has {} bytes", bytes.len());
    }
}
