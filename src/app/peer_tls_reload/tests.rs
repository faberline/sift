use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use axum::{routing::get, Router};
use raft_runtime::PeerTransport;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};

use super::{spawn_peer_tls_reload, PEER_TLS_RELOAD_INTERVAL};

struct Authority {
    cert: Certificate,
    key: KeyPair,
}

fn authority() -> Authority {
    let mut params = CertificateParams::new(Vec::new()).unwrap();
    params
        .distinguished_name
        .push(DnType::CommonName, "Sift reload test peer CA");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    Authority { cert, key }
}

fn atomic_write(path: &Path, bytes: &[u8]) {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, bytes).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

/// Write a new certificate for `name` signed by `authority` into `dir`, with
/// the three files replaced by write-then-rename.
fn write_material(dir: &Path, authority: &Authority, name: &str) -> peer_tls::PeerTlsConfig {
    let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
    params
        .distinguished_name
        .push(DnType::CommonName, format!("sift-peer-{name}"));
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    let key = KeyPair::generate().unwrap();
    let cert = params
        .signed_by(&key, &authority.cert, &authority.key)
        .unwrap();
    let config = peer_tls::PeerTlsConfig {
        cert: dir.join("tls.crt"),
        key: dir.join("tls.key"),
        ca: dir.join("ca.crt"),
        required: true,
    };
    atomic_write(&config.cert, cert.pem().as_bytes());
    atomic_write(&config.key, key.serialize_pem().as_bytes());
    atomic_write(&config.ca, authority.cert.pem().as_bytes());
    config
}

async fn wait_for_generation(transport: &PeerTransport, expected: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while transport.generation() != expected {
        assert!(
            Instant::now() < deadline,
            "generation stayed at {} instead of {expected}",
            transport.generation()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[test]
fn the_reload_interval_is_thirty_seconds() {
    assert_eq!(PEER_TLS_RELOAD_INTERVAL, Duration::from_secs(30));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_tls_reload_runs_only_when_the_files_change() {
    let ca = authority();
    let dir = tempfile::tempdir().unwrap();
    let config = write_material(dir.path(), &ca, "localhost");
    let transport = PeerTransport::from_config(&config).unwrap();
    assert_eq!(transport.generation(), 1);

    let task = spawn_peer_tls_reload(transport.clone(), config.clone(), Duration::from_millis(50));

    // Unchanged files: no reload.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(transport.generation(), 1);

    // One change: exactly one reload.
    write_material(dir.path(), &ca, "localhost");
    wait_for_generation(&transport, 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(transport.generation(), 2);

    // A broken certificate: the reload fails and the last good generation stays.
    atomic_write(&config.cert, b"not a certificate");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(transport.generation(), 2);

    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_tls_rotation_keeps_requests_ok_and_uses_the_new_certificate() {
    let ca = authority();
    let server_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let server_config = write_material(server_dir.path(), &ca, "localhost");
    let client_config = write_material(client_dir.path(), &ca, "client.local");
    let server = PeerTransport::from_config(&server_config).unwrap();
    let client = PeerTransport::from_config(&client_config).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new().route("/raftz", get(|| async { "authenticated" }));
    let serve_server = server.clone();
    let serve = tokio::spawn(async move {
        serve_server
            .serve(listener, router, std::future::pending::<()>())
            .await
    });
    let reload = spawn_peer_tls_reload(
        server.clone(),
        server_config.clone(),
        Duration::from_millis(50),
    );

    let http = client.http_client();
    let url = format!("https://localhost:{port}/raftz");
    let mut failed = 0;
    for i in 0..300 {
        if i == 100 {
            write_material(server_dir.path(), &ca, "localhost");
        }
        let ok = match http.get(&url).timeout(Duration::from_secs(3)).send().await {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                status == reqwest::StatusCode::OK && body == "authenticated"
            }
            Err(_) => false,
        };
        if !ok {
            failed += 1;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(failed, 0, "no request may fail during the rotation");
    wait_for_generation(&server, 2).await;

    // A request on a fresh connection after the rotation also succeeds.
    let fresh = client.http_client();
    let response = fresh
        .get(&url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "authenticated");

    reload.abort();
    serve.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn peer_tls_without_the_reload_task_keeps_the_old_generation() {
    let ca = authority();
    let dir = tempfile::tempdir().unwrap();
    let config = write_material(dir.path(), &ca, "localhost");
    let transport = PeerTransport::from_config(&config).unwrap();

    write_material(dir.path(), &ca, "localhost");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(transport.generation(), 1);
}
