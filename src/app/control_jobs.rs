//! Wire the leader worker to the same query API used by CLI and MCP.
use super::ServiceState;
use crate::alerting::{
    application::{
        delivery::DeliveryService,
        service::{AlertRule, AlertingService},
    },
    infrastructure::webhook::ConfiguredWebhook,
};
use crate::control::application::service::{ControlService, RecordKind};
use crate::error_reporting::application::service::ErrorReportingService;
use crate::query::application::sift_api_service::SiftApiClient;
use crate::shared_kernel::{
    projection_log::LogRecordV1,
    query_request::{QueryModeV1, QueryRequestV1, QuerySignalV1},
};
use anyhow::{Context, Result};
use std::{collections::BTreeSet, sync::Arc, time::Duration};

pub struct ControlWorker {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}
impl ControlWorker {
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}
impl ServiceState {
    pub fn start_control_worker(&self, local_endpoint: &str) -> Result<Option<ControlWorker>> {
        let Some(control) = self.control.clone() else {
            return Ok(None);
        };
        let projects: BTreeSet<String> = std::env::var("SIFT_CONTROL_PROJECTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        if projects.is_empty() {
            return Ok(None);
        }
        let endpoint =
            std::env::var("SIFT_QUERY_ENDPOINT").unwrap_or_else(|_| local_endpoint.into());
        let api = SiftApiClient::new(
            &endpoint,
            std::env::var("SIFT_TOKEN").ok(),
            Duration::from_secs(10),
        )?;
        let token_file = std::env::var("SIFT_TOKEN_FILE")
            .ok()
            .map(|path| service_auth::k8s::ProjectedTokenFile::new(path, "sift.axiom.dev"));
        let directory = std::env::var("SIFT_WEBHOOK_SECRET_DIRECTORY")
            .unwrap_or_else(|_| "/var/run/secrets/sift/webhooks".into());
        let hosts = std::env::var("SIFT_WEBHOOK_ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let webhook =
            Arc::new(ConfiguredWebhook::new(directory.into(), hosts).map_err(anyhow::Error::msg)?);
        let (stop, mut stopping) = tokio::sync::oneshot::channel();
        let serial = self.control_serial.clone();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut next_evaluation = 0;
            loop {
                tokio::select! { _ = &mut stopping => break, _ = interval.tick() => {} }
                if !matches!(control.is_leader().await, Ok(true)) {
                    continue;
                }
                let api = match &token_file {
                    Some(source) => match source.read() {
                        Ok(token) => api.with_token(Some(token.expose().into())),
                        Err(error) => {
                            tracing::warn!(%error, "control credential unavailable");
                            continue;
                        }
                    },
                    None => api.clone(),
                };
                let evaluate_rules = chrono::Utc::now().timestamp() >= next_evaluation;
                if evaluate_rules {
                    next_evaluation = chrono::Utc::now().timestamp() + 15;
                }
                let cycle = async {
                    let _serial = serial.lock().await;
                    run_cycle(
                        control.clone(),
                        &api,
                        &projects,
                        webhook.clone(),
                        evaluate_rules,
                    )
                    .await
                };
                tokio::select! {
                    _ = &mut stopping => break,
                    result = cycle => if let Err(error) = result { tracing::warn!(%error, "control cycle failed"); },
                }
            }
        });
        Ok(Some(ControlWorker { stop, task }))
    }
}

async fn run_cycle(
    control: Arc<ControlService>,
    api: &SiftApiClient,
    projects: &BTreeSet<String>,
    webhook: Arc<ConfiguredWebhook>,
    evaluate_rules: bool,
) -> Result<()> {
    let errors = ErrorReportingService::new(control.clone());
    let mut incomplete_projects = BTreeSet::new();
    for project in projects {
        let started = std::time::Instant::now();
        let mut complete = false;
        for _ in 0..32 {
            if !control.is_leader().await.map_err(anyhow::Error::msg)? {
                return Ok(());
            }
            let from = errors.cursor(project).await.map_err(anyhow::Error::msg)?;
            let mut request: QueryRequestV1 = serde_json::from_value(
                serde_json::json!({"version":1,"project":project,"signal":{"kind":"logs"}}),
            )?;
            request.mode = QueryModeV1::Sync;
            request.signal = QuerySignalV1::Logs { filter: None };
            request.limit = 1000;
            request.cursor = Some(
                crate::query::application::query_cursor::encode_query_cursor(
                    "logs",
                    &from.to_string(),
                ),
            );
            let response = match api.query(&request).await {
                Ok(response) if !response.partial => response,
                _ => {
                    tracing::warn!(%project, "error source unavailable or incomplete; cursor retained");
                    break;
                }
            };
            let records: Vec<LogRecordV1> = serde_json::from_value(
                response
                    .data
                    .get("records")
                    .cloned()
                    .context("missing error source records")?,
            )?;
            let mut cursor = from;
            for batch in records.chunks(500) {
                let through = batch.last().expect("nonempty error batch").cursor;
                errors
                    .ingest(project, cursor, through, batch)
                    .await
                    .map_err(anyhow::Error::msg)?;
                cursor = through;
            }
            let through = match response.next_cursor.as_deref() {
                Some(next) => {
                    crate::query::application::query_cursor::decode_query_cursor("logs", Some(next))
                        .map_err(|error| anyhow::anyhow!(error.message))?
                        .context("missing error source cursor")?
                        .parse::<u64>()?
                }
                None => response.watermark,
            };
            if through > cursor {
                errors
                    .ingest(project, cursor, through, &[])
                    .await
                    .map_err(anyhow::Error::msg)?;
            }
            if response.next_cursor.is_none() {
                complete = true;
                break;
            }
            if through <= from {
                anyhow::bail!("error source cursor did not advance");
            }
            if started.elapsed() > Duration::from_secs(3) {
                break;
            }
        }
        if !complete {
            incomplete_projects.insert(project.clone());
        }
    }
    let alerts = AlertingService::new(control.clone());
    for record in control
        .all(RecordKind::AlertRule)
        .await
        .map_err(anyhow::Error::msg)?
    {
        let Some(value) = record.value else {
            continue;
        };
        let rule: AlertRule = serde_json::from_value(value)?;
        if !evaluate_rules || !projects.contains(&rule.project) || !rule.enabled {
            continue;
        }
        let now = chrono::Utc::now().timestamp();
        let (values, cursor) = if incomplete_projects.contains(&rule.project)
            && matches!(
                rule.source,
                crate::alerting::application::service::RuleSource::NewError
                    | crate::alerting::application::service::RuleSource::ErrorRecurrence
            ) {
            (
                std::collections::BTreeMap::from([(
                    std::collections::BTreeMap::new(),
                    crate::alerting::application::service::Observation::Unknown(
                        "error source has not reached its complete watermark".into(),
                    ),
                )]),
                None,
            )
        } else {
            super::control_observations::observe(api, &control, &rule, now).await
        };
        alerts
            .evaluate(&record.id, &rule, values, cursor, now)
            .await
            .map_err(anyhow::Error::msg)?;
    }
    DeliveryService { control, webhook }
        .deliver_due(chrono::Utc::now().timestamp())
        .await
        .map_err(anyhow::Error::msg)
}
