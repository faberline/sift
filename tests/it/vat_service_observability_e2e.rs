// HANDWRITE-BEGIN gap="missing-generator:unit-test:vat-service-observability-e2e" tracker="pending-tracker" reason="Prove Lumen, Tape, Relay, and Defer compose with VAT capture and the Sift-owned collector through the shared stdout contract."
#![cfg(unix)]

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const SERVICES: [&str; 4] = ["lumen", "tape", "relay", "defer"];
const PARENT_SPAN: &str = "b7ad6b7169203331";
const CANARY: &str = "sift-vat-synthetic-authorization-canary";
// Core drops some Defer access logs under concurrent probes (faberline/sift#18, Core 1c8507e); remove the retry when the pinned Core has the repair.
const DEFER_ATTEMPTS: u32 = 3;

fn debug_binary(name: &str) -> PathBuf {
    if name == "sift" {
        return PathBuf::from(env!("CARGO_BIN_EXE_sift"));
    }
    if let Some(directory) = std::env::var_os("SIFT_VAT_BIN_DIR") {
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute(), "SIFT_VAT_BIN_DIR must be absolute");
        return directory.join(name);
    }
    // The other services come from their own repositories, installed on PATH.
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| PathBuf::from(name))
}

fn require_binary(path: &Path) {
    assert!(
        path.is_file(),
        "missing binary {}; install vat, lumen, tape, relay and defer from their faberline repositories",
        path.display()
    );
}

fn jsonl(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("VAT stdout JSONL"))
        .collect()
}

#[test]
#[ignore = "needs vat, lumen, tape, relay and defer installed on PATH"]
fn vat_managed_service_stdout_reaches_real_sift_query() {
    let vat = debug_binary("vat");
    let lumen = debug_binary("lumen");
    let tape = debug_binary("tape");
    let relay = debug_binary("relay");
    let defer = debug_binary("defer");
    let sift = debug_binary("sift");
    for binary in [&vat, &lumen, &tape, &relay, &defer, &sift] {
        require_binary(binary);
    }
    let probe = std::env::current_exe().expect("current E2E test executable");

    let project = tempfile::tempdir().unwrap();
    let vat_home = tempfile::tempdir().unwrap();
    let config = format!(
        r#"
version = 1
name = "service-sift-observability"

[workspace]
base = "."
workdir = "."
keep = "never"

[env]
LUMEN_AUTH = "off"
TAPE_AUTH = "off"
RELAY_AUTH = "off"
DEFER_AUTH = "off"
SIFT_AUTH = "off"
VAT_SERVICE_OBSERVABILITY_PROBE = "1"
RUST_LOG = "info"

[[services]]
id = "lumen"
cmd = [{lumen}, "serve", "--host", "127.0.0.1", "--port", "{{port}}", "--wal", "embedded", "--log-format", "json"]
ready_http = "http://127.0.0.1:{{port}}/readyz"
export = {{ LUMEN_URL = "http://{{host}}:{{port}}" }}
timeout_s = 30

[[services]]
id = "tape"
cmd = [{tape}, "serve", "--bind", "127.0.0.1:{{port}}", "--store", "tape.json", "--log-format", "json"]
ready_http = "http://127.0.0.1:{{port}}/readyz"
export = {{ TAPE_URL = "http://{{host}}:{{port}}" }}
timeout_s = 30

[[services]]
id = "relay"
cmd = [{relay}, "--bind", "127.0.0.1:{{port}}", "--data-dir", "relay-data", "--log-format", "json"]
ready_http = "http://127.0.0.1:{{port}}/readyz"
export = {{ RELAY_URL = "http://{{host}}:{{port}}" }}
timeout_s = 30

[[services]]
id = "defer"
cmd = [{defer}, "serve", "--bind", "127.0.0.1:{{port}}", "--data-dir", "defer-data", "--log-format", "json"]
ready_http = "http://127.0.0.1:{{port}}/readyz"
export = {{ DEFER_URL = "http://{{host}}:{{port}}" }}
timeout_s = 30

[[services]]
id = "sift"
cmd = [{sift}, "serve", "--host", "127.0.0.1", "--port", "{{port}}", "--grpc-port", "0", "--data-dir", "sift-data", "--log-level", "warn", "--log-format", "json"]
ready_http = "http://127.0.0.1:{{port}}/readyz"
export = {{ SIFT_URL = "http://{{host}}:{{port}}" }}
timeout_s = 30

[[runners]]
id = "observability"
requires = ["lumen", "tape", "relay", "defer", "sift"]
cmd = [{probe}, "--exact", {probe_test}, "--nocapture"]
timeout_s = 90
"#,
        lumen = serde_json::to_string(&lumen.to_string_lossy()).unwrap(),
        tape = serde_json::to_string(&tape.to_string_lossy()).unwrap(),
        relay = serde_json::to_string(&relay.to_string_lossy()).unwrap(),
        defer = serde_json::to_string(&defer.to_string_lossy()).unwrap(),
        sift = serde_json::to_string(&sift.to_string_lossy()).unwrap(),
        probe = serde_json::to_string(&probe.to_string_lossy()).unwrap(),
        probe_test = serde_json::to_string(&crate::support::test_name(
            module_path!(),
            "vat_runner_collects_every_service"
        ))
        .unwrap(),
    );
    std::fs::write(project.path().join("vat.toml"), config).unwrap();

    let output = Command::new(&vat)
        .current_dir(project.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", project.path())
        .env("VAT_HOME", vat_home.path())
        .args(["run", "observability"])
        .output()
        .expect("run real VAT service observability journey");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let events = jsonl(&output.stdout);
    let result = events
        .iter()
        .find(|event| event["type"] == "result")
        .expect("VAT result event");
    let vat_id = result["id"].as_str().expect("VAT id");
    assert!(
        !vat_home.path().join("vats").join(vat_id).exists(),
        "VAT workspace survived the run"
    );
    let endpoint_file = project.path().join("owned-endpoints.json");
    if let Ok(bytes) = std::fs::read(&endpoint_file) {
        let endpoints: Vec<SocketAddr> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(endpoints.len(), SERVICES.len() + 1);
        for endpoint in endpoints {
            assert!(endpoint.ip().is_loopback());
            let error = TcpStream::connect_timeout(&endpoint, Duration::from_millis(300))
                .expect_err("VAT service still listening");
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionRefused);
        }
    } else {
        assert!(
            !output.status.success(),
            "runner did not report owned endpoints"
        );
    }
    assert!(
        output.status.success(),
        "VAT journey failed: {}\n{}\n{}",
        stdout.replace(CANARY, "[redacted]"),
        String::from_utf8_lossy(&output.stderr).replace(CANARY, "[redacted]"),
        std::fs::read_to_string(project.path().join("runner-failure.txt")).unwrap_or_default()
    );
    assert!(
        !stdout.contains("axiom.service.log.v1"),
        "VAT replayed child telemetry into its own stdout"
    );
    assert!(!stdout.contains(CANARY), "VAT stdout leaked authorization");
    assert_eq!(result["ok"], true);
    assert_eq!(result["state"], "removed");
    let proof = std::fs::read_to_string(project.path().join("observability-proof.json")).unwrap();
    assert!(!proof.contains(CANARY));
    let mut proof: Value = serde_json::from_str(&proof).unwrap();
    assert_eq!(proof["records"].as_array().unwrap().len(), 16);
    project.close().expect("remove isolated VAT fixture");
    vat_home.close().expect("remove isolated VAT home");
    proof["result"] = json!("passed");
    println!("{proof}");
    println!("VAT workspace removed and all five loopback listeners closed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vat_runner_collects_every_service() {
    if std::env::var_os("VAT_SERVICE_OBSERVABILITY_PROBE").is_none() {
        return;
    }

    let proof_root = PathBuf::from(std::env::var_os("VAT_CONFIG_ROOT").unwrap());
    let failure_path = proof_root.join("runner-failure.txt");
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(
            &failure_path,
            info.to_string().replace(CANARY, "[redacted]"),
        );
    }));
    let sift_url = local_endpoint("SIFT");
    let logs = PathBuf::from(std::env::var_os("VAT_LOGS_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let producers: Vec<_> = SERVICES
        .into_iter()
        .map(|name| Producer {
            name,
            endpoint: local_endpoint(name),
            stdout: capture_path(&logs, name, "STDOUT"),
            stderr: capture_path(&logs, name, "STDERR"),
        })
        .collect();
    let endpoints: std::collections::BTreeSet<_> = producers
        .iter()
        .map(|producer| loopback_addr(&producer.endpoint))
        .chain(std::iter::once(loopback_addr(&sift_url)))
        .collect();
    assert_eq!(endpoints.len(), SERVICES.len() + 1);
    write_json(&proof_root.join("owned-endpoints.json"), json!(endpoints));

    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let baseline = exercise_producers(&client, &producers, 0).await;
    let offsets: Vec<_> = producers
        .iter()
        .map(|producer| {
            let summary = collect(producer, &sift_url);
            assert!(summary["accepted"].as_u64().unwrap() >= 2);
            assert_eq!(summary["start_offset"], 0);
            assert_eq!(summary["rejected"], 0);
            summary["final_offset"].as_u64().unwrap()
        })
        .collect();
    let mut proof = Vec::new();
    for request in &baseline {
        proof.push(verify_sift_record(&client, &sift_url, request).await);
    }

    let backlog = exercise_producers(&client, &producers, 1).await;
    // Reserve an endpoint that never acknowledges a batch.
    let stalled = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let stalled_url = format!("http://{}", stalled.local_addr().unwrap());
    for producer in &producers {
        let checkpoint = checkpoint_path(producer);
        let before = read_checkpoint(&checkpoint);
        let source_len = std::fs::metadata(&producer.stdout).unwrap().len();
        let failed = run_collector(producer, &checkpoint, &stalled_url, "1");
        let stderr = String::from_utf8_lossy(&failed.stderr);
        assert!(
            !failed.status.success(),
            "{} outage returned success",
            producer.name
        );
        assert!(!stderr.contains(CANARY), "collector logged authorization");
        assert!(
            stderr.contains("Sift ingest transport error"),
            "{} did not hit stalled receiver",
            producer.name
        );
        assert_eq!(
            std::fs::metadata(&producer.stdout).unwrap().len(),
            source_len
        );
        assert_outage_checkpoint(before, read_checkpoint(&checkpoint), source_len);
    }
    drop(stalled);
    for request in &backlog {
        assert!(query_by_trace(&client, &sift_url, &request.trace)
            .await
            .is_empty());
    }

    for (producer, offset) in producers.iter().zip(offsets) {
        let resumed = collect(producer, &sift_url);
        assert_eq!(resumed["start_offset"], offset);
        assert!(resumed["accepted"].as_u64().unwrap() >= 2);
        assert!(resumed["final_offset"].as_u64().unwrap() > offset);
        assert_eq!(resumed["rejected"], 0);
        for capture in [&producer.stdout, &producer.stderr] {
            let text = std::fs::read_to_string(capture).expect("read VAT capture");
            assert!(
                !text.contains(CANARY),
                "{} capture leaked authorization",
                producer.name
            );
        }
    }
    for request in &backlog {
        proof.push(verify_sift_record(&client, &sift_url, request).await);
    }
    for producer in &producers {
        // A separate checkpoint replays the same source with the same event IDs.
        let replay_checkpoint = PathBuf::from(format!("{}.replay.checkpoint.json", producer.name));
        let replay = collect_from(producer, &replay_checkpoint, &sift_url);
        assert_eq!(replay["start_offset"], 0);
        assert!(replay["lines"].as_u64().unwrap() >= 4);
        assert_eq!(replay["rejected"], 0);
    }
    for (request, original) in baseline.iter().chain(&backlog).zip(&proof) {
        assert_eq!(
            &verify_sift_record(&client, &sift_url, request).await,
            original
        );
    }
    write_json(
        &proof_root.join("observability-proof.json"),
        json!({
            "schema": "sift.local-service-observability-proof.v1",
            "producers": SERVICES.len(),
            "successful_requests": proof.iter().filter(|event| event["status"] == 200).count(),
            "rejected_requests": proof.iter().filter(|event| event["status"].as_u64().unwrap() >= 400).count(),
            "unchanged_resume_checkpoints_after_outage": SERVICES.len(),
            "replayed_sources": SERVICES.len(),
            "records": proof,
        }),
    );
}

struct Producer {
    name: &'static str,
    endpoint: String,
    stdout: PathBuf,
    stderr: PathBuf,
}

struct ObservedRequest {
    service: &'static str,
    trace: String,
    span: String,
    status: u16,
}

fn local_endpoint(service: &str) -> String {
    let endpoint = std::env::var(format!("{}_URL", service.to_ascii_uppercase())).unwrap();
    loopback_addr(&endpoint);
    endpoint
}

fn loopback_addr(endpoint: &str) -> SocketAddr {
    let address: SocketAddr = endpoint.strip_prefix("http://").unwrap().parse().unwrap();
    assert!(
        address.ip().is_loopback(),
        "unexpected non-loopback VAT endpoint"
    );
    address
}

fn capture_path(logs: &Path, service: &str, channel: &str) -> PathBuf {
    let key = format!("VAT_SERVICE_{}_{channel}_LOG", service.to_ascii_uppercase());
    let path = PathBuf::from(std::env::var_os(&key).unwrap_or_else(|| panic!("missing {key}")));
    let path = path.canonicalize().expect("resolve VAT capture path");
    assert!(path.starts_with(logs) && path.is_file());
    path
}

fn write_json(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

async fn exercise_producers(
    client: &reqwest::Client,
    producers: &[Producer],
    phase: usize,
) -> Vec<ObservedRequest> {
    use reqwest::Method;

    let defer_policy = r#"{"max_in_flight":1,"max_dispatch_per_tick":1,"max_dispatches_per_second":1,"max_burst_size":1,"lease_ttl_ms":30000,"retry_backoff_ms":1000}"#;
    let mut observed = Vec::new();
    for (index, producer) in producers.iter().enumerate() {
        let (method, path, body) = match producer.name {
            "lumen" => (Method::GET, "/collections", ""),
            "tape" => (Method::GET, "/topics/vat-producer/replay", ""),
            "relay" => (Method::GET, "/v1/vat-producer/len", ""),
            "defer" => (Method::PUT, "/v1/queues/vat-producer", defer_policy),
            _ => unreachable!("unexpected producer"),
        };
        for (outcome, status) in [200, 401].into_iter().enumerate() {
            let attempts = if producer.name == "defer" {
                DEFER_ATTEMPTS
            } else {
                1
            };
            for attempt in 0..attempts {
                let trace = format!(
                    "{:032x}",
                    1 + phase * 16 + index * 2 + outcome + (attempt as usize) * 0x1000
                );
                let mut request = client
                    .request(method.clone(), format!("{}{}", producer.endpoint, path))
                    .header("traceparent", format!("00-{trace}-{PARENT_SPAN}-01"));
                if status == 401 {
                    request = request.bearer_auth(CANARY);
                }
                if !body.is_empty() {
                    request = request
                        .header("content-type", "application/json")
                        .body(body);
                }
                let response = request.send().await.unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    status,
                    "{} {path}",
                    producer.name
                );
                assert!(!response.text().await.unwrap().contains(CANARY));
                let last_attempt = attempt + 1 == attempts;
                let span = if producer.name == "defer" && !last_attempt {
                    try_access_log(producer, &trace, status, Duration::from_secs(5)).await
                } else {
                    Some(wait_for_access_log(producer, &trace, status).await)
                };
                if let Some(span) = span {
                    observed.push(ObservedRequest {
                        service: producer.name,
                        trace,
                        span,
                        status,
                    });
                    break;
                }
            }
        }
    }
    observed
}

async fn wait_for_access_log(producer: &Producer, trace: &str, status: u16) -> String {
    try_access_log(producer, trace, status, Duration::from_secs(10))
        .await
        .unwrap_or_else(|| panic!("missing {} access log for trace {trace}", producer.name))
}

async fn try_access_log(
    producer: &Producer,
    trace: &str,
    status: u16,
    wait: Duration,
) -> Option<String> {
    let deadline = Instant::now() + wait;
    loop {
        let source = std::fs::read_to_string(&producer.stdout).expect("read VAT stdout");
        assert!(
            !source.contains(CANARY),
            "{} stdout leaked authorization",
            producer.name
        );
        if let Some(event) = source
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|event| {
                event["trace_id"] == trace && event["attributes"]["target"] == "http.access"
            })
        {
            assert_eq!(event["schema"], "axiom.service.log.v1");
            assert_eq!(event["service"]["name"], producer.name);
            assert_eq!(event["attributes"]["status"], status);
            assert_eq!(event["parent_span_id"], PARENT_SPAN);
            assert_eq!(event["trace_flags"], "01");
            let span = event["span_id"].as_str().expect("local response span");
            assert!(span.len() == 16 && span != PARENT_SPAN && span != "0000000000000000");
            return Some(span.to_owned());
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn defer_producer_with_stdout(root: &tempfile::TempDir, stdout: &str) -> Producer {
    let stdout_path = root.path().join("defer.stdout.jsonl");
    let stderr_path = root.path().join("defer.stderr.log");
    std::fs::write(&stdout_path, stdout).unwrap();
    std::fs::write(&stderr_path, "").unwrap();
    Producer {
        name: "defer",
        endpoint: String::new(),
        stdout: stdout_path,
        stderr: stderr_path,
    }
}

#[tokio::test]
async fn access_log_wait_gives_up_when_trace_never_appears() {
    let root = tempfile::tempdir().unwrap();
    let producer = defer_producer_with_stdout(
        &root,
        "{\"trace_id\":\"00000000000000000000000000000001\",\"attributes\":{\"target\":\"http.access\",\"status\":200}}\n",
    );
    let span = try_access_log(
        &producer,
        "00000000000000000000000000000007",
        200,
        Duration::from_millis(150),
    )
    .await;
    assert_eq!(span, None);
}

#[tokio::test]
async fn access_log_wait_returns_span_of_matching_trace() {
    let root = tempfile::tempdir().unwrap();
    let producer = defer_producer_with_stdout(
        &root,
        "{\"schema\":\"axiom.service.log.v1\",\"service\":{\"name\":\"defer\"},\"trace_id\":\"00000000000000000000000000000007\",\"parent_span_id\":\"b7ad6b7169203331\",\"span_id\":\"1111111111111111\",\"trace_flags\":\"01\",\"attributes\":{\"target\":\"http.access\",\"status\":200}}\n",
    );
    let span = try_access_log(
        &producer,
        "00000000000000000000000000000007",
        200,
        Duration::from_millis(150),
    )
    .await;
    assert_eq!(span, Some("1111111111111111".to_owned()));
}

fn checkpoint_path(producer: &Producer) -> PathBuf {
    PathBuf::from(format!("{}.collector.checkpoint.json", producer.name))
}

fn read_checkpoint(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read collector checkpoint"))
        .expect("decode collector checkpoint")
}

fn assert_outage_checkpoint(before: Value, mut after: Value, source_len: u64) {
    let files = before["files"]
        .as_object()
        .expect("checkpoint file identities");
    assert_eq!(files.len(), 1, "one file identity per producer");
    let (identity, file) = files.iter().next().unwrap();
    let prior_len = file["observed_len"]
        .as_u64()
        .expect("prior observed length");
    assert!(source_len > prior_len, "backlog must grow the source file");
    assert_eq!(after["files"][identity]["observed_len"], json!(source_len));

    // Discovery records the observed end before sending. No failed send may
    // change the acknowledged cursor, counters, anchor, or any other field.
    after["files"][identity]["observed_len"] = json!(prior_len);
    assert_eq!(
        after, before,
        "failed delivery changed acknowledged progress"
    );
}

fn run_collector(producer: &Producer, checkpoint: &Path, endpoint: &str, timeout: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sift"))
        .args(["collect", "--source"])
        .arg(&producer.stdout)
        .args([
            "--source-id",
            &format!("vat:{}:stdout", producer.name),
            "--endpoint",
            endpoint,
            "--project",
            "local",
            "--environment",
            "test",
            "--checkpoint",
        ])
        .arg(checkpoint)
        .args([
            "--quarantine",
            &format!("{}.collector.rejected.jsonl", producer.name),
            "--max-retries",
            "0",
            "--request-timeout-secs",
            timeout,
        ])
        .output()
        .unwrap_or_else(|error| panic!("start {} collector: {error}", producer.name))
}

fn collect(producer: &Producer, endpoint: &str) -> Value {
    collect_from(producer, &checkpoint_path(producer), endpoint)
}

fn collect_from(producer: &Producer, checkpoint: &Path, endpoint: &str) -> Value {
    let output = run_collector(producer, checkpoint, endpoint, "3");
    assert!(!String::from_utf8_lossy(&output.stderr).contains(CANARY));
    assert!(
        output.status.success(),
        "{} collector did not deliver",
        producer.name
    );
    serde_json::from_slice(&output.stdout).expect("collector summary")
}

async fn query_by_trace(client: &reqwest::Client, endpoint: &str, trace: &str) -> Vec<Value> {
    let page: Value = client
        .post(format!("{endpoint}/api/v1/query"))
        .json(&json!({
            "version": 1,
            "project": "local",
            "environment": "test",
            "signal": {"kind": "logs", "filter": {"op": "eq", "field": "trace_id", "value": trace}},
            "limit": 10
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .expect("Sift accepts trace-filtered logs query")
        .json()
        .await
        .unwrap();
    assert!(
        !page.to_string().contains(CANARY),
        "Sift query leaked authorization"
    );
    assert_eq!(page["partial"], false);
    page["data"]["records"]
        .as_array()
        .expect("Sift log records")
        .clone()
}

async fn verify_sift_record(
    client: &reqwest::Client,
    endpoint: &str,
    request: &ObservedRequest,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let records = query_by_trace(client, endpoint, &request.trace).await;
        let mut access = records
            .iter()
            .filter(|record| record["attributes"]["target"]["value"] == "http.access");
        if let Some(record) = access.next() {
            assert!(
                access.next().is_none(),
                "duplicate access log for {}",
                request.trace
            );
            assert_eq!(record["trace_id"], request.trace);
            assert_eq!(record["span_id"], request.span);
            assert_eq!(record["resource"]["service.name"], request.service);
            assert_eq!(record["attributes"]["parent_span_id"]["value"], PARENT_SPAN);
            assert_eq!(record["attributes"]["trace.flags"]["value"], "01");
            assert_eq!(record["attributes"]["status"]["value"], request.status);
            assert_eq!(record["attributes"]["target"]["value"], "http.access");
            let event = record["event_id"].as_str().expect("durable Sift event id");
            assert!(!event.is_empty());
            return json!({"service": request.service, "trace_id": request.trace, "event_id": event, "status": request.status});
        }
        assert!(
            Instant::now() < deadline,
            "Sift did not project {} trace {}",
            request.service,
            request.trace
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
// HANDWRITE-END
