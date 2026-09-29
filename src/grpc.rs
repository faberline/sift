//! The grpc module's public paths. The ingest context now owns the direct
//! OTLP/gRPC server and the operations context the OTLP/gRPC proxy; these
//! re-exports keep `sift::grpc::*` compiling for callers outside the crate.

pub use crate::ingest::interfaces::grpc::otlp_grpc_server::serve;
pub use crate::operations::interfaces::grpc::otlp_proxy::serve_proxy;
