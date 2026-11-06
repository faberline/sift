//! Encoding and decoding the opaque cursor a query page hands back.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;

use crate::app::interfaces::http::api_error::ApiError;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_text_keeps_the_existing_wire_format() {
        for (signal, value, text) in [
            ("logs", "10", "logs:MTA"),
            ("metrics", "雨", "metrics:6Zuo"),
            ("traces", "", "traces:"),
        ] {
            assert_eq!(encode_query_cursor(signal, value), text);
            assert_eq!(
                decode_query_cursor(signal, Some(text))
                    .unwrap_or_else(|error| panic!("{}", error.message))
                    .as_deref(),
                Some(value)
            );
        }
        assert_eq!(
            decode_query_cursor("logs", None).unwrap_or_else(|error| panic!("{}", error.message)),
            None
        );
    }

    #[tokio::test]
    async fn cursor_rejections_keep_the_existing_code_and_text() {
        use axum::response::IntoResponse;
        for (text, message) in [
            ("10", "cursor has an invalid format"),
            ("metrics:MTA", "metrics cursor cannot be used for logs"),
            ("logs:%", "cursor payload is not valid base64url"),
            ("logs:_w", "cursor payload is not UTF-8"),
        ] {
            let error = decode_query_cursor("logs", Some(text)).unwrap_err();
            assert_eq!(error.message, message);
            let response = error.into_response();
            assert_eq!(response.status().as_u16(), 400);
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["error"], "invalid_cursor");
        }
    }
}
