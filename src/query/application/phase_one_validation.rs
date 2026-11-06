//! Validation for tail and correlation requests.

use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::shared_kernel::query_request::{MAX_QUERY_LIMIT, QUERY_AST_VERSION};
use anyhow::{bail, Result};

impl LogTailRequestV1 {
    pub fn validate(&self) -> Result<()> {
        if self.version != QUERY_AST_VERSION {
            bail!(
                "unsupported query version {}; expected {QUERY_AST_VERSION}",
                self.version
            );
        }
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_QUERY_LIMIT}");
        }
        if self.wait_ms > 30_000 {
            bail!("wait_ms must not exceed 30000");
        }
        Ok(())
    }
}

impl CorrelationRequestV1 {
    pub fn validate(&self) -> Result<()> {
        if self.version != QUERY_AST_VERSION {
            bail!(
                "unsupported query version {}; expected {QUERY_AST_VERSION}",
                self.version
            );
        }
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_QUERY_LIMIT}");
        }
        if self.trace_id.as_deref().is_none_or(str::is_empty)
            && self.span_id.as_deref().is_none_or(str::is_empty)
            && self.service.as_deref().is_none_or(str::is_empty)
            && self.attributes.is_empty()
        {
            bail!("correlation requires trace_id, span_id, service, or attributes");
        }
        Ok(())
    }
}
