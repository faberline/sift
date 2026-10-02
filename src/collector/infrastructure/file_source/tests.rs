use super::*;
use service_collector::CollectorSource;

fn record(source: &mut FileSource) -> RawRecord {
    match source.next_record(1024).unwrap() {
        ReadOutcome::Record(record) => record,
        _ => panic!("expected file record"),
    }
}
fn acknowledge(source: &mut FileSource, record: &RawRecord) {
    source
        .commit(
            std::slice::from_ref(&record.cursor),
            CommitStats {
                accepted: 1,
                duplicates: 0,
                rejected: 0,
            },
        )
        .unwrap();
}
#[test]
fn restart_drains_renamed_file_and_reads_its_replacement_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.log");
    let checkpoint = dir.path().join("position.json");
    std::fs::write(&path, b"first\nsecond\n").unwrap();
    let mut source = FileSource::open(path.clone(), "fixture".into(), checkpoint.clone()).unwrap();
    let first = record(&mut source);
    acknowledge(&mut source, &first);
    std::fs::rename(&path, dir.path().join("app.log.1")).unwrap();
    std::fs::write(&path, b"replacement\n").unwrap();
    drop(source);
    let mut source = FileSource::open(path.clone(), "fixture".into(), checkpoint.clone()).unwrap();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let item = record(&mut source);
        seen.push((item.bytes.clone(), item.source_id.clone()));
        acknowledge(&mut source, &item);
    }
    assert!(seen.iter().any(|(bytes, _)| bytes == b"second\n"));
    assert!(seen.iter().any(|(bytes, _)| bytes == b"replacement\n"));
    assert_ne!(seen[0].1, seen[1].1);
    drop(source);
    let mut restarted = FileSource::open(path, "fixture".into(), checkpoint).unwrap();
    assert!(matches!(
        restarted.next_record(1024).unwrap(),
        ReadOutcome::Exhausted
    ));
    assert_eq!(restarted.progress().lost_bytes, 0);
}
#[test]
fn copytruncate_records_loss_and_assigns_a_new_source_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.log");
    let checkpoint = dir.path().join("position.json");
    std::fs::write(&path, b"first\nsecond\n").unwrap();
    let mut source = FileSource::open(path.clone(), "fixture".into(), checkpoint.clone()).unwrap();
    let first = record(&mut source);
    acknowledge(&mut source, &first);
    let _unprotected = record(&mut source);
    std::fs::write(&path, b"x\n").unwrap();
    let ReadOutcome::Rejection(loss) = source.next_record(1024).unwrap() else {
        panic!("expected visible loss");
    };
    source
        .commit(
            &[loss.cursor],
            CommitStats {
                accepted: 0,
                duplicates: 0,
                rejected: 1,
            },
        )
        .unwrap();
    let replacement = record(&mut source);
    assert_eq!(replacement.bytes, b"x\n");
    assert_ne!(replacement.source_id, first.source_id);
    acknowledge(&mut source, &replacement);
    drop(source);
    let restarted = FileSource::open(path, "fixture".into(), checkpoint).unwrap();
    assert_eq!(restarted.progress().lost_bytes, 7);
    assert_eq!(restarted.progress().lost_sources, 1);
}
