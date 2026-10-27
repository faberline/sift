//! Refusing a request the local disk cannot hold before it is decoded.

use anyhow::Result;

use crate::ingest::application::ingest_service::IngestService;
use crate::ingest::domain::admission_error::AdmissionError;
use crate::shared_kernel::storage_reservation::storage_reservation;

impl IngestService {
    pub(crate) fn ensure_local_capacity(
        &self,
        incoming_bytes: usize,
    ) -> Result<(), AdmissionError> {
        self.capacity
            .preflight(storage_reservation(incoming_bytes as u64, 1))
            .map_err(|error| AdmissionError::local_storage_backpressure(error.to_string()))
    }
}
