//! The storage-library limits and URI validation. Error sources stay intact.

use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::domain::archive_manifest_validator::{self, ArchiveManifestError};
use crate::archive::domain::manifest_validation_policy::{
    ManifestValidationPolicy, UriValidationError,
};

pub(crate) struct GcsManifestValidation;

impl ManifestValidationPolicy for GcsManifestValidation {
    type Error = UriValidationError;

    fn catalog_page_bytes(&self) -> usize {
        storage_segment::DEFAULT_CATALOG_PAGE_BYTES
    }

    fn validate_gc_uri(&self, uri: &str) -> Result<(), Self::Error> {
        service_backup::GcsSink::from_exact_uri(uri)
            .map(|_| ())
            .map_err(Into::into)
    }
}

pub(in crate::archive) fn validate_archive_manifest(
    manifest: &ArchiveManifest,
) -> Result<(), ArchiveManifestError> {
    archive_manifest_validator::validate_archive_manifest(manifest, &GcsManifestValidation)
}
