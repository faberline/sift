//! Parsing and building gs:// URIs for archive objects.

use anyhow::{bail, Context, Result};

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

pub(in crate::archive) fn split_gcs_uri(uri: &str) -> Result<(String, String)> {
    let value = uri
        .trim()
        .strip_prefix("gs://")
        .context("GCS object URI must start with gs://")?;
    let (bucket, key) = value
        .split_once('/')
        .context("GCS object URI must contain an object key")?;
    if bucket.is_empty() || key.is_empty() {
        bail!("GCS object URI must contain a bucket and object key");
    }
    Ok((bucket.to_string(), key.to_string()))
}

pub(in crate::archive) fn gcs_uri(bucket: &str, key: &str) -> String {
    format!("gs://{bucket}/{}", key.trim_start_matches('/'))
}

pub(in crate::archive) fn blob_hash_from_archive_uri(uri: &str) -> Option<String> {
    let file = uri.rsplit('/').next()?;
    let digest = file.strip_suffix(".blob")?;
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("sha256:{digest}"))
}
