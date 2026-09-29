//! The command line: the top-level parser and its subcommands. Each subcommand's
//! arguments live with the code that runs it.

use clap::{Parser, Subcommand};

use crate::acceptance_grpc::AcceptanceGrpcArgs;
use crate::acceptance_payload::AcceptancePayloadArgs;
use crate::backup::BackupArgs;
use crate::collect::CollectArgs;
use crate::connect::ConnectArgs;
use crate::k8s::{DockerfileArgs, K8sArgs};
use crate::mcp::McpArgs;
use crate::meta::{IssueArgs, LlmArgs, SpecArgs, UpgradeArgs};
use crate::query::QueryArgs;
use crate::restore::RestoreArgs;
use crate::serve_args::ServeArgs;
use crate::snapshot::SnapshotArgs;

#[derive(Parser)]
#[command(
    name = "sift",
    version,
    about = "Sift — one SRE product for logs, metrics, and traces"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Subcommand)]
pub(super) enum Command {
    /// Run the unified h2c/HTTP1 operational-event service.
    Serve(ServeArgs),
    /// Collect axiom.service.log.v1 JSONL from a file, stdin, or Kubernetes CRI logs.
    Collect(CollectArgs),
    /// Run one versioned logs, metrics, or traces query through the Sift API.
    Query(QueryArgs),
    /// Serve the read-only Sift MCP tools.
    Mcp(McpArgs),
    /// Write a consistent Sift journal snapshot to stdout or a local file.
    Snapshot(SnapshotArgs),
    /// Restore a journal snapshot from a shared backup object URI.
    Restore(RestoreArgs),
    /// Ship a live or explicitly offline journal snapshot through the shared backup contract.
    Backup(BackupArgs),
    /// Render source or release image Dockerfiles independently of Kubernetes.
    Dockerfile(DockerfileArgs),
    /// Render cluster CRD, operator control plane, or namespaced Sift instances.
    K8s(K8sArgs),
    /// Run a command through a managed Kubernetes port-forward to Sift.
    Connect(ConnectArgs),
    /// Print Sift's API contract or generate a typed client from it.
    Spec(SpecArgs),
    /// Print offline agent-facing operational documentation.
    Llm(LlmArgs),
    /// Check or install a released Sift binary.
    Upgrade(UpgradeArgs),
    /// Search, inspect, or file Sift issues.
    Issue(IssueArgs),
    /// Emit deterministic protocol bytes for the isolated acceptance runner.
    #[command(hide = true)]
    AcceptancePayload(AcceptancePayloadArgs),
    /// Emit immutable build provenance for candidate verification.
    #[command(hide = true)]
    AcceptanceBuildInfo,
    /// Send one valid and one invalid log through OTLP/gRPC for acceptance.
    #[command(hide = true)]
    AcceptanceGrpc(AcceptanceGrpcArgs),
}
