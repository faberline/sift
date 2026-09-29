//! The `sift` binary. main installs the TLS crypto provider, parses the command
//! line and runs the subcommand: `spec`, `llm`, `upgrade` and the build-info
//! acceptance probe inline, every other subcommand in its own module.

mod acceptance_grpc;
mod acceptance_payload;
mod backup;
mod cli;
mod collect;
mod connect;
mod k8s;
mod mcp;
mod meta;
mod query;
mod restore;
mod serve;
mod serve_args;
mod snapshot;
mod terminal_output;

use anyhow::Result;
use clap::Parser;
use serde_json::Value;

use crate::acceptance_grpc::acceptance_grpc;
use crate::acceptance_payload::acceptance_payload;
use crate::backup::backup;
use crate::cli::{Cli, Command};
use crate::collect::collect;
use crate::connect::connect;
use crate::k8s::{dockerfile, k8s};
use crate::mcp::mcp;
use crate::meta::{issue, spec_gen, SpecCommand, LLM_TOPICS, TOOL};
use crate::query::query;
use crate::restore::restore;
use crate::serve::serve;
use crate::snapshot::snapshot;
use crate::terminal_output::{print_json_terminal, print_json_text_terminal};

#[tokio::main]
async fn main() -> Result<()> {
    // The operator and online CLI pull both ring and aws-lc-rs through the
    // shared transport stack. Pick the ecosystem-wide provider before any
    // TLS client (notably kube) initializes it.
    peer_tls::install_default_crypto_provider();
    match Cli::parse().command {
        Command::Serve(args) => serve(args).await,
        Command::Collect(args) => collect(args).await,
        Command::Query(args) => query(args).await,
        Command::Mcp(args) => mcp(args).await,
        Command::Snapshot(args) => snapshot(args),
        Command::Restore(args) => restore(args),
        Command::Backup(args) => backup(args).await,
        Command::Dockerfile(args) => dockerfile(args),
        Command::K8s(args) => k8s(args).await,
        Command::Connect(args) => connect(args).await,
        Command::Spec(args) => match args.command {
            Some(SpecCommand::Gen(args)) => spec_gen(args),
            None => {
                let _ = args.format;
                print_json_terminal(serde_json::json!({
                    "openapi": serde_json::from_str::<Value>(&sift::openapi_json()?)?
                }))
            }
        },
        Command::Llm(args) => {
            let output = cli_std::llm::render(
                "sift",
                env!("CARGO_PKG_VERSION"),
                LLM_TOPICS,
                &args.topic,
                cli_std::llm::Format::parse(&args.format),
            )?;
            if args.format == "json" {
                print_json_text_terminal(&output)
            } else {
                println!("{output}");
                println!("next: done");
                Ok(())
            }
        }
        Command::Upgrade(args) => {
            cli_std::upgrade::run(
                &TOOL,
                cli_std::upgrade::Options {
                    check: args.check,
                    tag: args.tag,
                    force: args.force,
                    yes: args.yes,
                },
            )
            .await
        }
        Command::Issue(args) => issue(args).await,
        Command::AcceptancePayload(args) => acceptance_payload(args),
        Command::AcceptanceBuildInfo => print_json_terminal(serde_json::json!({
            "version": TOOL.version,
            "git_sha": TOOL.git_sha,
            "target": TOOL.target,
            "built_at": TOOL.built_at,
        })),
        Command::AcceptanceGrpc(args) => acceptance_grpc(args).await,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn installs_rustls_provider_before_kubernetes_cli_initializes_tls() {
        peer_tls::install_default_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
