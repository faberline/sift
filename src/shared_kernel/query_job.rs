//! Stable persisted query job data.

use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use serde::{Deserialize, Serialize};

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
