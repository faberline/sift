//! GCS emulator state is process-wide, so joint recovery has its own binary.
use serde_json::{json, Value};
use sift::{EventEnvelope, EventQuery, ServiceState, SignalKind};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use storage_object::ObjectStore;

async fn put(http: &reqwest::Client, endpoint: &str, path: &str, value: Value) {
    let response = http
        .put(format!("{endpoint}{path}"))
        .json(&value)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{path}: {status} {body}");
}
async fn records(http: &reqwest::Client, endpoint: &str, collection: &str) -> Vec<Value> {
    let value: Value = http
        .get(format!("{endpoint}/api/v1/{collection}?project=recovery"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    value["records"].as_array().unwrap().clone()
}
async fn latest(store: &Arc<storage_object::GcsObjectStore>) -> Value {
    let store = store.clone();
    tokio::task::spawn_blocking(move || {
        let object = store.get("joint/recovery/latest.json").unwrap();
        serde_json::from_slice(object.bytes()).unwrap()
    })
    .await
    .unwrap()
}

#[path = "it/joint_recovery_http.rs"]
mod http_reads;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joint_record_restores_exact_events_control_state_and_notification_ids() {
    std::env::set_var("RUST_LOG", "sift=debug");
    service_http::init_tracing(&service_http::HttpConfig::new(
        "127.0.0.1",
        7380,
        "debug",
        service_http::LogFormat::Pretty,
        30,
        8 * 1024 * 1024,
        None,
    ))
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let emulator_address = listener.local_addr().unwrap();
    drop(listener);
    let emulator = tokio::spawn(async move {
        vat::emulator::serve(
            vat::emulator::Kind::CloudStorage,
            &emulator_address.to_string(),
        )
        .await
        .unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(emulator_address)
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let root = tempfile::tempdir().unwrap();
    let destination = "gs://sift-joint-test/joint";
    std::env::set_var(
        "STORAGE_EMULATOR_HOST",
        format!("http://{emulator_address}"),
    );
    std::env::set_var("SIFT_ARCHIVE_DESTINATION", destination);
    std::env::set_var("SIFT_RECOVERY_DESTINATION", destination);
    std::env::set_var("SIFT_RECOVERY_PROJECT", "recovery");
    std::env::set_var("SIFT_CONTROL_PROJECTS", "recovery");
    std::env::set_var(
        "SIFT_WEBHOOK_SECRET_DIRECTORY",
        root.path().join("webhooks"),
    );
    let state = Arc::new(ServiceState::open(root.path().join("source")).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = sift::router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::new();
    let recovery = state
        .start_recovery_worker(&endpoint)
        .await
        .unwrap()
        .unwrap();
    let mut log = EventEnvelope::for_project(
        "recovery",
        "prod",
        "log-a",
        SignalKind::Log,
        json!({"message":"failed request 7","exception":{"type":"Timeout","stacktrace":"handler 42"}}),
    );
    log.severity = Some("ERROR".into());
    log.resource = BTreeMap::from([("service.name".into(), "checkout".into())]);
    let span_time = chrono::Utc::now().timestamp_nanos_opt().unwrap() as u64;
    let mut span = EventEnvelope::for_project(
        "recovery",
        "prod",
        "span-a",
        SignalKind::Span,
        json!({"name":"checkout","start_time_unix_nano":span_time,"end_time_unix_nano":span_time+5_000_000}),
    );
    span.trace_id = Some("1".repeat(32));
    span.span_id = Some("2".repeat(16));
    span.resource = log.resource.clone();
    let mut accepted = false;
    for _ in 0..200 {
        match state.append_events(vec![log.clone()]).await {
            Ok(_) => {
                accepted = true;
                break;
            }
            Err(error) => {
                assert!(
                    error.to_string().contains("remote protection"),
                    "unexpected write error: {error:?}"
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        accepted,
        "initial joint archive must unlock write admission"
    );
    state.append_events(vec![span]).await.unwrap();
    let time = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let metric = json!({"resourceMetrics":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"checkout"}}]},"scopeMetrics":[{"metrics":[{
        "name":"queue_depth","gauge":{"dataPoints":[{"timeUnixNano":time.to_string(),"asDouble":3.0}]}
    }]}]}]});
    let response = http
        .post(format!("{endpoint}/v1/metrics"))
        .header("x-sift-project", "recovery")
        .json(&metric)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let response: Value = response.json().await.unwrap();
    assert!(response.get("partialSuccess").is_none(), "{response}");
    put(&http, &endpoint, "/api/v1/saved-queries/latency", json!({"project":"recovery",
        "request_id":"saved","expected_revision":0,"value":{"request":{"version":2,"project":"recovery",
        "signal":{"kind":"metrics","expression":"queue_depth"}}}})).await;
    put(
        &http,
        &endpoint,
        "/api/v1/alert-rules/errors",
        json!({"project":"recovery",
        "request_id":"rule","expected_revision":0,"value":{"version":1,"project":"recovery",
        "source":{"kind":"logs_count","window_seconds":60,"comparison":"greater","threshold":0},
        "hold_seconds":0,"webhook":"missing-secret"}}),
    )
    .await;
    let control = state.start_control_worker(&endpoint).unwrap().unwrap();
    let mut firing = false;
    // Rules are evaluated every 15 s (control_jobs.rs: next_evaluation = now + 15),
    // and the first evaluation runs in the same second as the seeded log, which
    // falls outside its window. The next evaluation must be awaited: 20 s > 15 s.
    for _ in 0..400 {
        if records(&http, &endpoint, "alerts")
            .await
            .iter()
            .any(|r| r["value"]["phase"] == "firing")
        {
            firing = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(firing);
    assert_eq!(records(&http, &endpoint, "error-groups").await.len(), 1);
    control.stop().await;
    recovery.stop().await;
    let expected = state
        .journal()
        .query(EventQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(expected.len(), 3);
    let recovery = state
        .start_recovery_worker(&endpoint)
        .await
        .unwrap()
        .unwrap();
    let objects = Arc::new(
        tokio::task::spawn_blocking(|| {
            storage_object::GcsObjectStore::new("sift-joint-test", "").unwrap()
        })
        .await
        .unwrap(),
    );
    let mut record = None;
    for _ in 0..200 {
        let value = latest(&objects).await;
        if value["data"]["through_cursor"] == 3 {
            record = Some(value);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let record = record.expect("data and control capture must commit together");
    recovery.stop().await;
    let control_key = url::Url::parse(record["control_uri"].as_str().unwrap())
        .unwrap()
        .path()
        .trim_start_matches('/')
        .to_owned();
    let store = objects.clone();
    let snapshot =
        tokio::task::spawn_blocking(move || store.get(&control_key).unwrap().into_parts().1)
            .await
            .unwrap();
    let captured: Value =
        serde_json::from_slice(snapshot.strip_prefix(b"SIFT-CONTROL-1\n").unwrap()).unwrap();
    assert!(captured["records"]
        .as_object()
        .unwrap()
        .values()
        .any(|r| r["kind"] == "notification"));
    let pinned = root.path().join("record.json");
    std::fs::write(&pinned, serde_json::to_vec(&record).unwrap()).unwrap();
    let store_dir = root.path().join("restored-store");
    let control_dir = root.path().join("restored-control");
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_sift"))
        .args(["restore-complete", "--source", destination, "--store-dir"])
        .arg(&store_dir)
        .arg("--control-dir")
        .arg(&control_dir)
        .arg("--record-file")
        .arg(&pinned)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "restore CLI: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restored: Value = serde_json::from_slice(
        &std::fs::read(control_dir.join("control/product-state-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        restored["records"], captured["records"],
        "all control content and notification IDs survive"
    );
    assert_eq!(restored["applied_index"], 0);
    std::env::remove_var("SIFT_RECOVERY_DESTINATION");
    let reading_root = store_dir.clone();
    let actual = tokio::task::spawn_blocking(move || {
        let restored = ServiceState::open(&reading_root).unwrap();
        restored
            .journal()
            .query(EventQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(
        actual, expected,
        "event identities, content, cursors, and acknowledgement times survive"
    );
    http_reads::verify_restored_process(&store_dir).await;
    let mut bad = record;
    bad["control_index"] = json!(u64::MAX);
    std::fs::write(&pinned, serde_json::to_vec(&bad).unwrap()).unwrap();
    let empty_store = root.path().join("bad-store");
    let empty_control = root.path().join("bad-control");
    let paths = (empty_store.clone(), empty_control.clone(), pinned);
    assert!(tokio::task::spawn_blocking(move || {
        sift::operations::restore_complete_recovery_pinned(
            destination,
            &paths.0,
            &paths.1,
            Some(&paths.2),
        )
    })
    .await
    .unwrap()
    .is_err());
    assert!(
        !empty_store.exists() && !empty_control.exists(),
        "bad record must fail before restore writes"
    );
    server.abort();
    emulator.abort();
}
