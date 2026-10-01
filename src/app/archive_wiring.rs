//! Default storage wiring for archive entry points.

use std::sync::Arc;

use crate::archive::application::manifest_validation::ManifestValidator;
use crate::archive::infrastructure::manifest_validation::GcsManifestValidation;

impl Default for ManifestValidator {
    fn default() -> Self {
        Self::new(Arc::new(GcsManifestValidation))
    }
}
