//! A malformed request has the same admission error on every wire adapter.
pub(crate) fn invalid(
    code: &'static str,
    message: impl Into<String>,
) -> crate::ingest::domain::admission_error::AdmissionError {
    crate::ingest::domain::admission_error::AdmissionError::invalid(code, message)
}
