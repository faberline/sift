//! Deterministic offline rendering of Sift's deployment artifacts from the
//! checked-in Dockerfiles and Kubernetes manifests: the Dockerfile, CRD,
//! operator, collector and instance YAML.

use anyhow::{bail, Result};

use crate::operations::domain::image_reference::{validate_image, validate_manifest_value};
use crate::operations::domain::namespace_name::validate_namespace;

/// The published Sift image at this crate version: the tag that
/// `build.sh release` pins into every `k8s/**` manifest, so the operator and
/// collector renderers can substitute it byte-for-byte.
pub const DEFAULT_OPERATOR_IMAGE: &str =
    concat!("ghcr.io/faberline/sift:", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DockerfileVariant {
    Source,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceProfile {
    Dev,
    Staging,
    Prod,
    Template,
}

pub fn dockerfile(variant: DockerfileVariant, version: Option<&str>) -> Result<String> {
    match variant {
        DockerfileVariant::Source => {
            Ok(strip_ownership_markers(include_str!("../../../Dockerfile")))
        }
        DockerfileVariant::Release => {
            let version = version
                .map(|version| version.trim_start_matches("sift@"))
                .unwrap_or(env!("CARGO_PKG_VERSION"));
            if version.is_empty() {
                bail!("release Dockerfile version must not be empty");
            }
            Ok(
                strip_ownership_markers(include_str!("../../../Dockerfile.release")).replace(
                    "SIFT_VERSION=REPLACE_ME",
                    &format!("SIFT_VERSION={version}"),
                ),
            )
        }
    }
}

pub fn crd_yaml() -> String {
    strip_ownership_markers(include_str!("../../../k8s/crd/sift.yaml"))
}

pub fn operator_yaml(namespace: &str) -> Result<String> {
    operator_yaml_with_image(namespace, DEFAULT_OPERATOR_IMAGE)
}

pub fn operator_yaml_with_image(namespace: &str, image: &str) -> Result<String> {
    validate_namespace(namespace)?;
    validate_image(image)?;
    Ok(
        strip_ownership_markers(include_str!("../../../k8s/operator/operator.yaml"))
            .replace("sift-system", namespace)
            .replace(DEFAULT_OPERATOR_IMAGE, image),
    )
}

pub fn collector_yaml(namespace: &str, image: &str) -> Result<String> {
    validate_manifest_value("collector namespace", namespace)?;
    validate_manifest_value("collector image", image)?;
    Ok(
        strip_ownership_markers(include_str!("../../../k8s/collector/daemonset.yaml"))
            .replace("REPLACE_NAMESPACE", namespace)
            .replace(DEFAULT_OPERATOR_IMAGE, image),
    )
}

pub fn instance_yaml(profile: InstanceProfile) -> String {
    match profile {
        InstanceProfile::Dev => {
            strip_ownership_markers(include_str!("../../../k8s/instances/dev.yaml"))
        }
        InstanceProfile::Staging => {
            strip_ownership_markers(include_str!("../../../k8s/overlays/staging/sift.yaml"))
        }
        InstanceProfile::Prod => {
            strip_ownership_markers(include_str!("../../../k8s/overlays/prod/sift.yaml"))
        }
        InstanceProfile::Template => {
            strip_ownership_markers(include_str!("../../../k8s/overlays/template/sift.yaml"))
        }
    }
}

fn strip_ownership_markers(body: &str) -> String {
    body.lines()
        .filter(|line| !line.contains("HANDWRITE-BEGIN") && !line.contains("HANDWRITE-END"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
