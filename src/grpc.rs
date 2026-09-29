//! The OTLP/gRPC proxy: each export is authorized and forwarded to the
//! replicated store role. The ingest context now owns the direct OTLP/gRPC
//! server; `serve` keeps its public path.

use std::{future::Future, sync::Arc};

use opentelemetry_proto::tonic::collector::{
    logs::v1::{
        logs_service_client::LogsServiceClient,
        logs_service_server::{LogsService, LogsServiceServer},
        ExportLogsServiceRequest, ExportLogsServiceResponse,
    },
    metrics::v1::{
        metrics_service_client::MetricsServiceClient,
        metrics_service_server::{MetricsService, MetricsServiceServer},
        ExportMetricsServiceRequest, ExportMetricsServiceResponse,
    },
    trace::v1::{
        trace_service_client::TraceServiceClient,
        trace_service_server::{TraceService, TraceServiceServer},
        ExportTraceServiceRequest, ExportTraceServiceResponse,
    },
};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{codec::CompressionEncoding, transport::Channel, Request, Response, Status};

use crate::auth::SiftVerifier;
use crate::ingest::interfaces::grpc::otlp_grpc_server::authorize;

pub use crate::ingest::interfaces::grpc::otlp_grpc_server::serve;

#[derive(Clone)]
struct OtlpGrpcProxy {
    channel: Channel,
    verifier: Arc<SiftVerifier>,
}

/// Serve the public OTLP/gRPC port while keeping the durable append boundary
/// in the replicated store role.
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
    let channel =
        tonic::transport::Endpoint::from_shared(store_endpoint.to_string())?.connect_lazy();
    let service = OtlpGrpcProxy { channel, verifier };
    tonic::transport::Server::builder()
        .add_service(
            LogsServiceServer::new(service.clone())
                .accept_compressed(CompressionEncoding::Gzip)
                .send_compressed(CompressionEncoding::Gzip)
                .max_decoding_message_size(maximum_message_bytes),
        )
        .add_service(
            MetricsServiceServer::new(service.clone())
                .accept_compressed(CompressionEncoding::Gzip)
                .send_compressed(CompressionEncoding::Gzip)
                .max_decoding_message_size(maximum_message_bytes),
        )
        .add_service(
            TraceServiceServer::new(service)
                .accept_compressed(CompressionEncoding::Gzip)
                .send_compressed(CompressionEncoding::Gzip)
                .max_decoding_message_size(maximum_message_bytes),
        )
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
        .await?;
    Ok(())
}

#[tonic::async_trait]
impl LogsService for OtlpGrpcProxy {
    async fn export(
        &self,
        mut request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        authorize(request.metadata(), &self.verifier).await?;
        strip_compression_metadata(request.metadata_mut());
        let response = LogsServiceClient::new(self.channel.clone())
            .send_compressed(CompressionEncoding::Gzip)
            .accept_compressed(CompressionEncoding::Gzip)
            .export(request)
            .await?;
        Ok(Response::new(response.into_inner()))
    }
}

#[tonic::async_trait]
impl MetricsService for OtlpGrpcProxy {
    async fn export(
        &self,
        mut request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        authorize(request.metadata(), &self.verifier).await?;
        strip_compression_metadata(request.metadata_mut());
        let response = MetricsServiceClient::new(self.channel.clone())
            .send_compressed(CompressionEncoding::Gzip)
            .accept_compressed(CompressionEncoding::Gzip)
            .export(request)
            .await?;
        Ok(Response::new(response.into_inner()))
    }
}

#[tonic::async_trait]
impl TraceService for OtlpGrpcProxy {
    async fn export(
        &self,
        mut request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        authorize(request.metadata(), &self.verifier).await?;
        strip_compression_metadata(request.metadata_mut());
        let response = TraceServiceClient::new(self.channel.clone())
            .send_compressed(CompressionEncoding::Gzip)
            .accept_compressed(CompressionEncoding::Gzip)
            .export(request)
            .await?;
        Ok(Response::new(response.into_inner()))
    }
}

fn strip_compression_metadata(metadata: &mut tonic::metadata::MetadataMap) {
    metadata.remove("grpc-encoding");
    metadata.remove("grpc-accept-encoding");
}
