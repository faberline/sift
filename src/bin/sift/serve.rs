//! `sift serve`: runs the unified h2c/HTTP1 operational-event service. It opens
//! the data root, bootstraps the archive on a fresh volume, starts the projection
//! and archive workers, serves the Raft peers and OTLP/gRPC, mounts the router
//! for its role, and registers each drain and flush stage with the task
//! supervisor.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::extract::DefaultBodyLimit;
use sift::{auth::SiftVerifier, ServiceState};

use crate::serve_args::{LogFormat, RunRole, ServeArgs};

pub(super) async fn serve(args: ServeArgs) -> Result<()> {
    if args.ephemeral && (args.role != RunRole::All || production_environment()) {
        anyhow::bail!(
            "--ephemeral is forbidden for production Sift roles; use writable persistent storage"
        );
    }
    let ephemeral_root = args
        .ephemeral
        .then(|| tempfile::Builder::new().prefix("sift-ephemeral-").tempdir())
        .transpose()
        .context("create explicit ephemeral Sift root")?;
    let data_dir = ephemeral_root
        .as_ref()
        .map(|root| root.path())
        .unwrap_or(args.data_dir.as_path());
    let format = match args.log_format {
        LogFormat::Pretty => service_http::LogFormat::Pretty,
        LogFormat::Json => service_http::LogFormat::Json,
    };
    let config = service_http::HttpConfig::new(
        args.host.clone(),
        args.port,
        args.log_level,
        format,
        args.grace_secs,
        args.max_body_bytes,
        args.otlp_endpoint,
    );
    service_http::init_tracing(&config)?;

    if matches!(args.role, RunRole::All | RunRole::Store) {
        if let Ok(manifest_uri) = std::env::var("SIFT_BOOTSTRAP_ARCHIVE_MANIFEST_URI") {
            if !manifest_uri.trim().is_empty() {
                match sift::storage::archive::bootstrap_gcs_if_needed(&manifest_uri, data_dir)? {
                    Some(manifest) => tracing::info!(
                        source_manifest = manifest_uri,
                        source_cluster_id = manifest.source_cluster_id,
                        event_count = manifest.event_count,
                        "Sift fresh-volume archive bootstrap completed"
                    ),
                    None => tracing::info!(
                        source_manifest = manifest_uri,
                        "Sift archive bootstrap already completed; reusing restored volume"
                    ),
                }
            }
        }
    }

    let state = Arc::new(ServiceState::open_with_role(data_dir, args.role.into())?);
    let grace = Duration::from_secs(config.grace_secs.max(1));
    let reserve = Duration::from_secs((config.grace_secs / 10).min(5));
    let supervisor = server_lifecycle::TaskSupervisor::new(grace, reserve)?;
    let drain_state = state.clone();
    supervisor.register_hook(
        server_lifecycle::HookStage::AdmissionStop,
        "sift-ingest-admission",
        move |_| {
            let drain_state = drain_state.clone();
            async move {
                drain_state.start_drain();
                Ok(())
            }
        },
    )?;

    let projection_worker = Arc::new(tokio::sync::Mutex::new(Some(
        state.start_projection_worker(),
    )));
    let archive_worker = if matches!(args.role, RunRole::All | RunRole::Store) {
        let destination = std::env::var("SIFT_ARCHIVE_DESTINATION")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let interval = std::env::var("SIFT_ARCHIVE_INTERVAL_SECS")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<u64>()
            .context("SIFT_ARCHIVE_INTERVAL_SECS must be a positive integer")?;
        if interval == 0 {
            anyhow::bail!("SIFT_ARCHIVE_INTERVAL_SECS must be greater than zero");
        }
        Some(match destination {
            Some(destination) => {
                state.start_archive_worker(destination, Duration::from_secs(interval))
            }
            None => state.start_local_archive_worker(Duration::from_secs(interval)),
        })
    } else {
        None
    };
    let archive_worker = Arc::new(tokio::sync::Mutex::new(archive_worker));
    let drain_batches = state.clone();
    supervisor.register_hook(
        server_lifecycle::HookStage::DomainQuiesce,
        "sift-ingest-batches",
        move |_| {
            let drain_batches = drain_batches.clone();
            async move {
                drain_batches
                    .finish_drain()
                    .await
                    .map_err(|error| error.to_string())
            }
        },
    )?;
    let stop_archive = archive_worker.clone();
    supervisor.register_hook(
        server_lifecycle::HookStage::BackgroundStop,
        "sift-lifecycle-worker",
        move |_| {
            let stop_archive = stop_archive.clone();
            async move {
                if let Some(worker) = stop_archive.lock().await.take() {
                    worker.stop().await;
                }
                Ok(())
            }
        },
    )?;
    let flush_projections = projection_worker.clone();
    supervisor.register_hook(
        server_lifecycle::HookStage::FinalFlush,
        "sift-projections",
        move |_| {
            let flush_projections = flush_projections.clone();
            async move {
                if let Some(worker) = flush_projections.lock().await.take() {
                    worker.stop().await;
                }
                Ok(())
            }
        },
    )?;
    let verifier = Arc::new(SiftVerifier::from_env().await?);
    if let Some((transport, peer_port, raft_router)) = state.peer_server() {
        let listener = tokio::net::TcpListener::bind((args.host.as_str(), peer_port))
            .await
            .context("bind Sift peer mTLS listener")?;
        let address = listener
            .local_addr()
            .context("read Sift peer mTLS listener address")?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        tracing::info!(%address, "sift serving mutually authenticated Raft peers");
        let task = tokio::spawn(async move {
            transport
                .serve(listener, raft_router, async {
                    let _ = shutdown_rx.await;
                })
                .await
        });
        supervisor.register_oneshot_task(
            server_lifecycle::HookStage::TransportDrain,
            "sift-raft-peer",
            shutdown_tx,
            task,
        )?;
    }
    if matches!(args.role, RunRole::All | RunRole::Gateway | RunRole::Store) {
        let grpc_port = args
            .grpc_port
            .unwrap_or(if args.port == 7380 { 4317 } else { 0 });
        let listener = tokio::net::TcpListener::bind((args.host.as_str(), grpc_port))
            .await
            .context("bind Sift OTLP/gRPC listener")?;
        let address = listener
            .local_addr()
            .context("read OTLP/gRPC listener address")?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let grpc_state = state.clone();
        let grpc_verifier = verifier.clone();
        let grpc_store = (args.role == RunRole::Gateway).then(|| {
            std::env::var("SIFT_STORE_GRPC_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:4317".to_string())
        });
        let maximum_message_bytes = config.body_limit_bytes;
        tracing::info!(%address, "sift serving OTLP/gRPC");
        let task = tokio::spawn(async move {
            if let Some(store) = grpc_store {
                sift::grpc::serve_proxy(
                    listener,
                    &store,
                    grpc_verifier,
                    maximum_message_bytes,
                    async {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
            } else {
                sift::grpc::serve(listener, grpc_state, grpc_verifier, async {
                    let _ = shutdown_rx.await;
                })
                .await
            }
        });
        supervisor.register_oneshot_task(
            server_lifecycle::HookStage::TransportDrain,
            "sift-otlp-grpc",
            shutdown_tx,
            task,
        )?;
    }
    let internal_endpoint = local_http_endpoint(&args.host, args.port);
    let data_plane = match args.role {
        RunRole::Gateway => {
            let store = std::env::var("SIFT_STORE_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:7380".to_string());
            let query = std::env::var("SIFT_QUERY_ENDPOINT").unwrap_or_else(|_| store.clone());
            sift::proxy::gateway_router(&store, &query, config.body_limit_bytes)?
                .merge(sift::mcp::http_router(&internal_endpoint)?)
                .layer(axum::middleware::from_fn_with_state(
                    verifier,
                    sift::auth::auth_middleware,
                ))
        }
        RunRole::Query => {
            let store = std::env::var("SIFT_STORE_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:7380".to_string());
            sift::query_role_router(state.clone(), &store, config.body_limit_bytes)?.layer(
                axum::middleware::from_fn_with_state(verifier, sift::auth::auth_middleware),
            )
        }
        _ => sift::protected_router_with_mcp(state.clone(), verifier, &internal_endpoint)?,
    }
    .layer(DefaultBodyLimit::max(config.body_limit_bytes));
    let app =
        service_http::standard_probe_routes(state.clone(), Some(state.clone()), sift::openapi)
            .merge(data_plane)
            .layer(service_http::trace_layer())
            // Per-request Server-Timing response attribution, composed at
            // the same outermost position as trace_layer() above (#2490).
            .layer(axum::middleware::from_fn(
                service_http::server_timing_middleware,
            ));
    let listener = tokio::net::TcpListener::bind(config.bind_addr())
        .await
        .context("bind Sift service listener")?;
    tracing::info!(address = %config.bind_addr(), "sift serving HTTP/1.1 and h2c");
    let lifecycle = supervisor.lifecycle();
    let signal_supervisor = supervisor.clone();
    let signal_task = tokio::spawn(async move {
        service_http::wait_shutdown_signal().await;
        signal_supervisor
            .shutdown("signal", "Sift shutdown signal received")
            .await
    });
    let http_report = service_http::serve_with_lifecycle(
        listener,
        app,
        service_http::HttpServerOptions::default(),
        lifecycle,
    )
    .await;
    let shutdown_report = signal_task.await.context("join Sift shutdown supervisor")?;
    let failures = shutdown_report
        .outcomes
        .iter()
        .filter(|outcome| outcome.status != server_lifecycle::HookStatus::Completed)
        .map(|outcome| format!("{}: {:?}", outcome.name, outcome.status))
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        anyhow::bail!(
            "Sift shutdown did not complete cleanly: {}",
            failures.join(", ")
        );
    }
    tracing::info!(
        accepted = http_report.accepted,
        completed = http_report.completed,
        failed = http_report.failed,
        timed_out = http_report.timed_out,
        "Sift shared HTTP runtime stopped"
    );
    Ok(())
}

fn production_environment() -> bool {
    std::env::var("SIFT_PRODUCTION").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        )
    }) || std::env::var("SIFT_ENVIRONMENT").ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "prod" | "production"
        )
    })
}

fn local_http_endpoint(host: &str, port: u16) -> String {
    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        host if host.starts_with('[') => host.to_string(),
        host if host.contains(':') => format!("[{host}]"),
        host => host.to_string(),
    };
    format!("http://{host}:{port}")
}
