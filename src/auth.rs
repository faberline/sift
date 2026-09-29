//! The auth module's public paths. The access context now owns the auth
//! configuration, the verifier and the scoped bearer middleware; these
//! re-exports keep `sift::auth::*` compiling for callers outside the crate.

pub use crate::access::infrastructure::auth_config::SiftAuthConfig;
pub use crate::access::infrastructure::sift_verifier::SiftVerifier;
pub use crate::access::interfaces::http::scoped_authorization::auth_middleware;
