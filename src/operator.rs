//! The operator module's public paths. The operations context now owns the
//! Sift custom resource and the operator loop; these re-exports keep
//! `sift::operator::*` compiling for callers outside the crate.

pub use crate::operations::interfaces::crd::sift_spec::{
    ArchiveSpec, AuthMode, BackupSpec, BootstrapSpec, IngestSpec, PlacementSpec, Sift, SiftSpec,
    SiftStatus, StorageSpec,
};
pub use crate::operations::interfaces::operator::run;
