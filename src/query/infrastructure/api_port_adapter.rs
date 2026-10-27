//! Preserve the original HTTP calls, retry rules and client instances.

use crate::query::domain::api_port::SiftApiPort;
use crate::query::infrastructure::sift_api_client::NativeApiClient;
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use serde_json::Value;
use std::sync::Arc;

impl SiftApiPort for NativeApiClient {
    type Error = anyhow::Error;
    fn with_token(&self, token: Option<String>) -> Arc<dyn SiftApiPort<Error = Self::Error>> {
        Arc::new(NativeApiClient::with_token(self, token))
    }
    fn endpoint_origin(&self) -> String {
        self.endpoint.origin().ascii_serialization()
    }
    fn query<'a>(
        &'a self,
        request: &'a QueryRequestV1,
    ) -> PortFuture<'a, QueryResponseV1, Self::Error> {
        Box::pin(NativeApiClient::query(self, request))
    }
    fn query_value<'a>(
        &'a self,
        request: &'a QueryRequestV1,
    ) -> PortFuture<'a, Value, Self::Error> {
        Box::pin(NativeApiClient::query_value(self, request))
    }
    fn tail_logs<'a>(
        &'a self,
        request: &'a LogTailRequestV1,
    ) -> PortFuture<'a, Value, Self::Error> {
        Box::pin(NativeApiClient::tail_logs(self, request))
    }
    fn correlate<'a>(
        &'a self,
        request: &'a CorrelationRequestV1,
    ) -> PortFuture<'a, Value, Self::Error> {
        Box::pin(NativeApiClient::correlate(self, request))
    }
    fn get_trace<'a>(
        &'a self,
        project: &'a str,
        trace_id: &'a str,
        min_cursor: Option<u64>,
    ) -> PortFuture<'a, Value, Self::Error> {
        Box::pin(NativeApiClient::get_trace(
            self, project, trace_id, min_cursor,
        ))
    }
    fn list_services<'a>(
        &'a self,
        project: &'a str,
        environment: Option<&'a str>,
    ) -> PortFuture<'a, Value, Self::Error> {
        Box::pin(NativeApiClient::list_services(self, project, environment))
    }
}
