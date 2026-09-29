//! Refusing a request the local disk cannot hold before it is decoded.

use anyhow::Result;

use crate::ingest::domain::admission_error::AdmissionError;
use crate::ingest::domain::storage_reservation::storage_reservation;
use crate::ServiceState;

impl ServiceState {
    pub(crate) fn ensure_local_capacity(
        &self,
        incoming_bytes: usize,
    ) -> Result<(), AdmissionError> {
        self.local_capacity
            .preflight(storage_reservation(incoming_bytes as u64, 1))
            .map_err(|error| AdmissionError::local_storage_backpressure(error.to_string()))
    }
}
