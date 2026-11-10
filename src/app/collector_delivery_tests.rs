//! Collection survives an outage and the loss of an already framed file.
use super::{collector_wiring::run_collector_with_options, ServiceState};
use crate::collector::domain::{
    config::{CollectorConfig, SourceSpec},
    log_format::{LogFormat, LogOptions},
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[tokio::test]
async fn durable_multiline_replay_keeps_original_identity_after_source_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("app.log");
    std::fs::write(
        &source,
        b"ERROR failed request\n  at handler:42\nINFO next request\n",
    )
    .unwrap();
    let state = Arc::new(ServiceState::open(dir.path().join("store")).unwrap());
    let available = Arc::new(AtomicBool::new(false));
    let gate = available.clone();
    let app = crate::router(state.clone()).layer(axum::middleware::from_fn(move |request: axum::extract::Request, next: axum::middleware::Next| {
        let gate = gate.clone();
        async move {
            if !gate.load(Ordering::SeqCst) {
                use axum::response::IntoResponse;
                return (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({"error":"test_outage","message":"retry","retryable":true}))).into_response();
            }
            next.run(request).await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let checkpoint = dir.path().join("position.json");
    let mut config = CollectorConfig {
        source: SourceSpec::File(source.clone()),
        source_id: "fixture".into(),
        endpoint,
        token: None,
        token_file: None,
        token_audience: "sift.axiom.dev".into(),
        project: "p".into(),
        environment: "prod".into(),
        checkpoint_path: checkpoint.clone(),
        quarantine_path: dir.path().join("rejected.jsonl"),
        batch_size: 10,
        max_line_bytes: 1024,
        max_retries: 0,
        request_timeout: Duration::from_secs(2),
        initial_backoff: Duration::from_millis(10),
        follow: false,
        follow_poll_interval: Duration::from_millis(10),
    };
    let options = || LogOptions {
        format: LogFormat::Text,
        multiline_start: Some(r"^(ERROR|INFO)".into()),
    };
    assert!(run_collector_with_options(config.clone(), options())
        .await
        .is_err());
    let pending: Vec<crate::OperationalEventV2> =
        serde_json::from_slice(&std::fs::read(checkpoint.with_extension("pending.json")).unwrap())
            .unwrap();
    assert_eq!(pending.len(), 1);
    let frames: serde_json::Value =
        serde_json::from_slice(&std::fs::read(checkpoint.with_extension("framing.json")).unwrap())
            .unwrap();
    let decoder = crate::collector::infrastructure::generic_log_decoder::GenericLogDecoder {
        project: "p",
        environment: "prod",
        format: LogFormat::Text,
    };
    use crate::collector::domain::record_decoder::RecordDecoder;
    let originals: Vec<crate::OperationalEventV2> = frames["ready"]
        .as_array()
        .unwrap()
        .iter()
        .chain(frames.get("pending").filter(|value| !value.is_null()))
        .map(|raw| {
            decoder
                .decode(serde_json::from_value(raw.clone()).unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(
        originals.len(),
        2,
        "both complete records must be durable before delivery"
    );
    assert_eq!(pending[0].event_id, originals[0].event_id);
    std::fs::remove_file(source).unwrap();
    available.store(true, Ordering::SeqCst);
    config.max_retries = 1;
    let summary = run_collector_with_options(config, options()).await.unwrap();
    assert_eq!(summary.lost_bytes, 0);
    assert_eq!(
        state.journal().total_event_count(),
        2,
        "queue replay and framed replay must deduplicate"
    );
    state
        .projections()
        .catch_up(crate::projection::PROJECTION_LOGGING_STORE)
        .unwrap();
    let page = state
        .projections()
        .query_logs(&crate::projection::LogQuery::for_project("p"))
        .unwrap();
    assert_eq!(
        page.records[0].body_text,
        "ERROR failed request\n  at handler:42"
    );
    assert_eq!(page.records.len(), originals.len());
    for (record, original) in page.records.iter().zip(&originals) {
        assert_eq!(record.event_id, original.event_id);
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(&record.observed_at).unwrap(),
            chrono::DateTime::parse_from_rfc3339(&original.observed_at).unwrap()
        );
        assert_eq!(record.json_payload["raw"], original.payload["raw"]);
    }
    server.abort();
}
