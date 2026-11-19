//! Own test binary: this case points `STORAGE_EMULATOR_HOST` at a per-test
//! Cloud Storage emulator with `std::env::set_var`, and `storage-object` reads
//! that variable whenever it builds a GCS client, so inside the shared `it`
//! binary concurrent cases would reach each other's emulator.

use std::{collections::BTreeMap, time::Duration};

use chrono::{SecondsFormat, Utc};
use sift::{archive, DurableJournal, EventEnvelope, SignalKind};

fn event(id: &str, signal: SignalKind, occurred_at: chrono::DateTime<Utc>) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "retention-project",
        "prod",
        id,
        signal,
        serde_json::json!({"message": id}),
    );
    event.occurred_at = occurred_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
    event.observed_at.clone_from(&event.occurred_at);
    event.resource = BTreeMap::from([("service.name".into(), "retention-test".into())]);
    event
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_retention_days_drive_due_and_expiry() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let address_text = address.to_string();
    let emulator = tokio::spawn(async move {
        vat::emulator::serve(vat::emulator::Kind::CloudStorage, &address_text)
            .await
            .unwrap();
    });
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(address).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    std::env::set_var("STORAGE_EMULATOR_HOST", format!("http://{address}"));

    tokio::task::spawn_blocking(|| {
        let now = Utc::now();

        // Part 1: 30 days. Data 31 days old is due and expires; 29 days old stays.
        let data = tempfile::tempdir().unwrap();
        let journal = DurableJournal::open(data.path()).unwrap();
        assert_eq!(
            journal
                .append(event(
                    "old-log",
                    SignalKind::Log,
                    now - chrono::Duration::days(31)
                ))
                .unwrap()
                .cursor,
            1
        );
        assert_eq!(
            journal
                .append(event(
                    "fresh-log",
                    SignalKind::Log,
                    now - chrono::Duration::days(29)
                ))
                .unwrap()
                .cursor,
            2
        );
        archive::archive_journal_gcs(&journal, "gs://sift-retention/days30").unwrap();
        assert!(!archive::retention_due_at(data.path(), now).unwrap());
        assert!(archive::retention_due_with_days_at(data.path(), now, 30).unwrap());
        let expiry = archive::expire_committed_events_with_days_at(&journal, now, 30).unwrap();
        assert_eq!(expiry.expired_events, 1);
        assert_eq!(expiry.retained_events, 1);
        assert!(archive::expire_committed_events_with_days_at(&journal, now, 0).is_err());

        // Part 2: default 180 days. Data 31 days old stays; 181 days old expires.
        let default_data = tempfile::tempdir().unwrap();
        let default_journal = DurableJournal::open(default_data.path()).unwrap();
        default_journal
            .append(event(
                "old-log",
                SignalKind::Log,
                now - chrono::Duration::days(179),
            ))
            .unwrap();
        default_journal
            .append(event(
                "mid-log",
                SignalKind::Log,
                now - chrono::Duration::days(31),
            ))
            .unwrap();
        archive::archive_journal_gcs(&default_journal, "gs://sift-retention/days180").unwrap();
        assert!(!archive::retention_due_at(default_data.path(), now).unwrap());
        let none = archive::expire_committed_events_at(&default_journal, now).unwrap();
        assert_eq!(none.expired_events, 0);
        assert_eq!(none.retained_events, 2);
        let later = now + chrono::Duration::days(2);
        assert!(archive::retention_due_at(default_data.path(), later).unwrap());
        let expiry = archive::expire_committed_events_at(&default_journal, later).unwrap();
        assert_eq!(expiry.expired_events, 1);
        assert_eq!(expiry.retained_events, 1);
    })
    .await
    .unwrap();

    std::env::remove_var("STORAGE_EMULATOR_HOST");
    emulator.abort();
}
