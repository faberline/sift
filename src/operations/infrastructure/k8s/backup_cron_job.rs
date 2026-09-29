//! The scheduled backup runner: a CronJob that fetches a live snapshot and
//! ships it to the configured destination.

use serde_json::json;
use service_k8s::render::{self, RenderCtx};

use crate::operations::infrastructure::k8s::agent_workloads::{
    projected_client_token_mount, projected_client_token_volume,
};
use crate::operations::infrastructure::k8s::constants::{BACKUP_COMPONENT, HTTP_PORT};
use crate::operations::infrastructure::k8s::role_workloads::{
    container_security_context, role_selector_labels,
};
use crate::operations::interfaces::crd::sift_spec::{AuthMode, BackupSpec, SiftSpec};

pub(super) fn backup_cron_job_plan(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    backup: &BackupSpec,
) -> render::CronJobPlan {
    let backup_name = format!("{}-backup", cx.name);
    let mut args = vec![
        "backup".to_string(),
        "--url".to_string(),
        format!("http://{}.{}.svc.cluster.local:{HTTP_PORT}", cx.name, cx.ns),
        "--dest".to_string(),
        backup.destination.clone(),
    ];
    if let Some(seconds) = backup.retention_secs {
        args.push("--retention-secs".to_string());
        args.push(seconds.to_string());
    }
    let mut env = Vec::new();
    let mut volumes = Vec::new();
    let mut mounts = Vec::new();
    if matches!(spec.auth, AuthMode::Kubernetes) {
        args.extend([
            "--token-file".to_string(),
            "/var/run/secrets/sift/client/token".to_string(),
            "--token-audience".to_string(),
            "sift.axiom.dev".to_string(),
            "--project".to_string(),
            cx.name.to_string(),
        ]);
        volumes.push(projected_client_token_volume());
        mounts.push(projected_client_token_mount());
    } else if let Some(secret) = &backup.admin_token_secret {
        env.push(json!({
            "name": "SIFT_BACKUP_TOKEN",
            "valueFrom": {"secretKeyRef": {"name": secret, "key": "token"}}
        }));
    }
    let runtime = render::PodRuntimePolicy::restricted(
        &backup_name,
        serde_json::to_value(&spec.placement.node_selector).expect("node selector is JSON"),
    )
    .with_volumes(volumes)
    .with_restart_policy("OnFailure");
    let mut container = render::ContainerPlan::new("backup", &spec.image, args);
    container.env = env;
    container.volume_mounts = mounts;
    container.security_context = Some(container_security_context());
    container.resources = Some(json!({
        "requests":{"cpu":"100m","memory":"128Mi"},
        "limits":{"cpu":"500m","memory":"512Mi"}
    }));
    render::CronJobPlan {
        name: backup_name,
        schedule: backup.schedule.clone(),
        successful_jobs_history_limit: 3,
        failed_jobs_history_limit: 3,
        pod: render::PodPlan::new(BACKUP_COMPONENT, container, runtime)
            .with_selector_labels(role_selector_labels(BACKUP_COMPONENT)),
    }
}
