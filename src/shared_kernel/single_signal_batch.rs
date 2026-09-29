//! A durable batch holds events of exactly one signal.

use anyhow::{bail, Context, Result};

use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;

pub(crate) fn ensure_single_signal(events: &[EventEnvelope]) -> Result<()> {
    let signal = events
        .first()
        .context("Sift batch must not be empty")?
        .signal;
    if events.iter().any(|event| event.signal != signal) {
        bail!("Sift batch must contain exactly one signal");
    }
    Ok(())
}
