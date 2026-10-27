//! The role each HTTP route needs, and the Kubernetes verb a role asks the API
//! server about.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RouteRole {
    Read,
    Write,
    Admin,
}

pub(in crate::access) fn role_verb(role: RouteRole) -> &'static str {
    match role {
        RouteRole::Read => "get",
        RouteRole::Write => "create",
        RouteRole::Admin => "update",
    }
}

pub(in crate::access) fn http_role(method: &str, path: &str) -> RouteRole {
    if path.starts_with("/admin/") {
        RouteRole::Admin
    } else if matches!(
        path,
        "/v1/logs" | "/v1/metrics" | "/v1/traces" | "/prometheus/api/v1/write"
    ) || (method != "GET"
        && !matches!(
            path,
            "/api/v1/query"
                | "/api/v1/logs/tail"
                | "/api/v1/correlate"
                | "/prometheus/api/v1/query"
                | "/prometheus/api/v1/query_range"
        ))
    {
        RouteRole::Write
    } else {
        RouteRole::Read
    }
}
