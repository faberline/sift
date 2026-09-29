//! Forwarding a versioned query to the store, and decoding its answer.

use axum::body::{to_bytes, Body};
use axum::http::{HeaderMap, Method, Request};
use axum::response::Response;
use axum::Router;
use tower::ServiceExt as _;

use crate::query::interfaces::http::query_request_v1::QueryRequestV1;
use crate::query::interfaces::http::query_response_v1::QueryResponseV1;

pub(in crate::query) async fn forward_query_to_store(
    store: Router,
    headers: HeaderMap,
    request: &QueryRequestV1,
) -> std::result::Result<Response, String> {
    let body = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    let mut upstream = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/query")
        .body(Body::from(body))
        .map_err(|error| error.to_string())?;
    *upstream.headers_mut() = headers;
    Ok(store
        .oneshot(upstream)
        .await
        .expect("Sift store proxy router is infallible"))
}

pub(in crate::query) async fn decode_store_query_response(
    response: Response,
    max_body_bytes: usize,
) -> std::result::Result<QueryResponseV1, String> {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), max_body_bytes)
        .await
        .map_err(|error| format!("read store query response: {error}"))?;
    if !status.is_success() {
        let message = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|body| body["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
        return Err(format!("store query returned HTTP {status}: {message}"));
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("decode store query response: {error}"))
}
