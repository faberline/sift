//! Decoding a bounded, possibly gzip-compressed ingest request body.

use axum::http::{HeaderMap, StatusCode};
use bytes::Bytes;

use crate::ingest::application::admission_controller::AdmissionController;
use crate::ingest::domain::admission_error::AdmissionError;

impl AdmissionController {
    pub fn decode_body(&self, headers: &HeaderMap, body: Bytes) -> Result<Vec<u8>, AdmissionError> {
        let limits = service_http::ContentDecodeLimits::new(
            self.limits.max_compressed_body_bytes,
            self.limits.max_decoded_body_bytes,
        )
        .expect("validated Sift ingest limits are positive");
        service_http::decode_request_body(headers, body.as_ref(), limits).map_err(|error| {
            use service_http::ContentDecodeErrorKind as Kind;
            let (status, code) = match error.kind() {
                Kind::CompressedBodyTooLarge => {
                    (StatusCode::PAYLOAD_TOO_LARGE, "compressed_body_too_large")
                }
                Kind::DecodedBodyTooLarge => {
                    (StatusCode::PAYLOAD_TOO_LARGE, "decoded_body_too_large")
                }
                Kind::InvalidGzip => (StatusCode::BAD_REQUEST, "invalid_gzip"),
                Kind::UnsupportedContentEncoding => (
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "unsupported_content_encoding",
                ),
            };
            AdmissionError::new(status, code, error.to_string(), false, None)
        })
    }
}
