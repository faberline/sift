// HANDWRITE-BEGIN gap="sift-auth-route-contract-tests" tracker="1604" reason="Verify required bearer auth protects data-plane routes while probes remain reachable."
use std::{
    collections::HashMap,
    fs::File,
    net::TcpListener,
    process::{Child, Command},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use service_auth::{Role, TokenClaims};
use sift::{
    access::{SiftAuthConfig, SiftVerifier},
    openapi, protected_router, ServiceState,
};
use tower::ServiceExt;

fn required_app(data_dir: &std::path::Path) -> axum::Router {
    let verifier = Arc::new(SiftVerifier::new(SiftAuthConfig {
        required: true,
        tokens: HashMap::from([(
            "writer-token".to_string(),
            TokenClaims {
                subject: "sift-test".to_string(),
                roles: HashMap::from([("*".to_string(), Role::Write)]),
            },
        )]),
    }));
    let state = Arc::new(ServiceState::open(data_dir).expect("open journal"));
    service_http::standard_probe_routes(state.clone(), Some(state.clone()), openapi)
        .merge(protected_router(state, verifier))
}

fn valid_logs() -> serde_json::Value {
    serde_json::json!({
        "resourceLogs": [{
            "resource": {"attributes": [
                {"key": "service.name", "value": {"stringValue": "sift-test"}}
            ]},
            "scopeLogs": [{"logRecords": [{"body": {"stringValue": "ok"}}]}]
        }]
    })
}

struct SiftProcess(Child);

impl Drop for SiftProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn required_auth_protects_data_plane_but_not_operational_probes() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app(data_dir.path());

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/logs")
                .header("content-type", "application/json")
                .header("x-sift-project", "project-a")
                .body(Body::from(serde_json::to_vec(&valid_logs()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let accepted = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/logs")
                .header("content-type", "application/json")
                .header("x-sift-project", "project-a")
                .header("authorization", "Bearer writer-token")
                .body(Body::from(serde_json::to_vec(&valid_logs()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
}

#[tokio::test]
async fn real_server_protects_and_correlates_ingest_without_leaking_tokens() {
    const TOKEN: &str = "sift-test-secret-writer-83637";
    const UNKNOWN_TOKEN: &str = "sift-test-secret-unknown-83637";
    const TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";
    const SPAN_ID: &str = "b7ad6b7169203331";
    const PROJECT: &str = "project-a";

    let temp = tempfile::tempdir().unwrap();
    let registry = temp.path().join("tokens.json");
    std::fs::write(
        &registry,
        json!({(TOKEN): {"subject": "security-test", "roles": {(PROJECT): "write"}}}).to_string(),
    )
    .unwrap();
    let log_path = temp.path().join("sift.log");
    let log = File::create(&log_path).unwrap();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
        .to_string();
    let mut server = SiftProcess(
        Command::new(env!("CARGO_BIN_EXE_sift"))
            .args([
                "serve",
                "--host",
                "127.0.0.1",
                "--port",
                &port,
                "--grpc-port",
                "0",
                "--log-format",
                "json",
                "--max-body-bytes",
                "4096",
                "--data-dir",
            ])
            .arg(temp.path().join("data"))
            .env_clear()
            .env("SIFT_AUTH", "required")
            .env("SIFT_TOKEN_REGISTRY_FILE", registry)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("start Sift"),
    );
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if client
            .get(format!("{base}/readyz"))
            .send()
            .await
            .is_ok_and(|response| response.status() == StatusCode::OK)
        {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "Sift exited before readiness"
        );
        assert!(Instant::now() < deadline, "Sift did not become ready");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        client
            .get(format!("{base}/healthz"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let mut logs = valid_logs();
    let log_record = &mut logs["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
    log_record["traceId"] = json!(TRACE_ID);
    log_record["spanId"] = json!(SPAN_ID);
    let traceparent = format!("00-{TRACE_ID}-{SPAN_ID}-01");
    for (token, project, expected) in [
        (None, PROJECT, StatusCode::UNAUTHORIZED),
        (Some(UNKNOWN_TOKEN), PROJECT, StatusCode::UNAUTHORIZED),
        (Some(TOKEN), "other-project", StatusCode::FORBIDDEN),
    ] {
        let request = client
            .post(format!("{base}/v1/logs"))
            .header("x-sift-project", project)
            .header("traceparent", &traceparent)
            .json(&logs);
        let response = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
        .send()
        .await
        .unwrap();
        assert_eq!(response.status(), expected);
        let body = response.text().await.unwrap();
        assert!(!body.contains(TOKEN) && !body.contains(UNKNOWN_TOKEN));
    }
    let request = |path: &str| {
        client
            .post(format!("{base}{path}"))
            .header("x-sift-project", PROJECT)
            .header("traceparent", &traceparent)
            .bearer_auth(TOKEN)
    };
    assert_eq!(
        request("/v1/logs")
            .header("content-type", "application/json")
            .body(" ".repeat(4097))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let metrics = || client.get(format!("{base}/metrics")).send();
    let before = metrics()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(before.lines().any(|line| line == "sift_raw_events_total 0"));

    let start = chrono::Utc::now().timestamp_nanos_opt().unwrap() - 1_000_000;
    let spans = json!({"resourceSpans": [{"scopeSpans": [{"spans": [{
        "traceId": TRACE_ID,
        "spanId": SPAN_ID,
        "name": "authorized work",
        "startTimeUnixNano": start.to_string(),
        "endTimeUnixNano": (start + 1_000_000).to_string()
    }]}]}]});
    for (path, payload) in [("/v1/logs", logs), ("/v1/traces", spans)] {
        let response = request(path).json(&payload).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let receipt: Value = response.json().await.unwrap();
        assert_eq!(receipt, json!({}), "OTLP export rejected data at {path}");
    }

    let response = request("/api/v1/correlate")
        .json(&json!({"version": 1, "project": PROJECT, "trace_id": TRACE_ID, "limit": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let correlated: Value = response.json().await.unwrap();
    assert_eq!(correlated["partial"], false);
    assert_eq!(correlated["logs"].as_array().unwrap().len(), 1);
    assert_eq!(correlated["logs"][0]["body_text"], "ok");
    assert_eq!(correlated["logs"][0]["span_id"], SPAN_ID);
    assert_eq!(correlated["traces"].as_array().unwrap().len(), 1);
    assert_eq!(correlated["traces"][0]["trace_id"], TRACE_ID);
    assert_eq!(
        correlated["traces"][0]["spans"].as_array().unwrap().len(),
        1
    );
    assert_eq!(correlated["traces"][0]["spans"][0]["span_id"], SPAN_ID);
    assert_eq!(
        correlated["traces"][0]["spans"][0]["name"],
        "authorized work"
    );

    let after = metrics()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(after.lines().any(|line| line == "sift_raw_events_total 2"));
    drop(server);
    let output = std::fs::read_to_string(log_path).unwrap();
    assert!(
        !output.contains(TOKEN) && !output.contains(UNKNOWN_TOKEN),
        "Sift logs contain a test credential"
    );
    let entries: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).expect("structured Sift log"))
        .collect();
    for status in [401, 403, 413, 200] {
        assert!(
            entries.iter().any(|entry| {
                entry["schema"] == "axiom.service.log.v1"
                    && entry["trace_id"] == TRACE_ID
                    && entry["parent_span_id"] == SPAN_ID
                    && entry["attributes"]["target"] == "http.access"
                    && entry["attributes"]["uri"] == "/v1/logs"
                    && entry["attributes"]["status"] == status
            }),
            "no correlated access log for status {status}"
        );
    }
}

// HANDWRITE-END
