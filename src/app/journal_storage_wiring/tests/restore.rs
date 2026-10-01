use crate::app::journal_storage_wiring::restore_target_service;
use crate::journal::infrastructure::storage::blob_store::BlobStore;
use crate::journal::infrastructure::storage::shard_router::write_epoch_maps;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::data_layout::DataLayout;
use crate::shared_kernel::shard_route::{EpochMap, VIRTUAL_BUCKETS};

fn chain(error: anyhow::Error) -> Vec<String> {
    error.chain().map(ToString::to_string).collect()
}

#[test]
fn restore_target_preserves_epoch_bytes_and_the_separate_blob_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    let services = restore_target_service();
    assert!(!root.exists(), "assembly must not open restore resources");
    let layout = DataLayout::open(&root, StorageRole::All).unwrap();
    drop(layout);
    let epochs = [EpochMap {
        epoch: 1,
        activated_at_cursor: 0,
        bucket_to_shard: vec![0; VIRTUAL_BUCKETS],
    }];
    write_epoch_maps(&root, &epochs).unwrap();
    let old = std::fs::read(root.join("segments/epochs.json")).unwrap();
    services.write_epochs(&root, &epochs).unwrap();
    assert_eq!(
        std::fs::read(root.join("segments/epochs.json")).unwrap(),
        old
    );
    assert!(!root.join("archive-cache/blobs/sha256").exists());
    let journal = services.open_journal(&root).unwrap();
    let writer = services.open_blob_writer(&root, 65_536).unwrap();
    let bytes = b"fixed restored blob";
    let restored = writer.put(bytes, "identity".into()).unwrap();
    let native = BlobStore::open(&root, 65_536).unwrap();
    assert_eq!(restored, native.put(bytes, "identity").unwrap());
    assert_eq!(native.read(&restored.hash).unwrap(), bytes);
    drop(journal);
    // The extra writer owns no journal lease. Reopening the layout still works.
    DataLayout::open(&root, StorageRole::All).unwrap();
}

#[test]
fn restore_target_preserves_epoch_and_blob_open_error_chains() {
    let temp = tempfile::tempdir().unwrap();
    let services = restore_target_service();
    assert_eq!(
        chain(services.write_epochs(temp.path(), &[]).unwrap_err()),
        chain(write_epoch_maps(temp.path(), &[]).unwrap_err())
    );
    assert!(!temp.path().join("segments/epochs.json").exists());
    std::fs::write(temp.path().join("archive-cache"), b"blocking file").unwrap();
    assert_eq!(
        chain(
            services
                .open_blob_writer(temp.path(), 65_536)
                .err()
                .unwrap()
        ),
        chain(BlobStore::open(temp.path(), 65_536).unwrap_err())
    );
    assert!(!temp.path().join("layout.json").exists());
}
