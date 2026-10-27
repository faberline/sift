//! Deleting the objects a GC plan lists once a checkpoint has made them
//! unreachable.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::archive::application::archive_status_queries::committed_status;
use crate::archive::domain::control_paths::ARCHIVE_GC_PATH;

/// Delete obsolete remote archive objects after the replacement checkpoint is
/// durable on every voter. A single-node caller may use its local checkpoint
/// as the same barrier.
#[doc(hidden)]
pub(crate) fn finalize_archive_gc_after_checkpoint(
    services: &ArchiveServices,
    root: &Path,
) -> Result<usize> {
    let (deleted, complete) =
        finalize_archive_gc_batch_after_checkpoint(services, root, usize::MAX)?;
    if !complete {
        bail!("archive GC did not finish its unbounded finalization pass");
    }
    Ok(deleted)
}

/// Delete at most `max_objects` obsolete archive objects and persist progress
/// after every successful delete. A later process can resume from the saved
/// catalog cursor without keeping the full cleanup plan in memory.
#[doc(hidden)]
pub(crate) fn finalize_archive_gc_batch_after_checkpoint(
    services: &ArchiveServices,
    root: &Path,
    max_objects: usize,
) -> Result<(usize, bool)> {
    if max_objects == 0 {
        bail!("archive GC batch size must be greater than zero");
    }
    let path = root.join(ARCHIVE_GC_PATH);
    if !path.exists() {
        return Ok((0, true));
    }
    let mut pending = services.read_archive_gc_pending(&path)?;
    let committed = committed_status(services, root)?
        .context("archive GC requires the replacement manifest receipt")?;
    if committed.manifest_uri != pending.replacement_manifest_uri
        || committed.manifest_sha256 != pending.replacement_manifest_sha256
    {
        bail!("archive GC replacement manifest is not the committed archive identity");
    }
    let mut deleted = 0_usize;
    let catalog = services.resources.catalog_for_uri(&pending.gc_plan_uri)?;
    let mut reader = match pending.cursor.as_deref() {
        Some(cursor) => catalog.reader_after(&pending.gc_plan_root, cursor)?,
        None => catalog.reader(&pending.gc_plan_root)?,
    };
    for entry in &mut reader {
        let entry = entry?;
        if deleted == max_objects {
            return Ok((deleted, false));
        }
        let uri = String::from_utf8(entry.value).context("decode archive GC object URI")?;
        let target = services
            .resources
            .gc_target(&uri)
            .with_context(|| format!("validate archive GC object URI {uri}"))?;
        target
            .delete()
            .with_context(|| format!("delete obsolete archive object {uri}"))?;
        pending.cursor = Some(entry.key);
        deleted += 1;
        services.persist_archive_gc_pending(&path, &pending)?;
    }
    services.remove_archive_gc_pending(&path)?;
    Ok((deleted, true))
}
