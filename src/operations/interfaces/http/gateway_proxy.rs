//! The gateway: Sift's route policy for the shared reverse-proxy runtime, which
//! picks the store or query upstream for each request.

use anyhow::{bail, Context, Result};
use axum::Router;
use url::Url;

use crate::operations::application::gateway_policy::SiftRolePolicy;

pub fn gateway_router(
    store_endpoint: &str,
    query_endpoint: &str,
    max_body_bytes: usize,
) -> Result<Router> {
    router(store_endpoint, Some(query_endpoint), max_body_bytes)
}

pub fn query_router(store_endpoint: &str, max_body_bytes: usize) -> Result<Router> {
    router(store_endpoint, None, max_body_bytes)
}

fn router(
    store_endpoint: &str,
    query_endpoint: Option<&str>,
    max_body_bytes: usize,
) -> Result<Router> {
    let store = endpoint(store_endpoint, "store")?;
    let query = query_endpoint
        .map(|value| endpoint(value, "query"))
        .transpose()?;
    service_http::reverse_proxy_router(SiftRolePolicy::new(store, query, max_body_bytes))
}

fn endpoint(value: &str, role: &str) -> Result<Url> {
    let mut url = Url::parse(value).with_context(|| format!("parse Sift {role} endpoint"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("Sift {role} endpoint must use http or https and include a host");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("Sift {role} endpoint must not include a query or fragment");
    }
    url.set_path("");
    Ok(url)
}
