//! Encoding stored events as Parquet archive segments and decoding them back.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use arrow_array::{ArrayRef, RecordBatch, RecordBatchReader, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use bytes::Bytes;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::shared_kernel::stored_event::StoredEvent;

struct SiftParquetCodec;

impl storage_segment::RecordCodec<StoredEvent> for SiftParquetCodec {
    fn encode(&self, records: &[StoredEvent]) -> storage_segment::Result<Vec<u8>> {
        encode_parquet_inner(records).map_err(|error| storage_segment::SegmentError::Codec {
            message: error.to_string(),
        })
    }

    fn decode(&self, bytes: &[u8]) -> storage_segment::Result<Vec<StoredEvent>> {
        decode_parquet_inner(bytes).map_err(|error| storage_segment::SegmentError::Codec {
            message: error.to_string(),
        })
    }
}

pub(in crate::archive) struct SiftSignalPartitioner;

impl storage_segment::Partitioner<StoredEvent> for SiftSignalPartitioner {
    fn partition(&self, record: &StoredEvent) -> storage_segment::Result<String> {
        Ok(record.event.signal.to_string())
    }
}

fn parquet_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("cursor", DataType::UInt64, false),
        Field::new("acknowledged_at", DataType::Utf8, false),
        Field::new("event_id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, false),
        Field::new("environment", DataType::Utf8, false),
        Field::new("signal", DataType::Utf8, false),
        Field::new("occurred_at", DataType::Utf8, false),
        Field::new("event_json", DataType::Utf8, false),
    ]))
}

pub(in crate::archive) fn encode_parquet(events: &[StoredEvent]) -> Result<Vec<u8>> {
    storage_segment::RecordCodec::encode(&SiftParquetCodec, events).map_err(Into::into)
}

fn encode_parquet_inner(events: &[StoredEvent]) -> Result<Vec<u8>> {
    let event_json = events
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let columns: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from(
            events.iter().map(|event| event.cursor).collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.acknowledged_at.clone())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.event.event_id.clone())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.event.project.clone())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.event.environment.clone())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.event.signal.to_string())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            events
                .iter()
                .map(|event| event.event.occurred_at.clone())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(event_json)),
    ];
    let schema = parquet_schema();
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut output = Vec::new();
    {
        let mut writer = ArrowWriter::try_new(&mut output, schema, Some(properties))?;
        writer.write(&batch)?;
        writer.close()?;
    }
    Ok(output)
}

pub(in crate::archive) fn decode_parquet(bytes: &[u8]) -> Result<Vec<StoredEvent>> {
    storage_segment::RecordCodec::decode(&SiftParquetCodec, bytes).map_err(Into::into)
}

fn decode_parquet_inner(bytes: &[u8]) -> Result<Vec<StoredEvent>> {
    let mut reader = ParquetRecordBatchReaderBuilder::try_new(Bytes::copy_from_slice(bytes))?
        .with_batch_size(1_024)
        .build()?;
    if reader.schema() != parquet_schema() {
        bail!("archive Parquet schema is not the Sift v2 segment schema");
    }
    let mut events = Vec::new();
    for batch in &mut reader {
        events.extend(decode_parquet_batch(&batch?)?);
    }
    Ok(events)
}

pub(super) fn open_parquet_reader(path: &Path) -> Result<ParquetRecordBatchReader> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open cached Parquet segment {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .with_batch_size(1_024)
        .build()?;
    if reader.schema() != parquet_schema() {
        bail!("archive Parquet schema is not the Sift v2 segment schema");
    }
    Ok(reader)
}

pub(super) fn decode_parquet_batch(batch: &RecordBatch) -> Result<Vec<StoredEvent>> {
    if batch
        .columns()
        .iter()
        .any(|column| column.null_count() != 0)
    {
        bail!("archive Parquet segment contains a null required value");
    }
    let cursors = uint64_column(batch, 0, "cursor")?;
    let acknowledged = string_column(batch, 1, "acknowledged_at")?;
    let event_ids = string_column(batch, 2, "event_id")?;
    let projects = string_column(batch, 3, "project")?;
    let environments = string_column(batch, 4, "environment")?;
    let signals = string_column(batch, 5, "signal")?;
    let occurred = string_column(batch, 6, "occurred_at")?;
    let json = string_column(batch, 7, "event_json")?;
    let mut events = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        let event: StoredEvent = serde_json::from_str(json.value(row))?;
        if event.cursor != cursors.value(row)
            || event.acknowledged_at != acknowledged.value(row)
            || event.event.event_id != event_ids.value(row)
            || event.event.project != projects.value(row)
            || event.event.environment != environments.value(row)
            || event.event.signal.to_string() != signals.value(row)
            || event.event.occurred_at != occurred.value(row)
        {
            bail!("archive Parquet columns disagree with event_json");
        }
        events.push(event);
    }
    Ok(events)
}

fn uint64_column<'a>(batch: &'a RecordBatch, index: usize, name: &str) -> Result<&'a UInt64Array> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt64Array>()
        .with_context(|| format!("archive Parquet column {name} has the wrong type"))
}

fn string_column<'a>(batch: &'a RecordBatch, index: usize, name: &str) -> Result<&'a StringArray> {
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("archive Parquet column {name} has the wrong type"))
}
