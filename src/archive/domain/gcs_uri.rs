//! Pure GCS URI data and validation.

#[derive(Debug, thiserror::Error)]
pub(crate) enum GcsUriError {
    #[error("GCS object URI must start with gs://")]
    Prefix,
    #[error("GCS object URI must contain an object key")]
    Key,
    #[error("GCS object URI must contain a bucket and object key")]
    Bucket,
}

pub(in crate::archive) fn split_gcs_uri(uri: &str) -> Result<(String, String), GcsUriError> {
    let value = uri
        .trim()
        .strip_prefix("gs://")
        .ok_or(GcsUriError::Prefix)?;
    let (bucket, key) = value.split_once('/').ok_or(GcsUriError::Key)?;
    if bucket.is_empty() || key.is_empty() {
        return Err(GcsUriError::Bucket);
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
