//! The event constructors that read the system clock. The shared kernel's
//! `for_project_at` takes the time, and these pass it `Utc::now()`.

use chrono::Utc;
use serde_json::Value;

use crate::{OperationalEventV2, SignalKind};

impl OperationalEventV2 {
    /// Compatibility constructor for bootstrap callers. Ingest producers
    /// should use [`for_project`](Self::for_project) with explicit tenancy.
    pub fn new(event_id: impl Into<String>, signal: SignalKind, payload: Value) -> Self {
        Self::for_project("default", "default", event_id, signal, payload)
    }

    pub fn for_project(
        project: impl Into<String>,
        environment: impl Into<String>,
        event_id: impl Into<String>,
        signal: SignalKind,
        payload: Value,
    ) -> Self {
        Self::for_project_at(project, environment, event_id, signal, payload, Utc::now())
    }
}
