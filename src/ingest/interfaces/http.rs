//! The HTTP ingest endpoints: OTLP logs, traces and metrics, Prometheus remote
//! write, the bounded request-body decoder, and the event batch schemas.

pub(crate) mod batch;
pub(crate) mod otlp_handlers;
pub(crate) mod prometheus_remote_write;
pub(crate) mod request_body_decoder;
