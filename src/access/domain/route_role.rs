//! The role each HTTP route needs, and the Kubernetes verb a role asks the API
//! server about.

use axum::http::Method;
use service_auth::Role;

pub(in crate::access) fn role_verb(role: Role) -> &'static str {
    match role {
        Role::Read => "get",
        Role::Write => "create",
        Role::Admin => "update",
    }
}

pub(in crate::access) fn http_role(method: &Method, path: &str) -> Role {
    if path.starts_with("/admin/") {
        Role::Admin
    } else if matches!(
        path,
        "/v1/logs" | "/v1/metrics" | "/v1/traces" | "/prometheus/api/v1/write"
    ) || (*method != Method::GET
        && !matches!(
            path,
            "/api/v1/query"
                | "/api/v1/logs/tail"
                | "/api/v1/correlate"
                | "/prometheus/api/v1/query"
                | "/prometheus/api/v1/query_range"
        ))
    {
        Role::Write
    } else {
        Role::Read
    }
}
