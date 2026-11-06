//! The tool's own commands: `sift spec` and its client generator, `sift llm`'s
//! offline documentation, `sift upgrade` and `sift issue`, with the build
//! information and help topics they read.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct SpecArgs {
    /// Generate a typed API client rather than print the OpenAPI document.
    #[command(subcommand)]
    command: Option<SpecCommand>,
    #[arg(long, default_value = "openapi-json", value_enum)]
    format: SpecFormat,
}

#[derive(Clone, Copy, ValueEnum)]
enum SpecFormat {
    OpenapiJson,
    OpenapiYaml,
    JsonSchema,
}

#[derive(Subcommand)]
enum SpecCommand {
    /// Generate a typed client from Sift's own OpenAPI document.
    Gen(GenArgs),
}

#[derive(Args)]
struct GenArgs {
    /// Target language for the generated client.
    #[arg(long, value_enum)]
    lang: GenLang,
    /// Directory that receives the generated client files.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
enum GenLang {
    /// TypeScript types and fetch client.
    Ts,
    /// Python Pydantic models and HTTP/2 client.
    Py,
    /// Rust serde models and reqwest client.
    Rust,
}

#[derive(Args)]
pub(super) struct LlmArgs {
    #[arg(long, default_value = "outline")]
    pub(super) topic: String,
    #[arg(long, default_value = "md", value_parser = ["md", "json"])]
    pub(super) format: String,
}

#[derive(Args)]
pub(super) struct UpgradeArgs {
    #[arg(long)]
    pub(super) check: bool,
    #[arg(long = "version")]
    pub(super) tag: Option<String>,
    #[arg(long)]
    pub(super) force: bool,
    #[arg(short = 'y', long)]
    pub(super) yes: bool,
}

#[derive(Args)]
pub(super) struct IssueArgs {
    #[command(subcommand)]
    command: IssueCommand,
}

#[derive(Subcommand)]
enum IssueCommand {
    Search(IssueSearchArgs),
    View(IssueViewArgs),
    Create(IssueCreateArgs),
    Comment(IssueCommentArgs),
}

#[derive(Args)]
struct IssueSearchArgs {
    #[arg(value_name = "QUERY", num_args = 0..)]
    query: Vec<String>,
    #[arg(long, default_value = "open", value_parser = ["open", "closed", "all"])]
    state: String,
    #[arg(long, default_value_t = 20)]
    limit: u32,
}

#[derive(Args)]
struct IssueViewArgs {
    number: u64,
}

#[derive(Args)]
struct IssueCreateArgs {
    #[arg(short = 't', long)]
    title: Option<String>,
    #[arg(value_name = "MESSAGE", num_args = 0..)]
    message: Vec<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(short = 'y', long)]
    yes: bool,
}

#[derive(Args)]
struct IssueCommentArgs {
    number: u64,
    #[arg(value_name = "MESSAGE", num_args = 0..)]
    message: Vec<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(short = 'y', long)]
    yes: bool,
}

pub(super) const TOOL: cli_std::ToolInfo = cli_std::ToolInfo {
    project: "sift",
    repo: "faberline/sift",
    target: env!("SIFT_TARGET"),
    version: env!("CARGO_PKG_VERSION"),
    git_sha: env!("SIFT_GIT_SHA"),
    built_at: env!("SIFT_BUILT_AT"),
};

pub(super) const LLM_TOPICS: &[cli_std::llm::Topic] = &[
    cli_std::llm::Topic {
        id: "ingest",
        summary: "OTLP logs, metrics, traces, and durable acknowledgement",
        body: "# Sift ingest\n\nUse OTLP/HTTP `/v1/logs`, `/v1/metrics`, and `/v1/traces`, or OTLP/gRPC on port 4317. Metrics clients can also use Prometheus Remote Write 1.0 at `/prometheus/api/v1/write`. Use `sift collect --source <service.stdout.jsonl>` for file capture, `--source -` for stdin, or `--cri-root /var/log/pods --gcp-project <id>` for Kubernetes CRI logs. Collector checkpoints live under `/var/lib/sift/agent` by default. Accepted items have completed the durable Sift append path.",
    },
    cli_std::llm::Topic {
        id: "operations",
        summary: "persistent roles, unified query, MCP, and backup",
        body: "# Sift operations\n\nRun `sift serve` with a writable `/var/lib/sift`, or set `--data-dir`. The process serves HTTP/1.1 and h2c plus OTLP/gRPC. Use `sift query <request.json> --endpoint <url>` for the versioned logs, metrics, or traces query API. Use `sift mcp serve --stdio --endpoint <url>` for the same read-only capabilities through MCP. Scheduled backups use `sift backup --url <service> --dest <uri>` and may supply `--token` or `SIFT_BACKUP_TOKEN`.",
    },
];

pub(super) fn spec(args: SpecArgs) -> Result<()> {
    if let Some(SpecCommand::Gen(args)) = args.command {
        return spec_gen(args);
    }

    let document = sift::openapi();
    match args.format {
        SpecFormat::OpenapiJson => print_json_terminal(serde_json::json!({"openapi": document})),
        SpecFormat::OpenapiYaml => {
            print!("{}", serde_yaml::to_string(&document)?);
            println!("# next: done");
            Ok(())
        }
        SpecFormat::JsonSchema => {
            let mut definitions = serde_json::Map::new();
            for (name, schema) in document.components.unwrap_or_default().schemas {
                let mut schema = serde_json::to_value(schema)?;
                convert_schema(&mut schema);
                definitions.insert(name, schema);
            }
            print_json_terminal(serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$defs": definitions,
            }))
        }
    }
}

fn convert_schema(schema: &mut Value) {
    let Value::Object(object) = schema else {
        return;
    };

    if let Some(Value::String(reference)) = object.get_mut("$ref") {
        if let Some(name) = reference.strip_prefix("#/components/schemas/") {
            *reference = format!("#/$defs/{name}");
        }
    }
    object.remove("discriminator");
    let nullable = object.remove("nullable") == Some(Value::Bool(true));

    // Only visit schema keywords. Other objects can contain literal data.
    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        properties.values_mut().for_each(convert_schema);
    }
    for keyword in ["items", "additionalProperties", "not"] {
        if let Some(child) = object.get_mut(keyword) {
            convert_schema(child);
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(Value::Array(children)) = object.get_mut(keyword) {
            children.iter_mut().for_each(convert_schema);
        }
    }

    if nullable {
        *schema = serde_json::json!({"anyOf": [std::mem::take(schema), {"type": "null"}]});
    }
}

fn spec_gen(args: GenArgs) -> Result<()> {
    use openapi_codegen::{generate, GenOptions, HttpClient, Lang};

    let lang = match args.lang {
        GenLang::Ts => Lang::Ts,
        GenLang::Py => Lang::Py,
        GenLang::Rust => Lang::Rust,
    };
    let output = generate(
        &sift::openapi_json()?,
        &GenOptions {
            lang,
            target: None,
            spec_path: PathBuf::new(),
            out_dir: args.out.clone(),
            client_name: "createClient".to_string(),
            http_client: HttpClient::Fetch,
            emit_types: true,
            emit_client: true,
            emit_hooks: matches!(lang, Lang::Ts),
        },
    )?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("create generated client directory {}", args.out.display()))?;
    for file in output.files {
        let path = args.out.join(file.rel_path);
        std::fs::write(&path, file.contents)
            .with_context(|| format!("write generated client file {}", path.display()))?;
        println!("generated {}", path.display());
    }
    let entrypoint = match lang {
        Lang::Ts => "index.ts",
        Lang::Py => "__init__.py",
        Lang::Rust => "mod.rs",
    };
    println!("next: {}", args.out.join(entrypoint).display());
    Ok(())
}

pub(super) async fn issue(args: IssueArgs) -> Result<()> {
    match args.command {
        IssueCommand::Search(args) => {
            cli_std::issue::search(
                &TOOL,
                cli_std::issue::SearchOptions {
                    query: (!args.query.is_empty()).then(|| args.query.join(" ")),
                    state: args.state,
                    limit: args.limit,
                },
            )
            .await
        }
        IssueCommand::View(args) => cli_std::issue::view(&TOOL, args.number).await,
        IssueCommand::Create(args) => {
            let message = (!args.message.is_empty()).then(|| args.message.join(" "));
            let title = args
                .title
                .unwrap_or_else(|| "sift: issue report".to_string());
            cli_std::issue::create(
                &TOOL,
                cli_std::issue::CreateOptions {
                    title,
                    message,
                    label: vec!["app:sift".to_string()],
                    dry_run: args.dry_run,
                    yes: args.yes,
                    ..Default::default()
                },
            )
            .await
        }
        IssueCommand::Comment(args) => {
            cli_std::issue::comment(
                &TOOL,
                cli_std::issue::CommentOptions {
                    number: args.number,
                    message: (!args.message.is_empty()).then(|| args.message.join(" ")),
                    dry_run: args.dry_run,
                    yes: args.yes,
                    ..Default::default()
                },
            )
            .await
        }
    }
}
