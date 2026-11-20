//! Define the log record/query/page schema, fixed-field embedded Lumen index,
//! retention, snapshot, restore, and typed query behavior.

use std::collections::{BTreeMap, HashSet};
use std::sync::RwLock;

use crate::projection::domain::runtime_metadata::ProjectionDescriptor;
use anyhow::{bail, Context, Result};
use index_text::{
    Analyzer, DocumentId, DocumentVersion, FieldSpec, MatchOperator, MemoryTextIndex, TextDocument,
    TextIndex, TextIndexSnapshot, TextQuery, TextSchema,
};
use serde::{Deserialize, Serialize};

use crate::projection::domain::log::{
    projection_row_key, record_matches, LogPage, LogQuery, LogRecordV1, LogRetentionBoundary,
    LoggingState, DEFAULT_RETAINED_LOG_BYTES, DEFAULT_RETAINED_LOG_RECORDS, LOGGING_SCHEMA_VERSION,
    PROJECTION_LOGGING_STORE, RESOURCE_TYPE, SERVICE_NAME,
};
use crate::projection::domain::log_record_normalizer::normalize;
use crate::projection::domain::projection::Projection;
use crate::shared_kernel::sha256_hex::sha256_hex;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

#[derive(Deserialize, Serialize)]
struct LoggingSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_index: Option<TextIndexSnapshot>,
    records: BTreeMap<u64, LogRecordV1>,
    projection_cursor: u64,
    max_records: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retention_boundary: Option<LogRetentionBoundary>,
}

#[derive(Serialize)]
struct SemanticState<'a> {
    records: &'a BTreeMap<u64, LogRecordV1>,
    projection_cursor: u64,
    max_records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    retention_boundary: &'a Option<LogRetentionBoundary>,
}

pub struct LoggingProjection {
    text_index: MemoryTextIndex,
    state: RwLock<LoggingState>,
    max_records: usize,
    max_bytes: usize,
}

impl LoggingProjection {
    pub fn new() -> Result<Self> {
        Self::with_max_records(DEFAULT_RETAINED_LOG_RECORDS)
    }

    pub fn from_env() -> Result<Self> {
        let max_bytes = Self::bytes_from(std::env::var("SIFT_LOG_RETAINED_BYTES").ok().as_deref())?;
        Self::with_limits(DEFAULT_RETAINED_LOG_RECORDS, max_bytes)
    }

    fn bytes_from(value: Option<&str>) -> Result<usize> {
        let Some(text) = value else {
            return Ok(DEFAULT_RETAINED_LOG_BYTES);
        };
        let bytes: usize = text
            .parse()
            .context("SIFT_LOG_RETAINED_BYTES must be a positive integer")?;
        if bytes == 0 {
            bail!("SIFT_LOG_RETAINED_BYTES must be a positive integer");
        }
        Ok(bytes)
    }

    pub fn with_max_records(max_records: usize) -> Result<Self> {
        Self::with_limits(max_records, DEFAULT_RETAINED_LOG_BYTES)
    }

    pub fn with_limits(max_records: usize, max_bytes: usize) -> Result<Self> {
        if max_records == 0 {
            bail!("logging retention must keep at least one record");
        }
        if max_bytes == 0 {
            bail!("logging retention byte limit must be greater than zero");
        }
        let text_index =
            MemoryTextIndex::new(fixed_schema()?).context("create shared logging text index")?;
        Ok(Self {
            text_index,
            state: RwLock::new(LoggingState::default()),
            max_records,
            max_bytes,
        })
    }

    pub fn query(&self, query: &LogQuery) -> Result<LogPage> {
        let bounds = query.validate()?;
        let candidates = match query.text.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => Some(
                self.text_index
                    .search(
                        &TextQuery::match_text("body", text, MatchOperator::All),
                        self.max_records,
                    )?
                    .into_iter()
                    .map(|hit| hit.external_id.as_str().to_string())
                    .collect::<HashSet<_>>(),
            ),
            _ => None,
        };

        let state = self
            .state
            .read()
            .expect("logging projection state lock poisoned");
        let projection_cursor = state.records.keys().next_back().copied().unwrap_or(0);
        let mut matching = state
            .records
            .range((query.after_cursor.saturating_add(1))..)
            .filter(|(_, record)| record_matches(record, query, &bounds, candidates.as_ref()))
            .map(|(_, record)| record.clone())
            .take(query.limit + 1)
            .collect::<Vec<_>>();
        let has_more = matching.len() > query.limit;
        matching.truncate(query.limit);
        let next_cursor = matching
            .last()
            .map(|record| record.cursor)
            .unwrap_or(query.after_cursor);
        Ok(LogPage {
            records: matching,
            next_cursor,
            projection_cursor,
            has_more,
            incomplete: state.incomplete(query.after_cursor, &bounds),
        })
    }

    fn index(&self, record: &LogRecordV1) -> Result<()> {
        self.text_index.upsert(index_document(record))?;
        Ok(())
    }
}

impl Projection for LoggingProjection {
    type Error = anyhow::Error;

    fn descriptor(&self) -> ProjectionDescriptor {
        ProjectionDescriptor {
            name: PROJECTION_LOGGING_STORE.into(),
            schema_version: LOGGING_SCHEMA_VERSION,
            retention: format!("latest-{0}-records", self.max_records),
        }
    }

    fn apply_idempotent(&self, stored: &StoredEvent) -> Result<()> {
        if stored.event.signal != SignalKind::Log {
            return Ok(());
        }
        let record = normalize(stored);
        if self
            .state
            .read()
            .expect("logging projection state lock poisoned")
            .projection_cursor
            >= record.cursor
        {
            return Ok(());
        }
        self.index(&record)?;
        let mut state = self
            .state
            .write()
            .expect("logging projection state lock poisoned");
        state.projection_cursor = state.projection_cursor.max(record.cursor);
        let size = record_bytes(&record);
        state.records.insert(record.cursor, record);
        state.retained_bytes = state.retained_bytes.saturating_add(size);
        while state.records.len() > self.max_records
            || (state.retained_bytes > self.max_bytes && state.records.len() > 1)
        {
            let Some(oldest) = state.records.keys().next().copied() else {
                break;
            };
            self.text_index
                .delete(&DocumentId::new(projection_row_key(oldest)), None)?;
            let removed = state.records.remove(&oldest).expect("retained log exists");
            state.retained_bytes = state.retained_bytes.saturating_sub(record_bytes(&removed));
            let time = chrono::DateTime::parse_from_rfc3339(&removed.occurred_at)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc));
            state.retention_boundary = Some(match state.retention_boundary.take() {
                Some(previous) => LogRetentionBoundary {
                    through_cursor: previous.through_cursor.max(oldest),
                    latest_time: previous.latest_time.zip(time).map(|(a, b)| a.max(b)),
                },
                None => LogRetentionBoundary {
                    through_cursor: oldest,
                    latest_time: time,
                },
            });
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self
            .state
            .read()
            .expect("logging projection state lock poisoned");
        canonical_json(&LoggingSnapshot {
            text_index: Some(self.text_index.snapshot()?),
            records: state.records.clone(),
            projection_cursor: state.projection_cursor,
            max_records: self.max_records,
            retention_boundary: state.retention_boundary.clone(),
        })
    }

    fn restore(&self, bytes: &[u8]) -> Result<()> {
        let snapshot: LoggingSnapshot =
            serde_json::from_slice(bytes).context("decode logging projection snapshot")?;
        if snapshot.max_records != self.max_records {
            bail!(
                "logging snapshot retention {} does not match configured {}",
                snapshot.max_records,
                self.max_records
            );
        }
        let retention_boundary = snapshot.retention_boundary.or_else(|| {
            (snapshot.records.len() == self.max_records).then(|| LogRetentionBoundary {
                through_cursor: snapshot
                    .records
                    .keys()
                    .next()
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1),
                latest_time: None,
            })
        });
        let retained_bytes: usize = snapshot.records.values().map(record_bytes).sum();
        match snapshot.text_index {
            Some(text_index) if self.text_index.restore(&text_index).is_ok() => {}
            _ => self
                .text_index
                .rebuild(snapshot.records.values().map(index_document).collect())
                .context("rebuild logging index from retained projection records")?,
        }
        *self
            .state
            .write()
            .expect("logging projection state lock poisoned") = LoggingState {
            records: snapshot.records,
            projection_cursor: snapshot.projection_cursor,
            retention_boundary,
            retained_bytes,
        };
        Ok(())
    }

    fn semantic_digest(&self) -> Result<String> {
        let state = self
            .state
            .read()
            .expect("logging projection state lock poisoned");
        Ok(sha256_hex(serde_json::to_vec(&SemanticState {
            records: &state.records,
            projection_cursor: state.projection_cursor,
            max_records: self.max_records,
            retention_boundary: &state.retention_boundary,
        })?))
    }
}

fn fixed_schema() -> Result<TextSchema> {
    let mut fields = BTreeMap::new();
    fields.insert("body".into(), FieldSpec::text(Analyzer::WhitespaceLower));
    for name in [
        "project",
        "environment",
        "severity",
        "resource_type",
        "service_name",
        "trace_id",
        "span_id",
        "request_id",
        "session_id",
        "occurred_at",
        "coexistence_key",
    ] {
        fields.insert(name.into(), FieldSpec::keyword());
    }
    TextSchema::new(fields).map_err(Into::into)
}

fn index_document(record: &LogRecordV1) -> TextDocument {
    let mut document = TextDocument::new(
        DocumentId::new(projection_row_key(record.cursor)),
        DocumentVersion::new(record.cursor),
    )
    .with_field("body", &record.body_text)
    .with_field("project", &record.project)
    .with_field("environment", &record.environment)
    .with_field("occurred_at", &record.occurred_at)
    .with_field("coexistence_key", &record.coexistence_key);
    for (field, value) in [
        ("severity", record.severity.as_deref()),
        (
            "resource_type",
            record.resource.get(RESOURCE_TYPE).map(String::as_str),
        ),
        (
            "service_name",
            record.resource.get(SERVICE_NAME).map(String::as_str),
        ),
        ("trace_id", record.trace_id.as_deref()),
        ("span_id", record.span_id.as_deref()),
        ("request_id", record.request_id.as_deref()),
        ("session_id", record.session_id.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            document = document.with_field(field, value);
        }
    }
    document
}

/// The charged size of a record: its JSON size, as the metric point size is.
fn record_bytes(record: &LogRecordV1) -> usize {
    serde_json::to_vec(record)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(value)?)?;
    serde_json::to_vec(&value).map_err(Into::into)
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn eviction_boundary_survives_restart_and_detects_out_of_order_time() {
        let projection = LoggingProjection::with_max_records(2).unwrap();
        for (cursor, time) in [
            (1, "2026-10-02T00:00:09Z"),
            (2, "2026-10-02T00:00:01Z"),
            (3, "2026-10-02T00:00:10Z"),
        ] {
            let mut event = crate::EventEnvelope::for_project(
                "p",
                "prod",
                format!("e{cursor}"),
                SignalKind::Log,
                serde_json::json!({"message":"test"}),
            );
            event.occurred_at = time.into();
            projection
                .apply_idempotent(&StoredEvent {
                    cursor,
                    acknowledged_at: event.observed_at.clone(),
                    event,
                })
                .unwrap();
        }
        let restored = LoggingProjection::with_max_records(2).unwrap();
        restored.restore(&projection.snapshot().unwrap()).unwrap();
        let mut query = LogQuery::for_project("p");
        query.start_time = Some("2026-10-02T00:00:02Z".into());
        assert!(restored.query(&query).unwrap().incomplete);
        query.start_time = Some("2026-10-02T00:00:10Z".into());
        assert!(!restored.query(&query).unwrap().incomplete);
        query.start_time = None;
        query.after_cursor = 1;
        assert!(!restored.query(&query).unwrap().incomplete);
    }

    fn log_event(cursor: u64, text: &str) -> StoredEvent {
        let event = crate::EventEnvelope::for_project(
            "p",
            "prod",
            format!("e{cursor}"),
            SignalKind::Log,
            serde_json::json!({"message": text}),
        );
        StoredEvent {
            cursor,
            acknowledged_at: event.observed_at.clone(),
            event,
        }
    }

    fn filler(cursor: u64) -> StoredEvent {
        log_event(cursor, &"x".repeat(1000))
    }

    #[test]
    fn byte_limit_evicts_oldest_records_before_the_count_limit() {
        let projection = LoggingProjection::with_limits(1000, 4096).unwrap();
        for cursor in 1..=20 {
            projection.apply_idempotent(&filler(cursor)).unwrap();
        }
        let state = projection.state.read().unwrap();
        assert!(state.records.len() < 20);
        assert!(state.records.contains_key(&20));
        assert!(!state.records.contains_key(&1));
        assert!(state.retained_bytes <= 4096);
    }

    #[test]
    fn byte_limit_removes_evicted_records_from_the_text_index() {
        let projection = LoggingProjection::with_limits(1000, 4096).unwrap();
        let first = format!("firstmarker {}", "x".repeat(1000));
        projection.apply_idempotent(&log_event(1, &first)).unwrap();
        let mut query = LogQuery::for_project("p");
        query.text = Some("firstmarker".into());
        assert_eq!(projection.query(&query).unwrap().records.len(), 1);
        for cursor in 2..=20 {
            projection.apply_idempotent(&filler(cursor)).unwrap();
        }
        assert!(projection.query(&query).unwrap().records.is_empty());
    }

    #[test]
    fn one_record_larger_than_the_byte_limit_stays() {
        let projection = LoggingProjection::with_limits(1000, 10).unwrap();
        for cursor in 1..=3 {
            projection.apply_idempotent(&filler(cursor)).unwrap();
        }
        let state = projection.state.read().unwrap();
        assert_eq!(state.records.len(), 1);
        assert!(state.records.contains_key(&3));
    }

    #[test]
    fn restore_recomputes_retained_bytes() {
        let first = LoggingProjection::with_limits(1000, 1_000_000).unwrap();
        for cursor in 1..=5 {
            first.apply_idempotent(&filler(cursor)).unwrap();
        }
        let second = LoggingProjection::with_limits(1000, 1_000_000).unwrap();
        second.restore(&first.snapshot().unwrap()).unwrap();
        let before = first.state.read().unwrap().retained_bytes;
        let after = second.state.read().unwrap().retained_bytes;
        assert_eq!(before, after);
        assert!(after > 0);
    }

    #[test]
    fn with_limits_refuses_zero() {
        assert!(LoggingProjection::with_limits(0, 10).is_err());
        assert!(LoggingProjection::with_limits(10, 0).is_err());
    }

    #[test]
    fn bytes_from_reads_the_setting() {
        assert_eq!(LoggingProjection::bytes_from(None).unwrap(), 268_435_456);
        assert_eq!(LoggingProjection::bytes_from(Some("4096")).unwrap(), 4096);
        assert!(LoggingProjection::bytes_from(Some("0")).is_err());
        assert!(LoggingProjection::bytes_from(Some("abc")).is_err());
        assert!(LoggingProjection::bytes_from(Some("-1")).is_err());
    }
}
