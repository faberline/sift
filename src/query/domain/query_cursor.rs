//! The opaque cursor a query page hands back, per signal.

use anyhow::Result;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::ApiError;

pub(in crate::query) fn encode_query_cursor(signal: &str, value: &str) -> String {
    format!("{signal}:{}", URL_SAFE_NO_PAD.encode(value.as_bytes()))
}

pub(in crate::query) fn decode_query_cursor(
    signal: &str,
    cursor: Option<&str>,
) -> Result<Option<String>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let Some((actual_signal, encoded)) = cursor.split_once(':') else {
        return Err(ApiError::bad_request(
            "invalid_cursor",
            "cursor has an invalid format",
        ));
    };
    if actual_signal != signal {
        return Err(ApiError::bad_request(
            "invalid_cursor",
            format!("{actual_signal} cursor cannot be used for {signal}"),
        ));
    }
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
        ApiError::bad_request("invalid_cursor", "cursor payload is not valid base64url")
    })?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| ApiError::bad_request("invalid_cursor", "cursor payload is not UTF-8"))
}
