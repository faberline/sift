//! OTLP decoding's public paths. The ingest context now owns the decoders;
//! these re-exports keep `sift::ingest::otlp::*`, with the `wire` protobuf
//! types, compiling for callers outside the crate.

pub use crate::ingest::interfaces::otlp::otlp_codec::{
    decode, encode_response, normalize_payload, DecodedOtlp, EncodedOtlpResponse, OtlpItemError,
};
pub use transport_otlp::proto as wire;
pub use transport_otlp::{OtlpMediaType, OtlpSignal};
