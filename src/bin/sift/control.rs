//! Manage rules, saved queries, error status, and alert mutes through the API.
use crate::terminal_output::print_json_terminal;
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use reqwest::Method;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
#[derive(Clone, ValueEnum)]
enum Collection {
    AlertRules,
    Alerts,
    ErrorGroups,
    SavedQueries,
}
impl Collection {
    fn path(&self) -> &str {
        match self {
            Self::AlertRules => "alert-rules",
            Self::Alerts => "alerts",
            Self::ErrorGroups => "error-groups",
            Self::SavedQueries => "saved-queries",
        }
    }
}
#[derive(Args)]
pub(super) struct ControlArgs {
    #[arg(value_enum)]
    collection: Collection,
    #[arg(
        long,
        env = "SIFT_URL",
        default_value = "http://127.0.0.1:7380",
        global = true
    )]
    endpoint: String,
    #[arg(long, env = "SIFT_TOKEN", global = true)]
    token: Option<String>,
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    List {
        #[arg(long)]
        project: String,
        #[arg(long)]
        after: Option<String>,
    },
    /// Write a rule or saved query. The JSON includes project, request_id, expected_revision, value.
    Put { id: String, request: PathBuf },
    /// Delete a rule or saved query. The JSON includes project, request_id, expected_revision.
    Delete { id: String, request: PathBuf },
    /// Set an error group to open, resolved, or ignored.
    Status {
        id: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        expected_revision: u64,
        status: String,
    },
    /// Set the alert mute end as a Unix timestamp. Omit --until to unmute.
    Mute {
        id: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        until: Option<i64>,
    },
}
pub(super) async fn control(args: ControlArgs) -> Result<()> {
    let mut url = url::Url::parse(&args.endpoint)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("control endpoint requires http or https and a host");
    }
    url.set_path(&format!("/api/v1/{}", args.collection.path()));
    url.set_query(None);
    url.set_fragment(None);
    let is_put = matches!(&args.action, Action::Put { .. });
    let (method, body, project, id, suffix) = match args.action {
        Action::List { project, after } => {
            url.query_pairs_mut().append_pair("project", &project);
            if let Some(after) = after {
                url.query_pairs_mut().append_pair("after", &after);
            }
            (Method::GET, None, project, None, "")
        }
        Action::Put { id, request } | Action::Delete { id, request } => {
            if !matches!(
                args.collection,
                Collection::AlertRules | Collection::SavedQueries
            ) {
                bail!("put and delete apply to rules and saved queries");
            }
            let method = if is_put { Method::PUT } else { Method::DELETE };
            let bytes = std::fs::read(request)?;
            if bytes.len() > 1024 * 1024 {
                bail!("control request exceeds 1 MiB");
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            let project = value
                .get("project")
                .and_then(Value::as_str)
                .context("request requires project")?
                .to_string();
            (method, Some(value), project, Some(id), "")
        }
        Action::Status {
            id,
            project,
            request_id,
            expected_revision,
            status,
        } => {
            if !matches!(args.collection, Collection::ErrorGroups)
                || !["open", "resolved", "ignored"].contains(&status.as_str())
            {
                bail!("status requires error-groups and open, resolved, or ignored");
            }
            (
                Method::PUT,
                Some(
                    json!({"project":project,"request_id":request_id,"expected_revision":expected_revision,"status":status}),
                ),
                project,
                Some(id),
                "",
            )
        }
        Action::Mute {
            id,
            project,
            request_id,
            expected_revision,
            until,
        } => {
            if !matches!(args.collection, Collection::Alerts) {
                bail!("mute requires alerts");
            }
            (
                Method::PUT,
                Some(
                    json!({"project":project,"request_id":request_id,"expected_revision":expected_revision,"until":until}),
                ),
                project,
                Some(id),
                "mute",
            )
        }
    };
    if let Some(id) = id {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid endpoint"))?;
        segments.push(&id);
        if !suffix.is_empty() {
            segments.push(suffix);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut request = client
        .request(method, url)
        .header("x-sift-project", &project);
    if let Some(token) = args.token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
    let status = response.status();
    let value: Value = response.json().await.context("decode control response")?;
    if !status.is_success() {
        bail!("control request HTTP {status}: {value}");
    }
    print_json_terminal(value)
}
