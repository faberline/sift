//! The MCP tool handler: Sift's read-only tools, their arguments and results.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_router, ErrorData, Json};
use schemars1::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::query::application::sift_api_service::SiftApiClient;
use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, LogTailRequestV1};
use crate::shared_kernel::query_request::QueryRequestV1;

#[derive(Clone)]
pub(super) struct SiftMcpServer {
    api: SiftApiClient,
}

impl SiftMcpServer {
    pub(super) fn new(api: SiftApiClient) -> Self {
        Self { api }
    }
}

impl service_mcp::McpApplication for SiftMcpServer {
    fn with_bearer_token(&self, token: Option<String>) -> Self {
        Self::new(self.api.with_token(token))
    }
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct QueryToolArgs {
    /// A versioned QueryRequestV1 JSON object.
    request: Value,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct TraceToolArgs {
    /// Authorized Sift project.
    project: String,
    /// Trace identifier.
    trace_id: String,
    /// Optional read-your-write watermark.
    min_cursor: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct CorrelateToolArgs {
    /// A versioned CorrelationRequestV1 JSON object.
    request: Value,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct ListServicesToolArgs {
    /// Authorized Sift project.
    project: String,
    /// Optional environment filter.
    environment: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct TailLogsToolArgs {
    /// A versioned LogTailRequestV1 JSON object.
    request: Value,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "schemars1")]
struct ControlToolArgs {
    project: String,
    after: Option<String>,
}

#[tool_router]
impl SiftMcpServer {
    #[tool(
        name = "sift_query",
        description = "Query Sift logs, metrics, or traces with a V1 or V2 request",
        annotations(read_only_hint = true)
    )]
    async fn sift_query(
        &self,
        Parameters(args): Parameters<QueryToolArgs>,
    ) -> std::result::Result<Json<Value>, ErrorData> {
        if args.request.get("version").and_then(Value::as_u64) == Some(2) {
            let request = parse_tool_request(args.request, "QueryRequestV2")?;
            return self
                .api
                .query_v2(&request)
                .await
                .and_then(|response| serde_json::to_value(response).map_err(Into::into))
                .map(Json)
                .map_err(tool_api_error);
        }
        let request: QueryRequestV1 = parse_tool_request(args.request, "QueryRequestV1")?;
        request
            .validate()
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        self.api
            .query_value(&request)
            .await
            .map(Json)
            .map_err(tool_api_error)
    }

    #[tool(
        name = "sift_get_trace",
        description = "Get one trace by project and trace ID",
        annotations(read_only_hint = true)
    )]
    async fn sift_get_trace(
        &self,
        Parameters(args): Parameters<TraceToolArgs>,
    ) -> std::result::Result<Json<Value>, ErrorData> {
        if args.project.trim().is_empty() || args.trace_id.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "project and trace_id must not be empty",
                None,
            ));
        }
        self.api
            .get_trace(&args.project, &args.trace_id, args.min_cursor)
            .await
            .map(Json)
            .map_err(tool_api_error)
    }

    #[tool(
        name = "sift_correlate",
        description = "Find related logs, metrics, and traces",
        annotations(read_only_hint = true)
    )]
    async fn sift_correlate(
        &self,
        Parameters(args): Parameters<CorrelateToolArgs>,
    ) -> std::result::Result<Json<Value>, ErrorData> {
        let request: CorrelationRequestV1 =
            parse_tool_request(args.request, "CorrelationRequestV1")?;
        request
            .validate()
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        self.api
            .correlate(&request)
            .await
            .map(Json)
            .map_err(tool_api_error)
    }

    #[tool(
        name = "sift_list_services",
        description = "List services seen by Sift in one project",
        annotations(read_only_hint = true)
    )]
    async fn sift_list_services(
        &self,
        Parameters(args): Parameters<ListServicesToolArgs>,
    ) -> std::result::Result<Json<Value>, ErrorData> {
        if args.project.trim().is_empty()
            || args
                .environment
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ErrorData::invalid_params(
                "project and environment must not be empty",
                None,
            ));
        }
        self.api
            .list_services(&args.project, args.environment.as_deref())
            .await
            .map(Json)
            .map_err(tool_api_error)
    }

    #[tool(
        name = "sift_tail_logs",
        description = "Wait for and return new Sift log records",
        annotations(read_only_hint = true)
    )]
    async fn sift_tail_logs(
        &self,
        Parameters(args): Parameters<TailLogsToolArgs>,
    ) -> std::result::Result<Json<Value>, ErrorData> {
        let request: LogTailRequestV1 = parse_tool_request(args.request, "LogTailRequestV1")?;
        request
            .validate()
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        self.api
            .tail_logs(&request)
            .await
            .map(Json)
            .map_err(tool_api_error)
    }

    #[tool(
        name = "sift_list_alert_rules",
        description = "Read alert rules in one project",
        annotations(read_only_hint = true)
    )]
    async fn sift_list_alert_rules(
        &self,
        Parameters(args): Parameters<ControlToolArgs>,
    ) -> Result<Json<Value>, ErrorData> {
        self.read_control(args, "alert-rules").await
    }
    #[tool(
        name = "sift_list_alerts",
        description = "Read alert states in one project",
        annotations(read_only_hint = true)
    )]
    async fn sift_list_alerts(
        &self,
        Parameters(args): Parameters<ControlToolArgs>,
    ) -> Result<Json<Value>, ErrorData> {
        self.read_control(args, "alerts").await
    }
    #[tool(
        name = "sift_list_error_groups",
        description = "Read error groups in one project",
        annotations(read_only_hint = true)
    )]
    async fn sift_list_error_groups(
        &self,
        Parameters(args): Parameters<ControlToolArgs>,
    ) -> Result<Json<Value>, ErrorData> {
        self.read_control(args, "error-groups").await
    }
    #[tool(
        name = "sift_list_saved_queries",
        description = "Read saved queries in one project",
        annotations(read_only_hint = true)
    )]
    async fn sift_list_saved_queries(
        &self,
        Parameters(args): Parameters<ControlToolArgs>,
    ) -> Result<Json<Value>, ErrorData> {
        self.read_control(args, "saved-queries").await
    }
}

impl SiftMcpServer {
    async fn read_control(
        &self,
        args: ControlToolArgs,
        collection: &str,
    ) -> Result<Json<Value>, ErrorData> {
        self.api
            .control_records(&args.project, collection, args.after.as_deref())
            .await
            .map(Json)
            .map_err(tool_api_error)
    }
}

#[rmcp::tool_handler]
impl rmcp::ServerHandler for SiftMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sift", env!("CARGO_PKG_VERSION")))
            .with_instructions("Read-only logs, metrics, traces, and correlation tools for Sift")
    }
}

fn parse_tool_request<T: DeserializeOwned>(
    value: Value,
    name: &str,
) -> std::result::Result<T, ErrorData> {
    serde_json::from_value(value)
        .map_err(|error| ErrorData::invalid_params(format!("invalid {name}: {error}"), None))
}

fn tool_api_error(error: anyhow::Error) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

/// Names are exposed for contract tests and client discovery documentation.
pub fn tool_names() -> Vec<String> {
    let mut names = SiftMcpServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}
