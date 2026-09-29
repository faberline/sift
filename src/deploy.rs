//! The deploy module's public paths. The operations context now owns the
//! offline rendering of Sift's deployment artifacts; these re-exports keep
//! `sift::deploy::*` compiling for callers outside the crate.

pub use crate::operations::infrastructure::manifest_bundle::{
    collector_yaml, crd_yaml, dockerfile, instance_yaml, operator_yaml, operator_yaml_with_image,
    DockerfileVariant, InstanceProfile, DEFAULT_OPERATOR_IMAGE,
};
