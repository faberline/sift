//! Decoding a signal endpoint's payload into operational events, and encoding
//! the partial-success response in the request's media type.

use anyhow::{bail, Result};
use transport_otlp::{OtlpMediaType, OtlpSignal};

use crate::ingest::interfaces::otlp::json_logs_decoder::decode_logs_json;
use crate::ingest::interfaces::otlp::json_metrics_decoder::decode_metrics_json;
use crate::ingest::interfaces::otlp::json_traces_decoder::decode_traces_json;
use crate::ingest::interfaces::otlp::proto_logs_decoder::decode_logs_proto;
use crate::ingest::interfaces::otlp::proto_metrics_decoder::decode_metrics_proto;
use crate::ingest::interfaces::otlp::proto_traces_decoder::decode_traces_proto;
use crate::OperationalEventV2;

#[derive(Clone, Debug)]
pub struct OtlpItemError {
    pub event_id: Option<String>,
    pub message: String,
}

pub struct DecodedOtlp {
    pub items: Vec<std::result::Result<OperationalEventV2, OtlpItemError>>,
}

impl DecodedOtlp {
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

pub struct EncodedOtlpResponse {
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub fn decode(
    signal: OtlpSignal,
    media: OtlpMediaType,
    body: &[u8],
    project: &str,
) -> Result<DecodedOtlp> {
    let payload = transport_otlp::decode_payload(signal, media, body)?;
    normalize_payload(payload, project)
}

pub fn normalize_payload(
    payload: transport_otlp::DecodedPayload,
    project: &str,
) -> Result<DecodedOtlp> {
    let items = match payload {
        transport_otlp::DecodedPayload::Logs(request) => decode_logs_proto(request, project)?,
        transport_otlp::DecodedPayload::Metrics(request) => decode_metrics_proto(request, project)?,
        transport_otlp::DecodedPayload::Traces(request) => decode_traces_proto(request, project)?,
        transport_otlp::DecodedPayload::Json {
            signal: OtlpSignal::Logs,
            value,
        } => decode_logs_json(&value, project)?,
        transport_otlp::DecodedPayload::Json {
            signal: OtlpSignal::Metrics,
            value,
        } => decode_metrics_json(&value, project)?,
        transport_otlp::DecodedPayload::Json {
            signal: OtlpSignal::Traces,
            value,
        } => decode_traces_json(&value, project)?,
    };
    if items.is_empty() {
        bail!("OTLP request contains no signal items");
    }
    Ok(DecodedOtlp { items })
}

pub fn encode_response(
    signal: OtlpSignal,
    media: OtlpMediaType,
    rejected: usize,
    messages: &[String],
) -> Result<EncodedOtlpResponse> {
    let encoded = transport_otlp::encode_response(
        signal,
        media,
        &transport_otlp::PartialSuccess::new(rejected, messages.join("; ")),
    )?;
    Ok(EncodedOtlpResponse {
        content_type: encoded.content_type,
        body: encoded.body,
    })
}

pub(super) fn item_error(event_id: Option<String>, message: impl Into<String>) -> OtlpItemError {
    OtlpItemError {
        event_id,
        message: message.into(),
    }
}
