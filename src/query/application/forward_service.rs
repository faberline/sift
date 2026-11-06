//! Forward queries and decode bounded job replies through the transport port.

use crate::query::domain::forward_port::{ForwardBody, QueryForwardPort};
use crate::shared_kernel::forward_data::{BodyLength, ForwardFrame, ForwardHeaders};
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use bytes::Bytes;
use std::sync::Arc;

pub(crate) struct ForwardBodyService {
    inner: Box<dyn ForwardBody<Error = axum::Error>>,
}

impl ForwardBodyService {
    pub(crate) async fn next_frame(&mut self) -> Result<Option<ForwardFrame>, axum::Error> {
        self.inner.next_frame().await
    }
    async fn read_bounded(self, limit: usize) -> Result<Bytes, axum::Error> {
        self.inner.read_bounded(limit).await
    }
    pub(crate) fn size_hint(&self) -> BodyLength {
        self.inner.size_hint()
    }
    pub(crate) fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
}

pub(crate) struct ForwardResponse {
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) headers: ForwardHeaders,
    pub(crate) body: ForwardBodyService,
}

pub(crate) struct QueryForwardService {
    port: Arc<dyn QueryForwardPort<Error = String, BodyError = axum::Error>>,
    max_body_bytes: usize,
}

impl QueryForwardService {
    pub(crate) fn new(
        port: Arc<dyn QueryForwardPort<Error = String, BodyError = axum::Error>>,
        max_body_bytes: usize,
    ) -> Self {
        Self {
            port,
            max_body_bytes,
        }
    }
    pub(crate) async fn forward(
        &self,
        headers: ForwardHeaders,
        request: &QueryRequestV1,
    ) -> Result<ForwardResponse, String> {
        let reply = self.port.forward(headers, request).await?;
        Ok(ForwardResponse {
            status: reply.status,
            status_text: reply.status_text,
            headers: reply.headers,
            body: ForwardBodyService { inner: reply.body },
        })
    }
    pub(crate) async fn decode(
        &self,
        response: ForwardResponse,
    ) -> Result<QueryResponseV1, String> {
        let bytes = response
            .body
            .read_bounded(self.max_body_bytes)
            .await
            .map_err(|error| format!("read store query response: {error}"))?;
        if !(200..300).contains(&response.status) {
            let message = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|body| body["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).into_owned());
            return Err(format!(
                "store query returned HTTP {}: {message}",
                response.status_text
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("decode store query response: {error}"))
    }
}
