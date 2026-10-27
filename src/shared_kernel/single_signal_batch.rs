//! A durable batch holds events of exactly one signal. Each caller names its
//! batch, and the error reads "<batch> must ...".

use thiserror::Error;

use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;

#[derive(Debug, Error)]
pub(crate) enum SingleSignalError {
    #[error("{batch} must not be empty")]
    Empty { batch: &'static str },
    #[error("{batch} must contain exactly one signal")]
    MixedSignals { batch: &'static str },
}

pub(crate) fn ensure_single_signal(
    batch: &'static str,
    events: &[EventEnvelope],
) -> Result<(), SingleSignalError> {
    let signal = events
        .first()
        .ok_or(SingleSignalError::Empty { batch })?
        .signal;
    if events.iter().any(|event| event.signal != signal) {
        return Err(SingleSignalError::MixedSignals { batch });
    }
    Ok(())
}
