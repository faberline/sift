//! What the collector reads and how it judges it: the configuration and its
//! bounds, a record and the cursor that commits it, the quarantine entry, the
//! service-log rules, and CRI's framing and workload identity.

pub(crate) mod config;
pub(crate) mod cri;
pub(crate) mod quarantine;
pub(crate) mod record;
pub(crate) mod service_log;
