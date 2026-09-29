//! `sift dockerfile` and `sift k8s`: render the source or release image
//! Dockerfiles, the cluster CRD, the operator control plane and namespaced Sift
//! instances, run the operator, and write each artifact to stdout or a file.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use sift::deploy::{DockerfileVariant, InstanceProfile};

#[derive(Args)]
pub(super) struct DockerfileArgs {
    #[command(subcommand)]
    command: DockerfileCommand,
}

#[derive(Subcommand)]
enum DockerfileCommand {
    /// Render a source-build or release-binary Dockerfile.
    Render(DockerfileRenderArgs),
}

#[derive(Args)]
struct DockerfileRenderArgs {
    #[arg(long, value_enum, default_value_t = DockerfileVariantArg::Release)]
    variant: DockerfileVariantArg,
    #[arg(long)]
    version: Option<String>,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum DockerfileVariantArg {
    Source,
    Release,
}

#[derive(Args)]
pub(super) struct K8sArgs {
    #[command(subcommand)]
    command: K8sCommand,
}

#[derive(Subcommand)]
enum K8sCommand {
    /// Render the cluster-scoped Sift custom resource definition.
    Crd(K8sCrdArgs),
    /// Render or run the Sift controller control plane.
    Operator(K8sOperatorArgs),
    /// Render one namespaced Sift custom resource.
    Instance(K8sInstanceArgs),
    /// Render the Sift-owned node CRI collector DaemonSet.
    Collector(K8sCollectorArgs),
}

#[derive(Args)]
struct K8sCollectorArgs {
    #[command(subcommand)]
    command: K8sCollectorCommand,
}

#[derive(Subcommand)]
enum K8sCollectorCommand {
    Render(K8sCollectorRenderArgs),
}

#[derive(Args)]
struct K8sCollectorRenderArgs {
    #[arg(long, default_value = "sift-system")]
    namespace: String,
    #[arg(long, default_value = sift::deploy::DEFAULT_OPERATOR_IMAGE)]
    image: String,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct K8sCrdArgs {
    #[command(subcommand)]
    command: K8sCrdCommand,
}

#[derive(Subcommand)]
enum K8sCrdCommand {
    Render(K8sFileOutputArgs),
}

#[derive(Args)]
struct K8sOperatorArgs {
    #[command(subcommand)]
    command: K8sOperatorCommand,
}

#[derive(Subcommand)]
enum K8sOperatorCommand {
    /// Render service account, RBAC, and controller deployment assets.
    Render(K8sOperatorRenderArgs),
    /// Controller image entrypoint. The deployed image runs this command.
    Run,
}

#[derive(Args)]
struct K8sOperatorRenderArgs {
    #[arg(long, default_value = "sift-system")]
    namespace: String,
    #[arg(long, default_value = sift::deploy::DEFAULT_OPERATOR_IMAGE)]
    image: String,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args)]
struct K8sInstanceArgs {
    #[command(subcommand)]
    command: K8sInstanceCommand,
}

#[derive(Subcommand)]
enum K8sInstanceCommand {
    Render(K8sInstanceRenderArgs),
}

#[derive(Args)]
struct K8sInstanceRenderArgs {
    #[arg(long, value_enum, default_value_t = K8sInstanceProfileArg::Dev)]
    profile: K8sInstanceProfileArg,
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum K8sInstanceProfileArg {
    Dev,
    Staging,
    Prod,
    Template,
}

#[derive(Args)]
struct K8sFileOutputArgs {
    #[arg(long)]
    out: Option<PathBuf>,
}

pub(super) fn dockerfile(args: DockerfileArgs) -> Result<()> {
    match args.command {
        DockerfileCommand::Render(args) => {
            let variant = match args.variant {
                DockerfileVariantArg::Source => DockerfileVariant::Source,
                DockerfileVariantArg::Release => DockerfileVariant::Release,
            };
            let body = sift::deploy::dockerfile(variant, args.version.as_deref())?;
            let file_name = match variant {
                DockerfileVariant::Source => "Dockerfile",
                DockerfileVariant::Release => "Dockerfile.release",
            };
            write_artifact(
                args.out.as_deref(),
                file_name,
                &body,
                "docker build -f Dockerfile -t sift:dev .",
            )
        }
    }
}

pub(super) async fn k8s(args: K8sArgs) -> Result<()> {
    match args.command {
        K8sCommand::Crd(args) => match args.command {
            K8sCrdCommand::Render(args) => write_artifact(
                args.out.as_deref(),
                "sift-crd.yaml",
                &sift::deploy::crd_yaml(),
                "kubectl apply -f -",
            ),
        },
        K8sCommand::Operator(args) => match args.command {
            K8sOperatorCommand::Render(args) => write_artifact(
                args.out.as_deref(),
                "sift-operator.yaml",
                &sift::deploy::operator_yaml_with_image(&args.namespace, &args.image)?,
                "kubectl apply -f -",
            ),
            K8sOperatorCommand::Run => operator_run().await,
        },
        K8sCommand::Instance(args) => match args.command {
            K8sInstanceCommand::Render(args) => {
                let profile = match args.profile {
                    K8sInstanceProfileArg::Dev => InstanceProfile::Dev,
                    K8sInstanceProfileArg::Staging => InstanceProfile::Staging,
                    K8sInstanceProfileArg::Prod => InstanceProfile::Prod,
                    K8sInstanceProfileArg::Template => InstanceProfile::Template,
                };
                write_artifact(
                    args.out.as_deref(),
                    "sift.yaml",
                    &sift::deploy::instance_yaml(profile),
                    "kubectl apply -f -",
                )
            }
        },
        K8sCommand::Collector(args) => match args.command {
            K8sCollectorCommand::Render(args) => write_artifact(
                args.out.as_deref(),
                "sift-collector.yaml",
                &sift::deploy::collector_yaml(&args.namespace, &args.image)?,
                "kubectl apply -f -",
            ),
        },
    }
}

async fn operator_run() -> Result<()> {
    sift::operator::run().await
}

fn write_artifact(
    out: Option<&std::path::Path>,
    file_name: &str,
    body: &str,
    next: &str,
) -> Result<()> {
    match out {
        None => {
            print!("{body}");
            println!("next: {next}");
        }
        Some(out) => {
            let path = if out.is_dir() {
                std::fs::create_dir_all(out)
                    .with_context(|| format!("create artifact directory {}", out.display()))?;
                out.join(file_name)
            } else {
                if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("create artifact output directory {}", parent.display())
                    })?;
                }
                out.to_path_buf()
            };
            std::fs::write(&path, body)
                .with_context(|| format!("write deployment artifact {}", path.display()))?;
            println!("wrote {}", path.display());
            println!("next: kubectl apply -f {}", path.display());
        }
    }
    Ok(())
}
