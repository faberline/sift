//! The new CLI and HTTP paths run against a real, restarted Sift process.
use chrono::Utc;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tokio::process::{Child, Command};

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn server(root: &Path, port: u16, grpc: u16) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sift"));
    command
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--grpc-port",
            &grpc.to_string(),
            "--data-dir",
            root.to_str().unwrap(),
        ])
        .env("SIFT_AUTH", "off")
        .env("SIFT_CONTROL_PROJECTS", "production-test")
        .env("SIFT_WEBHOOK_ALLOWED_HOSTS", "")
        .env(
            "SIFT_WEBHOOK_SECRET_DIRECTORY",
            root.join("absent-webhooks"),
        )
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    for name in [
        "SIFT_TOKEN",
        "SIFT_TOKEN_FILE",
        "SIFT_ARCHIVE_DESTINATION",
        "SIFT_RECOVERY_DESTINATION",
        "SIFT_QUERY_ENDPOINT",
        "SIFT_STORE_ENDPOINT",
        "SIFT_PEER_URLS",
        "SIFT_NODE_ID",
        "SIFT_VOTER_IDS",
    ] {
        command.env_remove(name);
    }
    let mut child = command.spawn().unwrap();
    let client = reqwest::Client::new();
    for _ in 0..200 {
        if client
            .get(format!("http://127.0.0.1:{port}/readyz"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return child;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "Sift exited before readiness"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("Sift did not become ready");
}
async fn cli(endpoint: &str, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_sift"))
        .args(args)
        .env("SIFT_URL", endpoint)
        .env_remove("SIFT_TOKEN")
        .env_remove("SIFT_TOKEN_FILE")
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
async fn record(endpoint: &str, collection: &str, condition: impl Fn(&Value) -> bool) -> Value {
    let client = reqwest::Client::new();
    for _ in 0..240 {
        let body: Value = client
            .get(format!(
                "{endpoint}/api/v1/{collection}?project=production-test"
            ))
            .header("x-sift-project", "production-test")
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(found) = body["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| condition(r))
        {
            return found.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{collection} did not reach its expected state");
}
fn write(path: &Path, value: Value) {
    std::fs::write(path, format!("{value}\n")).unwrap();
}
#[tokio::test]
async fn collect_query_group_resolve_recur_mute_and_restart_use_public_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("server");
    let port = port();
    let grpc = crate::production_workflow::port();
    let endpoint = format!("http://127.0.0.1:{port}");
    let mut service = server(&root, port, grpc).await;
    let source = temp.path().join("application.jsonl");
    let collector = temp.path().join("collector");
    write(
        &source,
        json!({"level":"ERROR","service":"checkout","message":"request 7 failed",
        "exception":{"type":"Timeout","stacktrace":"checkout handler 91"},"timestamp":Utc::now().to_rfc3339()}),
    );
    let collect = [
        "collect",
        "--source",
        source.to_str().unwrap(),
        "--data-dir",
        collector.to_str().unwrap(),
        "--project",
        "production-test",
        "--environment",
        "prod",
    ];
    assert_eq!(cli(&endpoint, &collect).await["accepted"], 1);
    let group = record(&endpoint, "error-groups", |r| r["value"]["count"] == 1).await;
    assert_eq!(group["value"]["exception_type"], "Timeout");
    assert_eq!(group["value"]["status"], "open");
    let id = group["id"].as_str().unwrap();
    let revision = group["revision"].as_u64().unwrap().to_string();
    cli(
        &endpoint,
        &[
            "control",
            "error-groups",
            "status",
            id,
            "--project",
            "production-test",
            "--request-id",
            "resolve-1",
            "--expected-revision",
            &revision,
            "resolved",
        ],
    )
    .await;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&source)
        .unwrap();
    writeln!(file,"{}",json!({"level":"ERROR","service":"checkout","message":"request 42 failed",
        "exception":{"type":"Timeout","stacktrace":"checkout handler 92"},"timestamp":Utc::now().to_rfc3339()})).unwrap();
    assert_eq!(cli(&endpoint, &collect).await["accepted"], 1);
    let reopened = record(&endpoint, "error-groups", |r| r["value"]["count"] == 2).await;
    assert_eq!(reopened["id"], id);
    assert_eq!(reopened["value"]["status"], "open");
    assert_eq!(reopened["value"]["recurrence_count"], 1);
    let time = Utc::now().timestamp_nanos_opt().unwrap();
    let metric = json!({"resourceMetrics":[{"resource":{"attributes":[
        {"key":"service.name","value":{"stringValue":"checkout"}},
        {"key":"deployment.environment.name","value":{"stringValue":"prod"}}]},
        "scopeMetrics":[{"metrics":[{"name":"requests_total","sum":{"aggregationTemporality":2,
            "isMonotonic":true,"dataPoints":[{"timeUnixNano":time.to_string(),"asDouble":20.0}]}}]}]}]});
    let response = reqwest::Client::new()
        .post(format!("{endpoint}/v1/metrics"))
        .header("x-sift-project", "production-test")
        .json(&metric)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    // One point sent over all three transports must stay one stored point.
    use transport_otlp::proto as wire;
    let attribute = |key: &str, value: &str| wire::KeyValue {
        key: key.into(),
        value: Some(wire::AnyValue {
            value: Some(wire::any_value::Value::StringValue(value.into())),
        }),
        ..Default::default()
    };
    let proto = wire::ExportMetricsServiceRequest {
        resource_metrics: vec![wire::ResourceMetrics {
            resource: Some(wire::Resource {
                attributes: vec![
                    attribute("service.name", "checkout"),
                    attribute("deployment.environment.name", "prod"),
                ],
                ..Default::default()
            }),
            scope_metrics: vec![wire::ScopeMetrics {
                metrics: vec![wire::Metric {
                    name: "requests_total".into(),
                    data: Some(wire::metric::Data::Sum(wire::Sum {
                        aggregation_temporality: 2,
                        is_monotonic: true,
                        data_points: vec![wire::NumberDataPoint {
                            time_unix_nano: time as u64,
                            value: Some(wire::number_data_point::Value::AsDouble(20.0)),
                            ..Default::default()
                        }],
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    };
    use prost14::Message;
    assert!(reqwest::Client::new()
        .post(format!("{endpoint}/v1/metrics"))
        .header("x-sift-project", "production-test")
        .header("Content-Type", "application/x-protobuf")
        .body(proto.encode_to_vec())
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let mut grpc_client=opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient::connect(
        format!("http://127.0.0.1:{grpc}")).await.unwrap();
    let mut grpc_request = tonic::Request::new(proto);
    grpc_request
        .metadata_mut()
        .insert("x-sift-project", "production-test".parse().unwrap());
    grpc_client.export(grpc_request).await.unwrap();
    let raw: Value = reqwest::Client::new()
        .post(format!("{endpoint}/api/v1/query"))
        .header("x-sift-project", "production-test")
        .json(&json!({"version":1,"project":"production-test",
            "signal":{"kind":"metrics","name":"requests_total","function":"raw"}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(raw["data"]["series"].as_array().unwrap().len(), 1);
    assert_eq!(
        raw["data"]["series"][0]["points"].as_array().unwrap().len(),
        1
    );
    let query = temp.path().join("query.json");
    write(
        &query,
        json!({"version":2,"project":"production-test","signal":{"kind":"metrics","expression":"sum(requests_total)"}}),
    );
    let answer = cli(&endpoint, &["query", query.to_str().unwrap()]).await;
    assert_eq!(answer["data"]["result"][0]["value"][1], "20");
    assert_eq!(answer["partial"], false);
    let saved = temp.path().join("saved.json");
    write(
        &saved,
        json!({"project":"production-test","request_id":"save-1","expected_revision":0,
        "value":{"request":{"version":2,"project":"production-test","signal":{"kind":"metrics","expression":"sum(requests_total)"}}}}),
    );
    cli(
        &endpoint,
        &[
            "control",
            "saved-queries",
            "put",
            "q",
            saved.to_str().unwrap(),
        ],
    )
    .await;
    let rule = temp.path().join("rule.json");
    write(
        &rule,
        json!({"project":"production-test","request_id":"rule-1","expected_revision":0,"value":{
        "version":1,"project":"production-test","environment":"prod","source":{"kind":"metrics",
            "expression":"sum(requests_total)","comparison":"greater","threshold":10.0},"webhook":"validation"}}),
    );
    cli(
        &endpoint,
        &["control", "alert-rules", "put", "r", rule.to_str().unwrap()],
    )
    .await;
    let alert = record(&endpoint, "alerts", |r| r["value"]["phase"] == "firing").await;
    let alert_id = alert["id"].as_str().unwrap();
    let revision = alert["revision"].as_u64().unwrap().to_string();
    let mute = (Utc::now().timestamp() + 60).to_string();
    cli(
        &endpoint,
        &[
            "control",
            "alerts",
            "mute",
            alert_id,
            "--project",
            "production-test",
            "--request-id",
            "mute-1",
            "--expected-revision",
            &revision,
            "--until",
            &mute,
        ],
    )
    .await;
    service.kill().await.unwrap();
    service.wait().await.unwrap();
    let mut service = server(&root, port, grpc).await;
    let restored = record(&endpoint, "error-groups", |r| r["id"] == id).await;
    assert_eq!(restored["value"]["count"], 2);
    assert_eq!(restored["value"]["recurrence_count"], 1);
    let alerts = cli(
        &endpoint,
        &["control", "alerts", "list", "--project", "production-test"],
    )
    .await;
    assert_eq!(alerts["records"][0]["id"], alert_id);
    assert_eq!(
        alerts["records"][0]["value"]["mute_until"],
        mute.parse::<i64>().unwrap()
    );
    let saved = cli(
        &endpoint,
        &[
            "control",
            "saved-queries",
            "list",
            "--project",
            "production-test",
        ],
    )
    .await;
    assert_eq!(saved["records"][0]["id"], "q");
    assert_eq!(
        cli(&endpoint, &["query", query.to_str().unwrap()]).await["data"]["result"][0]["value"][1],
        "20"
    );
    service.kill().await.unwrap();
    service.wait().await.unwrap();
}
