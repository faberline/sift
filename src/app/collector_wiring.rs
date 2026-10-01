//! Create the collector adapters before entering its application runtime.

use crate::collector::application::run_collector::CollectorSummary;
use crate::collector::application::runtime;
use crate::collector::domain::config::CollectorConfig;
use crate::collector::infrastructure::client::CollectorClient;
use crate::collector::infrastructure::service_log_decoder::ServiceLogDecoder;
use crate::collector::infrastructure::source::open_source;
use anyhow::Result;

pub async fn run_collector(config: CollectorConfig) -> Result<CollectorSummary> {
    config.validate()?;
    let mut source = open_source(&config)?;
    let mut client = CollectorClient::new(
        &config.endpoint,
        &config.project,
        config.token.clone(),
        config.request_timeout,
    )?;
    if let Some(path) = &config.token_file {
        client = client.with_projected_token_file(path.clone(), config.token_audience.clone());
    }
    runtime::run(
        &config,
        &mut *source,
        &ServiceLogDecoder {
            project: &config.project,
            environment: &config.environment,
        },
        &client,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::collector::domain::config::{
        SourceSpec, DEFAULT_BATCH_SIZE, DEFAULT_MAX_LINE_BYTES, DEFAULT_MAX_RETRIES,
    };
    use crate::collector::infrastructure::checkpoint::CollectorCheckpoint;

    #[tokio::test]
    async fn truncated_file_is_refused_before_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("service.jsonl");
        let checkpoint_path = temp.path().join("checkpoint.json");
        std::fs::write(&source, b"{}\n").unwrap();
        let mut checkpoint = CollectorCheckpoint::new("fixture");
        checkpoint.offset = 100;
        checkpoint.save(&checkpoint_path).unwrap();
        let config = CollectorConfig {
            source: SourceSpec::File(source),
            source_id: "fixture".to_string(),
            endpoint: "http://127.0.0.1:7380".to_string(),
            token: None,
            token_file: None,
            token_audience: "sift.axiom.dev".to_string(),
            project: "local".to_string(),
            environment: "test".to_string(),
            checkpoint_path,
            quarantine_path: temp.path().join("rejected.jsonl"),
            batch_size: DEFAULT_BATCH_SIZE,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            max_retries: DEFAULT_MAX_RETRIES,
            request_timeout: Duration::from_secs(1),
            initial_backoff: Duration::from_millis(1),
            follow: false,
            follow_poll_interval: Duration::from_millis(1),
        };
        let error = run_collector(config).await.unwrap_err().to_string();
        assert!(error.contains("truncated or rotated"));
    }
}
