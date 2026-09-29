//! The event batch schemas' public paths. The ingest context now owns them;
//! these re-exports keep `sift::ingest::batch::*` compiling for callers
//! outside the crate.

pub use crate::ingest::interfaces::http::batch::{
    decode_item, event_id_hint, BatchItemResult, BatchOutcome, EventWriteRequest,
    EventWriteResponse, IngestErrorDetail,
};
