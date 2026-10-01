//! Storage-library conversions for the persisted catalog identity.

use crate::shared_kernel::catalog_manifest::{CatalogPageRef, CatalogRoot};

impl From<storage_segment::CatalogPageRef> for CatalogPageRef {
    fn from(value: storage_segment::CatalogPageRef) -> Self {
        Self {
            key: value.key,
            sha256: value.sha256,
            bytes: value.bytes,
            entry_count: value.entry_count,
            first_key: value.first_key,
            last_key: value.last_key,
        }
    }
}

impl From<&CatalogPageRef> for storage_segment::CatalogPageRef {
    fn from(value: &CatalogPageRef) -> Self {
        Self {
            key: value.key.clone(),
            sha256: value.sha256.clone(),
            bytes: value.bytes,
            entry_count: value.entry_count,
            first_key: value.first_key.clone(),
            last_key: value.last_key.clone(),
        }
    }
}

impl From<storage_segment::CatalogRoot> for CatalogRoot {
    fn from(value: storage_segment::CatalogRoot) -> Self {
        Self {
            format_version: value.format_version,
            height: value.height,
            entry_count: value.entry_count,
            page_bytes_limit: value.page_bytes_limit,
            root: value.root.into(),
        }
    }
}

impl From<&CatalogRoot> for storage_segment::CatalogRoot {
    fn from(value: &CatalogRoot) -> Self {
        Self {
            format_version: value.format_version,
            height: value.height,
            entry_count: value.entry_count,
            page_bytes_limit: value.page_bytes_limit,
            root: (&value.root).into(),
        }
    }
}
