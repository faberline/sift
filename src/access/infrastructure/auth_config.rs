//! The auth configuration: whether auth is required, and the static tokens read
//! from the token registry file or the legacy inline variable.

use std::collections::HashMap;

use anyhow::{bail, Result};
use service_auth::TokenClaims;

const TOKEN_REGISTRY_FILE_ENV: &str = "SIFT_TOKEN_REGISTRY_FILE";
const LEGACY_TOKENS_ENV: &str = "SIFT_TOKENS";

#[derive(Debug, Clone)]
pub struct SiftAuthConfig {
    pub required: bool,
    pub tokens: HashMap<String, TokenClaims>,
}

impl SiftAuthConfig {
    pub fn open() -> Self {
        Self {
            required: false,
            tokens: HashMap::new(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let required = match std::env::var("SIFT_AUTH") {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "required" => true,
                "off" | "disabled" => false,
                other => bail!("SIFT_AUTH must be `off`, `disabled`, or `required`; got `{other}`"),
            },
            Err(std::env::VarError::NotPresent) => false,
            Err(error) => bail!("SIFT_AUTH must be valid UTF-8: {error}"),
        };
        let tokens = service_auth::load_registry(
            required,
            TOKEN_REGISTRY_FILE_ENV,
            std::env::var(TOKEN_REGISTRY_FILE_ENV).ok().as_deref(),
            LEGACY_TOKENS_ENV,
            std::env::var(LEGACY_TOKENS_ENV).ok().as_deref(),
        )?;
        Ok(Self { required, tokens })
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::*;

    #[test]
    fn open_config_has_no_tokens() {
        let config = SiftAuthConfig::open();
        assert!(!config.required);
        assert!(config.tokens.is_empty());
    }
}
