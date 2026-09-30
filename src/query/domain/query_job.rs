//! An async query job: its status, its record, and the ids it may have.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Error)]
#[error("query id has an invalid format")]
pub(in crate::query) struct InvalidQueryId;

pub(in crate::query) fn validate_id(query_id: &str) -> Result<(), InvalidQueryId> {
    if query_id.len() != 32 || !query_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(InvalidQueryId);
    }
    Ok(())
}
