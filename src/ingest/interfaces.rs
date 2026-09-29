//! How telemetry arrives: the OTLP/HTTP, Prometheus remote-write and OTLP/gRPC
//! endpoints, and the OTLP and remote-write decoders behind them.

pub(crate) mod grpc;
pub(crate) mod http;
pub(crate) mod otlp;
pub(crate) mod remote_write;
