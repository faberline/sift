//! The values rendered into a manifest: a bounded single token, and for the
//! operator image, only characters an OCI image reference allows.

use anyhow::{bail, Result};

pub(in crate::operations) fn validate_manifest_value(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > 512
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        bail!("{name} must be a nonempty single bounded token");
    }
    Ok(())
}

pub(in crate::operations) fn validate_image(image: &str) -> Result<()> {
    validate_manifest_value("operator image", image)?;
    if image.chars().any(|character| {
        !character.is_ascii_alphanumeric()
            && !matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        bail!("operator image contains characters outside an OCI image reference");
    }
    Ok(())
}
