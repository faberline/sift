//! signal service over the original resource port.

use crate::archive::domain::resource_ports::*;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;

pub(crate) struct SignalService {
    port: Box<dyn ArchiveSignalPort<Error = anyhow::Error>>,
}
impl SignalService {
    pub(crate) fn new(port: Box<dyn ArchiveSignalPort<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(crate) fn peek_cursor(&mut self) -> Result<Option<u64>> {
        self.port.peek_cursor()
    }
    pub(crate) fn pop_event(&mut self) -> Result<Option<StoredEvent>> {
        self.port.pop_event()
    }
}
