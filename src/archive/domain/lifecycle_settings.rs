//! How long an all-voter checkpoint may take, and how many objects a GC batch
//! deletes.

pub(in crate::archive) const ALL_VOTER_CHECKPOINT_ATTEMPT: std::time::Duration =
    std::time::Duration::from_secs(30);

pub(in crate::archive) const ARCHIVE_GC_BATCH_OBJECTS: usize = 128;
