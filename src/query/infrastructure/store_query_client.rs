//! Router and body adapters retain the original store forwarding transport.

use crate::query::domain::forward_port::{
    ForwardBody, ForwardFuture, ForwardReply, QueryForwardPort,
};
use crate::shared_kernel::forward_data::{BodyLength, ForwardFrame, ForwardHeaders};
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::query_request::QueryRequestV1;
use axum::body::{to_bytes, Body, HttpBody};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use axum::Router;
use bytes::Bytes;
use tower::ServiceExt as _;

pub(crate) struct StoreQueryForwarder {
    pub(crate) store: Router,
}

impl QueryForwardPort for StoreQueryForwarder {
    type Error = String;
    type BodyError = axum::Error;
    fn forward<'a>(
        &'a self,
        headers: ForwardHeaders,
        request: &'a QueryRequestV1,
    ) -> ForwardFuture<'a, Self::Error, Self::BodyError> {
        Box::pin(async move {
            let body = serde_json::to_vec(request).map_err(|error| error.to_string())?;
            let mut upstream = Request::builder()
                .method(Method::POST)
                .uri("/api/v1/query")
                .body(Body::from(body))
                .map_err(|error| error.to_string())?;
            let mut native = HeaderMap::new();
            for (name, value) in headers {
                native.append(
                    HeaderName::from_bytes(name.as_bytes()).map_err(|error| error.to_string())?,
                    HeaderValue::from_bytes(&value).map_err(|error| error.to_string())?,
                );
            }
            *upstream.headers_mut() = native;
            let response = self
                .store
                .clone()
                .oneshot(upstream)
                .await
                .expect("Sift store proxy router is infallible");
            let (parts, body) = response.into_parts();
            Ok(ForwardReply {
                status: parts.status.as_u16(),
                status_text: parts.status.to_string(),
                headers: header_data(&parts.headers),
                body: Box::new(RouterBody { body }),
            })
        })
    }
}

fn header_data(headers: &HeaderMap) -> ForwardHeaders {
    headers
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
        .collect()
}

struct RouterBody {
    body: Body,
}

impl ForwardBody for RouterBody {
    type Error = axum::Error;
    fn next_frame(&mut self) -> PortFuture<'_, Option<ForwardFrame>, Self::Error> {
        Box::pin(async move {
            loop {
                let frame =
                    std::future::poll_fn(|cx| std::pin::Pin::new(&mut self.body).poll_frame(cx))
                        .await;
                let Some(frame) = frame else { return Ok(None) };
                match frame?.into_data() {
                    Ok(data) => return Ok(Some(ForwardFrame::Data(data))),
                    Err(frame) => {
                        if let Ok(headers) = frame.into_trailers() {
                            return Ok(Some(ForwardFrame::Trailers(header_data(&headers))));
                        }
                    }
                }
            }
        })
    }
    fn read_bounded(self: Box<Self>, limit: usize) -> PortFuture<'static, Bytes, Self::Error> {
        Box::pin(to_bytes(self.body, limit))
    }
    fn size_hint(&self) -> BodyLength {
        let hint = self.body.size_hint();
        BodyLength {
            lower: hint.lower(),
            upper: hint.upper(),
        }
    }
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
}
