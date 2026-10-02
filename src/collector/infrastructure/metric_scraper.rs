//! Kubernetes discovery is limited to Pods on the agent's own node.
use crate::collector::domain::prometheus_text::{parse_text, ScrapedSample};
use anyhow::{bail, Context, Result};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

pub struct ScrapeConfig {
    pub endpoint: String,
    pub project: String,
    pub environment: String,
    pub node: String,
    pub namespace: Option<String>,
    pub token: Option<String>,
    pub token_file: Option<PathBuf>,
    pub state: PathBuf,
    pub interval: Duration,
    pub once: bool,
}
pub async fn run(config: ScrapeConfig) -> Result<()> {
    if config.node.is_empty() || config.project.is_empty() || config.interval.is_zero() {
        bail!("scraper requires node, project, and a positive interval");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let projected = config
        .token_file
        .as_ref()
        .map(|p| service_auth::k8s::ProjectedTokenFile::new(p, "sift.axiom.dev"));
    let mut ingest = url::Url::parse(&config.endpoint)?;
    if !matches!(ingest.scheme(), "http" | "https") || ingest.host_str().is_none() {
        bail!("invalid scrape ingest endpoint");
    }
    ingest.set_path("/v1/metrics");
    ingest.set_query(None);
    ingest.set_fragment(None);
    let pending = config.state.join("pending-metrics.json");
    if let Some(mut queue) = super::metric_queue::MetricQueue::load(
        &pending,
        &config.project,
        &config.environment,
        &config.node,
    )? {
        drain(
            &mut queue,
            &pending,
            &client,
            &ingest,
            &config,
            projected.as_ref(),
        )
        .await?;
    }
    let kube = kube::Client::try_default()
        .await
        .context("create Kubernetes discovery client")?;
    let pods: Api<Pod> = match &config.namespace {
        Some(ns) => Api::namespaced(kube, ns),
        None => Api::all(kube),
    };
    let mut ticks = tokio::time::interval(config.interval);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticks.tick().await;
        let discovered = match pods
            .list(&ListParams::default().fields(&format!("spec.nodeName={}", config.node)))
            .await
        {
            Ok(pods) => pods,
            Err(error) => {
                if config.once {
                    return Err(error.into());
                }
                tracing::warn!(%error, "metric target discovery failed");
                continue;
            }
        };
        for pod in discovered.items {
            if pod.metadata.uid.as_deref().is_none_or(str::is_empty) {
                continue;
            }
            if pod.spec.as_ref().and_then(|s| s.node_name.as_deref()) != Some(config.node.as_str())
            {
                continue;
            }
            let annotations = pod.metadata.annotations.as_ref();
            if annotations
                .and_then(|a| a.get("prometheus.io/scrape"))
                .map(String::as_str)
                != Some("true")
            {
                continue;
            }
            if pod
                .metadata
                .labels
                .as_ref()
                .and_then(|l| l.get("sift.axiom.dev/scrape"))
                .map(String::as_str)
                != Some("true")
            {
                continue;
            }
            let Some(ip) = pod.status.as_ref().and_then(|s| s.pod_ip.as_deref()) else {
                continue;
            };
            let url = match target_url(ip, annotations) {
                Ok(url) => url,
                Err(error) => {
                    tracing::warn!(pod=pod.metadata.name.as_deref(),%error,"invalid metric target skipped");
                    continue;
                }
            };
            let samples = match fetch(&client, url).await {
                Ok(samples) => samples,
                Err(error) => {
                    tracing::warn!(pod=pod.metadata.name.as_deref(),%error,"metric scrape failed");
                    continue;
                }
            };
            let resource = BTreeMap::from([
                (
                    "service.name".into(),
                    pod.metadata
                        .labels
                        .as_ref()
                        .and_then(|l| l.get("app.kubernetes.io/name"))
                        .cloned()
                        .unwrap_or_else(|| pod.metadata.name.clone().unwrap_or_default()),
                ),
                (
                    "k8s.pod.uid".into(),
                    pod.metadata.uid.clone().unwrap_or_default(),
                ),
                (
                    "k8s.pod.name".into(),
                    pod.metadata.name.clone().unwrap_or_default(),
                ),
                (
                    "k8s.namespace.name".into(),
                    pod.metadata.namespace.clone().unwrap_or_default(),
                ),
                ("k8s.node.name".into(), config.node.clone()),
                ("sift.environment".into(), config.environment.clone()),
            ]);
            let now = chrono::Utc::now()
                .timestamp_nanos_opt()
                .context("scrape clock outside supported range")?;
            let mut queue = super::metric_queue::MetricQueue {
                project: config.project.clone(),
                environment: config.environment.clone(),
                node: config.node.clone(),
                next: 0,
                batches: samples
                    .chunks(1000)
                    .map(|batch| encode(batch, &resource, now))
                    .collect::<Result<Vec<_>>>()?,
            };
            queue.save(&pending)?;
            drain(
                &mut queue,
                &pending,
                &client,
                &ingest,
                &config,
                projected.as_ref(),
            )
            .await?;
        }
        if config.once {
            return Ok(());
        }
    }
}
fn target_url(ip: &str, annotations: Option<&BTreeMap<String, String>>) -> Result<url::Url> {
    let port = annotations
        .and_then(|a| a.get("prometheus.io/port"))
        .map(String::as_str)
        .unwrap_or("9090")
        .parse::<u16>()?;
    let path = annotations
        .and_then(|a| a.get("prometheus.io/path"))
        .map(String::as_str)
        .unwrap_or("/metrics");
    if port == 0 || !path.starts_with('/') || path.starts_with("//") {
        bail!("invalid annotated metric port or path");
    }
    let address = std::net::SocketAddr::new(ip.parse()?, port);
    let mut url = url::Url::parse(&format!("http://{address}"))?;
    url.set_path(path);
    Ok(url)
}
async fn drain(
    queue: &mut super::metric_queue::MetricQueue,
    path: &std::path::Path,
    client: &reqwest::Client,
    ingest: &url::Url,
    config: &ScrapeConfig,
    projected: Option<&service_auth::k8s::ProjectedTokenFile>,
) -> Result<()> {
    while queue.next < queue.batches.len() {
        let bytes = serde_json::to_vec(&queue.batches[queue.next])?;
        match deliver(client, ingest, config, projected, &bytes).await {
            Ok(()) => {
                queue.next += 1;
                queue.save(path)?;
            }
            Err(error) => {
                tracing::warn!(%error, "pending metric delivery retained");
                if config.once {
                    return Err(error);
                }
                tokio::time::sleep(config.interval.min(Duration::from_secs(30))).await;
            }
        }
    }
    Ok(())
}

async fn fetch(client: &reqwest::Client, url: url::Url) -> Result<Vec<ScrapedSample>> {
    let mut response = client
        .get(url)
        .header("Accept", "text/plain;version=0.0.4")
        .send()
        .await?
        .error_for_status()?;
    if response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.starts_with("text/plain"))
    {
        bail!("scrape requires Prometheus text 0.0.4");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            bail!("scrape response exceeds 8 MiB");
        }
        bytes.extend(chunk);
    }
    parse_text(std::str::from_utf8(&bytes)?).map_err(anyhow::Error::msg)
}
pub(crate) fn encode(
    samples: &[ScrapedSample],
    resource: &BTreeMap<String, String>,
    now: i64,
) -> Result<Value> {
    let metrics=samples.iter().map(|sample| {
        let end=sample.timestamp_millis.map(|t| t.checked_mul(1_000_000).context("scrape timestamp overflow")).transpose()?.unwrap_or(now);
        if end<=0 { bail!("invalid scrape timestamp"); }
        let point=json!({"timeUnixNano":end.to_string(),"asDouble":sample.value,"attributes":sample.labels.iter()
            .map(|(key,value)| json!({"key":key,"value":{"stringValue":value}})).collect::<Vec<_>>()});
        let data=if sample.counter { json!({"sum":{"aggregationTemporality":2,"isMonotonic":true,"dataPoints":[point]}}) }
            else { json!({"gauge":{"dataPoints":[point]}}) };
        let mut metric=data; metric["name"]=sample.name.clone().into(); Ok(metric)
    }).collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"resourceMetrics":[{"resource":{"attributes":resource.iter().map(|(key,value)|json!({"key":key,"value":{"stringValue":value}})).collect::<Vec<_>>()},"scopeMetrics":[{"metrics":metrics}]}]}),
    )
}
async fn deliver(
    client: &reqwest::Client,
    url: &url::Url,
    config: &ScrapeConfig,
    projected: Option<&service_auth::k8s::ProjectedTokenFile>,
    bytes: &[u8],
) -> Result<()> {
    let token = projected.map(|p| p.read()).transpose()?;
    let mut request = client
        .post(url.clone())
        .header("x-sift-project", &config.project)
        .header("Content-Type", "application/json")
        .body(bytes.to_vec());
    if let Some(token) = token.as_ref() {
        request = request.bearer_auth(token.expose());
    } else if let Some(token) = &config.token {
        request = request.bearer_auth(token);
    }
    let response: Value = request.send().await?.error_for_status()?.json().await?;
    if response
        .pointer("/partialSuccess/rejectedDataPoints")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0)
        != 0
    {
        bail!("Sift rejected metric points; pending batch retained");
    }
    Ok(())
}
