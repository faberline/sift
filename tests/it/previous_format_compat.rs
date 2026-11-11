//! The current binary opens the WAL format that `main` at cb80ef0 wrote.

use std::{collections::BTreeMap, fs, path::Path};

use sift::{
    journal::SignalWal, DurableJournal, EventEnvelope, EventQuery, MetricPoint, MetricTemporality,
    SignalKind, StoredEvent,
};

const FIXED_TIME: &str = "2026-01-01T00:00:00Z";

fn fixture_wal() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wal-v1/wal")
}

fn event(id: &str, signal: SignalKind) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        id,
        signal,
        match signal {
            SignalKind::Span => serde_json::json!({
                "name": "request",
                "start_time_unix_nano": 10,
                "end_time_unix_nano": 20
            }),
            _ => serde_json::json!({"message": id}),
        },
    );
    event.occurred_at = FIXED_TIME.into();
    event.observed_at = FIXED_TIME.into();
    event.resource = BTreeMap::from([("service.name".into(), "checkout".into())]);
    if signal == SignalKind::Metric {
        event.metric = Some(MetricPoint {
            name: "requests".into(),
            value: 1.0,
            stale: false,
            unit: None,
            temporality: MetricTemporality::Delta,
            exemplars: Vec::new(),
        });
    }
    if signal == SignalKind::Span {
        event.trace_id = Some("trace-1".into());
        event.span_id = Some("span-1".into());
    }
    event
}

fn expected_events() -> Vec<StoredEvent> {
    [
        (1, "wal-log-1", SignalKind::Log),
        (2, "wal-metric-1", SignalKind::Metric),
        (3, "wal-span-1", SignalKind::Span),
    ]
    .into_iter()
    .map(|(cursor, id, signal)| StoredEvent {
        cursor,
        acknowledged_at: FIXED_TIME.into(),
        event: event(id, signal),
    })
    .collect()
}

fn assert_same_events(got: &[StoredEvent], want: &[StoredEvent]) {
    if serde_json::to_value(got).unwrap() != serde_json::to_value(want).unwrap() {
        panic!("events differ");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Opens a temporary copy of the fixture, so the fixture is never written to.
fn open_copy() -> (tempfile::TempDir, DurableJournal) {
    let temp = tempfile::tempdir().unwrap();
    copy_dir(&fixture_wal(), &temp.path().join("wal"));
    let journal = DurableJournal::open(temp.path()).unwrap();
    (temp, journal)
}

#[test]
fn capture_wal_v1_fixture() {
    if std::env::var_os("SIFT_CAPTURE_WAL_V1").is_none() {
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wal-v1");
    let wal = SignalWal::open(&root).unwrap();
    for stored in expected_events() {
        wal.append(&stored).unwrap();
    }
}

#[test]
fn current_binary_opens_the_previous_format_wal() {
    let (temp, journal) = open_copy();
    let got = journal
        .query(EventQuery {
            signal: None,
            after: 0,
            limit: 10,
        })
        .unwrap();
    assert_same_events(&got, &expected_events());

    let cursors: Vec<u64> = SignalWal::open(temp.path())
        .unwrap()
        .recovered_events()
        .unwrap()
        .iter()
        .map(|stored| stored.cursor)
        .collect();
    assert_eq!(cursors, vec![1, 2, 3]);
}

#[test]
#[should_panic(expected = "events differ")]
fn dropping_one_event_from_the_expected_list_is_detected() {
    let (_temp, journal) = open_copy();
    let got = journal
        .query(EventQuery {
            signal: None,
            after: 0,
            limit: 10,
        })
        .unwrap();
    let mut want = expected_events();
    want.remove(1);
    assert_same_events(&got, &want);
}
