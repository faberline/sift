//! A record as a source hands it to the collector: its bytes, the enrichment it
//! carries and the cursor that commits it, and the rejection a source reports
//! in a record's place.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::collector::domain::quarantine::QuarantineEntry;
use crate::AttributeValue;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct RecordEnrichment {
    pub(crate) resource: BTreeMap<String, String>,
    pub(crate) attributes: BTreeMap<String, AttributeValue>,
    pub(crate) cloud_logging_coexistence: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RawRecord {
    pub(crate) source_id: String,
    pub(crate) line: u64,
    pub(crate) offset: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) cursor: SourceCursor,
    pub(crate) enrichment: RecordEnrichment,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SourceRejection {
    pub(crate) entry: QuarantineEntry,
    pub(crate) cursor: SourceCursor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FileAnchor {
    pub(crate) width: u16,
    pub(crate) sha256: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum SourceCursor {
    Linear {
        next_offset: u64,
        next_line: u64,
    },
    Cri {
        identity: String,
        next_offset: u64,
        next_line: u64,
        observed_len: u64,
        #[serde(default)]
        anchor: Option<FileAnchor>,
        #[serde(default)]
        generation: Option<u64>,
    },
    CriLoss {
        identity: String,
        lost_bytes: u64,
        #[serde(default)]
        generation: Option<u64>,
    },
}
