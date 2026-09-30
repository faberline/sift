//! A durable batch holds events of exactly one signal.

use thiserror::Error;

use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;

#[derive(Debug, Error)]
pub(crate) enum SingleSignalError {
    #[error("Sift batch must not be empty")]
    Empty,
    #[error("Sift batch must contain exactly one signal")]
    MixedSignals,
}

pub(crate) fn ensure_single_signal(events: &[EventEnvelope]) -> Result<(), SingleSignalError> {
    let signal = events.first().ok_or(SingleSignalError::Empty)?.signal;
    if events.iter().any(|event| event.signal != signal) {
        return Err(SingleSignalError::MixedSignals);
    }
    Ok(())
}
