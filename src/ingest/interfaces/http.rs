//! The HTTP ingest endpoints: OTLP logs, traces and metrics, Prometheus remote
//! write, and the bounded request-body decoder.

pub(crate) mod otlp_handlers;
pub(crate) mod prometheus_remote_write;
pub(crate) mod request_body_decoder;
