//! Inspecting the committed archive catalog and the pending GC plan.

use anyhow::Result;

use crate::archive::infrastructure::archive_catalog::{load_archive_catalog, load_gc_catalog};
use crate::shared_kernel::archive_manifest::{ArchiveBlob, ArchiveManifest, ArchiveSegment};

#[doc(hidden)]
pub fn inspect_archive_catalog(
    manifest: &ArchiveManifest,
) -> Result<(Vec<ArchiveSegment>, Vec<ArchiveBlob>)> {
    load_archive_catalog(manifest)
}

#[doc(hidden)]
pub fn inspect_archive_gc_plan(manifest: &ArchiveManifest) -> Result<Vec<String>> {
    load_gc_catalog(manifest)
}
