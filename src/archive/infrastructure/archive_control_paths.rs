//! Where the archive's control records live under the data root, and their
//! format versions.

pub(super) const ARCHIVE_COMMIT_FORMAT_VERSION: u16 = 3;

pub(super) const ARCHIVE_COMMIT_PATH: &str = "control/archive-commit.json";

pub(in crate::archive) const LOCAL_COMMIT_FORMAT_VERSION: u16 = 2;

pub(in crate::archive) const LOCAL_COMMIT_PATH: &str = "control/local-segment-commit.json";

pub(super) const ARCHIVE_GC_FORMAT_VERSION: u16 = 3;

pub(in crate::archive) const ARCHIVE_GC_PATH: &str = "control/archive-gc-pending.json";

pub(super) const ARCHIVE_GC_STAGED_PATH: &str = "control/archive-gc-staged.json";

pub(in crate::archive) const LOCAL_BLOB_GC_PATH: &str = "control/local-blob-gc.json";

pub(super) const LOCAL_BLOB_GC_COMPLETE_PATH: &str = "control/local-blob-gc-complete.json";

pub(super) const LOCAL_BLOB_GC_FORMAT_VERSION: u16 = 3;

pub(super) const ARCHIVE_UPLOAD_INTENT_PATH: &str = "control/archive-upload-intent.json";

pub(super) const ARCHIVE_UPLOAD_INTENT_FORMAT_VERSION: u16 = 1;

pub(in crate::archive) const RESTORE_STATE_PATH: &str = ".sift-restore.json";

pub(in crate::archive) const RESTORE_STAGE_DIR: &str = ".sift-restore-stage";

pub(in crate::archive) const RESTORE_STATE_FORMAT_VERSION: u16 = 1;
