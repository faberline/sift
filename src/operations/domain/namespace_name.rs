//! The operator namespace: a lowercase DNS label.

use anyhow::{bail, Result};

pub(in crate::operations) fn validate_namespace(namespace: &str) -> Result<()> {
    if namespace.is_empty()
        || namespace.len() > 63
        || namespace.starts_with('-')
        || namespace.ends_with('-')
        || namespace.chars().any(|character| {
            !character.is_ascii_lowercase() && !character.is_ascii_digit() && character != '-'
        })
    {
        bail!("operator namespace must be a valid lowercase DNS label");
    }
    Ok(())
}
