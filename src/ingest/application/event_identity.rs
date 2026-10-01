//! The stable OTLP event identity used by each decoder.
pub(crate) fn stable_id(kind: &str, project: &str, identity: &str) -> String {
    crate::ingest::domain::otlp_event_id::stable_id(kind, project, identity)
}
