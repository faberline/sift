//! The project and admin checks a handler runs on the principal the middleware
//! attached: write or read access to one project, or wildcard admin access.

use anyhow::Result;
use service_auth::{Role, RoleMapPrincipal};

use crate::ApiError;

pub(crate) fn authorize_project(
    principal: Option<&RoleMapPrincipal>,
    project: &str,
) -> Result<(), ApiError> {
    authorize_project_role(principal, project, Role::Write)
}

pub(crate) fn authorize_project_read(
    principal: Option<&RoleMapPrincipal>,
    project: &str,
) -> Result<(), ApiError> {
    authorize_project_role(principal, project, Role::Read)
}

pub(crate) fn authorize_global_admin(principal: Option<&RoleMapPrincipal>) -> Result<(), ApiError> {
    match principal {
        None | Some(RoleMapPrincipal::Open) => Ok(()),
        Some(principal) => principal.ensure("*", Role::Admin).map_err(|denied| {
            ApiError::forbidden(format!(
                "subject `{}` lacks wildcard admin access required for this admin operation",
                denied.subject
            ))
        }),
    }
}

fn authorize_project_role(
    principal: Option<&RoleMapPrincipal>,
    project: &str,
    role: Role,
) -> Result<(), ApiError> {
    match principal {
        None | Some(RoleMapPrincipal::Open) => Ok(()),
        Some(principal) => principal.ensure(project, role).map_err(|denied| {
            ApiError::forbidden(format!(
                "subject `{}` lacks {:?} access to project `{}`",
                denied.subject, denied.needed, denied.resource
            ))
        }),
    }
}
