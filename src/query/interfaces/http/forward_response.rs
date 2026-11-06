//! Preserve forwarded status, headers, body frames, trailers and size hints.

use crate::app::interfaces::http::api_error::ApiError;
use crate::query::application::forward_service::{ForwardBodyService, ForwardResponse};
use crate::shared_kernel::forward_data::{BodyLength, ForwardFrame, ForwardHeaders};
use axum::body::{Body, HttpBody};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use http_body::{Frame, SizeHint};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) fn render_forward_response(reply: ForwardResponse) -> Result<Response, ApiError> {
    let status = StatusCode::from_u16(reply.status)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let headers =
        native_headers(reply.headers).map_err(|error| ApiError::internal(error.to_string()))?;
    let mut response = Response::new(Body::new(ForwardHttpBody::new(reply.body)));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    Ok(response)
}

fn native_headers(headers: ForwardHeaders) -> Result<HeaderMap, axum::Error> {
    let mut native = HeaderMap::new();
    for (name, value) in headers {
        native.append(
            HeaderName::from_bytes(name.as_bytes()).map_err(axum::Error::new)?,
            HeaderValue::from_bytes(&value).map_err(axum::Error::new)?,
        );
    }
    Ok(native)
}

type ReadOutcome = (
    ForwardBodyService,
    Result<Option<ForwardFrame>, axum::Error>,
);
type PendingRead = Pin<Box<dyn Future<Output = ReadOutcome> + Send>>;

struct ForwardHttpBody {
    source: Option<ForwardBodyService>,
    pending: Option<PendingRead>,
    length: BodyLength,
    end: bool,
    done: bool,
}

impl ForwardHttpBody {
    fn new(source: ForwardBodyService) -> Self {
        let length = source.size_hint();
        let end = source.is_end_stream();
        Self {
            source: Some(source),
            pending: None,
            length,
            end,
            done: false,
        }
    }
}

impl HttpBody for ForwardHttpBody {
    type Data = Bytes;
    type Error = axum::BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.done {
            return Poll::Ready(None);
        }
        if self.pending.is_none() {
            let Some(mut source) = self.source.take() else {
                return Poll::Ready(None);
            };
            self.pending = Some(Box::pin(async move {
                let result = source.next_frame().await;
                (source, result)
            }));
        }
        let Some(pending) = self.pending.as_mut() else {
            return Poll::Ready(None);
        };
        let Poll::Ready((source, result)) = pending.as_mut().poll(cx) else {
            return Poll::Pending;
        };
        self.pending = None;
        self.length = source.size_hint();
        self.end = source.is_end_stream();
        self.source = Some(source);
        match result {
            Ok(Some(ForwardFrame::Data(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Ok(Some(ForwardFrame::Trailers(headers))) => Poll::Ready(Some(
                native_headers(headers)
                    .map(Frame::trailers)
                    .map_err(axum::Error::into_inner),
            )),
            Ok(None) => {
                self.done = true;
                self.source = None;
                Poll::Ready(None)
            }
            Err(error) => {
                self.done = true;
                self.source = None;
                Poll::Ready(Some(Err(error.into_inner())))
            }
        }
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        hint.set_lower(self.length.lower);
        if let Some(upper) = self.length.upper {
            hint.set_upper(upper)
        }
        hint
    }
    fn is_end_stream(&self) -> bool {
        self.done || (self.pending.is_none() && self.end)
    }
}
