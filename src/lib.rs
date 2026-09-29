//! Sift's service core for logs, metrics, and traces. The canonical per-signal
//! WAL is fsynced before acknowledgement. Rebuildable indexes are never a
//! second source of truth.

mod access;
pub mod api;
mod app;
mod archive;
pub mod auth;
pub mod backup;
pub mod collector;
pub mod deploy;
pub mod durability;
pub mod event;
pub mod grpc;
pub mod ingest;
mod journal;
pub mod mcp;
mod node;
mod operations;
pub mod operator;
pub mod projection;
pub mod prometheus;
pub mod proxy;
mod query;
mod shared_kernel;
pub mod storage;

pub use crate::app::interfaces::http::openapi::{openapi, openapi_json};
pub use crate::app::interfaces::http::router::{
    protected_router, protected_router_with_mcp, router,
};
pub use crate::app::service_state::ServiceState;
pub use crate::archive::interfaces::archive_worker::ArchiveWorker;
pub use crate::ingest::domain::governance_policy::{GovernancePolicy, GovernancePolicySet};
pub use crate::journal::domain::append_result::AppendResult;
pub use crate::journal::domain::event_query::EventQuery;
pub use crate::journal::infrastructure::durable_journal::DurableJournal;
pub use crate::journal::infrastructure::journal_projection_read_session::JournalProjectionReadSession;
pub use crate::operations::interfaces::http::integrity_report_v1::{
    IntegrityArchiveV1, IntegrityReportV1, IntegritySignalV1, IntegritySignalsV1,
    IntegrityStorageV1, IntegrityWalBytesV1, IntegrityWatermarksV1,
};
pub use crate::projection::interfaces::projection_worker::ProjectionWorker;
pub use crate::query::interfaces::http::query_role_router::query_role_router;
pub use crate::shared_kernel::stored_event::StoredEvent;
pub use event::{
    decode_event_json, AttributeValue, ContentBlobRef, EventEnvelope, IncomingEvent,
    InstrumentationScope, MetricExemplar, MetricPoint, MetricTemporality, OperationalEventV2,
    SignalKind, EVENT_SCHEMA_URL, EVENT_SCHEMA_VERSION,
};
