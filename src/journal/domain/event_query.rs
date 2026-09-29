//! A query for journal events after a cursor.

use serde::Deserialize;

use crate::event::SignalKind;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EventQuery {
    pub signal: Option<SignalKind>,
    pub after: u64,
    pub limit: usize,
}
