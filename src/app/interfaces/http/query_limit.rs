//! Refuse a query request when too many queries already run.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::app::service_state::ServiceState;

pub(crate) async fn limit_queries(
    State(state): State<Arc<ServiceState>>,
    request: Request,
    next: Next,
) -> Response {
    match state.admission().acquire_query() {
        Ok(_permit) => next.run(request).await,
        Err(error) => super::api_error::ApiError::from_admission(error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::app::service_state::ServiceState;
    use crate::ingest::IngestLimits;

    fn get_services() -> Request<Body> {
        Request::builder()
            .uri("/api/v1/services")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn query_routes_return_429_while_the_limit_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(
            ServiceState::open_with_ingest_limits(
                dir.path(),
                IngestLimits {
                    max_concurrent_queries: 1,
                    ..IngestLimits::default()
                },
            )
            .unwrap(),
        );
        let app = crate::router(state.clone());

        let permit = state.admission().acquire_query().unwrap();
        let response = app.clone().oneshot(get_services()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "query_concurrency_exceeded");

        drop(permit);
        let response = app.clone().oneshot(get_services()).await.unwrap();
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        // Ingest is not limited by the query limit.
        let held = state.admission().acquire_query().unwrap();
        let ingest = Request::builder()
            .method("POST")
            .uri("/v1/logs")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(ingest).await.unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("query_concurrency_exceeded"));
        drop(held);
    }
}
