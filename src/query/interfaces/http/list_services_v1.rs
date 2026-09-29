//! Listing the services a project has sent telemetry from.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Query, State};
use axum::Json;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::event::SignalKind;
use crate::query::interfaces::http::phase_one::{
    ServiceListResponseV1, ServiceQueryV1, ServiceSummaryV1,
};
use crate::{journal::domain::event_query::EventQuery, ApiError, ServiceState};

pub(crate) async fn list_services_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(query): Query<ServiceQueryV1>,
) -> Result<Json<ServiceListResponseV1>, ApiError> {
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
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &query.project,
    )?;
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
    Ok(Json(ServiceListResponseV1 {
        services,
        watermark: after,
    }))
}
