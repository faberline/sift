//! Loading the governance policy set from the JSON file
//! SIFT_GOVERNANCE_POLICY_FILE names.

use std::path::Path;
use std::{env, fs};

use anyhow::{Context, Result};

use crate::ingest::domain::governance_policy::GovernancePolicySet;

const POLICY_FILE_ENV: &str = "SIFT_GOVERNANCE_POLICY_FILE";

impl GovernancePolicySet {
    pub fn from_env() -> Result<Self> {
        let Ok(path) = env::var(POLICY_FILE_ENV) else {
            return Ok(Self::default());
        };
        Self::from_path(path)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let policies: Self = serde_json::from_slice(
            &fs::read(path)
                .with_context(|| format!("read Sift governance policy {}", path.display()))?,
        )
        .with_context(|| format!("decode Sift governance policy {}", path.display()))?;
        policies.validate()?;
        Ok(policies)
    }
}
