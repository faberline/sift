//! Enforce compressed and decoded sizes, event count and size, per-project
//! quota, concurrency, draining, and overload errors.

use std::time::Duration;

use axum::http::StatusCode;

use crate::ingest::domain::admission_error::AdmissionError;
use crate::ingest::domain::ingest_limits::IngestLimits;

#[derive(Debug)]
pub struct AdmissionController {
    pub(in crate::ingest) limits: IngestLimits,
    projects: service_http::WeightedAdmission<String>,
}

impl AdmissionController {
    pub fn new(limits: IngestLimits) -> anyhow::Result<Self> {
        limits.validate()?;
        let policy = service_http::WeightedAdmissionConfig::new(
            limits.max_concurrent_requests_per_project,
            limits.max_items_per_project_window,
            Duration::from_secs(limits.quota_window_secs),
            65_536,
        )?;
        Ok(Self {
            limits,
            projects: service_http::WeightedAdmission::new(policy),
        })
    }

    pub fn limits(&self) -> &IngestLimits {
        &self.limits
    }

    pub fn validate_item_count(&self, item_count: usize) -> Result<(), AdmissionError> {
        if item_count == 0 {
            return Err(AdmissionError::invalid(
                "empty_batch",
                "ingest request must contain at least one item",
            ));
        }
        if item_count > self.limits.max_events_per_batch {
            return Err(AdmissionError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "batch_too_large",
                format!(
                    "batch contains {item_count} items; maximum is {}",
                    self.limits.max_events_per_batch
                ),
                false,
                None,
            ));
        }
        Ok(())
    }

    pub fn validate_event_bytes(&self, bytes: usize) -> Result<(), AdmissionError> {
        if bytes > self.limits.max_event_bytes {
            return Err(AdmissionError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "event_too_large",
                format!(
                    "event is {bytes} bytes; maximum is {}",
                    self.limits.max_event_bytes
                ),
                false,
                None,
            ));
        }
        Ok(())
    }

    pub fn acquire(
        &self,
        project: &str,
        item_count: usize,
        draining: bool,
    ) -> Result<AdmissionPermit, AdmissionError> {
        if project.trim().is_empty() {
            return Err(AdmissionError::invalid(
                "missing_project",
                "x-sift-project or an event project is required",
            ));
        }
        self.validate_item_count(item_count)?;
        self.projects
            .acquire(project.to_string(), item_count, draining)
            .map_err(|error| match error {
                service_http::WeightedAdmissionError::Draining => AdmissionError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_draining",
                    "Sift is draining and cannot accept new writes",
                    true,
                    Some(1),
                ),
                service_http::WeightedAdmissionError::ConcurrencyExceeded { .. } => {
                    AdmissionError::new(
                        StatusCode::TOO_MANY_REQUESTS,
                        "project_concurrency_exceeded",
                        "project has too many concurrent ingest requests",
                        true,
                        Some(1),
                    )
                }
                service_http::WeightedAdmissionError::QuotaExceeded { retry_after, .. } => {
                    AdmissionError::new(
                        StatusCode::TOO_MANY_REQUESTS,
                        "project_quota_exceeded",
                        "project ingest quota exceeded for the current window",
                        true,
                        Some(
                            retry_after
                                .as_secs()
                                .saturating_add(u64::from(retry_after.subsec_nanos() > 0))
                                .max(1),
                        ),
                    )
                }
                service_http::WeightedAdmissionError::KeyLimitExceeded => AdmissionError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "admission_capacity_exceeded",
                    "too many projects are active in the ingest admission window",
                    true,
                    Some(1),
                ),
                service_http::WeightedAdmissionError::ZeroWeight => {
                    AdmissionError::invalid("empty_batch", "ingest request must not be empty")
                }
            })
    }
}

pub type AdmissionPermit = service_http::ConcurrencyLease<String>;
