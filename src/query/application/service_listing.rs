//! List services by scanning the journal application reader.

use crate::access::application::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::shared_kernel::event::SignalKind;
use crate::shared_kernel::event_query::EventQuery;
use crate::shared_kernel::query_phase_one::{
    ServiceListResponseV1, ServiceQueryV1, ServiceSummaryV1,
};
use anyhow::Result;
use service_auth::RoleMapPrincipal;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn list_services(
    state: &ServiceState,
    principal: Option<&RoleMapPrincipal>,
    query: ServiceQueryV1,
) -> Result<ServiceListResponseV1, ApiError> {
    if query.project.trim().is_empty()
        || query
            .environment
            .as_deref()
            .is_some_and(|environment| environment.trim().is_empty())
    {
        return Err(ApiError::bad_request(
            "invalid_service_query",
            "project and environment must not be empty",
        ));
    }
    authorize_project_read(principal, &query.project)?;
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    let mut summaries =
        BTreeMap::<String, (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)>::new();
    let mut after = 0;
    loop {
        let events = state
            .journal
            .query(EventQuery {
                signal: None,
                after,
                limit: 10_000,
            })
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if events.is_empty() {
            break;
        }
        for stored in &events {
            after = after.max(stored.cursor);
            let event = &stored.event;
            if event.project != query.project
                || query
                    .environment
                    .as_ref()
                    .is_some_and(|environment| &event.environment != environment)
            {
                continue;
            }
            let signal = match event.signal {
                SignalKind::Log => "logs",
                SignalKind::Metric => "metrics",
                SignalKind::Span => "traces",
            };
            let Some(service) = event
                .resource
                .get("service.name")
                .filter(|service| !service.trim().is_empty())
            else {
                continue;
            };
            let entry = summaries.entry(service.clone()).or_default();
            entry.0.insert(event.environment.clone());
            if let Some(version) = event
                .resource
                .get("service.version")
                .filter(|version| !version.trim().is_empty())
            {
                entry.1.insert(version.clone());
            }
            entry.2.insert(signal.into());
        }
        if events.len() < 10_000 {
            break;
        }
    }
    let services = summaries
        .into_iter()
        .map(
            |(name, (environments, versions, signals))| ServiceSummaryV1 {
                name,
                environments: environments.into_iter().collect(),
                versions: versions.into_iter().collect(),
                signals: signals.into_iter().collect(),
            },
        )
        .collect();
    Ok(ServiceListResponseV1 {
        services,
        watermark: after,
    })
}
