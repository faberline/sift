//! Structural contracts that keep shared runtime mechanics out of Sift.

#[test]
fn group_commit_is_owned_by_service_executor() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Sift manifest");
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
            .expect("read Sift library source")
    })
    .collect();
    let coordinator = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/infrastructure/ingest_batch_coordinator.rs"
    ))
    .expect("read Sift ingest batch coordinator");
    let append = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/application/append_events.rs"
    ))
    .expect("read Sift ingest append");
    let drain = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/application/drain_ingest.rs"
    ))
    .expect("read Sift ingest drain");
    let source = library + &coordinator + &append + &drain;

    assert!(
        manifest.contains("service-executor ="),
        "Sift must compose the shared group-commit runtime"
    );
    assert!(
        source.contains("impl service_executor::GroupCommitRequest for IngestBatchRequest"),
        "Sift must provide only its batch data adapter"
    );
    assert!(
        !source.contains("async fn run_ingest_batcher"),
        "Sift must not own the batch timer and fan-out loop"
    );
}

#[test]
fn ingest_admission_mechanics_are_owned_by_service_http() {
    let limits =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ingest/limits.rs"))
            .expect("read Sift ingest limits");
    let controller = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/application/admission_controller.rs"
    ))
    .expect("read Sift ingest admission controller");
    let decoder = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/interfaces/http/request_body_decoder.rs"
    ))
    .expect("read Sift ingest body decoder");
    let limit_values = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/domain/ingest_limits.rs"
    ))
    .expect("read Sift ingest limit values");
    let limits_env = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/infrastructure/ingest_limits_env.rs"
    ))
    .expect("read Sift ingest limits from the environment");
    let error = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/domain/admission_error.rs"
    ))
    .expect("read Sift ingest admission error");
    let source = limits + &controller + &decoder + &limit_values + &limits_env + &error;

    assert!(
        source.contains("service_http::WeightedAdmission"),
        "Sift must compose shared weighted admission"
    );
    assert!(
        source.contains("service_http::decode_request_body"),
        "Sift must compose shared bounded gzip decoding"
    );
    for local_mechanism in [
        "struct ProjectAdmission",
        "impl Drop for AdmissionPermit",
        "GzDecoder",
    ] {
        assert!(
            !source.contains(local_mechanism),
            "Sift must not retain {local_mechanism}"
        );
    }
}

#[test]
fn reverse_proxy_runtime_is_owned_by_service_http() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/operations/interfaces/http/gateway_proxy.rs"
    ))
    .expect("read Sift proxy adapter");

    assert!(
        source.contains("impl service_http::ReverseProxyPolicy for SiftRolePolicy"),
        "Sift must provide only its upstream selection policy"
    );
    for local_mechanism in ["reqwest::Client", "async fn forward", "fn is_hop_header"] {
        assert!(
            !source.contains(local_mechanism),
            "Sift must not retain {local_mechanism}"
        );
    }
}

#[test]
fn persistent_query_job_transitions_use_service_executor() {
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
            .expect("read Sift library source")
    })
    .collect();
    let query_v1 = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/query/interfaces/http/query_v1.rs"
    ))
    .expect("read Sift query handlers");
    let query_role = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/query/interfaces/http/query_role_router.rs"
    ))
    .expect("read Sift query role router");
    let source = library + &query_v1 + &query_role;

    assert!(
        source.contains("service_executor::JobRunner::new"),
        "Sift must run persistent work through the shared job runner"
    );
    assert!(
        !source.contains("let job_store = worker_state.query_jobs.clone();"),
        "Sift must not own query-job transition control flow"
    );
}

#[test]
fn shutdown_order_and_task_failures_use_server_lifecycle() {
    let mut paths: Vec<_> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bin/sift"))
        .expect("list Sift binary source")
        .map(|entry| entry.expect("read Sift binary source entry").path())
        .collect();
    paths.sort();
    let source: String = paths
        .iter()
        .map(|path| std::fs::read_to_string(path).expect("read Sift binary source"))
        .collect();

    assert!(
        source.contains("server_lifecycle::TaskSupervisor::new"),
        "the binary must compose the shared task supervisor"
    );
    assert!(
        source.contains("server_lifecycle::HookStage::FinalFlush"),
        "Sift must map its projection flush into the shared shutdown order"
    );
    assert!(
        !source.contains("if let Some((shutdown, task)) = grpc"),
        "the binary must not hand-roll task shutdown and joins"
    );
}

#[test]
fn scoped_bearer_middleware_is_owned_by_service_auth() {
    let facade = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/auth.rs"))
        .expect("read Sift auth adapter");
    let scoped = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/access/interfaces/http/scoped_authorization.rs"
    ))
    .expect("read Sift scoped authorization");
    let source = facade + &scoped;

    assert!(
        source.contains("impl service_auth::ScopedAuthorization for SiftVerifier"),
        "Sift must provide route, project, and role policy through the shared trait"
    );
    assert!(
        source.contains("pub use service_auth::scoped_authorization_middleware as auth_middleware"),
        "Sift must reuse the shared bearer middleware"
    );
    assert!(
        !source.contains("pub async fn auth_middleware"),
        "Sift must not retain the middleware control flow"
    );
}

#[test]
fn replicated_host_startup_is_owned_by_raft_runtime() {
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
            .expect("read Sift library source")
    })
    .collect();
    let membership = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/journal/infrastructure/raft/sift_membership_policy.rs"
    ))
    .expect("read Sift membership policy");
    let source = library + &membership;

    assert!(
        source.contains("impl raft_runtime::MembershipPolicy for SiftMembershipPolicy"),
        "Sift must provide only its three-voter membership policy"
    );
    assert!(
        source.contains("raft_runtime::ReplicaHostBuilder::new"),
        "Sift must compose the shared replicated-host startup"
    );
    for local_mechanism in [
        "ClusterTopology::from_env_with_scheme",
        "PeerTransport::from_config",
        "RaftStore::open",
        "RaftHost::spawn_with_peer_transport",
    ] {
        assert!(
            !source.contains(local_mechanism),
            "Sift must not retain {local_mechanism}"
        );
    }
}

#[test]
fn kubernetes_workloads_are_owned_by_service_k8s() {
    let source: String = [
        "/src/operations/interfaces/crd/sift_spec.rs",
        "/src/operations/interfaces/operator.rs",
        "/src/operations/infrastructure/k8s/constants.rs",
        "/src/operations/infrastructure/k8s/managed_service.rs",
        "/src/operations/infrastructure/k8s/api_endpoint_discovery.rs",
        "/src/operations/infrastructure/k8s/role_workloads.rs",
        "/src/operations/infrastructure/k8s/agent_workloads.rs",
        "/src/operations/infrastructure/k8s/backup_cron_job.rs",
        "/src/operations/infrastructure/k8s/network_policy.rs",
    ]
    .iter()
    .map(|path| {
        std::fs::read_to_string(format!("{}{path}", env!("CARGO_MANIFEST_DIR")))
            .expect("read Sift operator adapter")
    })
    .collect();

    assert!(
        source.contains("render::WorkloadPlan::new"),
        "Sift must compose one shared typed workload plan"
    );
    assert!(
        source.contains("render::NetworkPolicyPlan::new"),
        "Sift role reachability must use the typed network policy plan"
    );
    for local_renderer in [
        "fn stateful_role(",
        "fn deployment_role(",
        "fn agent_daemonset(",
        "fn disruption_budget(",
        "fn network_policy(",
    ] {
        assert!(
            !source.contains(local_renderer),
            "Sift must not retain {local_renderer}"
        );
    }
}

#[test]
fn live_backup_http_transport_is_owned_by_service_backup() {
    let application = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/operations/application/journal_backup.rs"
    ))
    .expect("read Sift backup adapter");
    let client = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/operations/infrastructure/live_snapshot_client.rs"
    ))
    .expect("read Sift live snapshot client");
    let source = application + &client;

    assert!(
        source.contains("service_backup::AdminSnapshotTransport"),
        "Sift must compose the shared admin snapshot transport"
    );
    assert!(
        source.contains("service_backup::AdminSnapshotRequest"),
        "Sift must provide only its project and credential policy"
    );
    for local_mechanism in [
        "reqwest::Client::builder",
        "ProjectedTokenFile::new",
        "while diagnostic.len()",
    ] {
        assert!(
            !source.contains(local_mechanism),
            "Sift must not retain {local_mechanism}"
        );
    }
}

#[test]
fn logging_text_index_is_owned_by_index_text() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/projection/infrastructure/logging_projection.rs"
    ))
    .expect("read Sift logging projection");

    assert!(source.contains("index_text::"));
    assert!(source.contains("MemoryTextIndex"));
    assert!(!source.contains("lumen::"));
    assert!(!source.contains("CreateCollectionRequest"));
}

#[test]
fn manifest_last_archive_flow_is_owned_by_storage_segment() {
    let facade = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/storage/archive.rs"
    ))
    .expect("read Sift archive adapter");
    let upload = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/archive/application/upload_archive_snapshot.rs"
    ))
    .expect("read Sift archive upload");
    let expire = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/archive/application/expire_committed_events.rs"
    ))
    .expect("read Sift archive expiration");
    let codec = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/archive/infrastructure/parquet_event_codec.rs"
    ))
    .expect("read Sift archive codec");
    let source = facade + &upload + &expire + &codec;

    assert!(source.contains("storage_segment::ArchiveCoordinator"));
    assert!(source.contains("impl storage_segment::RecordCodec<StoredEvent>"));
    assert!(!source.contains("sink.put_object"));
}

#[test]
fn typed_projection_flow_is_owned_by_service_projection() {
    let runtime = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/projection/application/projection_runtime.rs"
    ))
    .expect("read Sift projection runtime");
    let adapter = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/projection/infrastructure/service_projection_adapter.rs"
    ))
    .expect("read Sift projection adapter");
    let source = runtime + &adapter;

    assert!(source.contains("service_projection::ProjectionRegistry"));
    assert!(source.contains("ProjectionHandle<StoredEvent, LoggingProjection>"));
    for local_mechanism in ["downcast_ref", "struct ProjectionSlot", "fn persist("] {
        assert!(
            !source.contains(local_mechanism),
            "Sift must not retain {local_mechanism}"
        );
    }
}

#[test]
fn otlp_wire_and_direct_grpc_runtime_are_owned_by_transport_otlp() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read Sift manifest");
    let normalizer =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ingest/otlp.rs"))
            .expect("read Sift OTLP adapter");
    let grpc_proxy = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/operations/interfaces/grpc/otlp_proxy.rs"
    ))
    .expect("read Sift gRPC adapter");
    let grpc_server = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/interfaces/grpc/otlp_grpc_server.rs"
    ))
    .expect("read Sift OTLP/gRPC server");
    let grpc = grpc_proxy + &grpc_server;

    assert!(manifest.contains("transport-otlp ="));
    assert!(normalizer.contains("pub use transport_otlp::proto as wire"));
    assert!(grpc.contains("transport_otlp::serve_grpc"));
    assert!(grpc.contains("impl transport_otlp::OtlpConsumer for SiftGrpcConsumer"));
    assert!(!std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ingest/otlp/wire.rs"
    ))
    .exists());
}
