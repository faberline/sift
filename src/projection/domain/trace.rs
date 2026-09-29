//! Spans with their links and events as the trace projection keeps them, a
//! trace with its diagnostics, the query and page over traces, the trace state,
//! and the topology rules: cycles, the critical path and correlations.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{AttributeValue, InstrumentationScope};

pub const PROJECTION_TRACE_STORE: &str = "trace-store";
pub const TRACE_SCHEMA_VERSION: u32 = 3;
pub const MAX_TRACE_QUERY_LIMIT: usize = 1_000;
pub const DEFAULT_RETAINED_TRACE_SPANS: usize = 100_000;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct SpanLinkV1 {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct SpanEventV1 {
    pub name: String,
    pub time_unix_nano: u64,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct SpanRecordV1 {
    pub cursor: u64,
    pub event_id: String,
    pub project: String,
    pub environment: String,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(default)]
    pub links: Vec<SpanLinkV1>,
    #[serde(default)]
    pub events: Vec<SpanEventV1>,
    pub resource: BTreeMap<String, String>,
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope: Option<InstrumentationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl SpanRecordV1 {
    fn duration(&self) -> u64 {
        self.end_time_unix_nano
            .saturating_sub(self.start_time_unix_nano)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct TraceResultV1 {
    pub project: String,
    pub trace_id: String,
    pub spans: Vec<SpanRecordV1>,
    pub root_span_ids: Vec<String>,
    pub partial: bool,
    pub gaps: Vec<String>,
    pub cycles: Vec<Vec<String>>,
    pub critical_path_span_ids: Vec<String>,
    pub duration_unix_nano: u64,
    pub correlation_ids: BTreeMap<String, Vec<String>>,
    pub projection_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TraceQuery {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_duration_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_trace_id: Option<String>,
    #[serde(default = "default_trace_query_limit")]
    pub limit: usize,
}

impl TraceQuery {
    pub fn for_project(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            environment: None,
            start_time_unix_nano: None,
            end_time_unix_nano: None,
            service: None,
            operation: None,
            min_duration_unix_nano: None,
            max_duration_unix_nano: None,
            status: None,
            attributes: BTreeMap::new(),
            after_trace_id: None,
            limit: default_trace_query_limit(),
        }
    }

    pub(in crate::projection) fn validate(&self) -> Result<()> {
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_TRACE_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_TRACE_QUERY_LIMIT}");
        }
        if self
            .start_time_unix_nano
            .zip(self.end_time_unix_nano)
            .is_some_and(|(start, end)| start >= end)
        {
            bail!("start_time_unix_nano must be earlier than end_time_unix_nano");
        }
        if self
            .min_duration_unix_nano
            .zip(self.max_duration_unix_nano)
            .is_some_and(|(min, max)| min > max)
        {
            bail!("minimum trace duration must not exceed maximum trace duration");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct TracePage {
    pub traces: Vec<TraceResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_trace_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
}

#[derive(Default, Deserialize, Serialize)]
pub(in crate::projection) struct TraceState {
    pub(in crate::projection) traces: BTreeMap<String, BTreeMap<String, SpanRecordV1>>,
    #[serde(default)]
    pub(in crate::projection) location_by_cursor: BTreeMap<u64, TraceLocation>,
    pub(in crate::projection) conflicts: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub(in crate::projection) projection_cursor: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::projection) struct TraceLocation {
    pub(in crate::projection) trace_key: String,
    pub(in crate::projection) span_id: String,
}

pub(in crate::projection) fn trace_key(project: &str, trace_id: &str) -> String {
    format!("{project}\u{1f}{trace_id}")
}

pub(in crate::projection) fn remove_tracked_cursor(state: &mut TraceState, cursor: u64) {
    let Some(location) = state.location_by_cursor.remove(&cursor) else {
        return;
    };
    let mut remove_trace = false;
    if let Some(trace) = state.traces.get_mut(&location.trace_key) {
        if trace
            .get(&location.span_id)
            .is_some_and(|record| record.cursor == cursor)
        {
            trace.remove(&location.span_id);
        }
        remove_trace = trace.is_empty();
    }
    if remove_trace {
        state.traces.remove(&location.trace_key);
        state.conflicts.remove(&location.trace_key);
        return;
    }
    let remove_conflicts = if let Some(conflicts) = state.conflicts.get_mut(&location.trace_key) {
        conflicts.remove(&location.span_id);
        conflicts.is_empty()
    } else {
        false
    };
    if remove_conflicts {
        state.conflicts.remove(&location.trace_key);
    }
}

fn default_trace_query_limit() -> usize {
    100
}

pub(in crate::projection) fn trace_matches(trace: &TraceResultV1, query: &TraceQuery) -> bool {
    if query
        .min_duration_unix_nano
        .is_some_and(|minimum| trace.duration_unix_nano < minimum)
        || query
            .max_duration_unix_nano
            .is_some_and(|maximum| trace.duration_unix_nano > maximum)
    {
        return false;
    }
    let spans = trace.spans.iter().filter(|span| {
        query
            .environment
            .as_deref()
            .is_none_or(|environment| span.environment == environment)
            && query
                .start_time_unix_nano
                .is_none_or(|start| span.end_time_unix_nano >= start)
            && query
                .end_time_unix_nano
                .is_none_or(|end| span.start_time_unix_nano < end)
    });
    let spans = spans.collect::<Vec<_>>();
    if spans.is_empty() {
        return false;
    }
    query.service.as_deref().is_none_or(|service| {
        spans
            .iter()
            .any(|span| span.resource.get("service.name").map(String::as_str) == Some(service))
    }) && query
        .operation
        .as_deref()
        .is_none_or(|operation| spans.iter().any(|span| span.name == operation))
        && query.status.as_deref().is_none_or(|status| {
            spans.iter().any(|span| {
                span.status_code
                    .as_deref()
                    .is_some_and(|actual| actual.eq_ignore_ascii_case(status))
            })
        })
        && query.attributes.iter().all(|(key, value)| {
            spans
                .iter()
                .any(|span| span.attributes.get(key) == Some(value))
        })
}

pub(in crate::projection) fn detect_cycles(
    by_id: &BTreeMap<String, &SpanRecordV1>,
) -> Vec<Vec<String>> {
    let mut unique = BTreeMap::<String, Vec<String>>::new();
    for start in by_id.keys() {
        let mut positions = HashMap::<String, usize>::new();
        let mut path = Vec::<String>::new();
        let mut current = start.clone();
        loop {
            if let Some(position) = positions.get(&current).copied() {
                let mut cycle = path[position..].to_vec();
                cycle.sort();
                cycle.dedup();
                unique.entry(cycle.join("\u{1f}")).or_insert(cycle);
                break;
            }
            positions.insert(current.clone(), path.len());
            path.push(current.clone());
            let Some(parent) = by_id
                .get(&current)
                .and_then(|span| span.parent_span_id.as_ref())
                .filter(|parent| by_id.contains_key(*parent))
            else {
                break;
            };
            current = parent.clone();
        }
    }
    unique.into_values().collect()
}

pub(in crate::projection) fn best_path(
    span_id: &str,
    by_id: &BTreeMap<String, &SpanRecordV1>,
    children: &BTreeMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
) -> (u128, Vec<String>) {
    let Some(span) = by_id.get(span_id) else {
        return (0, Vec::new());
    };
    if !visiting.insert(span_id.into()) {
        return (0, Vec::new());
    }
    let child = children
        .get(span_id)
        .into_iter()
        .flatten()
        .map(|child| best_path(child, by_id, children, visiting))
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
        .unwrap_or_default();
    visiting.remove(span_id);
    let mut path = Vec::with_capacity(child.1.len() + 1);
    path.push(span_id.into());
    path.extend(child.1);
    (u128::from(span.duration()) + child.0, path)
}

pub(in crate::projection) fn correlations(spans: &[SpanRecordV1]) -> BTreeMap<String, Vec<String>> {
    let mut values = BTreeMap::<String, BTreeSet<String>>::new();
    for span in spans {
        values
            .entry("span_ids".into())
            .or_default()
            .insert(span.span_id.clone());
        if let Some(request_id) = &span.request_id {
            values
                .entry("request_ids".into())
                .or_default()
                .insert(request_id.clone());
        }
        if let Some(session_id) = &span.session_id {
            values
                .entry("session_ids".into())
                .or_default()
                .insert(session_id.clone());
        }
        for link in &span.links {
            values
                .entry("linked_trace_ids".into())
                .or_default()
                .insert(link.trace_id.clone());
            values
                .entry("linked_span_ids".into())
                .or_default()
                .insert(link.span_id.clone());
        }
    }
    values
        .into_iter()
        .map(|(key, values)| (key, values.into_iter().collect()))
        .collect()
}
