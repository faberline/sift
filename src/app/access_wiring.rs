//! Connect the same authentication adapter to all access use cases.

use crate::access::application::authorization_service::AuthorizationService;
use crate::access::infrastructure::sift_verifier::SiftVerifier;
use std::sync::Arc;

impl AuthorizationService {
    pub fn from_verifier(verifier: Arc<SiftVerifier>) -> Self {
        Self::new(verifier)
    }
}
