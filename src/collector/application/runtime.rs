//! One collector run: a single source through service_collector's bounded
//! decode, quarantine, delivery, ack and checkpoint loop.

use std::time::Duration;

use anyhow::Result;

use crate::collector::application::run_collector::CollectorSummary;
use crate::collector::domain::config::CollectorConfig;
use crate::collector::domain::quarantine::QuarantineEntry;
use crate::collector::domain::record::RawRecord;
use crate::collector::domain::record::{SourceCursor, SourceRejection};
use crate::collector::domain::record_decoder::RecordDecoder;

struct SiftRecordDecoder<'a>(&'a dyn RecordDecoder<Error = QuarantineEntry>);

impl service_collector::RecordDecoder<RawRecord> for SiftRecordDecoder<'_> {
    type Item = crate::OperationalEventV2;
    type Rejection = QuarantineEntry;

    fn decode(&self, record: RawRecord) -> Result<Self::Item, Self::Rejection> {
        self.0.decode(record)
    }
}

pub(crate) async fn run<S, B>(
    config: &CollectorConfig,
    source: &mut S,
    decoder: &dyn RecordDecoder<Error = QuarantineEntry>,
    sink: &B,
) -> Result<CollectorSummary>
where
    S: service_collector::CollectorSource<
            Cursor = SourceCursor,
            Error = anyhow::Error,
            Record = RawRecord,
            Rejection = SourceRejection,
        > + ?Sized,
    B: service_collector::BatchSink<crate::OperationalEventV2>,
{
    let mut quarantine =
        service_collector::JsonlQuarantine::<QuarantineEntry>::new(&config.quarantine_path);
    let report = service_collector::run_collector_with_delivery_mode(
        source,
        &SiftRecordDecoder(decoder),
        sink,
        &mut quarantine,
        service_collector::RuntimeConfig {
            batch_size: config.batch_size,
            max_record_bytes: config.max_line_bytes,
            retry: service_collector::RetryPolicy::new(
                config.max_retries,
                config.initial_backoff,
                Duration::from_secs(5),
            )?,
            follow: config.follow,
            follow_poll_interval: config.follow_poll_interval,
        },
        if config.follow {
            service_collector::DeliveryRetryMode::UntilCancelled
        } else {
            service_collector::DeliveryRetryMode::Bounded
        },
    )
    .await?;

    Ok(CollectorSummary {
        source_id: config.source_id.clone(),
        start_offset: report.progress.start_offset,
        final_offset: report.progress.final_offset,
        lines: report.lines,
        accepted: report.accepted,
        duplicates: report.duplicates,
        rejected: report.rejected,
        lost_bytes: report.progress.lost_bytes,
        lost_sources: report.progress.lost_sources,
    })
}
