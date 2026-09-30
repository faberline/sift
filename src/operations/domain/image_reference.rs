//! The values rendered into a manifest: a bounded single token, and for the
//! operator image, only characters an OCI image reference allows.

use thiserror::Error;

#[derive(Debug, Error)]
pub(in crate::operations) enum ManifestValueError {
    #[error("{0} must be a nonempty single bounded token")]
    NotSingleToken(String),
    #[error("operator image contains characters outside an OCI image reference")]
    NotImageReference,
}

pub(in crate::operations) fn validate_manifest_value(
    name: &str,
    value: &str,
) -> Result<(), ManifestValueError> {
    if value.trim().is_empty()
        || value.len() > 512
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(ManifestValueError::NotSingleToken(name.to_owned()));
    }
    Ok(())
}

pub(in crate::operations) fn validate_image(image: &str) -> Result<(), ManifestValueError> {
    validate_manifest_value("operator image", image)?;
    if image.chars().any(|character| {
        !character.is_ascii_alphanumeric()
            && !matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        return Err(ManifestValueError::NotImageReference);
    }
    Ok(())
}
