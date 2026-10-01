//! The versioned query response and its stats.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryStatsV1 {
    pub elapsed_ms: u64,
    pub scanned: usize,
    pub returned: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryResponseV1 {
    pub data: serde_json::Value,
    pub next_cursor: Option<String>,
    pub watermark: u64,
    pub partial: bool,
    pub warnings: Vec<String>,
    pub stats: QueryStatsV1,
    pub query_id: Option<String>,
}

impl QueryResponseV1 {
    pub fn complete(
        data: serde_json::Value,
        next_cursor: Option<String>,
        watermark: u64,
        scanned: usize,
        returned: usize,
        elapsed: Duration,
    ) -> Self {
        Self {
            data,
            next_cursor,
            watermark,
            partial: false,
            warnings: Vec::new(),
            stats: QueryStatsV1 {
                elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
                scanned,
                returned,
            },
            query_id: None,
        }
    }
}
