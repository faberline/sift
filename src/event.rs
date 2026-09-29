//! The versioned event model's public paths. The shared kernel owns the
//! event model and the ingest context owns the governance policy; these
//! re-exports keep `sift::event::*` compiling for callers outside the crate.

pub use crate::ingest::domain::governance_policy::{GovernancePolicy, GovernancePolicySet};
pub use crate::shared_kernel::event::{
    decode_event_json, AttributeValue, ContentBlobRef, IncomingEvent, InstrumentationScope,
    MetricExemplar, MetricPoint, MetricTemporality, OperationalEventV2, SignalKind,
    EVENT_SCHEMA_URL, EVENT_SCHEMA_VERSION,
};

/// Short name used by the internal ingest and storage layers.
pub use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;
