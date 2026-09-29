//! Define the log record/query/page schema, fixed-field embedded Lumen index,
//! retention, snapshot, restore, and typed query behavior.

use std::collections::{BTreeMap, HashSet};
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use index_text::{
    Analyzer, FieldSpec, MatchOperator, MemoryTextIndex, TextDocument, TextIndex,
    TextIndexSnapshot, TextQuery, TextSchema,
};
use serde::{Deserialize, Serialize};
use service_projection::ProjectionDescriptor;
use sha2::{Digest, Sha256};

use crate::projection::application::log_record_normalizer::normalize;
use crate::projection::application::projection_runtime::Projection;
use crate::projection::domain::log::{
    projection_row_key, record_matches, LogPage, LogQuery, LogRecordV1, LoggingState,
    DEFAULT_RETAINED_LOG_RECORDS, LOGGING_SCHEMA_VERSION, PROJECTION_LOGGING_STORE, RESOURCE_TYPE,
    SERVICE_NAME,
};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

#[derive(Deserialize, Serialize)]
struct LoggingSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_index: Option<TextIndexSnapshot>,
    records: BTreeMap<u64, LogRecordV1>,
    projection_cursor: u64,
    max_records: usize,
}

#[derive(Serialize)]
struct SemanticState<'a> {
    records: &'a BTreeMap<u64, LogRecordV1>,
    projection_cursor: u64,
    max_records: usize,
}

pub struct LoggingProjection {
    text_index: MemoryTextIndex,
    state: RwLock<LoggingState>,
    max_records: usize,
}

impl LoggingProjection {
    pub fn new() -> Result<Self> {
        Self::with_max_records(DEFAULT_RETAINED_LOG_RECORDS)
    }

    pub fn with_max_records(max_records: usize) -> Result<Self> {
        if max_records == 0 {
            bail!("logging retention must keep at least one record");
        }
        let text_index =
            MemoryTextIndex::new(fixed_schema()?).context("create shared logging text index")?;
        Ok(Self {
            text_index,
            state: RwLock::new(LoggingState::default()),
            max_records,
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
                    .map(|hit| hit.external_id)
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
        })
    }

    fn index(&self, record: &LogRecordV1) -> Result<()> {
        self.text_index.upsert(index_document(record))?;
        Ok(())
    }
}

impl Projection for LoggingProjection {
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
        state.records.insert(record.cursor, record);
        while state.records.len() > self.max_records {
            let Some(oldest) = state.records.keys().next().copied() else {
                break;
            };
            self.text_index.delete(&projection_row_key(oldest), None)?;
            state.records.remove(&oldest);
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
        };
        Ok(())
    }

    fn semantic_digest(&self) -> Result<String> {
        let state = self
            .state
            .read()
            .expect("logging projection state lock poisoned");
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(
            &SemanticState {
                records: &state.records,
                projection_cursor: state.projection_cursor,
                max_records: self.max_records,
            },
        )?)))
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
    let mut document = TextDocument::new(projection_row_key(record.cursor), record.cursor)
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

fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(value)?)?;
    serde_json::to_vec(&value).map_err(Into::into)
}
