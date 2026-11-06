// HANDWRITE-BEGIN gap="sift-agent-cli-contract-tests" tracker="1604" reason="Verify CLI output, typed client generation, issue reads, and upgrade checks."
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::Query, http::HeaderMap, routing::get, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

#[test]
fn machine_readable_cli_output_is_valid_json_and_terminal() {
    let output = Command::new(env!("CARGO_BIN_EXE_sift"))
        .args(["llm", "--format", "json"])
        .output()
        .expect("run sift llm");
    assert!(output.status.success(), "{output:?}");

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("machine-readable output must remain JSON");
    assert_eq!(value["next"], "done");
    assert!(value["topics"].is_array());
}

#[test]
fn spec_gen_writes_a_typed_client_entrypoint() {
    let output_dir = tempfile::tempdir().expect("temporary generated-client directory");
    let output = Command::new(env!("CARGO_BIN_EXE_sift"))
        .args([
            "spec",
            "gen",
            "--lang",
            "rust",
            "--out",
            output_dir.path().to_str().expect("utf-8 temporary path"),
        ])
        .output()
        .expect("run sift spec gen");
    assert!(output.status.success(), "{output:?}");
    assert!(output_dir.path().join("mod.rs").is_file());
    assert!(String::from_utf8_lossy(&output.stdout).contains("next:"));
}

#[tokio::test]
async fn issue_search_and_view_use_sift_scope_without_credentials() {
    let courier = Router::new()
        .route(
            "/v1/issues/faberline/sift",
            get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert!(!headers.contains_key("authorization"));
                assert_eq!(
                    query,
                    HashMap::from([
                        ("state".into(), "closed".into()),
                        ("q".into(), "label:\"app:sift\" retry window".into()),
                        ("limit".into(), "3".into()),
                    ])
                );
                Json(json!({"items": [{"number": 52, "state": "closed", "title": "Retry limit"}]}))
            }),
        )
        .route(
            "/v1/issues/faberline/sift/52",
            get(|headers: HeaderMap| async move {
                assert!(!headers.contains_key("authorization"));
                Json(json!({
                    "state": "closed",
                    "title": "Retry limit",
                    "html_url": "https://github.com/faberline/sift/issues/52",
                    "body": "The retry window is bounded."
                }))
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, courier).await.unwrap() });
    let search = sift_stdout(
        isolated_sift()
            .env("AXIOM_COURIER_URL", &url)
            .env("ALL_PROXY", &url)
            .env("NO_PROXY", "127.0.0.1")
            .args([
                "issue", "search", "retry", "window", "--state", "closed", "--limit", "3",
            ]),
    )
    .await;
    let view = sift_stdout(
        isolated_sift()
            .env("AXIOM_COURIER_URL", &url)
            .env("ALL_PROXY", &url)
            .env("NO_PROXY", "127.0.0.1")
            .args(["issue", "view", "52"]),
    )
    .await;
    server.abort();

    assert_eq!(search, "#52 [closed] Retry limit\nnext: done\n");
    assert_eq!(
        view,
        "#52 [closed] Retry limit\nhttps://github.com/faberline/sift/issues/52\n---\nThe retry window is bounded.\nnext: done\n"
    );
}

#[tokio::test]
async fn upgrade_check_reads_the_sift_release_list_without_installing() {
    let temp = tempfile::tempdir().unwrap();
    let certificate_path = temp.path().join("test-root.pem");
    let certificate = rcgen::generate_simple_self_signed(vec!["api.github.com".into()]).unwrap();
    std::fs::write(&certificate_path, certificate.cert.pem()).unwrap();
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![certificate.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der()).into(),
    )
    .unwrap();

    // The shared upgrade client uses a fixed HTTPS origin. Only this child
    // process uses the test CA and proxy, which never opens an upstream socket.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .expect("upgrade check did not use the loopback proxy")
            .unwrap();
        tokio::task::spawn_blocking(move || {
            let mut socket = socket.into_std().unwrap();
            socket.set_nonblocking(false).unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            socket.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let connect = read_request_head(&mut socket);
            assert_eq!(connect.lines().next(), Some("CONNECT api.github.com:443 HTTP/1.1"));
            socket.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").unwrap();

            let connection = rustls::ServerConnection::new(Arc::new(tls)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let request = read_request_head(&mut stream);
            assert_eq!(
                request.lines().next(),
                Some("GET /repos/faberline/sift/releases?per_page=100 HTTP/1.1")
            );
            assert!(!request.lines().any(|line| line
                .split_once(':')
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case("authorization"))));
            let response = r#"[{"tag_name":"sift@9.8.7"},{"tag_name":"sift@10.0.0-rc.1"},{"tag_name":"core@99.0.0"}]"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
            stream.flush().unwrap();
        })
        .await
        .unwrap();
    });

    let check = sift_stdout(
        isolated_sift()
            .env("HTTPS_PROXY", proxy_url)
            .env("SSL_CERT_FILE", certificate_path)
            .args(["upgrade", "--check"]),
    )
    .await;
    server.await.unwrap();

    assert_eq!(
        check,
        concat!(
            "current: ",
            env!("CARGO_PKG_VERSION"),
            "\nlatest:  9.8.7 (sift@9.8.7)\n→ run `sift upgrade` to update\nnext: sift upgrade\n"
        )
    );
}

fn isolated_sift() -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sift"));
    // An empty PATH also prevents the shared CLI from invoking the user's gh.
    command.env_clear().env("PATH", "").kill_on_drop(true);
    command
}

async fn sift_stdout(command: &mut tokio::process::Command) -> String {
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .expect("Sift CLI did not exit")
        .expect("run Sift CLI");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn read_request_head(stream: &mut impl Read) -> String {
    let mut stream = BufReader::new(stream.take(8192));
    let mut head = String::new();
    loop {
        let start = head.len();
        assert_ne!(
            stream.read_line(&mut head).unwrap(),
            0,
            "incomplete request"
        );
        if &head[start..] == "\r\n" {
            return head;
        }
    }
}

// HANDWRITE-END
