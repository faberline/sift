//! Parsing and building gs:// URIs for archive objects.

use anyhow::{bail, Result};

pub(in crate::archive) fn gcs_destination(
    destination: &service_backup::BackupDestination,
) -> Result<(String, String)> {
    let service_backup::BackupDestination::Gcs {
        bucket,
        prefix,
        credentials_secret,
    } = destination
    else {
        bail!("{} is not a GCS backup destination", destination.identity());
    };
    if let Some(secret) = credentials_secret {
        bail!(
            "GCS credentials_secret `{secret}` is not supported; use ADC and GKE Workload Identity"
        );
    }
    Ok((
        bucket.clone(),
        if prefix.is_empty() {
            "backup".to_string()
        } else {
            prefix.trim_matches('/').to_string()
        },
    ))
}
