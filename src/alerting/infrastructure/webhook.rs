//! Secrets are mounted files. The durable rule stores only the reference.
use crate::alerting::domain::{delivery_port::WebhookPort, state::Notification};
use crate::shared_kernel::port_future::PortFuture;
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

pub(crate) struct ConfiguredWebhook {
    pub secret_directory: PathBuf,
    pub allowed_hosts: BTreeSet<String>,
    pub client: reqwest::Client,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Destination {
    url: String,
    bearer_token: Option<String>,
}
impl ConfiguredWebhook {
    pub fn new(secret_directory: PathBuf, allowed_hosts: BTreeSet<String>) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            secret_directory,
            allowed_hosts,
            client,
        })
    }
    fn destination(&self, reference: &str) -> Result<Destination, String> {
        if reference.is_empty()
            || reference.len() > 128
            || !reference
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err("invalid Webhook Secret reference".into());
        }
        let bytes = std::fs::read(self.secret_directory.join(format!("{reference}.json")))
            .map_err(|_| "Webhook Secret is unavailable")?;
        if bytes.len() > 16 * 1024 {
            return Err("Webhook Secret exceeds 16 KiB".into());
        }
        let destination: Destination =
            serde_json::from_slice(&bytes).map_err(|_| "Webhook Secret is invalid")?;
        let url =
            url::Url::parse(&destination.url).map_err(|_| "Webhook destination URL is invalid")?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !url
                .host_str()
                .is_some_and(|host| self.allowed_hosts.contains(host))
        {
            return Err("Webhook destination is outside the HTTPS host allowlist".into());
        }
        Ok(destination)
    }
}
impl WebhookPort for ConfiguredWebhook {
    fn send<'a>(&'a self, notification: &'a Notification) -> PortFuture<'a, (), String> {
        Box::pin(async move {
            let destination = self.destination(&notification.webhook)?;
            let mut request = self
                .client
                .post(&destination.url)
                .header("Idempotency-Key", &notification.id)
                .header("X-Sift-Notification-Id", &notification.id)
                .json(&notification.payload);
            if let Some(token) = destination.bearer_token {
                request = request.bearer_auth(token);
            }
            // Do not include URLs, response bodies, or credentials in errors.
            let response = request
                .send()
                .await
                .map_err(|_| "Webhook request failed".to_string())?;
            if !response.status().is_success() {
                return Err(format!(
                    "Webhook returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
