//! Storage-specific manifest limits and URI checks.

pub(crate) type UriValidationError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub(crate) trait ManifestValidationPolicy: Send + Sync {
    type Error;

    fn catalog_page_bytes(&self) -> usize;
    fn validate_gc_uri(&self, uri: &str) -> Result<(), Self::Error>;
}
