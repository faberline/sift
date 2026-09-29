//! Operations' rules: what a namespace name and an image reference must look
//! like before they go into a manifest, and which routes the gateway sends to
//! the query role.

pub(crate) mod image_reference;
pub(crate) mod namespace_name;
pub(crate) mod route_class;
