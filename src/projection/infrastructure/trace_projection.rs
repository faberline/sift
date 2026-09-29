//! The trace projection: spans decoded from trace events and kept per trace
//! under bounded retention, traces assembled with their diagnostics, and the
//! snapshot and restore that persist them.

use std::collections::{BTreeMap, HashSet};
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use service_projection::ProjectionDescriptor;
use sha2::{Digest, Sha256};

use crate::projection::application::projection_runtime::Projection;
use crate::projection::domain::trace::{
    best_path, correlations, detect_cycles, remove_tracked_cursor, trace_key, trace_matches,
    SpanEventV1, SpanLinkV1, SpanRecordV1, TraceLocation, TracePage, TraceQuery, TraceResultV1,
    TraceState, DEFAULT_RETAINED_TRACE_SPANS, PROJECTION_TRACE_STORE, TRACE_SCHEMA_VERSION,
};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

#[derive(Default, Deserialize)]
struct SpanPayload {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, alias = "parentSpanId")]
    parent_span_id: Option<String>,
    #[serde(default, alias = "startTimeUnixNano")]
    start_time_unix_nano: u64,
    #[serde(default, alias = "endTimeUnixNano")]
    end_time_unix_nano: u64,
    #[serde(default)]
    status: Option<SpanStatusPayload>,
    #[serde(default)]
    links: Vec<SpanLinkV1>,
    #[serde(default)]
    events: Vec<SpanEventV1>,
}

#[derive(Default, Deserialize)]
struct SpanStatusPayload {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
}

pub struct TraceProjection {
    state: RwLock<TraceState>,
    max_spans: usize,
}

impl TraceProjection {
    pub fn new() -> Self {
        Self::with_max_spans(DEFAULT_RETAINED_TRACE_SPANS)
            .expect("default trace span retention is valid")
    }

    pub fn with_max_spans(max_spans: usize) -> Result<Self> {
        if max_spans == 0 {
            bail!("trace retention must keep at least one span");
        }
        Ok(Self {
            state: RwLock::new(TraceState::default()),
            max_spans,
        })
    }

    pub fn get_trace(&self, project: &str, trace_id: &str) -> Result<Option<TraceResultV1>> {
        if project.trim().is_empty() || trace_id.trim().is_empty() {
            bail!("project and trace_id must not be empty");
        }
        let state = self.state.read().expect("trace projection lock poisoned");
        let key = trace_key(project, trace_id);
        let Some(records) = state.traces.get(&key) else {
            return Ok(None);
        };
        let mut spans = records.values().cloned().collect::<Vec<_>>();
        spans.sort_by(|left, right| {
            left.start_time_unix_nano
                .cmp(&right.start_time_unix_nano)
                .then_with(|| left.span_id.cmp(&right.span_id))
                .then_with(|| left.cursor.cmp(&right.cursor))
        });
        let by_id = spans
            .iter()
            .map(|span| (span.span_id.clone(), span))
            .collect::<BTreeMap<_, _>>();
        let mut gaps = Vec::new();
        let mut roots = Vec::new();
        for span in &spans {
            match span.parent_span_id.as_deref() {
                None | Some("") => roots.push(span.span_id.clone()),
                Some(parent) if !by_id.contains_key(parent) => {
                    roots.push(span.span_id.clone());
                    gaps.push(format!("missing_parent:{}:{parent}", span.span_id));
                }
                Some(_) => {}
            }
        }
        if let Some(conflicts) = state.conflicts.get(&key) {
            gaps.extend(
                conflicts
                    .iter()
                    .map(|span| format!("conflicting_span:{span}")),
            );
        }
        roots.sort();
        gaps.sort();
        let cycles = detect_cycles(&by_id);
        let mut children = BTreeMap::<String, Vec<String>>::new();
        for span in &spans {
            if let Some(parent) = span
                .parent_span_id
                .as_ref()
                .filter(|id| by_id.contains_key(*id))
            {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(span.span_id.clone());
            }
        }
        for values in children.values_mut() {
            values.sort();
        }
        let candidates = if roots.is_empty() {
            by_id.keys().cloned().collect::<Vec<_>>()
        } else {
            roots.clone()
        };
        let critical_path_span_ids = candidates
            .iter()
            .map(|root| best_path(root, &by_id, &children, &mut HashSet::new()))
            .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
            .map(|(_, path)| path)
            .unwrap_or_default();
        let duration_unix_nano = spans
            .iter()
            .map(|span| span.end_time_unix_nano)
            .max()
            .zip(spans.iter().map(|span| span.start_time_unix_nano).min())
            .map(|(end, start)| end.saturating_sub(start))
            .unwrap_or(0);
        Ok(Some(TraceResultV1 {
            project: project.into(),
            trace_id: trace_id.into(),
            correlation_ids: correlations(&spans),
            spans,
            root_span_ids: roots.clone(),
            partial: !gaps.is_empty() || !cycles.is_empty() || roots.is_empty(),
            gaps,
            cycles,
            critical_path_span_ids,
            duration_unix_nano,
            projection_cursor: state.projection_cursor,
        }))
    }

    pub fn query(&self, query: &TraceQuery) -> Result<TracePage> {
        query.validate()?;
        let prefix = format!("{}\u{1f}", query.project);
        let trace_ids = {
            let state = self.state.read().expect("trace projection lock poisoned");
            state
                .traces
                .keys()
                .filter_map(|key| key.strip_prefix(&prefix))
                .filter(|trace_id| {
                    query
                        .after_trace_id
                        .as_deref()
                        .is_none_or(|after| *trace_id > after)
                })
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let mut traces = Vec::new();
        for trace_id in trace_ids {
            let Some(trace) = self.get_trace(&query.project, &trace_id)? else {
                continue;
            };
            if trace_matches(&trace, query) {
                traces.push(trace);
                if traces.len() > query.limit {
                    break;
                }
            }
        }
        let has_more = traces.len() > query.limit;
        traces.truncate(query.limit);
        let projection_cursor = traces
            .iter()
            .map(|trace| trace.projection_cursor)
            .max()
            .unwrap_or_else(|| {
                self.state
                    .read()
                    .expect("trace projection lock poisoned")
                    .projection_cursor
            });
        Ok(TracePage {
            next_trace_id: traces.last().map(|trace| trace.trace_id.clone()),
            traces,
            projection_cursor,
            has_more,
        })
    }
}

impl Default for TraceProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl Projection for TraceProjection {
    fn descriptor(&self) -> ProjectionDescriptor {
        ProjectionDescriptor {
            name: PROJECTION_TRACE_STORE.into(),
            schema_version: TRACE_SCHEMA_VERSION,
            retention: "raw-journal-retention".into(),
        }
    }

    fn apply_idempotent(&self, stored: &StoredEvent) -> Result<()> {
        if stored.event.signal != SignalKind::Span {
            return Ok(());
        }
        let event = &stored.event;
        let trace_id = event
            .trace_id
            .as_deref()
            .context("span event requires trace_id")?;
        let span_id = event
            .span_id
            .as_deref()
            .context("span event requires span_id")?;
        let payload: SpanPayload = serde_json::from_value(event.payload.clone())
            .context("decode canonical span payload")?;
        if payload.end_time_unix_nano < payload.start_time_unix_nano {
            bail!("span end_time_unix_nano must not precede start_time_unix_nano");
        }
        let status_code = payload
            .status
            .as_ref()
            .and_then(|status| status.code.as_ref())
            .map(value_as_string);
        let status_message = payload.status.and_then(|status| status.message);
        let record = SpanRecordV1 {
            cursor: stored.cursor,
            event_id: event.event_id.clone(),
            project: event.project.clone(),
            environment: event.environment.clone(),
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: payload.parent_span_id.filter(|parent| !parent.is_empty()),
            name: if payload.name.is_empty() {
                span_id.into()
            } else {
                payload.name
            },
            kind: payload.kind,
            start_time_unix_nano: payload.start_time_unix_nano,
            end_time_unix_nano: payload.end_time_unix_nano,
            status_code,
            status_message,
            links: payload.links,
            events: payload.events,
            resource: event.resource.clone(),
            attributes: event.attributes.clone(),
            instrumentation_scope: event.instrumentation_scope.clone(),
            request_id: event.request_id.clone(),
            session_id: event.session_id.clone(),
        };
        let key = trace_key(&event.project, trace_id);
        let mut state = self.state.write().expect("trace projection lock poisoned");
        if state.projection_cursor >= stored.cursor {
            return Ok(());
        }
        state.projection_cursor = state.projection_cursor.max(stored.cursor);
        let conflicting = state
            .traces
            .get(&key)
            .and_then(|trace| trace.get(span_id))
            .is_some_and(|existing| existing != &record);
        if let Some(previous_cursor) = state
            .traces
            .get(&key)
            .and_then(|trace| trace.get(span_id))
            .map(|previous| previous.cursor)
        {
            remove_tracked_cursor(&mut state, previous_cursor);
        }
        if conflicting {
            state
                .conflicts
                .entry(key.clone())
                .or_default()
                .insert(span_id.into());
        }
        state
            .traces
            .entry(key.clone())
            .or_default()
            .insert(span_id.into(), record);
        state.location_by_cursor.insert(
            stored.cursor,
            TraceLocation {
                trace_key: key,
                span_id: span_id.into(),
            },
        );
        while state.location_by_cursor.len() > self.max_spans {
            let Some(oldest_cursor) = state.location_by_cursor.keys().next().copied() else {
                break;
            };
            remove_tracked_cursor(&mut state, oldest_cursor);
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self.state.read().expect("trace projection lock poisoned");
        serde_json::to_vec(&*state).map_err(Into::into)
    }

    fn restore(&self, bytes: &[u8]) -> Result<()> {
        let mut state: TraceState =
            serde_json::from_slice(bytes).context("decode trace projection snapshot")?;
        let mut rows = Vec::new();
        for (trace_key, trace) in &state.traces {
            for (span_id, record) in trace {
                rows.push((
                    record.cursor,
                    TraceLocation {
                        trace_key: trace_key.clone(),
                        span_id: span_id.clone(),
                    },
                ));
            }
        }
        state.location_by_cursor.clear();
        for (cursor, location) in rows {
            state.projection_cursor = state.projection_cursor.max(cursor);
            state.location_by_cursor.insert(cursor, location);
        }
        while state.location_by_cursor.len() > self.max_spans {
            let Some(oldest_cursor) = state.location_by_cursor.keys().next().copied() else {
                break;
            };
            remove_tracked_cursor(&mut state, oldest_cursor);
        }
        *self.state.write().expect("trace projection lock poisoned") = state;
        Ok(())
    }

    fn semantic_digest(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(self.snapshot()?)))
    }
}

fn value_as_string(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
