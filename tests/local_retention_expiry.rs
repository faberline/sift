//! Local expiry: removes old local data in an install with no archive
//! destination, using an injected clock. No GCS emulator and no env var.

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use sift::{
    archive, DurableJournal, EventEnvelope, EventQuery, MetricPoint, MetricTemporality, SignalKind,
};

/// A log carries `pad` bytes so that removing it frees disk space. A metric
/// needs a metric point and a span needs trace and span ids and a span payload.
fn event(id: &str, signal: SignalKind, occurred_at: DateTime<Utc>, pad: usize) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "local-expiry-project",
        "prod",
        id,
        signal,
        serde_json::json!({"message": id, "pad": "x".repeat(pad)}),
    );
    event.occurred_at = occurred_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
    event.observed_at.clone_from(&event.occurred_at);
    event.resource = BTreeMap::from([("service.name".into(), "local-expiry-test".into())]);
    match signal {
        SignalKind::Log => {}
        SignalKind::Metric => {
            event.payload = serde_json::json!({});
            event.metric = Some(MetricPoint {
                name: "requests.total".into(),
                value: 42.0,
                stale: false,
                unit: Some("1".into()),
                temporality: MetricTemporality::Gauge,
                exemplars: Vec::new(),
            });
        }
        SignalKind::Span => {
            let start = u64::try_from(occurred_at.timestamp_nanos_opt().unwrap()).unwrap();
            event.trace_id = Some(format!("trace-{id}"));
            event.span_id = Some(format!("span-{id}"));
            event.payload = serde_json::json!({
                "name": "GET /checkout",
                "kind": "server",
                "start_time_unix_nano": start,
                "end_time_unix_nano": start + 1_000_000,
                "status": {"code": "ok", "message": "complete"},
                "links": [],
                "events": []
            });
        }
    }
    event
}

fn directory_bytes(root: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    total
}

fn ids(journal: &DurableJournal) -> Vec<String> {
    let mut ids: Vec<String> = journal
        .query(EventQuery::default())
        .unwrap()
        .into_iter()
        .map(|stored| stored.event.event_id)
        .collect();
    ids.sort();
    ids
}

/// Appends three old events (31 days before `now`, padded) and three fresh
/// events (29 days before `now`), then commits them locally.
fn committed_journal(data: &std::path::Path, now: DateTime<Utc>) -> DurableJournal {
    let journal = DurableJournal::open(data).unwrap();
    let old_at = now - chrono::Duration::days(31);
    let fresh_at = now - chrono::Duration::days(29);
    for (id, signal) in [
        ("old-log", SignalKind::Log),
        ("old-metric", SignalKind::Metric),
        ("old-span", SignalKind::Span),
    ] {
        journal.append(event(id, signal, old_at, 20_000)).unwrap();
    }
    for (id, signal) in [
        ("fresh-log", SignalKind::Log),
        ("fresh-metric", SignalKind::Metric),
        ("fresh-span", SignalKind::Span),
    ] {
        journal.append(event(id, signal, fresh_at, 0)).unwrap();
    }
    archive::archive_journal_local(&journal).unwrap();
    journal
}

const FRESH_IDS: [&str; 3] = ["fresh-log", "fresh-metric", "fresh-span"];

#[test]
fn local_expiry_removes_old_data_and_keeps_fresh_data() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);
    let before = directory_bytes(data.path());

    let receipt = archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();

    assert_eq!(receipt.removed_events, 3);
    assert_eq!(receipt.retained_events, 3);
    assert_eq!(
        receipt.oldest_retained_unix_nano,
        (now - chrono::Duration::days(29)).timestamp_nanos_opt()
    );
    let mut fresh: Vec<String> = FRESH_IDS.iter().map(|id| id.to_string()).collect();
    fresh.sort();
    assert_eq!(ids(&journal), fresh);
    assert!(directory_bytes(data.path()) < before);
}

#[test]
fn local_expiry_twice_removes_nothing_more() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);
    archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();

    let second = archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();

    assert_eq!(second.removed_events, 0);
    assert_eq!(second.retained_events, 3);
}

#[test]
fn local_archive_still_commits_after_expiry() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);
    archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();
    journal
        .append(event("later-log", SignalKind::Log, now, 0))
        .unwrap();

    archive::archive_journal_local(&journal).unwrap();

    let mut expected: Vec<String> = FRESH_IDS.iter().map(|id| id.to_string()).collect();
    expected.push("later-log".into());
    expected.sort();
    assert_eq!(ids(&journal), expected);
}

#[test]
fn local_expiry_survives_a_reopen() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);
    archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();
    drop(journal);

    let reopened = DurableJournal::open(data.path()).unwrap();
    let mut fresh: Vec<String> = FRESH_IDS.iter().map(|id| id.to_string()).collect();
    fresh.sort();
    assert_eq!(ids(&reopened), fresh);
    archive::archive_journal_local(&reopened).unwrap();
}

#[test]
fn local_expiry_before_the_data_is_old_removes_nothing() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);

    let receipt =
        archive::expire_local_events_with_days_at(&journal, now - chrono::Duration::days(5), 30)
            .unwrap();

    assert_eq!(receipt.removed_events, 0);
    assert_eq!(receipt.retained_events, 6);
    assert_eq!(ids(&journal).len(), 6);
}

#[test]
fn local_expiry_without_a_local_commit_returns_zero_counts() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = DurableJournal::open(data.path()).unwrap();
    journal
        .append(event(
            "old-log",
            SignalKind::Log,
            now - chrono::Duration::days(31),
            0,
        ))
        .unwrap();

    let receipt = archive::expire_local_events_with_days_at(&journal, now, 30).unwrap();

    assert_eq!(receipt.removed_events, 0);
    assert_eq!(receipt.retained_events, 0);
    assert_eq!(receipt.oldest_retained_unix_nano, None);
    assert_eq!(ids(&journal), vec!["old-log".to_string()]);
}

#[test]
fn local_expiry_rejects_days_outside_one_to_one_hundred_eighty() {
    let now = Utc::now();
    let data = tempfile::tempdir().unwrap();
    let journal = committed_journal(data.path(), now);

    assert!(archive::expire_local_events_with_days_at(&journal, now, 0).is_err());
    assert!(archive::expire_local_events_with_days_at(&journal, now, 181).is_err());
    assert!(archive::expire_local_events_with_days_at(&journal, now, 30).is_ok());
}
