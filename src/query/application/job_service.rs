//! Query job use cases and the shared runner's persistence hook.

use crate::query::domain::job_store_port::QueryJobStorePort;
use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use anyhow::Result;
use std::sync::Arc;

pub(crate) struct QueryJobService {
    port: Arc<dyn QueryJobStorePort<Error = anyhow::Error>>,
}

impl QueryJobService {
    pub(crate) fn new(port: Arc<dyn QueryJobStorePort<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(crate) fn create(&self, request: QueryRequestV1) -> Result<QueryJobV1> {
        self.port.create(request)
    }
    pub(crate) fn get(&self, query_id: &str) -> Result<Option<QueryJobV1>> {
        self.port.get(query_id)
    }
    fn mark_running(&self, query_id: &str) -> Result<()> {
        self.port.mark_running(query_id)
    }
    fn succeed(&self, query_id: &str, result: QueryResponseV1) -> Result<()> {
        self.port.succeed(query_id, result)
    }
    fn fail(&self, query_id: &str, message: String) -> Result<()> {
        self.port.fail(query_id, message)
    }
}

impl service_executor::JobState<QueryResponseV1> for QueryJobService {
    type Error = anyhow::Error;
    type Id = String;

    fn mark_running(&self, id: &Self::Id) -> Result<()> {
        QueryJobService::mark_running(self, id)
    }

    fn succeed(&self, id: &Self::Id, output: QueryResponseV1) -> Result<()> {
        QueryJobService::succeed(self, id, output)
    }

    fn fail(&self, id: &Self::Id, message: String) -> Result<()> {
        QueryJobService::fail(self, id, message)
    }
}
