//! The joint recovery worker and the shared write-admission guard.
use super::ServiceState;
use crate::operations::infrastructure::recovery_store::RecoveryStore;
use crate::shared_kernel::{
    recovery_record::{DataRecovery, RecoveryRecord},
    sha256_hex::sha256_hex,
};
use anyhow::{bail, Context, Result};
use std::{sync::Arc, time::Duration};

pub struct RecoveryWorker {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

pub fn restore_complete_recovery(
    destination: &str,
    store_dir: &std::path::Path,
    control_dir: &std::path::Path,
) -> Result<RecoveryRecord> {
    restore_complete_recovery_pinned(destination, store_dir, control_dir, None)
}
pub fn restore_complete_recovery_pinned(
    destination: &str,
    store_dir: &std::path::Path,
    control_dir: &std::path::Path,
    record_file: Option<&std::path::Path>,
) -> Result<RecoveryRecord> {
    if store_dir == control_dir {
        bail!("store and control restore directories must differ");
    }
    for path in [store_dir, control_dir] {
        if path.exists() && std::fs::read_dir(path)?.next().is_some() {
            bail!("restore requires empty directories");
        }
    }
    let remote = RecoveryStore::new(destination)?;
    let record = if let Some(path) = record_file {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            bail!("pinned recovery record must be a regular file of at most 64 KiB");
        }
        let record: RecoveryRecord = serde_json::from_slice(&std::fs::read(path)?)?;
        record.validate().map_err(anyhow::Error::msg)?;
        record
    } else {
        remote.latest()?
    };
    remote.verify_data_manifest(&record)?;
    let control = remote.control_snapshot(&record)?;
    crate::control::infrastructure::state_machine::ControlStateMachine::validate_bytes(&control)?;
    super::archive_wiring::restore_gcs(&record.data.manifest_uri, store_dir)?;
    crate::control::infrastructure::state_machine::ControlStateMachine::bootstrap(
        control_dir,
        &control,
    )?;
    Ok(record)
}
impl RecoveryWorker {
    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}
impl ServiceState {
    pub(crate) fn archive_serial(&self) -> Arc<tokio::sync::Mutex<()>> {
        self.archive_serial.clone()
    }
    pub(crate) async fn capture_recovery_data(&self) -> Result<DataRecovery> {
        if self.control.is_some() && self.raft.is_some() {
            bail!("recovery data must be requested from the store role");
        }
        if let Some(raft) = &self.raft {
            if !raft.is_leader().await {
                bail!("retry recovery data capture on the store leader");
            }
        }
        let destination = std::env::var("SIFT_ARCHIVE_DESTINATION")
            .context("remote data archive is not configured")?;
        if !destination.starts_with("gs://") {
            bail!("production recovery requires a GCS data archive");
        }
        let _serial = self.archive_serial.lock().await;
        let captured_at = chrono::Utc::now().timestamp();
        let minimum = self.journal.last_cursor();
        crate::archive::application::lifecycle_tick::run_lifecycle_tick(
            &self.lifecycle_context(Some(destination)),
        )
        .await;
        let status = crate::archive::committed_status(self.journal.storage().root())?
            .context("data archive did not commit")?;
        if status.watermarks.max_cursor() < minimum || !status.manifest_uri.starts_with("gs://") {
            bail!("data recovery prefix is incomplete");
        }
        Ok(DataRecovery {
            manifest_uri: status.manifest_uri,
            manifest_sha256: status.manifest_sha256,
            protected_at: captured_at,
            through_cursor: status.watermarks.max_cursor(),
        })
    }
    pub async fn start_recovery_worker(
        &self,
        local_endpoint: &str,
    ) -> Result<Option<RecoveryWorker>> {
        let Ok(destination) = std::env::var("SIFT_RECOVERY_DESTINATION") else {
            return Ok(None);
        };
        let store =
            Arc::new(tokio::task::spawn_blocking(move || RecoveryStore::new(&destination)).await??);
        let state = self.clone();
        let endpoint =
            std::env::var("SIFT_STORE_ENDPOINT").unwrap_or_else(|_| local_endpoint.into());
        let project =
            std::env::var("SIFT_RECOVERY_PROJECT").context("SIFT_RECOVERY_PROJECT is required")?;
        let mut url = url::Url::parse(&endpoint)?;
        url.set_path("/api/v1/recovery/data");
        url.set_query(None);
        url.set_fragment(None);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let (stop, mut stopping) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_commit = None;
            loop {
                tokio::select! { _=&mut stopping=>break,_=interval.tick()=>{} }
                if let Some(control) = &state.control {
                    if matches!(control.is_leader().await, Ok(true))
                        && last_commit.is_none_or(|last: i64| {
                            chrono::Utc::now().timestamp().saturating_sub(last) >= 300
                        })
                    {
                        let result = tokio::select! { _=&mut stopping=>break,result=commit_joint(&state,&store,&http,&url,&project)=>result };
                        match result {
                            Ok(()) => last_commit = Some(chrono::Utc::now().timestamp()),
                            Err(error) => {
                                tracing::warn!(%error,"joint recovery record was not committed")
                            }
                        }
                    }
                }
                let reader = store.clone();
                let result = tokio::select! { _=&mut stopping=>break,result=tokio::task::spawn_blocking(move||reader.latest())=>result };
                match result {
                    Ok(Ok(record)) => {
                        if let Err(error) = state
                            .recovery_guard
                            .accept_record(&record, chrono::Utc::now().timestamp())
                        {
                            tracing::warn!(%error,"invalid remote recovery record");
                        }
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(%error,"remote recovery record is unavailable")
                    }
                    Err(error) => tracing::warn!(%error,"remote recovery reader failed"),
                }
            }
        });
        Ok(Some(RecoveryWorker { stop, task }))
    }
}
async fn commit_joint(
    state: &ServiceState,
    store: &Arc<RecoveryStore>,
    http: &reqwest::Client,
    url: &url::Url,
    project: &str,
) -> Result<()> {
    let _serial = state.control_serial.lock().await;
    let control = state
        .control_service()
        .map_err(|e| anyhow::anyhow!(e.message))?;
    let mut request = http
        .post(url.clone())
        .header("x-sift-project", project)
        .json(&serde_json::json!({"project":project}));
    if let Ok(path) = std::env::var("SIFT_TOKEN_FILE") {
        let token = service_auth::k8s::ProjectedTokenFile::new(path, "sift.axiom.dev").read()?;
        request = request.bearer_auth(token.expose());
    } else if let Ok(token) = std::env::var("SIFT_TOKEN") {
        request = request.bearer_auth(token);
    }
    let data: DataRecovery = request.send().await?.error_for_status()?.json().await?;
    if !control.is_leader().await.map_err(anyhow::Error::msg)? {
        bail!("control leadership changed before recovery capture");
    }
    let snapshot = control.snapshot_bytes().map_err(anyhow::Error::msg)?;
    let payload = snapshot
        .strip_prefix(b"SIFT-CONTROL-1\n")
        .context("unexpected control snapshot format")?;
    let decoded: serde_json::Value = serde_json::from_slice(payload)?;
    for value in decoded["records"]
        .as_object()
        .context("missing control records")?
        .values()
    {
        if value["kind"] == "error_group"
            && value["value"]["last_cursor"]
                .as_u64()
                .is_some_and(|cursor| cursor > data.through_cursor)
        {
            bail!("control error state is ahead of the data recovery prefix; retry capture");
        }
    }
    let now = chrono::Utc::now().timestamp();
    let digest = sha256_hex(&snapshot);
    let record = RecoveryRecord {
        version: 1,
        created_at: now,
        protected_at: data.protected_at.min(now),
        data,
        control_uri: store.control_uri(&digest),
        control_sha256: digest,
        control_index: decoded["applied_index"]
            .as_u64()
            .context("missing control snapshot index")?,
    };
    let writer = store.clone();
    let persisted_record = record.clone();
    tokio::task::spawn_blocking(move || writer.commit(&record, &snapshot)).await??;
    let previous = control
        .list(
            project,
            crate::control::application::service::RecordKind::Recovery,
        )
        .await
        .map_err(anyhow::Error::msg)?
        .into_iter()
        .find(|record| record.id == "latest");
    control
        .transact(
            format!("recovery-{}", persisted_record.control_sha256),
            vec![crate::control::application::service::Change {
                project: project.into(),
                kind: crate::control::application::service::RecordKind::Recovery,
                id: "latest".into(),
                expected_revision: previous.map_or(0, |r| r.revision),
                value: Some(serde_json::to_value(persisted_record)?),
            }],
        )
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(())
}
