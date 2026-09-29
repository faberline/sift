//! External structure contract for the shared Remote Write transport.

#[test]
fn sift_keeps_only_remote_write_domain_conversion() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Sift manifest");
    let prometheus =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/prometheus.rs"))
            .expect("read Sift Prometheus adapter");
    let consumer = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/interfaces/remote_write/remote_write_consumer.rs"
    ))
    .expect("read Sift Remote Write consumer");
    let adapter = prometheus + &consumer;
    let library: String = [
        "/src/lib.rs",
        "/src/app/service_state.rs",
        "/src/app/interfaces/readiness_hook.rs",
        "/src/app/interfaces/metrics_provider.rs",
        "/src/app/interfaces/http/api_error.rs",
        "/src/app/interfaces/http/router.rs",
        "/src/app/interfaces/http/openapi.rs",
    ]
    .iter()
    .map(|path| {
        std::fs::read_to_string(format!("{}{path}", env!("CARGO_MANIFEST_DIR")))
            .expect("read Sift runtime")
    })
    .collect();
    let endpoint = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/interfaces/http/prometheus_remote_write.rs"
    ))
    .expect("read Sift Remote Write endpoint");
    let runtime = library + &endpoint;

    assert!(manifest.contains("metrics-remote-write ="));
    assert!(adapter.contains("pub use metrics_remote_write::{proto as remote"));
    assert!(adapter.contains("impl metrics_remote_write::RemoteWriteConsumer"));
    assert!(!adapter.contains("pub mod remote"));
    assert!(runtime.contains("metrics_remote_write::validate_headers"));
    assert!(runtime.contains("metrics_remote_write::decode_snappy"));
    assert!(!runtime.contains("snap::raw::Decoder"));
}
