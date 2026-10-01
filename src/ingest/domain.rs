//! What ingest admits and on what terms: the limits a request is held to and
//! the error it is refused with, the governance policy applied before an event
//! is journaled, the id an OTLP item is given, and the local storage a batch
//! reserves.

pub(crate) mod admission_error;
pub(crate) mod governance_policy;
pub(crate) mod ingest_limits;
pub(crate) mod otlp_event_id;

pub(crate) mod batch_submission;
