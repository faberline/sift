//! The HTTP client MCP tools and `sift query` call the Sift API through, with
//! its retries.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use url::Url;

use crate::query::interfaces::http::phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::query::interfaces::http::query_request_v1::{QueryModeV1, QueryRequestV1};
use crate::query::interfaces::http::query_response_v1::QueryResponseV1;

const SIFT_API_MAX_RETRIES: usize = 2;
const SIFT_API_MAX_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Small client shared by the CLI and MCP tools.
#[derive(Clone)]
pub struct SiftApiClient {
    pub(in crate::query) endpoint: Url,
    token: Option<Arc<str>>,
    http: reqwest::Client,
    timeout: Duration,
}

impl SiftApiClient {
    pub fn new(endpoint: &str, token: Option<String>, timeout: Duration) -> Result<Self> {
        let mut endpoint = Url::parse(endpoint).context("parse Sift API endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            bail!("Sift API endpoint must use http or https");
        }
        if endpoint.cannot_be_a_base()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            bail!("Sift API endpoint must be a base URL without query or fragment");
        }
        if !endpoint.path().ends_with('/') {
            endpoint.set_path(&format!("{}/", endpoint.path()));
        }
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("build Sift API client")?;
        Ok(Self {
            endpoint,
            token: token.map(Arc::from),
            http,
            timeout,
        })
    }

    pub(in crate::query) fn with_token(&self, token: Option<String>) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            token: token.map(Arc::from),
            http: self.http.clone(),
            timeout: self.timeout,
        }
    }

    pub async fn query(&self, request: &QueryRequestV1) -> Result<QueryResponseV1> {
        self.query_response(request).await
    }

    pub(in crate::query) async fn query_value(&self, request: &QueryRequestV1) -> Result<Value> {
        self.query_response(request).await
    }

    async fn query_response<R: DeserializeOwned>(&self, request: &QueryRequestV1) -> Result<R> {
        // Only explicit sync mode guarantees that this call cannot create a
        // persistent query job. Auto and async fail closed on retry.
        self.post_json(
            "api/v1/query",
            &request.project,
            request,
            request.mode == QueryModeV1::Sync,
        )
        .await
    }

    pub(in crate::query) async fn tail_logs(&self, request: &LogTailRequestV1) -> Result<Value> {
        self.post_json("api/v1/logs/tail", &request.project, request, true)
            .await
    }

    pub(in crate::query) async fn correlate(
        &self,
        request: &CorrelationRequestV1,
    ) -> Result<Value> {
        self.post_json("api/v1/correlate", &request.project, request, true)
            .await
    }

    pub(in crate::query) async fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
        min_cursor: Option<u64>,
    ) -> Result<Value> {
        let mut url = self.endpoint.join("api/v1/traces/")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Sift endpoint cannot contain path segments"))?
            .pop_if_empty()
            .push(trace_id);
        let mut query = vec![("project", project.to_string())];
        if let Some(cursor) = min_cursor {
            query.push(("min_cursor", cursor.to_string()));
        }
        let request =
            self.authorize_project(self.http.request(Method::GET, url).query(&query), project);
        self.send(request, true).await
    }

    pub(in crate::query) async fn list_services(
        &self,
        project: &str,
        environment: Option<&str>,
    ) -> Result<Value> {
        let url = self.endpoint.join("api/v1/services")?;
        let mut query = vec![("project", project.to_string())];
        if let Some(environment) = environment {
            query.push(("environment", environment.to_string()));
        }
        let request =
            self.authorize_project(self.http.request(Method::GET, url).query(&query), project);
        self.send(request, true).await
    }

    async fn post_json<T, R>(
        &self,
        path: &str,
        project: &str,
        value: &T,
        replay_safe: bool,
    ) -> Result<R>
    where
        T: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.endpoint.join(path)?;
        let request =
            self.authorize_project(self.http.request(Method::POST, url).json(value), project);
        self.send(request, replay_safe).await
    }

    fn authorize_project(&self, request: RequestBuilder, project: &str) -> RequestBuilder {
        let request = request.header("x-sift-project", project);
        match &self.token {
            Some(token) => request.bearer_auth(token.as_ref()),
            None => request,
        }
    }

    async fn send<R: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        replay_safe: bool,
    ) -> Result<R> {
        let started = Instant::now();
        let mut request = request;
        for retry in 0..=SIFT_API_MAX_RETRIES {
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                bail!("Sift API request exceeded its total timeout");
            }
            let next_request = request.try_clone();
            let response = request
                .timeout(remaining)
                .send()
                .await
                .context("send Sift API request")?;
            let status = response.status();
            let headers = response.headers().clone();
            let body = response.bytes().await.context("read Sift API response")?;
            if status.is_success() {
                return serde_json::from_slice(&body).context("decode Sift API response");
            }
            let retry_delay =
                service_http::retry_delay_from_detailed_error(status, &headers, &body);
            let remaining = self.timeout.saturating_sub(started.elapsed());
            if replay_safe && retry < SIFT_API_MAX_RETRIES {
                if let (Some(next_request), Some(delay)) = (next_request, retry_delay) {
                    if delay <= SIFT_API_MAX_RETRY_DELAY && delay < remaining {
                        tracing::warn!(
                            %status,
                            retry = retry + 1,
                            retry_after_seconds = delay.as_secs(),
                            "retrying detailed retryable Sift API response"
                        );
                        tokio::time::sleep(delay).await;
                        request = next_request;
                        continue;
                    }
                }
            }
            let message = String::from_utf8_lossy(&body);
            bail!("Sift API returned {status}: {}", truncate(&message, 8_192));
        }
        unreachable!("bounded Sift API retry loop always returns")
    }
}

fn truncate(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
