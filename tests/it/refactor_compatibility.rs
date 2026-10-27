//! Bytes captured on 5ae45b9. Do not refresh them during the refactor.

use std::collections::BTreeMap;

use sift::archive::ArchiveManifest;
use sift::projection::{LoggingProjection, MetricProjection, Projection, TraceProjection};
use sift::query::{QueryJobV1, QueryRequestV1, QueryResponseV1};
use sift::{
    AttributeValue, EventEnvelope, MetricPoint, MetricTemporality, SignalKind, StoredEvent,
};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/refactor_compatibility"
);
const TIME: &str = "2026-09-30T12:34:56.123456789Z";

fn golden(name: &str, bytes: &[u8]) -> Vec<u8> {
    let path = std::path::Path::new(FIXTURES).join(name);
    let expected = std::fs::read(path).unwrap();
    assert_eq!(bytes, expected, "fixture {name} changed");
    expected
}

fn stored(cursor: u64, signal: SignalKind) -> StoredEvent {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        format!("fixed-event-{cursor}"),
        signal,
        match signal {
            SignalKind::Log => serde_json::json!({"message": "fixed log 雨", "level": "INFO"}),
            SignalKind::Span => serde_json::json!({
                "name": "fixed-span", "start_time_unix_nano": 123456789,
                "end_time_unix_nano": 223456789,
                "events": [], "links": []
            }),
            SignalKind::Metric => serde_json::json!({}),
        },
    );
    event.occurred_at = TIME.into();
    event.observed_at = TIME.into();
    event
        .resource
        .insert("service.name".into(), "fixed-service".into());
    event.attributes = BTreeMap::from([
        ("service.name".into(), AttributeValue::from("fixed-service")),
        ("count".into(), AttributeValue::Int(42)),
        ("enabled".into(), AttributeValue::Bool(true)),
    ]);
    if signal == SignalKind::Span {
        event.trace_id = Some("0123456789abcdef0123456789abcdef".into());
        event.span_id = Some("0123456789abcdef".into());
    }
    if signal == SignalKind::Metric {
        event.metric = Some(MetricPoint {
            name: "requests.total".into(),
            value: 42.5,
            stale: false,
            unit: Some("1".into()),
            temporality: MetricTemporality::Cumulative,
            exemplars: vec![],
        });
    }
    StoredEvent {
        cursor,
        acknowledged_at: TIME.into(),
        event,
    }
}

#[test]
fn event_bytes_read_and_reencode_without_change() {
    for (cursor, signal, name) in [
        (1, SignalKind::Log, "event-log.json"),
        (2, SignalKind::Metric, "event-metric.json"),
        (3, SignalKind::Span, "event-span.json"),
    ] {
        let bytes = golden(name, &serde_json::to_vec(&stored(cursor, signal)).unwrap());
        let old: StoredEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&old).unwrap(), bytes);
    }
}

#[test]
fn manifest_bytes_read_and_reencode_without_change() {
    let page = serde_json::json!({"key":"page/fixed", "sha256":"a".repeat(64),
        "bytes":512,"entry_count":7,"first_key":"segment/a","last_key":"segment/a"});
    let root = serde_json::json!({"format_version":1,"height":1,"entry_count":7,
        "page_bytes_limit":65536,"root":page});
    let mut gc_root = root.clone();
    gc_root["entry_count"] = 1.into();
    gc_root["root"]["entry_count"] = 1.into();
    let manifest: ArchiveManifest = serde_json::from_value(serde_json::json!({
        "format_version":10,"generated_at":TIME,"source_cluster_id":"fixed-cluster",
        "source_node_id":"fixed-node","raft_snapshot_index":3,"event_count":3,
        "event_id_digest_algorithm":"xor-sha256-v1","event_id_sha256":"b".repeat(64),
        "event_content_digest_algorithm":"xor-sha256-v1","event_content_sha256":"c".repeat(64),
        "retention_generation":7,"watermarks":{"logs":1,"metrics":2,"traces":3},
        "retained_watermarks":{"logs":1,"metrics":2,"traces":3},
        "oldest_event_time_unix_nano":123456789,
        "epochs":[{"epoch":1,"activated_at_cursor":1,"bucket_to_shard":vec![0u16;4096]}],
        "catalog_uri":"gs://bucket/fixed/catalog.json","catalog_root":root,
        "segment_count":3,"blob_count":1,"dedupe_receipt_count":3,
        "gc_plan_uri":"gs://bucket/fixed/gc.json","gc_plan_root":gc_root,"gc_object_count":1,
        "retention_delta":{"source_manifest_uri":"gs://bucket/old/manifest.json",
            "source_manifest_sha256":"d".repeat(64),"source_generation":6,
            "source_event_count":4,"source_event_content_sha256":"e".repeat(64),
            "cutoff_unix_nano":123456789},
        "retention_scan":{"cutoff_unix_nano":123456789,"after_catalog_key":"segment/a",
            "oldest_retained_event_time_unix_nano":123456789}
    }))
    .unwrap();
    let bytes = golden("manifest.json", &serde_json::to_vec(&manifest).unwrap());
    let old: ArchiveManifest = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&old).unwrap(), bytes);
}

#[test]
fn query_job_bytes_read_and_reencode_without_change() {
    let request: QueryRequestV1 = serde_json::from_value(serde_json::json!({
        "version":1,"project":"project-a","environment":"prod",
        "time_range":{"start":TIME,"end":"2026-10-01T12:34:56.123456789Z"},
        "signal":{"kind":"logs"},"limit":100,"cursor":"logs:MTA","mode":"async"
    }))
    .unwrap();
    let result = QueryResponseV1::complete(
        serde_json::json!({"records":[]}),
        Some("logs:MjA".into()),
        20,
        10,
        0,
        std::time::Duration::from_millis(17),
    );
    let job: QueryJobV1 = serde_json::from_value(serde_json::json!({
        "query_id":"0123456789abcdef0123456789abcdef","project":"project-a",
        "status":"succeeded","created_at":TIME,"updated_at":TIME,
        "request":request,"result":result
    }))
    .unwrap();
    let bytes = golden("query-job.json", &serde_json::to_vec(&job).unwrap());
    let old: QueryJobV1 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&old).unwrap(), bytes);
}

#[test]
fn projection_snapshots_read_and_reencode_without_change() {
    let log = LoggingProjection::with_max_records(10).unwrap();
    let metric = MetricProjection::with_limits(10, 10).unwrap();
    let trace = TraceProjection::with_max_spans(10).unwrap();
    for (projection, signal, cursor, name) in [
        (
            &log as &dyn Projection<Error = anyhow::Error>,
            SignalKind::Log,
            1,
            "projection-log.json",
        ),
        (
            &metric as &dyn Projection<Error = anyhow::Error>,
            SignalKind::Metric,
            2,
            "projection-metric.json",
        ),
        (
            &trace as &dyn Projection<Error = anyhow::Error>,
            SignalKind::Span,
            3,
            "projection-trace.json",
        ),
    ] {
        projection
            .apply_idempotent(&stored(cursor, signal))
            .unwrap();
        let bytes = golden(name, &projection.snapshot().unwrap());
        projection.restore(&bytes).unwrap();
        assert_eq!(projection.snapshot().unwrap(), bytes);
    }
}

#[test]
fn openapi_bytes_do_not_change() {
    golden(
        "openapi.json",
        &serde_json::to_vec(&sift::openapi()).unwrap(),
    );
}

#[test]
fn cli_help_bytes_do_not_change() {
    fn collect(path: Vec<String>, pages: &mut BTreeMap<String, String>) {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_sift"))
            .args(&path)
            .arg("--help")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "help for {path:?}: {output:?}");
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        let children = help
            .split_once("Commands:\n")
            .map(|(_, tail)| {
                tail.lines()
                    .take_while(|line| !line.is_empty())
                    .filter_map(|line| line.strip_prefix("  "))
                    .filter_map(|line| line.split_whitespace().next())
                    .filter(|name| *name != "help")
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        pages.insert(path.join(" "), help);
        for child in children {
            let mut next = path.clone();
            next.push(child);
            collect(next, pages);
        }
    }
    let mut pages = BTreeMap::new();
    collect(Vec::new(), &mut pages);
    for hidden in [
        "acceptance-payload",
        "acceptance-build-info",
        "acceptance-grpc",
    ] {
        collect(vec![hidden.into()], &mut pages);
    }
    assert!(pages.len() > 14, "capture subcommands as well as the root");
    golden("cli-help.json", &serde_json::to_vec(&pages).unwrap());
}
