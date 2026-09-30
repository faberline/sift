//! Where ingest meets the process and the journal: the limits read from the
//! environment, the governance policy file, and the group-commit batch the
//! journal commits.

pub(crate) mod governance_policy_file;
pub(crate) mod ingest_batch_coordinator;
pub(crate) mod ingest_limits_env;
