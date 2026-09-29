//! The node: one Sift process's own data root. It owns the private, versioned
//! data directory and its layout manifest, the role the process runs as, and
//! the local disk-capacity guard that refuses writes before the volume fills.

pub(crate) mod domain;
pub(crate) mod infrastructure;
