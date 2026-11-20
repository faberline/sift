use std::collections::VecDeque;

use crate::journal::application::journal_runtime::JournalRuntime;
use crate::journal::domain::resident_window::{resident_event_bytes, ResidentWindow};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

fn stored(cursor: u64, pad: usize) -> StoredEvent {
    let mut event = crate::EventEnvelope::for_project(
        "p",
        "prod",
        format!("e{cursor}"),
        SignalKind::Log,
        serde_json::json!({"message": "x".repeat(pad)}),
    );
    event.resource =
        std::collections::BTreeMap::from([("service.name".to_string(), "svc".to_string())]);
    StoredEvent {
        cursor,
        acknowledged_at: event.observed_at.clone(),
        event,
    }
}

fn cursors(events: &VecDeque<StoredEvent>) -> Vec<u64> {
    events.iter().map(|event| event.cursor).collect()
}

#[test]
fn oldest_events_leave_when_the_bytes_pass_the_bound() {
    let one = resident_event_bytes(&stored(1, 1000));
    let byte_limit = 2 * one + one / 2;
    let mut state = ResidentWindow::default();
    for cursor in 1..=4 {
        JournalRuntime::push_resident(&mut state, stored(cursor, 1000), 1000, byte_limit).unwrap();
    }
    assert_eq!(cursors(&state.recent_events), vec![3, 4]);
    assert!(state.resident_bytes <= byte_limit);
    assert_eq!(
        state.resident_bytes,
        resident_event_bytes(&stored(3, 1000)) + resident_event_bytes(&stored(4, 1000))
    );
}

#[test]
fn one_event_larger_than_the_bound_stays_alone() {
    let one = resident_event_bytes(&stored(1, 1000));
    let byte_limit = one / 2;

    let mut single = ResidentWindow::default();
    JournalRuntime::push_resident(&mut single, stored(1, 1000), 1000, byte_limit).unwrap();
    assert_eq!(cursors(&single.recent_events), vec![1]);

    let mut state = ResidentWindow::default();
    JournalRuntime::push_resident(&mut state, stored(1, 1000), 1000, byte_limit).unwrap();
    JournalRuntime::push_resident(&mut state, stored(2, 1000), 1000, byte_limit).unwrap();
    assert_eq!(cursors(&state.recent_events), vec![2]);
    assert_eq!(state.resident_bytes, resident_event_bytes(&stored(2, 1000)));
}

#[test]
fn the_count_limit_still_applies_and_bytes_follow() {
    let mut state = ResidentWindow::default();
    for cursor in 1..=4 {
        JournalRuntime::push_resident(&mut state, stored(cursor, 1000), 2, usize::MAX).unwrap();
    }
    assert_eq!(cursors(&state.recent_events), vec![3, 4]);
    assert_eq!(
        state.resident_bytes,
        resident_event_bytes(&stored(3, 1000)) + resident_event_bytes(&stored(4, 1000))
    );
}

#[test]
fn recompute_matches_the_running_total() {
    let one = resident_event_bytes(&stored(1, 1000));
    let mut state = ResidentWindow::default();
    for cursor in 1..=4 {
        JournalRuntime::push_resident(&mut state, stored(cursor, 1000), 1000, 2 * one + one / 2)
            .unwrap();
    }
    let running = state.resident_bytes;
    state.recompute_resident_bytes();
    assert_eq!(state.resident_bytes, running);

    let removed = state.recent_events.pop_front().unwrap();
    state.recompute_resident_bytes();
    assert_eq!(
        state.resident_bytes,
        running - resident_event_bytes(&removed)
    );
}

#[test]
fn evicted_event_leaves_the_cursor_map() {
    let one = resident_event_bytes(&stored(1, 1000));
    let mut state = ResidentWindow::default();
    for cursor in 1..=4 {
        JournalRuntime::push_resident(&mut state, stored(cursor, 1000), 1000, 2 * one + one / 2)
            .unwrap();
    }
    assert!(!state
        .recent_cursors_by_event_id
        .contains_key(&("p".to_string(), "e1".to_string())));
    assert!(state
        .recent_cursors_by_event_id
        .contains_key(&("p".to_string(), "e4".to_string())));
}
