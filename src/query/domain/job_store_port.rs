//! Persisted query jobs without filesystem or runner dependencies.

use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;

pub(crate) trait QueryJobStorePort: Send + Sync {
    type Error;
    fn create(&self, request: QueryRequestV1) -> Result<QueryJobV1, Self::Error>;
    fn get(&self, query_id: &str) -> Result<Option<QueryJobV1>, Self::Error>;
    fn mark_running(&self, query_id: &str) -> Result<(), Self::Error>;
    fn succeed(&self, query_id: &str, result: QueryResponseV1) -> Result<(), Self::Error>;
    fn fail(&self, query_id: &str, message: String) -> Result<(), Self::Error>;
}
