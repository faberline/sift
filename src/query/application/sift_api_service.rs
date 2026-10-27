//! API read use cases over an injected port.

use crate::query::domain::api_port::SiftApiPort;
use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct SiftApiClient {
    port: Arc<dyn SiftApiPort<Error = anyhow::Error>>,
}

impl SiftApiClient {
    pub(crate) fn from_port(port: Arc<dyn SiftApiPort<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(in crate::query) fn with_token(&self, token: Option<String>) -> Self {
        Self::from_port(self.port.with_token(token))
    }
    pub(in crate::query) fn endpoint_origin(&self) -> String {
        self.port.endpoint_origin()
    }
    pub async fn query(&self, request: &QueryRequestV1) -> Result<QueryResponseV1> {
        self.port.query(request).await
    }
    pub(in crate::query) async fn query_value(&self, request: &QueryRequestV1) -> Result<Value> {
        self.port.query_value(request).await
    }
    pub(in crate::query) async fn tail_logs(&self, request: &LogTailRequestV1) -> Result<Value> {
        self.port.tail_logs(request).await
    }
    pub(in crate::query) async fn correlate(
        &self,
        request: &CorrelationRequestV1,
    ) -> Result<Value> {
        self.port.correlate(request).await
    }
    pub(in crate::query) async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        min_cursor: Option<u64>,
    ) -> Result<Value> {
        self.port.get_trace(project, trace_id, min_cursor).await
    }
    pub(in crate::query) async fn list_services(
        &self,
        project: &str,
        environment: Option<&str>,
    ) -> Result<Value> {
        self.port.list_services(project, environment).await
    }
}
