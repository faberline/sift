//! How far each signal's journal is archived: the highest cursor per signal
//! that a committed archive covers.

use serde::{Deserialize, Serialize};

use crate::shared_kernel::event::SignalKind;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchiveWatermarks {
    pub logs: u64,
    pub metrics: u64,
    pub traces: u64,
}

impl ArchiveWatermarks {
    pub(crate) fn through(self, signal: SignalKind) -> u64 {
        match signal {
            SignalKind::Log => self.logs,
            SignalKind::Metric => self.metrics,
            SignalKind::Span => self.traces,
        }
    }

    pub(crate) fn covers(self, signal: SignalKind, cursor: u64) -> bool {
        cursor <= self.through(signal)
    }

    pub(crate) fn include(&mut self, signal: SignalKind, cursor: u64) {
        match signal {
            SignalKind::Log => self.logs = self.logs.max(cursor),
            SignalKind::Metric => self.metrics = self.metrics.max(cursor),
            SignalKind::Span => self.traces = self.traces.max(cursor),
        }
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            logs: self.logs.max(other.logs),
            metrics: self.metrics.max(other.metrics),
            traces: self.traces.max(other.traces),
        }
    }

    pub(crate) fn max_cursor(self) -> u64 {
        self.logs.max(self.metrics).max(self.traces)
    }
}
