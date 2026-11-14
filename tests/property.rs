//! Property tests over the public API: the service log decoder, the OTLP
//! decoder, the signal WAL and the Raft state machine.

use std::sync::Arc;

use chrono::{SecondsFormat, TimeZone, Utc};
use proptest::prelude::*;
use sift::{
    collector::decode_service_log,
    ingest::{decode_otlp, OtlpMediaType, OtlpSignal},
    journal::{encode_raft_batch_at_for_diagnostics, SiftStateMachine, SignalWal},
    DurableJournal, EventEnvelope, SignalKind, StoredEvent,
};

const SERVICE_LOG_SCHEMA: &str = "axiom.service.log.v1";

fn decision_time() -> String {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn log_event(project: &str, id: &str, message: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        project,
        "test",
        id,
        SignalKind::Log,
        serde_json::json!({"message": message}),
    );
    event
        .resource
        .insert("service.name".into(), "property".into());
    event
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any byte line gives Ok or Err from decode_service_log, never a panic.
    #[test]
    fn service_log_decoder_never_panics(
        bytes in prop::collection::vec(any::<u8>(), 0..512),
        offset in any::<u64>(),
    ) {
        let _ = decode_service_log(&bytes, "src", offset, "proj", "env");
    }

    /// Decoding the same valid JSON line twice gives the same event.
    #[test]
    fn service_log_decoder_is_deterministic(message in "[a-zA-Z0-9 ]{1,40}") {
        let line = serde_json::json!({
            "schema": SERVICE_LOG_SCHEMA,
            "timestamp": "2026-07-17T10:00:00Z",
            "severity": "INFO",
            "service": {"name": "lumen", "version": "0.4.21"},
            "event": "property",
            "message": message,
        })
        .to_string();
        // observed_at is the collector's wall clock unless enrichment supplies it,
        // so it is the one field that may differ between two decodes.
        let without_observed_at = |event: &sift::OperationalEventV2| {
            let mut value = serde_json::to_value(event).unwrap();
            value.as_object_mut().unwrap().remove("observed_at");
            value
        };
        let first = decode_service_log(line.as_bytes(), "src", 7, "proj", "env").unwrap();
        let second = decode_service_log(line.as_bytes(), "src", 7, "proj", "env").unwrap();
        prop_assert_eq!(without_observed_at(&first), without_observed_at(&second));
    }

    /// Any body for any signal and media type gives Ok or Err from decode_otlp, never a panic.
    #[test]
    fn otlp_decoder_never_panics(
        signal in prop_oneof![
            Just(OtlpSignal::Logs),
            Just(OtlpSignal::Metrics),
            Just(OtlpSignal::Traces),
        ],
        media in prop_oneof![Just(OtlpMediaType::Json), Just(OtlpMediaType::Protobuf)],
        bytes in prop_oneof![
            prop::collection::vec(any::<u8>(), 0..512),
            "\\PC{0,200}".prop_map(String::into_bytes),
        ],
    ) {
        let _ = decode_otlp(signal, media, &bytes, "proj");
    }

    /// append_batch then recovered_events returns the same events in the same order.
    #[test]
    fn wal_batch_round_trips(
        start in 1u64..1000,
        messages in prop::collection::vec("[a-z ]{0,20}", 1..6),
    ) {
        let root = tempfile::tempdir().unwrap();
        let wal = SignalWal::open(root.path()).unwrap();
        let events: Vec<StoredEvent> = messages
            .iter()
            .enumerate()
            .map(|(i, message)| StoredEvent {
                cursor: start + i as u64,
                acknowledged_at: "2026-01-01T00:00:00Z".to_string(),
                event: log_event("proj", &format!("wal-{i}"), message),
            })
            .collect();
        wal.append_batch(&events).unwrap();
        prop_assert_eq!(wal.recovered_events().unwrap(), events);
    }

    /// A batch whose cursors are not contiguous is always rejected.
    #[test]
    fn wal_rejects_non_contiguous_cursors(start in 1u64..1000, gap in 2u64..10) {
        let root = tempfile::tempdir().unwrap();
        let wal = SignalWal::open(root.path()).unwrap();
        let events = vec![
            StoredEvent {
                cursor: start,
                acknowledged_at: "2026-01-01T00:00:00Z".to_string(),
                event: log_event("proj", "gap-a", "a"),
            },
            StoredEvent {
                cursor: start + gap,
                acknowledged_at: "2026-01-01T00:00:00Z".to_string(),
                event: log_event("proj", "gap-b", "b"),
            },
        ];
        prop_assert!(wal.append_batch(&events).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// Applying the same command sequence to two fresh state machines gives the same outcomes and the same event count.
    #[test]
    fn raft_apply_is_deterministic(ids in prop::collection::vec("[a-z]{1,3}", 1..8)) {
        let at = decision_time();
        let mut machines = Vec::new();
        for _ in 0..2 {
            let root = tempfile::tempdir().unwrap();
            let journal = Arc::new(DurableJournal::open(root.path()).unwrap());
            machines.push((root, journal.clone(), SiftStateMachine::new(journal)));
        }

        for (position, id) in ids.iter().enumerate() {
            let index = position as u64 + 1;
            let command = encode_raft_batch_at_for_diagnostics(
                vec![log_event("raft-prop", id, "deterministic")],
                &at,
            )
            .unwrap();
            let mut outcomes = Vec::new();
            for (_, _, machine) in &machines {
                machine.apply_local(index, &command).unwrap();
                let results = machine.take_append_outcomes(index).unwrap();
                outcomes.push(
                    results
                        .iter()
                        .map(|result| (result.raw_cursor, result.duplicate))
                        .collect::<Vec<_>>(),
                );
            }
            prop_assert_eq!(&outcomes[0], &outcomes[1]);
        }

        let counts: Vec<u64> = machines
            .iter()
            .map(|(_, journal, _)| journal.total_event_count())
            .collect();
        prop_assert_eq!(counts[0], counts[1]);
        prop_assert_eq!(
            machines[0].2.applied_commit_index(),
            machines[1].2.applied_commit_index()
        );
    }
}
