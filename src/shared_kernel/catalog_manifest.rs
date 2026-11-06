//! The catalog's persisted identity. Storage adapters convert these values to
//! the storage library's types. Field order is part of the manifest contract.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogPageRef {
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
    pub entry_count: u64,
    pub first_key: String,
    pub last_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogRoot {
    pub format_version: u16,
    pub height: u16,
    pub entry_count: u64,
    pub page_bytes_limit: u32,
    pub root: CatalogPageRef,
}
