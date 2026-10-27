//! Construct query adapters and inject the store fallback at the assembly root.

use crate::app::service_state::ServiceState;
use crate::query::application::forward_service::QueryForwardService;
use crate::query::application::job_service::QueryJobService;
use crate::query::application::query_role_service::QueryRoleService;
use crate::query::application::sift_api_service::SiftApiClient;
use crate::query::infrastructure::file_query_job_store::QueryJobStore;
use crate::query::infrastructure::sift_api_client::NativeApiClient;
use crate::query::infrastructure::store_query_client::StoreQueryForwarder;
use anyhow::Result;
use axum::Router;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

impl SiftApiClient {
    pub fn new(endpoint: &str, token: Option<String>, timeout: Duration) -> Result<Self> {
        Ok(Self::from_port(Arc::new(NativeApiClient::new(
            endpoint, token, timeout,
        )?)))
    }
}

impl QueryJobService {
    pub(crate) fn open(root: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new(Arc::new(QueryJobStore::open(root)?)))
    }
}

pub fn query_role_router(
    state: Arc<ServiceState>,
    store_endpoint: &str,
    max_body_bytes: usize,
) -> Result<Router> {
    let store = crate::operations::interfaces::http::gateway_proxy::query_router(
        store_endpoint,
        max_body_bytes,
    )?;
    let forwarding = Arc::new(QueryForwardService::new(
        Arc::new(StoreQueryForwarder {
            store: store.clone(),
        }),
        max_body_bytes,
    ));
    Ok(
        crate::query::interfaces::http::query_role_router::build_query_role_router(
            Arc::new(QueryRoleService::new(state, forwarding)),
            store,
        ),
    )
}

#[cfg(test)]
mod tests;
