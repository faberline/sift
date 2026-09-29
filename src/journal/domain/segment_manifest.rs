//! A segment's state and manifest, and where an appended event landed.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::journal::domain::shard_route::Route;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    Sealed,
    Moved,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SegmentManifest {
    pub segment_id: String,
    pub epoch: u64,
    pub shard: u16,
    pub bucket_min: u16,
    pub bucket_max: u16,
    pub first_cursor: u64,
    pub last_cursor: u64,
    pub event_count: u64,
    #[serde(default)]
    pub min_event_time_unix_nano: i64,
    #[serde(default)]
    pub max_event_time_unix_nano: i64,
    pub bytes: u64,
    pub sha256: String,
    pub state: SegmentState,
    pub local_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_uri: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendLocation {
    pub route: Route,
    pub segment_id: String,
    pub path: PathBuf,
}
