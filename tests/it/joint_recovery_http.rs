//! Public reads after a real joint GCS restore.
use serde_json::{json, Value};
use std::time::Duration;

pub async fn verify_restored_process(root: &std::path::Path) {
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let endpoint = format!("http://127.0.0.1:{port}");
    let diagnostics = root.parent().unwrap().join("restore-server.log");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sift"))
        .args([
            "serve",
            "--role",
            "store",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--data-dir",
        ])
        .arg(root)
        .env_remove("SIFT_RECOVERY_DESTINATION")
        .env("SIFT_ARCHIVE_INTERVAL_SECS", "3600")
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&diagnostics).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let http = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..100 {
        if let Ok(response) = http
            .post(format!("{endpoint}/api/v1/query"))
            .json(&json!({"version":1,"project":"recovery","signal":{"kind":"logs"}}))
            .send()
            .await
        {
            if response.status().is_success() {
                let value: Value = response.json().await.unwrap();
                if value["partial"] == false
                    && value["data"]["records"].as_array().unwrap().len() == 1
                {
                    ready = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        ready,
        "restored serving process: {}",
        std::fs::read_to_string(&diagnostics).unwrap()
    );
    let metric: Value = http.post(format!("{endpoint}/api/v2/query"))
        .json(&json!({"version":2,"project":"recovery","signal":{"kind":"metrics","expression":"queue_depth"}}))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(metric["data"]["result"][0]["value"][1], "3");
    let trace_reply = http
        .get(format!(
            "{endpoint}/api/v1/traces/{}?project=recovery",
            "1".repeat(32)
        ))
        .send()
        .await
        .unwrap();
    let status = trace_reply.status();
    let trace: Value = trace_reply.json().await.unwrap();
    assert!(
        status.is_success(),
        "trace {status}: {trace}; server: {}",
        std::fs::read_to_string(&diagnostics).unwrap()
    );
    assert_eq!(trace["spans"].as_array().unwrap().len(), 1);
    let services: Value = http
        .get(format!("{endpoint}/api/v1/services?project=recovery"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(services["services"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "checkout"));
    let correlated: Value = http
        .post(format!("{endpoint}/api/v1/correlate"))
        .json(&json!({"version":1,"project":"recovery","service":"checkout"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(correlated["logs"].as_array().unwrap().len(), 1);
    let tail: Value = http
        .post(format!("{endpoint}/api/v1/logs/tail"))
        .json(&json!({"version":1,"project":"recovery","wait_ms":1}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tail["data"]["records"].as_array().unwrap().len(), 1);
    let integrity: Value = http
        .get(format!("{endpoint}/admin/integrity?project=recovery"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(integrity["event_count"], 3);
    let backup = tokio::process::Command::new(env!("CARGO_BIN_EXE_sift"))
        .args([
            "backup",
            "--url",
            &endpoint,
            "--dest",
            "gs://sift-joint-test/live-backup",
        ])
        .output()
        .await
        .unwrap();
    assert!(
        backup.status.success(),
        "live GCS backup: {}",
        String::from_utf8_lossy(&backup.stderr)
    );
    child.start_kill().unwrap();
    child.wait().await.unwrap();
}
