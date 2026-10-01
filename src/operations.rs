//! Operations: running Sift as a service. It owns the Kubernetes operator and
//! the Sift custom resource it reconciles, the deployment artifacts rendered
//! offline from checked-in templates, off-node snapshot backup and restore, the
//! gateway's reverse-proxy route policy, the OTLP/gRPC proxy, and the admin
//! endpoints that export a snapshot or verify one project's integrity.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;

pub use crate::operations::{
    application::journal_backup::{backup_journal, restore_journal},
    infrastructure::{
        live_snapshot_client::{
            backup_live_journal, backup_live_journal_authenticated, fetch_live_snapshot,
            fetch_live_snapshot_authenticated,
        },
        manifest_bundle::{
            collector_yaml, crd_yaml, dockerfile, instance_yaml, operator_yaml,
            operator_yaml_with_image, DockerfileVariant, InstanceProfile, DEFAULT_OPERATOR_IMAGE,
        },
    },
    interfaces::{
        crd::sift_spec::{
            ArchiveSpec, AuthMode, BackupSpec, BootstrapSpec, IngestSpec, PlacementSpec, Sift,
            SiftSpec, SiftStatus, StorageSpec,
        },
        http::gateway_proxy::{gateway_router, query_router},
        operator::run as run_operator,
    },
};

pub use crate::app::otlp_wiring::serve_proxy as serve_otlp_grpc_proxy;
