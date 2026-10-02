//! Choose the request's upstream from the configured store and query roles.

use crate::operations::domain::route_class::{is_control_path, is_query_path};
use axum::http::{Method, Uri};
use std::time::Duration;
use url::Url;

#[derive(Clone)]
pub(crate) struct SiftRolePolicy {
    store: Url,
    query: Option<Url>,
    control: Option<Url>,
    max_body_bytes: usize,
}

impl SiftRolePolicy {
    pub(crate) fn new(store: Url, query: Option<Url>, max_body_bytes: usize) -> Self {
        Self {
            store,
            query,
            control: None,
            max_body_bytes,
        }
    }
    pub(crate) fn with_control(mut self, control: Url) -> Self {
        self.control = Some(control);
        self
    }
}

impl service_http::ReverseProxyPolicy for SiftRolePolicy {
    fn select_upstream(
        &self,
        _method: &Method,
        uri: &Uri,
    ) -> Result<Url, service_http::ReverseProxySelectionError> {
        if let Some(control) = self
            .control
            .as_ref()
            .filter(|_| is_control_path(uri.path()))
        {
            return Ok(control.clone());
        }
        match &self.query {
            Some(query) if is_query_path(uri.path()) => Ok(query.clone()),
            _ => Ok(self.store.clone()),
        }
    }

    fn max_body_bytes(&self) -> usize {
        self.max_body_bytes
    }
    fn upstream_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
}
