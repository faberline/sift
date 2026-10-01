//! Inspecting the committed archive catalog and the pending GC plan.

use crate::archive::application::archive_services::ArchiveServices;

use anyhow::Result;

use crate::archive::application::catalog_reads::{load_archive_catalog, load_gc_catalog};
use crate::shared_kernel::archive_manifest::{ArchiveBlob, ArchiveManifest, ArchiveSegment};

#[doc(hidden)]
pub(crate) fn inspect_archive_catalog(
    services: &ArchiveServices,
    manifest: &ArchiveManifest,
) -> Result<(Vec<ArchiveSegment>, Vec<ArchiveBlob>)> {
    load_archive_catalog(services, manifest)
}

#[doc(hidden)]
pub(crate) fn inspect_archive_gc_plan(
    services: &ArchiveServices,
    manifest: &ArchiveManifest,
) -> Result<Vec<String>> {
    load_gc_catalog(services, manifest)
}
