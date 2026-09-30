//! Ingest: telemetry admitted within bounded limits, decoded from OTLP and
//! Prometheus remote write into operational events, governed, and appended
//! through the group-commit queue to the replicated journal, behind the
//! OTLP/HTTP, OTLP/gRPC and remote-write endpoints.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;

pub use crate::ingest::{
    application::admission_controller::{AdmissionController, AdmissionPermit},
    domain::{
        admission_error::AdmissionError,
        ingest_limits::{IngestLimits, IngestLimitsError},
    },
    interfaces::{
        grpc::otlp_grpc_server::serve as serve_otlp_grpc,
        otlp::otlp_codec::{
            decode as decode_otlp, encode_response as encode_otlp_response,
            normalize_payload as normalize_otlp_payload, DecodedOtlp, EncodedOtlpResponse,
            OtlpItemError,
        },
        remote_write::remote_write_consumer::{decode_remote_write, DecodedWrite},
    },
};
pub use transport_otlp::{OtlpMediaType, OtlpSignal};
