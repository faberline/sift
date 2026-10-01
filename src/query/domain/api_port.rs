//! Read-only Sift API access with native errors and no HTTP client dependency.

use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use serde_json::Value;
use std::sync::Arc;

pub(crate) trait SiftApiPort: Send + Sync {
    type Error: 'static;
    fn with_token(&self, token: Option<String>) -> Arc<dyn SiftApiPort<Error = Self::Error>>;
    fn endpoint_origin(&self) -> String;
    fn query<'a>(
        &'a self,
        request: &'a QueryRequestV1,
    ) -> PortFuture<'a, QueryResponseV1, Self::Error>;
    fn query_value<'a>(&'a self, request: &'a QueryRequestV1)
        -> PortFuture<'a, Value, Self::Error>;
    fn tail_logs<'a>(&'a self, request: &'a LogTailRequestV1)
        -> PortFuture<'a, Value, Self::Error>;
    fn correlate<'a>(
        &'a self,
        request: &'a CorrelationRequestV1,
    ) -> PortFuture<'a, Value, Self::Error>;
    fn get_trace<'a>(
        &'a self,
        project: &'a str,
        trace_id: &'a str,
        min_cursor: Option<u64>,
    ) -> PortFuture<'a, Value, Self::Error>;
    fn list_services<'a>(
        &'a self,
        project: &'a str,
        environment: Option<&'a str>,
    ) -> PortFuture<'a, Value, Self::Error>;
}
