//! The Kubernetes objects the operator renders through service_k8s: the
//! constants and labels, the managed-service adapter, the store, control,
//! query, gateway and agent workloads, the backup CronJob, and the network
//! policies.

pub(crate) mod agent_workloads;
pub(crate) mod api_endpoint_discovery;
pub(crate) mod backup_cron_job;
pub(crate) mod constants;
pub(crate) mod managed_service;
pub(crate) mod network_policy;
pub(crate) mod role_workloads;
