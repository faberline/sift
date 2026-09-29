//! The HTTP side of access: the scoped bearer-token middleware and the project
//! and admin checks the handlers run on the principal it attaches.

pub(crate) mod project_authorization;
pub(crate) mod scoped_authorization;
