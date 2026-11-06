//! Upload referenced blobs before the catalog and manifest commit.

use crate::archive::application::catalog_service::CatalogService;
use crate::archive::application::object_service::TransactionService;
use crate::archive::application::spill_service::SpillService;
use crate::archive::domain::archive_catalog_item::{blob_catalog_key, ArchiveCatalogItem};
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::archive::domain::control_values::SpillBlobReference;
use crate::archive::domain::gcs_uri::gcs_uri;
use crate::journal::application::raw_event_service::RawEventService as RawStorage;
use crate::shared_kernel::archive_manifest::{ArchiveBlob, ArchiveManifest};
use anyhow::{bail, Context, Result};

pub(super) fn upload_referenced_blobs(
    storage: &RawStorage,
    referenced: &SpillService,
    previous: Option<&ArchiveManifest>,
    previous_catalog: Option<&CatalogService>,
    archive_prefix: &str,
    bucket: &str,
    archive_transaction: &mut TransactionService,
    catalog_updates: &mut SpillService,
    live_objects: &mut SpillService,
) -> Result<u64> {
    let mut new_blob_count = 0_u64;
    for entry in referenced.reader()? {
        let counted: SpillBlobReference =
            serde_json::from_slice(&entry?.value).context("decode archive spill blob reference")?;
        let catalog_key = format!("blob/{}", counted.reference.hash);
        let existing = match (previous, previous_catalog) {
            (Some(previous), Some(catalog)) => catalog
                .lookup(&previous.catalog_root, &catalog_key)?
                .map(decode_archive_catalog_entry)
                .transpose()?,
            _ => None,
        };
        let blob = if let Some(ArchiveCatalogItem::Blob(mut blob)) = existing {
            if blob.reference != counted.reference || blob.reference_count == 0 {
                bail!(
                    "immutable blob {} changed after archive commit",
                    counted.reference.hash
                );
            }
            blob.reference_count = blob.reference_count.saturating_add(counted.count);
            blob
        } else if existing.is_some() {
            bail!("archive blob catalog key resolved to a segment");
        } else {
            let bytes = storage.read_blob(&counted.reference.hash)?;
            if bytes.len() as u64 != counted.reference.size {
                bail!(
                    "blob {} size changed before archive",
                    counted.reference.hash
                );
            }
            let digest = counted.reference.hash.trim_start_matches("sha256:");
            let key = format!("{archive_prefix}/blobs/{digest}.blob");
            archive_transaction.put(crate::archive::domain::resource_ports::ArchiveObject::new(
                key.clone(),
                bytes,
                "application/octet-stream",
            ))?;
            new_blob_count = new_blob_count.saturating_add(1);
            ArchiveBlob {
                reference: counted.reference,
                object_uri: gcs_uri(bucket, &key),
                reference_count: counted.count,
            }
        };
        catalog_updates.upsert(blob_catalog_key(&blob), serde_json::to_vec(&blob)?)?;
        live_objects.insert_uri(&blob.object_uri)?;
    }

    Ok(new_blob_count)
}
