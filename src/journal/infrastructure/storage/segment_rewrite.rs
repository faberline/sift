//! Rewriting segments: reconciled and derived segments, evicting a sealed
//! segment, and moving immutable segments.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::journal::domain::segment_manifest::{AppendLocation, SegmentManifest, SegmentState};
use crate::journal::domain::shard_route::{bucket_for, Route};
use crate::journal::infrastructure::storage::segment_files::{
    event_time_unix_nano, read_events, sha256_file, verify_segment,
};
use crate::journal::infrastructure::storage::segment_store::SegmentStore;
use crate::shared_kernel::stored_event::StoredEvent;

impl SegmentStore {
    /// Materialize a prefix taken from a hash-verified archive checkpoint.
    /// The archive is authoritative, so its row can replace a conflicting row
    /// at the same local cursor. Normal retention continues to use the stricter
    pub(crate) fn write_reconciled_segment(
        &self,
        source_segment_id: &str,
        retained: &[StoredEvent],
    ) -> Result<Option<SegmentManifest>> {
        self.write_derived_segment(source_segment_id, retained)
    }

    fn write_derived_segment(
        &self,
        source_segment_id: &str,
        retained: &[StoredEvent],
    ) -> Result<Option<SegmentManifest>> {
        if retained.is_empty() {
            bail!("retained local segment cannot be empty");
        }
        if retained
            .windows(2)
            .any(|pair| pair[0].cursor >= pair[1].cursor)
        {
            bail!("retained local segment cursors must be strictly increasing");
        }

        let mut state = self.inner.lock().expect("segment state lock poisoned");
        let Some(source) = state.sealed.get(source_segment_id).cloned() else {
            return Ok(None);
        };
        verify_segment(&source)?;

        let mut identity = Sha256::new();
        identity.update(source.epoch.to_le_bytes());
        identity.update(source.shard.to_le_bytes());
        for event in retained {
            let encoded = serde_json::to_vec(event)?;
            identity.update(event.cursor.to_le_bytes());
            identity.update((encoded.len() as u64).to_le_bytes());
            identity.update(encoded);
        }
        let identity = hex::encode(identity.finalize());
        let segment_id = format!("retained-{}", &identity[..32]);
        if let Some(existing) = state.sealed.get(&segment_id).cloned() {
            verify_segment(&existing)?;
            if read_events(&existing.local_path)? != retained {
                bail!("retained segment identity collision for {segment_id}");
            }
            return Ok(Some(existing));
        }

        let parent = source
            .local_path
            .parent()
            .context("sealed segment has no parent directory")?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let sealed_path = parent.join(format!("{segment_id}.framed"));
        if sealed_path.exists() {
            if read_events(&sealed_path)? != retained {
                bail!(
                    "uncommitted retained segment {} has unexpected contents",
                    sealed_path.display()
                );
            }
        } else {
            let rewrite_path = parent.join(format!("{segment_id}.rewrite"));
            match fs::remove_file(&rewrite_path) {
                Ok(()) => storage_durable::sync_parent_dir(&rewrite_path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let mut writer = storage_durable::FramedLogWriter::open(
                &rewrite_path,
                storage_durable::FsyncPolicy::Interval,
            )?;
            for event in retained {
                writer.append(event.cursor, &serde_json::to_vec(event)?)?;
            }
            writer.sync()?;
            drop(writer);
            fs::rename(&rewrite_path, &sealed_path).with_context(|| {
                format!(
                    "commit retained segment {} -> {}",
                    rewrite_path.display(),
                    sealed_path.display()
                )
            })?;
            storage_durable::sync_parent_dir(&sealed_path)?;
        }
        fs::set_permissions(&sealed_path, fs::Permissions::from_mode(0o600))?;

        let mut bucket_min = u16::MAX;
        let mut bucket_max = 0_u16;
        let mut min_event_time_unix_nano = i64::MAX;
        let mut max_event_time_unix_nano = i64::MIN;
        for event in retained {
            let bucket = bucket_for(&event.event.event_id);
            let event_time = event_time_unix_nano(event)?;
            bucket_min = bucket_min.min(bucket);
            bucket_max = bucket_max.max(bucket);
            min_event_time_unix_nano = min_event_time_unix_nano.min(event_time);
            max_event_time_unix_nano = max_event_time_unix_nano.max(event_time);
        }
        let manifest = SegmentManifest {
            segment_id: segment_id.clone(),
            epoch: source.epoch,
            shard: source.shard,
            bucket_min,
            bucket_max,
            first_cursor: retained.first().expect("retained is non-empty").cursor,
            last_cursor: retained.last().expect("retained is non-empty").cursor,
            event_count: retained.len() as u64,
            min_event_time_unix_nano,
            max_event_time_unix_nano,
            bytes: fs::metadata(&sealed_path)?.len(),
            sha256: sha256_file(&sealed_path)?,
            state: SegmentState::Sealed,
            local_path: sealed_path,
            object_uri: None,
        };
        self.write_manifest(&manifest)?;
        for event in retained {
            state.cursors.insert(
                event.cursor,
                (
                    event.event.event_id.clone(),
                    AppendLocation {
                        route: Route {
                            epoch: source.epoch,
                            shard: source.shard,
                            bucket: bucket_for(&event.event.event_id),
                        },
                        segment_id: segment_id.clone(),
                        path: manifest.local_path.clone(),
                    },
                ),
            );
        }
        state.sealed.insert(segment_id, manifest.clone());
        Ok(Some(manifest))
    }

    /// Remove one local immutable copy after its remote archive receipt is
    /// durable. Renaming the manifest first is the crash boundary: a restart
    /// cannot reopen the segment after that rename.
    pub(crate) fn evict_segment(
        &self,
        segment_id: &str,
        receipt_root: &Path,
    ) -> Result<Option<SegmentManifest>> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        let Some(manifest) = state.sealed.get(segment_id).cloned() else {
            return Ok(None);
        };
        verify_segment(&manifest)?;
        fs::create_dir_all(receipt_root)?;
        fs::set_permissions(receipt_root, fs::Permissions::from_mode(0o700))?;
        let source_manifest = self.manifests_root.join(format!("{segment_id}.json"));
        let receipt = receipt_root.join(format!("{segment_id}.json"));
        if receipt.exists() {
            let prior: SegmentManifest = serde_json::from_slice(&fs::read(&receipt)?)?;
            if prior != manifest {
                bail!("local eviction receipt for {segment_id} changed");
            }
            if source_manifest.exists() {
                fs::remove_file(&source_manifest)?;
                storage_durable::sync_parent_dir(&source_manifest)?;
            }
        } else {
            fs::rename(&source_manifest, &receipt).with_context(|| {
                format!(
                    "move local segment manifest {} to eviction receipt {}",
                    source_manifest.display(),
                    receipt.display()
                )
            })?;
            fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600))?;
            storage_durable::sync_parent_dir(&source_manifest)?;
            storage_durable::sync_parent_dir(&receipt)?;
        }
        match fs::remove_file(&manifest.local_path) {
            Ok(()) => storage_durable::sync_parent_dir(&manifest.local_path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        state.sealed.remove(segment_id);
        state
            .cursors
            .retain(|_, (_, location)| location.segment_id != segment_id);
        Ok(Some(manifest))
    }

    pub fn active_paths(&self) -> Vec<PathBuf> {
        self.inner
            .lock()
            .expect("segment state lock poisoned")
            .active
            .values()
            .map(|active| active.path.clone())
            .collect()
    }

    pub fn move_segment(&self, segment_id: &str, destination: &Path) -> Result<SegmentManifest> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        let mut manifest = state
            .sealed
            .get(segment_id)
            .cloned()
            .with_context(|| format!("unknown sealed segment {segment_id}"))?;
        verify_segment(&manifest)?;
        fs::create_dir_all(destination)?;
        let target = destination.join(
            manifest
                .local_path
                .file_name()
                .context("segment path has no file name")?,
        );
        match fs::rename(&manifest.local_path, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                let bytes = fs::read(&manifest.local_path)?;
                storage_durable::atomic_write(
                    &target,
                    &bytes,
                    storage_durable::FsyncPolicy::Always,
                )?;
                let mut copied = manifest.clone();
                copied.local_path = target.clone();
                verify_segment(&copied)?;
                fs::remove_file(&manifest.local_path)?;
                storage_durable::sync_parent_dir(&manifest.local_path)?;
            }
            Err(error) => return Err(error.into()),
        }
        storage_durable::sync_parent_dir(&target)?;
        manifest.local_path = target.clone();
        manifest.state = SegmentState::Moved;
        verify_segment(&manifest)?;
        self.write_manifest(&manifest)?;
        state
            .sealed
            .insert(segment_id.to_string(), manifest.clone());
        Ok(manifest)
    }
}
