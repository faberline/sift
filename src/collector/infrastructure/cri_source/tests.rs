//! Unit tests for the file handles that the CRI source keeps for each file it
//! has discovered, so a log that the kubelet deletes is still read to its end.
use super::*;

use std::fs;
use std::path::Path;

use service_collector::CollectorSource as _;

use crate::collector::domain::config::CriMetadata;

const POD_DIRECTORY: &str = "prod_pod-0_uid-1";
const FULL_HELLO: &str = "2026-07-17T10:00:00.123456789Z stdout F hello\n";
const FULL_WORLD: &str = "2026-07-17T10:00:01.123456789Z stdout F world\n";
const PARTIAL_HALF: &str = "2026-07-17T10:00:00.123456789Z stdout P half\n";

fn write_log(root: &Path, content: &str) {
    let directory = root.join(POD_DIRECTORY).join("app");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("0.log"), content).unwrap();
}

fn source_for(root: &Path, checkpoint: &Path) -> CriSource {
    let config = CriSourceConfig {
        root: root.to_path_buf(),
        metadata: CriMetadata {
            gcp_project: "p".into(),
            cluster: None,
            location: None,
            node: None,
        },
        namespaces: Vec::new(),
    };
    CriSource::open(config, checkpoint.to_path_buf()).unwrap()
}

#[test]
fn deleted_finished_log_is_read_to_the_end_through_the_held_handle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pods");
    fs::create_dir_all(&root).unwrap();
    write_log(&root, &format!("{FULL_HELLO}{FULL_WORLD}"));
    let mut source = source_for(&root, &tmp.path().join("checkpoint.json"));

    fs::remove_dir_all(root.join(POD_DIRECTORY)).unwrap();
    source.refresh().unwrap();

    let ReadOutcome::Record(first) = source.next_record(1024).unwrap() else {
        panic!("the first frame of the deleted file must still be read");
    };
    assert_eq!(first.bytes, b"hello");
    source
        .commit(std::slice::from_ref(&first.cursor), CommitStats::default())
        .unwrap();

    let ReadOutcome::Record(second) = source.next_record(1024).unwrap() else {
        panic!("the second frame of the deleted file must still be read");
    };
    assert_eq!(second.bytes, b"world");
    source
        .commit(std::slice::from_ref(&second.cursor), CommitStats::default())
        .unwrap();

    source.refresh().unwrap();
    assert_eq!(source.progress().lost_bytes(), 0);
    assert_eq!(source.progress().lost_sources(), 0);
    assert!(!matches!(
        source.next_record(1024).unwrap(),
        ReadOutcome::Rejection(_)
    ));
}

#[test]
fn deleted_log_with_an_unfinished_frame_is_released_and_counted_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pods");
    fs::create_dir_all(&root).unwrap();
    write_log(&root, PARTIAL_HALF);
    let mut source = source_for(&root, &tmp.path().join("checkpoint.json"));

    fs::remove_dir_all(root.join(POD_DIRECTORY)).unwrap();
    for _ in 0..3 {
        source.refresh().unwrap();
    }

    let ReadOutcome::Rejection(rejection) = source.next_record(1024).unwrap() else {
        panic!("an unfinished frame of a deleted file must end as a loss");
    };
    assert_eq!(rejection.entry.code, "source_lost");
}

#[test]
fn present_log_is_still_read_without_regression() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("pods");
    fs::create_dir_all(&root).unwrap();
    write_log(&root, FULL_HELLO);
    let mut source = source_for(&root, &tmp.path().join("checkpoint.json"));

    let ReadOutcome::Record(record) = source.next_record(1024).unwrap() else {
        panic!("a present file must be read as before");
    };
    assert_eq!(record.bytes, b"hello");
}
