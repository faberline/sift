//! An async query job: its status, its record, and the ids it may have.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::query::interfaces::http::query_request_v1::QueryRequestV1;
use crate::query::interfaces::http::query_response_v1::QueryResponseV1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryJobStatusV1 {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryJobV1 {
    pub query_id: String,
    pub project: String,
    pub status: QueryJobStatusV1,
    pub created_at: String,
    pub updated_at: String,
    pub request: QueryRequestV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<QueryResponseV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(in crate::query) fn validate_id(query_id: &str) -> Result<()> {
    if query_id.len() != 32 || !query_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        bail!("query id has an invalid format");
    }
    Ok(())
}
