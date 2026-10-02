//! A fsynced pending batch. Delivery retries retain the exact event.
use super::client::CollectorClient;
use crate::collector::domain::config::CollectorConfig;
use crate::OperationalEventV2;
use anyhow::{Context, Result};
use service_collector::{BatchSink, DeliveryFailure, DeliveryReceipt};
use std::path::PathBuf;

pub(crate) struct DurableDelivery {
    pub client: CollectorClient,
    pub path: PathBuf,
    pub namespace: Option<String>,
}

impl DurableDelivery {
    pub async fn replay(&self, config: &CollectorConfig) -> Result<()> {
        if self.path.exists() {
            let metadata = std::fs::symlink_metadata(&self.path)?;
            if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
                anyhow::bail!("pending collector batch must be a regular file within 64 MiB");
            }
            let pending: Vec<OperationalEventV2> = serde_json::from_slice(
                &std::fs::read(&self.path).context("read pending collector batch")?,
            )?;
            if pending.len() > 1000
                || pending.iter().any(|event| {
                    event.project != config.project
                        || event.environment != config.environment
                        || match &config.source {
                            crate::collector::domain::config::SourceSpec::Cri(cri) => {
                                cri.namespace.as_ref().is_some_and(|namespace| {
                                    event.resource.get("k8s.namespace.name") != Some(namespace)
                                })
                            }
                            _ => false,
                        }
                })
            {
                anyhow::bail!(
                    "pending batch exceeds quota or belongs to another project or environment"
                );
            }
            if !pending.is_empty() {
                let mut attempts = 0usize;
                let mut backoff = config.initial_backoff;
                loop {
                    match self.client.send(&pending).await {
                        Ok(_) => break,
                        Err(error)
                            if error.is_retryable()
                                && (config.follow || attempts < config.max_retries) =>
                        {
                            tracing::warn!("pending collector delivery will retry");
                            tokio::time::sleep(backoff.min(std::time::Duration::from_secs(5)))
                                .await;
                            backoff = backoff.saturating_mul(2);
                            attempts = attempts.saturating_add(1);
                        }
                        Err(error) => return Err(anyhow::anyhow!("pending delivery: {error}")),
                    }
                }
                self.clear()?;
            }
        }
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        // Source replay after a crash uses the same source-derived IDs.
        storage_durable::atomic_write(&self.path, b"[]", storage_durable::FsyncPolicy::Always)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl BatchSink<OperationalEventV2> for DurableDelivery {
    async fn send(
        &self,
        events: &[OperationalEventV2],
    ) -> Result<DeliveryReceipt, DeliveryFailure> {
        if self.namespace.as_ref().is_some_and(|namespace| {
            events
                .iter()
                .any(|event| event.resource.get("k8s.namespace.name") != Some(namespace))
        }) {
            return Err(DeliveryFailure::permanent(
                "queued log belongs to another Kubernetes namespace; keep its source state",
            ));
        }
        let bytes = serde_json::to_vec(events)
            .map_err(|error| DeliveryFailure::permanent(error.to_string()))?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(DeliveryFailure::permanent(
                "pending collector batch exceeds 64 MiB",
            ));
        }
        storage_durable::atomic_write(&self.path, &bytes, storage_durable::FsyncPolicy::Always)
            .map_err(|error| {
                DeliveryFailure::retryable(format!("persist pending batch: {error}"))
            })?;
        let receipt = self.client.send(events).await?;
        self.clear()
            .map_err(|error| DeliveryFailure::retryable(format!("clear pending batch: {error}")))?;
        Ok(receipt)
    }
}
