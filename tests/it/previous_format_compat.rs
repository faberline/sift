//! The current binary opens the data root that `DurableJournal` wrote at the
//! `main` commit recorded in `tests/fixtures/wal-v1/README.md`.

use std::{collections::BTreeMap, fs, path::Path};

use sift::{
    DurableJournal, EventEnvelope, EventQuery, MetricPoint, MetricTemporality, SignalKind,
    StoredEvent,
};

const FIXED_TIME: &str = "2026-01-01T00:00:00Z";

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wal-v1/root")
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

/// Compares the events without `acknowledged_at`: the journal sets it to the
/// clock on append, so the fixture's value is not fixed.
fn assert_same_events(got: &[StoredEvent], want: &[StoredEvent]) {
    let normalize = |events: &[StoredEvent]| -> Vec<StoredEvent> {
        events
            .iter()
            .map(|stored| StoredEvent {
                cursor: stored.cursor,
                acknowledged_at: FIXED_TIME.into(),
                event: stored.event.clone(),
            })
            .collect()
    };
    if serde_json::to_value(normalize(got)).unwrap()
        != serde_json::to_value(normalize(want)).unwrap()
    {
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

/// Opens a temporary copy of the fixture data root, so the fixture is never written to.
fn open_copy() -> (tempfile::TempDir, DurableJournal) {
    let temp = tempfile::tempdir().unwrap();
    copy_dir(&fixture_root(), temp.path());
    let journal = DurableJournal::open(temp.path()).unwrap();
    (temp, journal)
}

#[test]
fn capture_wal_v1_fixture() {
    if std::env::var_os("SIFT_CAPTURE_WAL_V1").is_none() {
        return;
    }
    let root = fixture_root();
    match fs::remove_dir_all(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("{error}"),
    }
    fs::create_dir_all(&root).unwrap();
    let journal = DurableJournal::open(&root).unwrap();
    for stored in expected_events() {
        journal.append(stored.event.clone()).unwrap();
    }
    drop(journal);
}

#[test]
fn current_binary_opens_the_previous_format_wal() {
    let (_temp, journal) = open_copy();
    let got = journal
        .query(EventQuery {
            signal: None,
            after: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(got.len(), 3);
    let cursors: Vec<u64> = got.iter().map(|stored| stored.cursor).collect();
    assert_eq!(cursors, vec![1, 2, 3]);
    assert_same_events(&got, &expected_events());
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
