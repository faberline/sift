//! Enforce compressed and decoded sizes, event count and size, per-project
//! quota, concurrency, draining, and overload errors.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::ingest::domain::admission_error::{AdmissionError, AdmissionStatus};
use crate::ingest::domain::ingest_limits::IngestLimits;

#[derive(Debug)]
pub struct AdmissionController {
    pub(in crate::ingest) limits: IngestLimits,
    projects: service_http::WeightedAdmission<String>,
    inflight_body_bytes: Arc<AtomicUsize>,
    active_queries: Arc<AtomicUsize>,
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
            inflight_body_bytes: Arc::new(AtomicUsize::new(0)),
            active_queries: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Hold `bytes` of decoded request body against the limit over all projects.
    /// The check and the add are one compare-and-swap step.
    pub fn acquire_body_bytes(&self, bytes: usize) -> Result<BodyBytesPermit, AdmissionError> {
        let limit = self.limits.max_inflight_body_bytes;
        self.inflight_body_bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(bytes).filter(|next| *next <= limit)
            })
            .map_err(|_| {
                AdmissionError::new(
                    AdmissionStatus::TooManyRequests,
                    "inflight_body_bytes_exceeded",
                    "too many request body bytes are in flight; retry later",
                    true,
                    Some(1),
                )
            })?;
        Ok(BodyBytesPermit {
            counter: Arc::clone(&self.inflight_body_bytes),
            bytes,
        })
    }

    /// Hold one slot of the concurrent query limit. The check and the add are
    /// one compare-and-swap step.
    pub fn acquire_query(&self) -> Result<QueryPermit, AdmissionError> {
        let limit = self.limits.max_concurrent_queries;
        self.active_queries
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                current.checked_add(1).filter(|next| *next <= limit)
            })
            .map_err(|_| {
                AdmissionError::new(
                    AdmissionStatus::TooManyRequests,
                    "query_concurrency_exceeded",
                    "too many concurrent queries; retry later",
                    true,
                    Some(1),
                )
            })?;
        Ok(QueryPermit {
            counter: Arc::clone(&self.active_queries),
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
                AdmissionStatus::PayloadTooLarge,
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
                AdmissionStatus::PayloadTooLarge,
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
                    AdmissionStatus::ServiceUnavailable,
                    "service_draining",
                    "Sift is draining and cannot accept new writes",
                    true,
                    Some(1),
                ),
                service_http::WeightedAdmissionError::ConcurrencyExceeded { .. } => {
                    AdmissionError::new(
                        AdmissionStatus::TooManyRequests,
                        "project_concurrency_exceeded",
                        "project has too many concurrent ingest requests",
                        true,
                        Some(1),
                    )
                }
                service_http::WeightedAdmissionError::QuotaExceeded { retry_after, .. } => {
                    AdmissionError::new(
                        AdmissionStatus::TooManyRequests,
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
                    AdmissionStatus::ServiceUnavailable,
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

/// Holds decoded request bytes against the in-flight body limit until dropped.
#[derive(Debug)]
pub struct BodyBytesPermit {
    counter: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for BodyBytesPermit {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::SeqCst);
    }
}

/// Holds one slot of the concurrent query limit until dropped.
#[derive(Debug)]
pub struct QueryPermit {
    counter: Arc<AtomicUsize>,
}

impl Drop for QueryPermit {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_bytes_limit_refuses_then_passes_after_release() {
        let controller = AdmissionController::new(IngestLimits {
            max_inflight_body_bytes: 100,
            max_decoded_body_bytes: 100,
            max_compressed_body_bytes: 100,
            ..IngestLimits::default()
        })
        .unwrap();
        let a = controller.acquire_body_bytes(60).unwrap();
        let refused = controller.acquire_body_bytes(41).unwrap_err();
        assert_eq!(refused.code, "inflight_body_bytes_exceeded");
        assert_eq!(refused.status, AdmissionStatus::TooManyRequests);
        let b = controller.acquire_body_bytes(40).unwrap();
        drop(a);
        drop(b);
        assert!(controller.acquire_body_bytes(100).is_ok());
    }

    #[test]
    fn query_limit_refuses_then_passes_after_release() {
        let controller = AdmissionController::new(IngestLimits {
            max_concurrent_queries: 2,
            ..IngestLimits::default()
        })
        .unwrap();
        let a = controller.acquire_query().unwrap();
        let _b = controller.acquire_query().unwrap();
        let refused = controller.acquire_query().unwrap_err();
        assert_eq!(refused.code, "query_concurrency_exceeded");
        drop(a);
        assert!(controller.acquire_query().is_ok());
    }
}
