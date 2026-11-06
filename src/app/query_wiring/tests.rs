use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use anyhow::Result;
use axum::body::{Body, HttpBody};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use bytes::Bytes;
use http_body::{Frame, SizeHint};

use crate::query::application::forward_service::QueryForwardService;
use crate::query::application::job_service::QueryJobService;
use crate::query::domain::job_store_port::QueryJobStorePort;
use crate::query::infrastructure::store_query_client::StoreQueryForwarder;
use crate::query::interfaces::http::forward_response::render_forward_response;
use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_request::{QueryModeV1, QueryRequestV1};
use crate::shared_kernel::query_response::QueryResponseV1;

struct FramesBody {
    frames: VecDeque<std::io::Result<Frame<Bytes>>>,
}

impl HttpBody for FramesBody {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<std::io::Result<Frame<Bytes>>>> {
        Poll::Ready(self.frames.pop_front())
    }
    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        hint.set_exact(
            self.frames
                .iter()
                .filter_map(|frame| frame.as_ref().ok()?.data_ref())
                .map(|data| data.len() as u64)
                .sum(),
        );
        hint
    }
    fn is_end_stream(&self) -> bool {
        self.frames.is_empty()
    }
}

fn request() -> QueryRequestV1 {
    serde_json::from_value(serde_json::json!({
        "version": 1, "project": "port-compatibility", "mode": "sync", "signal": {"kind": "logs"}
    }))
    .unwrap()
}

fn forwarding(store: Router, limit: usize) -> QueryForwardService {
    QueryForwardService::new(Arc::new(StoreQueryForwarder { store }), limit)
}

#[tokio::test]
async fn forwarded_frames_keep_headers_trailers_and_lengths() {
    let store = Router::new().route(
        "/api/v1/query",
        post(
            |headers: HeaderMap, Json(request): Json<QueryRequestV1>| async move {
                assert_eq!(request.mode, QueryModeV1::Sync);
                assert_eq!(headers["authorization"], "Bearer port-token");
                assert_eq!(headers["x-sift-project"], "port-compatibility");
                assert_eq!(
                    headers
                        .get_all("x-extra")
                        .iter()
                        .map(|v| v.to_str().unwrap())
                        .collect::<Vec<_>>(),
                    ["one", "two"]
                );
                let mut trailers = HeaderMap::new();
                trailers.insert("x-checksum", HeaderValue::from_static("same"));
                let mut response = Response::new(Body::new(FramesBody {
                    frames: VecDeque::from([
                        Ok(Frame::data(Bytes::from_static(b"first"))),
                        Ok(Frame::data(Bytes::from_static(b"second"))),
                        Ok(Frame::trailers(trailers)),
                    ]),
                }));
                *response.status_mut() = StatusCode::PARTIAL_CONTENT;
                response
                    .headers_mut()
                    .append("x-extra", HeaderValue::from_static("a"));
                response
                    .headers_mut()
                    .append("x-extra", HeaderValue::from_static("b"));
                response
            },
        ),
    );
    let reply = forwarding(store, 1024)
        .forward(
            vec![
                ("content-type".into(), b"application/json".to_vec()),
                ("authorization".into(), b"Bearer port-token".to_vec()),
                ("x-sift-project".into(), b"port-compatibility".to_vec()),
                ("x-extra".into(), b"one".to_vec()),
                ("x-extra".into(), b"two".to_vec()),
            ],
            &request(),
        )
        .await
        .unwrap();
    let response = render_forward_response(reply)
        .map_err(|error| error.message)
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .headers()
            .get_all("x-extra")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    let mut body = response.into_body();
    assert_eq!(body.size_hint().exact(), Some(11));
    for (expected, remaining) in [(b"first".as_slice(), 6), (b"second".as_slice(), 0)] {
        let frame = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.data_ref().unwrap().as_ref(), expected);
        assert_eq!(body.size_hint().exact(), Some(remaining));
    }
    let trailer = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(trailer.trailers_ref().unwrap()["x-checksum"], "same");
    assert!(
        std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
            .await
            .is_none()
    );
    assert!(body.is_end_stream());
}

fn fault_body() -> Body {
    Body::new(FramesBody {
        frames: VecDeque::from([Err(std::io::Error::other("native response read failed"))]),
    })
}

fn error_chain(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut next = Some(error);
    while let Some(error) = next {
        chain.push(error.to_string());
        next = error.source();
    }
    chain
}

#[tokio::test]
async fn forwarding_preserves_body_error_chain_and_bounded_decode_errors() {
    let mut original = fault_body();
    let original_error = std::future::poll_fn(|cx| Pin::new(&mut original).poll_frame(cx))
        .await
        .unwrap()
        .unwrap_err();
    let store = Router::new().route(
        "/api/v1/query",
        post(|| async { Response::new(fault_body()) }),
    );
    let reply = forwarding(store, 1024)
        .forward(Vec::new(), &request())
        .await
        .unwrap();
    let mut body = render_forward_response(reply)
        .map_err(|error| error.message)
        .unwrap()
        .into_body();
    let forwarded_error = std::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error_chain(&forwarded_error), error_chain(&original_error));

    let original_error = axum::body::to_bytes(Body::from("long reply"), 3)
        .await
        .unwrap_err();
    let store = Router::new().route(
        "/api/v1/query",
        post(|| async { Response::new(Body::from("long reply")) }),
    );
    let service = forwarding(store, 3);
    let reply = service.forward(Vec::new(), &request()).await.unwrap();
    assert_eq!(
        service.decode(reply).await.unwrap_err(),
        format!("read store query response: {original_error}")
    );

    let store = Router::new().route(
        "/api/v1/query",
        post(|| async {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"message": "node behind"})),
            )
        }),
    );
    let service = forwarding(store, 1024);
    let reply = service.forward(Vec::new(), &request()).await.unwrap();
    assert_eq!(
        service.decode(reply).await.unwrap_err(),
        "store query returned HTTP 503 Service Unavailable: node behind"
    );
}

struct JobFault {
    stop: &'static str,
    trace: Mutex<Vec<&'static str>>,
}

impl JobFault {
    fn record(&self, point: &'static str) -> Result<()> {
        self.trace.lock().unwrap().push(point);
        if self.stop == point {
            return Err(
                anyhow::Error::new(std::io::Error::other("native job disk failure")).context(point),
            );
        }
        Ok(())
    }
}

impl QueryJobStorePort for JobFault {
    type Error = anyhow::Error;
    fn create(&self, _request: QueryRequestV1) -> Result<QueryJobV1> {
        self.record("create")?;
        unreachable!("create failure is requested by this fixture")
    }
    fn get(&self, _id: &str) -> Result<Option<QueryJobV1>> {
        Ok(None)
    }
    fn mark_running(&self, _id: &str) -> Result<()> {
        self.record("running")
    }
    fn succeed(&self, _id: &str, _result: QueryResponseV1) -> Result<()> {
        self.record("succeed")
    }
    fn fail(&self, _id: &str, _message: String) -> Result<()> {
        self.record("fail")
    }
}

#[tokio::test]
async fn job_port_failures_keep_native_chain_and_runner_order() {
    let port = Arc::new(JobFault {
        stop: "create",
        trace: Mutex::new(Vec::new()),
    });
    let service = QueryJobService::new(port.clone());
    let error = service.create(request()).unwrap_err();
    assert_eq!(
        error.chain().map(ToString::to_string).collect::<Vec<_>>(),
        ["create", "native job disk failure"]
    );
    assert!(error
        .root_cause()
        .downcast_ref::<std::io::Error>()
        .is_some());
    assert_eq!(*port.trace.lock().unwrap(), ["create"]);

    for (stop, execution_fails, expected_trace, executions) in [
        ("running", false, vec!["running"], 0),
        ("succeed", false, vec!["running", "succeed"], 1),
        ("fail", true, vec!["running", "fail"], 1),
    ] {
        let port = Arc::new(JobFault {
            stop,
            trace: Mutex::new(Vec::new()),
        });
        let service = Arc::new(QueryJobService::new(port.clone()));
        let counter = Arc::new(AtomicUsize::new(0));
        let worker_counter = counter.clone();
        let runner = service_executor::JobRunner::new(service);
        let report = runner
            .spawn_blocking("fixed-job-id".to_string(), (), move |()| {
                worker_counter.fetch_add(1, Ordering::SeqCst);
                if execution_fails {
                    return Err("query rejected");
                }
                Ok(QueryResponseV1::complete(
                    serde_json::Value::Null,
                    None,
                    0,
                    0,
                    0,
                    std::time::Duration::ZERO,
                ))
            })
            .await
            .unwrap();
        assert_eq!(report.persistence_error.as_deref(), Some(stop));
        assert_eq!(counter.load(Ordering::SeqCst), executions);
        assert_eq!(*port.trace.lock().unwrap(), expected_trace);
    }
}
