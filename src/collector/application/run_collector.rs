//! The collector's entry point, and the summary a finished run reports.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::collector::application::runtime;
use crate::collector::domain::config::CollectorConfig;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CollectorSummary {
    pub source_id: String,
    pub start_offset: u64,
    pub final_offset: u64,
    pub lines: u64,
    pub accepted: u64,
    pub duplicates: u64,
    pub rejected: u64,
    pub lost_bytes: u64,
    pub lost_sources: u64,
}

pub async fn run_collector(config: CollectorConfig) -> Result<CollectorSummary> {
    config.validate()?;
    runtime::run(config).await
}
