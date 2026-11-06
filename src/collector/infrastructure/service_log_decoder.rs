//! Bind the borrowed service-log mapper to the collector decoding port.

use crate::collector::domain::quarantine::QuarantineEntry;
use crate::collector::domain::record::RawRecord;
use crate::collector::domain::record_decoder::RecordDecoder;
use crate::collector::infrastructure::service_log_mapper::decode_service_log_enriched;

pub(crate) struct ServiceLogDecoder<'a> {
    pub(crate) project: &'a str,
    pub(crate) environment: &'a str,
}

impl RecordDecoder for ServiceLogDecoder<'_> {
    type Error = QuarantineEntry;

    fn decode(&self, record: RawRecord) -> Result<crate::OperationalEventV2, Self::Error> {
        decode_service_log_enriched(
            &record.bytes,
            &record.source_id,
            record.offset,
            self.project,
            self.environment,
            &record.enrichment,
        )
        .map_err(|error| {
            QuarantineEntry::invalid_line(
                &record.source_id,
                record.line,
                record.offset,
                "invalid_service_log",
                error.to_string(),
                &record.bytes,
            )
        })
    }
}
