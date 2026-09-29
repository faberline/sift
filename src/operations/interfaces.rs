//! How operations is reached: the Sift custom resource and the operator loop,
//! the gateway's reverse proxy, the OTLP/gRPC proxy, and the admin HTTP
//! endpoints.

pub(crate) mod crd;
pub(crate) mod grpc;
pub(crate) mod http;
pub(crate) mod operator;
