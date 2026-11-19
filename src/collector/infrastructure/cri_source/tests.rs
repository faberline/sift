//! Unit tests for the file handles that the CRI source keeps for each file it
//! has discovered, so a log that the kubelet deletes is still read to its end.
use super::*;

use std::fs;
use std::path::Path;

use service_collector::CollectorSource as _;

use crate::collector::domain::config::CriMetadata;
use crate::collector::domain::pod_labels::PodLabelSet;

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
        pod_labels: None,
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

const POD_A_LINE: &str = "2026-07-17T10:00:00.123456789Z stdout F hello-a\n";
const POD_B_LINE: &str = "2026-07-17T10:00:00.123456789Z stdout F hello-b\n";

fn write_pod_log(root: &Path, pod_directory: &str, content: &str) {
    let directory = root.join(pod_directory).join("app");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("0.log"), content).unwrap();
}

fn source_with_labels(
    root: &Path,
    checkpoint: &Path,
    pod_labels: Option<PodLabelSet>,
) -> CriSource {
    let config = CriSourceConfig {
        root: root.to_path_buf(),
        metadata: CriMetadata {
            gcp_project: "p".into(),
            cluster: None,
            location: None,
            node: None,
        },
        namespaces: vec!["prod".into()],
        pod_labels,
    };
    CriSource::open(config, checkpoint.to_path_buf()).unwrap()
}

/// Reads and commits every record that is ready now and returns their bytes.
fn drain(source: &mut CriSource) -> Vec<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        match source.next_record(1024).unwrap() {
            ReadOutcome::Record(record) => {
                bytes.push(record.bytes.clone());
                source
                    .commit(std::slice::from_ref(&record.cursor), CommitStats::default())
                    .unwrap();
            }
            ReadOutcome::Rejection(rejection) => panic!("unexpected rejection {rejection:?}"),
            ReadOutcome::Pending | ReadOutcome::Exhausted => return bytes,
        }
    }
}

fn two_pod_root(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let root = tmp.path().join("pods");
    fs::create_dir_all(&root).unwrap();
    write_pod_log(&root, "prod_pod-a_uid-a", POD_A_LINE);
    write_pod_log(&root, "prod_pod-b_uid-b", POD_B_LINE);
    root
}

fn set_of(uids: &[&str]) -> PodLabelSet {
    let set = PodLabelSet::default();
    set.replace(uids.iter().map(|uid| uid.to_string()));
    set
}

#[test]
fn only_the_labelled_pod_is_collected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = two_pod_root(&tmp);
    let mut source = source_with_labels(
        &root,
        &tmp.path().join("checkpoint.json"),
        Some(set_of(&["uid-a"])),
    );

    let bytes = drain(&mut source);
    assert_eq!(bytes, vec![b"hello-a".to_vec()]);
}

#[test]
fn no_selector_collects_both_pods() {
    let tmp = tempfile::tempdir().unwrap();
    let root = two_pod_root(&tmp);
    let mut source = source_with_labels(&root, &tmp.path().join("checkpoint.json"), None);

    let mut bytes = drain(&mut source);
    bytes.sort();
    assert_eq!(bytes, vec![b"hello-a".to_vec(), b"hello-b".to_vec()]);
}

#[test]
fn empty_set_collects_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = two_pod_root(&tmp);
    let mut source = source_with_labels(
        &root,
        &tmp.path().join("checkpoint.json"),
        Some(PodLabelSet::default()),
    );

    assert!(drain(&mut source).is_empty());
}

#[test]
fn label_added_later_starts_collection() {
    let tmp = tempfile::tempdir().unwrap();
    let root = two_pod_root(&tmp);
    let set = PodLabelSet::default();
    let mut source = source_with_labels(
        &root,
        &tmp.path().join("checkpoint.json"),
        Some(set.clone()),
    );
    assert!(drain(&mut source).is_empty());

    set.replace(["uid-b".to_string()]);
    source.refresh().unwrap();

    assert_eq!(drain(&mut source), vec![b"hello-b".to_vec()]);
}

#[test]
fn label_removed_later_stops_collection_without_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let root = two_pod_root(&tmp);
    let set = set_of(&["uid-a"]);
    let mut source = source_with_labels(
        &root,
        &tmp.path().join("checkpoint.json"),
        Some(set.clone()),
    );
    assert_eq!(drain(&mut source), vec![b"hello-a".to_vec()]);

    set.replace(Vec::<String>::new());
    let log = root.join("prod_pod-a_uid-a").join("app").join("0.log");
    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    std::io::Write::write_all(&mut file, POD_A_LINE.as_bytes()).unwrap();
    drop(file);
    source.refresh().unwrap();

    assert!(drain(&mut source).is_empty());
    assert_eq!(source.progress().lost_bytes(), 0);
    assert_eq!(source.progress().lost_sources(), 0);
}
