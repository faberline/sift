//! The HTTP side of operations: the gateway's reverse proxy and the admin
//! backup and integrity endpoints.

pub(crate) mod admin_backup;
pub(crate) mod admin_integrity;
pub(crate) mod gateway_proxy;
pub(crate) mod integrity_report_v1;
