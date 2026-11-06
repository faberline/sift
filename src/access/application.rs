//! Access use cases shared by HTTP, gRPC and project data handlers.

pub(crate) mod authorization_service;
pub(crate) mod project_authorization;

#[cfg(test)]
mod authorization_service_tests;
