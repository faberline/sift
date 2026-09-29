//! Where queries go outside the process: the on-disk job store, the client the
//! query role forwards to the store with, and the HTTP client MCP tools call
//! the Sift API through.

pub(crate) mod file_query_job_store;
pub(crate) mod sift_api_client;
pub(crate) mod store_query_client;
