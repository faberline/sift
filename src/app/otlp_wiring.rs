//! Compose the public OTLP servers with access's application service.

use crate::access::application::authorization_service::AuthorizationService;
use crate::access::infrastructure::sift_verifier::SiftVerifier;
use crate::app::service_state::ServiceState;
use std::future::Future;
use std::sync::Arc;

pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    state: Arc<ServiceState>,
    verifier: Arc<SiftVerifier>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    crate::ingest::interfaces::grpc::otlp_grpc_server::serve(
        listener,
        state,
        Arc::new(AuthorizationService::from_verifier(verifier)),
        shutdown,
    )
    .await
}

pub async fn serve_proxy<F>(
    listener: tokio::net::TcpListener,
    store_endpoint: &str,
    verifier: Arc<SiftVerifier>,
    maximum_message_bytes: usize,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    crate::operations::interfaces::grpc::otlp_proxy::serve_proxy(
        listener,
        store_endpoint,
        Arc::new(AuthorizationService::from_verifier(verifier)),
        maximum_message_bytes,
        shutdown,
    )
    .await
}
