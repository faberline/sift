//! The operator namespace: a lowercase DNS label.

use thiserror::Error;

#[derive(Debug, Error)]
#[error("operator namespace must be a valid lowercase DNS label")]
pub(in crate::operations) struct InvalidNamespace;

pub(in crate::operations) fn validate_namespace(namespace: &str) -> Result<(), InvalidNamespace> {
    if namespace.is_empty()
        || namespace.len() > 63
        || namespace.starts_with('-')
        || namespace.ends_with('-')
        || namespace.chars().any(|character| {
            !character.is_ascii_lowercase() && !character.is_ascii_digit() && character != '-'
        })
    {
        return Err(InvalidNamespace);
    }
    Ok(())
}
