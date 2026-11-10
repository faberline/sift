//! Content-addressed control snapshots and a conditional latest pointer.
use crate::shared_kernel::{recovery_record::RecoveryRecord, sha256_hex::sha256_hex};
use anyhow::{bail, Context, Result};
use storage_object::{ObjectStore, ObjectStoreError, PutCondition};
pub(crate) struct RecoveryStore {
    store: storage_object::GcsObjectStore,
    bucket: String,
    prefix: String,
}
impl RecoveryStore {
    pub fn new(uri: &str) -> Result<Self> {
        let url = url::Url::parse(uri)?;
        if url.scheme() != "gs"
            || url.host_str().is_none()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("recovery destination must be a GCS prefix");
        }
        let bucket = url.host_str().expect("GCS bucket").to_string();
        let prefix = format!("{}/recovery", url.path().trim_matches('/'));
        if prefix.starts_with('/') {
            bail!("recovery destination requires a nonempty prefix");
        }
        Ok(Self {
            store: storage_object::GcsObjectStore::new(&bucket, "")?,
            bucket,
            prefix,
        })
    }
    pub fn latest(&self) -> Result<RecoveryRecord> {
        let object = self.store.get(&format!("{}/latest.json", self.prefix))?;
        if object.bytes.len() > 64 * 1024 {
            bail!("recovery record exceeds 64 KiB");
        }
        let record: RecoveryRecord = serde_json::from_slice(&object.bytes)?;
        record.validate().map_err(anyhow::Error::msg)?;
        Ok(record)
    }
    pub fn commit(&self, record: &RecoveryRecord, snapshot: &[u8]) -> Result<()> {
        record.validate().map_err(anyhow::Error::msg)?;
        if snapshot.len() > 64 * 1024 * 1024 || sha256_hex(snapshot) != record.control_sha256 {
            bail!("control snapshot size or digest mismatch");
        }
        let key = format!("{}/control-{}.bin", self.prefix, record.control_sha256);
        if record.control_uri != format!("gs://{}/{key}", self.bucket) {
            bail!("control recovery URI differs from destination");
        }
        match self.store.put(
            &key,
            snapshot,
            "application/octet-stream",
            PutCondition::IfAbsent,
        ) {
            Ok(_) => {}
            Err(ObjectStoreError::PreconditionFailed { .. }) => {
                if sha256_hex(&self.store.get(&key)?.bytes) != record.control_sha256 {
                    bail!("existing control snapshot digest mismatch");
                }
            }
            Err(error) => return Err(error.into()),
        }
        let bytes = serde_json::to_vec(record)?;
        let immutable = format!(
            "{}/records/{}-{}.json",
            self.prefix,
            record.protected_at,
            sha256_hex(&bytes)
        );
        match self.store.put(
            &immutable,
            &bytes,
            "application/json",
            PutCondition::IfAbsent,
        ) {
            Ok(_) => {}
            Err(ObjectStoreError::PreconditionFailed { .. }) => {
                if self.store.get(&immutable)?.bytes != bytes {
                    bail!("existing immutable recovery record differs");
                }
            }
            Err(error) => return Err(error.into()),
        }
        let latest = format!("{}/latest.json", self.prefix);
        let condition = match self.store.get(&latest) {
            Ok(previous) => {
                let prior: RecoveryRecord = serde_json::from_slice(&previous.bytes)?;
                prior.validate().map_err(anyhow::Error::msg)?;
                if prior.protected_at > record.protected_at {
                    bail!("recovery pointer cannot move backwards");
                }
                PutCondition::IfVersion(previous.meta.version)
            }
            Err(ObjectStoreError::NotFound { .. }) => PutCondition::IfAbsent,
            Err(error) => return Err(error.into()),
        };
        self.store
            .put(&latest, &bytes, "application/json", condition)
            .context("commit complete recovery pointer")?;
        Ok(())
    }
    pub fn control_uri(&self, digest: &str) -> String {
        format!("gs://{}/{}/control-{digest}.bin", self.bucket, self.prefix)
    }
    pub fn control_snapshot(&self, record: &RecoveryRecord) -> Result<Vec<u8>> {
        record.validate().map_err(anyhow::Error::msg)?;
        if record.control_uri != self.control_uri(&record.control_sha256) {
            bail!("control snapshot is outside the configured recovery prefix");
        }
        let key = format!("{}/control-{}.bin", self.prefix, record.control_sha256);
        let bytes = self.store.get(&key)?.bytes;
        if bytes.len() > 64 * 1024 * 1024 || sha256_hex(&bytes) != record.control_sha256 {
            bail!("control snapshot digest or size mismatch");
        }
        let payload = bytes
            .strip_prefix(b"SIFT-CONTROL-1\n")
            .context("invalid control snapshot framing")?;
        let state: serde_json::Value = serde_json::from_slice(payload)?;
        if state["applied_index"].as_u64() != Some(record.control_index) {
            bail!("control snapshot index differs from recovery record");
        }
        Ok(bytes)
    }
    pub fn verify_data_manifest(&self, record: &RecoveryRecord) -> Result<()> {
        let url = url::Url::parse(&record.data.manifest_uri)?;
        let archive_prefix = self
            .prefix
            .strip_suffix("/recovery")
            .context("archive prefix")?;
        if url.scheme() != "gs"
            || url.host_str() != Some(self.bucket.as_str())
            || !url
                .path()
                .trim_start_matches('/')
                .starts_with(&format!("{archive_prefix}/"))
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("data manifest is outside the configured recovery bucket");
        }
        let object = self.store.get(url.path().trim_start_matches('/'))?;
        if sha256_hex(&object.bytes) != record.data.manifest_sha256 {
            bail!("data recovery manifest digest mismatch");
        }
        let manifest: crate::shared_kernel::archive_manifest::ArchiveManifest =
            serde_json::from_slice(&object.bytes)?;
        if manifest.watermarks.max_cursor() != record.data.through_cursor {
            bail!("data recovery watermark differs from recovery record");
        }
        Ok(())
    }
}
