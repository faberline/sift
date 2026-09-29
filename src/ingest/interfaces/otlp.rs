//! OTLP payload decoding: JSON and protobuf logs, traces and metrics normalized
//! into operational events, and the partial-success response for each media
//! type.

pub(crate) mod json_logs_decoder;
pub(crate) mod json_metrics_decoder;
pub(crate) mod json_traces_decoder;
pub(crate) mod otlp_codec;
pub(crate) mod otlp_event_builder;
pub(crate) mod otlp_json_attributes;
pub(crate) mod otlp_json_values;
pub(crate) mod otlp_proto_values;
pub(crate) mod proto_logs_decoder;
pub(crate) mod proto_metrics_decoder;
pub(crate) mod proto_traces_decoder;
