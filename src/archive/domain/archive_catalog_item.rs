//! A row of the archive catalog, and the keys segments, receipts and blobs sort
//! under.

use crate::archive::domain::archive_manifest::{ArchiveBlob, ArchiveDedupeReceipt, ArchiveSegment};

// Catalog rows are decoded and consumed one at a time. Keeping the concrete
// values here avoids an allocation on every segment in the query hot path.
#[allow(clippy::large_enum_variant)]
pub(in crate::archive) enum ArchiveCatalogItem {
    Segment(ArchiveSegment),
    Blob(ArchiveBlob),
    Receipt(ArchiveDedupeReceipt),
}

pub(in crate::archive) fn segment_catalog_key(segment: &ArchiveSegment) -> String {
    format!(
        "segment/{}/{:020}/{}",
        segment.signal, segment.source.first_cursor, segment.source.segment_id
    )
}

pub(in crate::archive) fn dedupe_receipt_catalog_key(receipt: &ArchiveDedupeReceipt) -> String {
    format!(
        "receipt/{:020}/{:020}/{}",
        receipt.max_acknowledged_at_unix_nano, receipt.first_cursor, receipt.sha256
    )
}

pub(in crate::archive) fn blob_catalog_key(blob: &ArchiveBlob) -> String {
    format!("blob/{}", blob.reference.hash)
}
