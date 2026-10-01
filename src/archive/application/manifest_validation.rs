//! Validate a manifest through the injected storage policy.

use std::sync::Arc;

use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::domain::archive_manifest_validator::{self, ArchiveManifestError};
use crate::archive::domain::manifest_validation_policy::{
    ManifestValidationPolicy, UriValidationError,
};

pub(crate) struct ManifestValidator {
    policy: Arc<dyn ManifestValidationPolicy<Error = UriValidationError>>,
}

impl ManifestValidator {
    pub(crate) fn new(
        policy: Arc<dyn ManifestValidationPolicy<Error = UriValidationError>>,
    ) -> Self {
        Self { policy }
    }

    pub(crate) fn validate(&self, manifest: &ArchiveManifest) -> Result<(), ArchiveManifestError> {
        archive_manifest_validator::validate_archive_manifest(manifest, self.policy.as_ref())
    }
}

pub(crate) fn validate_archive_manifest(
    manifest: &ArchiveManifest,
) -> Result<(), ArchiveManifestError> {
    ManifestValidator::default().validate(manifest)
}
