//! Required behavior across the new product contexts.
use super::ServiceState;
use crate::alerting::application::{
    delivery::DeliveryService,
    service::{AlertRule, AlertingService, Observation},
};
use crate::alerting::domain::{delivery_port::WebhookPort, state::Notification};
use crate::control::application::service::{Change, RecordKind};
use crate::error_reporting::application::service::ErrorReportingService;
use crate::shared_kernel::port_future::PortFuture;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn rule(hold: u64) -> AlertRule {
    serde_json::from_value(json!({"version":1,"project":"p","source":{
        "kind":"logs_count","window_seconds":60,"comparison":"greater","threshold":0},
        "hold_seconds":hold,"webhook":"test"}))
    .unwrap()
}
fn read_value(records: Vec<crate::control::ControlRecord>) -> Value {
    records.into_iter().find_map(|r| r.value).unwrap()
}
#[tokio::test]
async fn control_restart_retains_cas_receipts_and_project_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap();
    let change = Change {
        project: "p".into(),
        kind: RecordKind::SavedQuery,
        id: "q".into(),
        expected_revision: 0,
        value: Some(json!({"name":"first"})),
    };
    let receipt = control
        .transact("first".into(), vec![change.clone()])
        .await
        .unwrap();
    assert_eq!(
        control
            .transact("first".into(), vec![change.clone()])
            .await
            .unwrap()
            .index,
        receipt.index
    );
    assert!(control
        .transact("conflict".into(), vec![change.clone()])
        .await
        .unwrap_err()
        .contains("revision conflict"));
    assert!(control
        .list("other", RecordKind::SavedQuery)
        .await
        .unwrap()
        .is_empty());
    drop(state);
    let restarted = ServiceState::open(dir.path()).unwrap();
    let control = restarted.control_service().unwrap();
    assert_eq!(
        read_value(control.list("p", RecordKind::SavedQuery).await.unwrap())["name"],
        "first"
    );
    assert_eq!(
        control
            .transact("first".into(), vec![change])
            .await
            .unwrap()
            .index,
        receipt.index
    );
    let changed = Change {
        project: "p".into(),
        kind: RecordKind::SavedQuery,
        id: "q".into(),
        expected_revision: 1,
        value: Some(json!({"name":"second"})),
    };
    assert!(control
        .transact("first".into(), vec![changed.clone()])
        .await
        .unwrap_err()
        .contains("reused"));
    control
        .transact("second".into(), vec![changed])
        .await
        .unwrap();
}

#[tokio::test]
async fn error_groups_dedupe_reopen_and_keep_counting_ignored_errors() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap().clone();
    let errors = ErrorReportingService::new(control.clone());
    let log = |cursor, message: &str| {
        serde_json::from_value(json!({"project":"p","environment":"prod","cursor":cursor,
        "event_id":format!("e{cursor}"),"occurred_at":"2026-10-01T00:00:00Z","observed_at":"2026-10-01T00:00:00Z",
        "resource":{"service.name":"api","service.version":"1"},"attributes":{},"severity":"ERROR",
        "body_text":message,"json_payload":{"message":message},"coexistence_key":"test"})).unwrap()
    };
    errors
        .ingest("p", 0, 1, &[log(1, "failed request 123")])
        .await
        .unwrap();
    let record = control
        .list("p", RecordKind::ErrorGroup)
        .await
        .unwrap()
        .remove(0);
    let id = record.id;
    errors
        .status("p", &id, 1, "resolve".into(), "resolved")
        .await
        .unwrap();
    errors
        .ingest("p", 1, 2, &[log(2, "failed request 456")])
        .await
        .unwrap();
    let record = control
        .list("p", RecordKind::ErrorGroup)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(record.value.as_ref().unwrap()["status"], "open");
    assert_eq!(record.value.as_ref().unwrap()["count"], 2);
    errors
        .status("p", &id, record.revision, "ignore".into(), "ignored")
        .await
        .unwrap();
    errors
        .ingest("p", 2, 3, &[log(3, "failed request 789")])
        .await
        .unwrap();
    assert!(errors
        .ingest("p", 2, 3, &[log(3, "failed request 789")])
        .await
        .is_err());
    let value = read_value(control.list("p", RecordKind::ErrorGroup).await.unwrap());
    assert_eq!(value["status"], "ignored");
    assert_eq!(value["count"], 3);
    assert!(control
        .list("other", RecordKind::ErrorGroup)
        .await
        .unwrap()
        .is_empty());
}

#[derive(Default)]
struct RetryingWebhook {
    ids: Mutex<Vec<String>>,
}
impl WebhookPort for RetryingWebhook {
    fn send<'a>(&'a self, n: &'a Notification) -> PortFuture<'a, (), String> {
        Box::pin(async move {
            let mut ids = self.ids.lock().unwrap();
            ids.push(n.id.clone());
            if ids.len() == 1 {
                Err("temporary failure".into())
            } else {
                Ok(())
            }
        })
    }
}
#[tokio::test]
async fn unknown_does_not_recover_and_notification_retry_retains_id_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap().clone();
    let alerts = AlertingService::new(control.clone());
    let rule = rule(30);
    let yes = || BTreeMap::from([(BTreeMap::new(), Observation::Condition(true))]);
    alerts.evaluate("r", &rule, yes(), None, 100).await.unwrap();
    assert!(control
        .list("p", RecordKind::Notification)
        .await
        .unwrap()
        .is_empty());
    alerts.evaluate("r", &rule, yes(), None, 130).await.unwrap();
    alerts
        .evaluate(
            "r",
            &rule,
            BTreeMap::from([(BTreeMap::new(), Observation::Unknown("scan limit".into()))]),
            None,
            140,
        )
        .await
        .unwrap();
    assert_eq!(
        read_value(control.list("p", RecordKind::Alert).await.unwrap())["phase"],
        "unknown"
    );
    assert_eq!(
        control
            .list("p", RecordKind::Notification)
            .await
            .unwrap()
            .len(),
        1
    );
    let webhook = Arc::new(RetryingWebhook::default());
    DeliveryService {
        control: control.clone(),
        webhook: webhook.clone(),
    }
    .deliver_due(140)
    .await
    .unwrap();
    drop(alerts);
    drop(control);
    drop(state);
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap().clone();
    DeliveryService {
        control: control.clone(),
        webhook: webhook.clone(),
    }
    .deliver_due(150)
    .await
    .unwrap();
    let ids = webhook.ids.lock().unwrap().clone();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
    let notifications = control.list("p", RecordKind::Notification).await.unwrap();
    assert_eq!(notifications[0].value.as_ref().unwrap()["attempts"], 2);
    let alerts = AlertingService::new(control.clone());
    alerts
        .evaluate(
            "r",
            &rule,
            BTreeMap::from([(BTreeMap::new(), Observation::Condition(false))]),
            None,
            160,
        )
        .await
        .unwrap();
    assert_eq!(
        control
            .list("p", RecordKind::Notification)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn mute_changes_state_without_sending_and_no_data_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap().clone();
    let alerts = AlertingService::new(control.clone());
    let mut rule = rule(0);
    rule.mute_until = Some(200);
    alerts
        .evaluate(
            "r",
            &rule,
            BTreeMap::from([(BTreeMap::new(), Observation::Condition(true))]),
            None,
            100,
        )
        .await
        .unwrap();
    assert_eq!(
        read_value(control.list("p", RecordKind::Alert).await.unwrap())["phase"],
        "firing"
    );
    assert!(control
        .list("p", RecordKind::Notification)
        .await
        .unwrap()
        .is_empty());
    alerts
        .evaluate("r", &rule, BTreeMap::new(), None, 110)
        .await
        .unwrap();
    assert_eq!(
        read_value(control.list("p", RecordKind::Alert).await.unwrap())["phase"],
        "unknown"
    );
}

#[tokio::test]
async fn unmute_sends_the_current_episode_and_error_events_do_not_claim_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open(dir.path()).unwrap();
    let control = state.control_service().unwrap().clone();
    let alerts = AlertingService::new(control.clone());
    let mut rule = rule(0);
    rule.mute_until = Some(200);
    let yes = || BTreeMap::from([(BTreeMap::new(), Observation::Condition(true))]);
    alerts
        .evaluate("muted", &rule, yes(), None, 100)
        .await
        .unwrap();
    alerts
        .evaluate("muted", &rule, yes(), None, 201)
        .await
        .unwrap();
    alerts
        .evaluate("muted", &rule, yes(), None, 202)
        .await
        .unwrap();
    assert_eq!(
        control
            .list("p", RecordKind::Notification)
            .await
            .unwrap()
            .len(),
        1
    );
    rule.mute_until = None;
    rule.source = crate::alerting::application::service::RuleSource::NewError;
    for (now, cursor) in [(300, 1), (301, 1), (302, 2)] {
        alerts
            .evaluate(
                "event",
                &rule,
                BTreeMap::from([(
                    BTreeMap::from([("error_group".into(), "g".into())]),
                    Observation::Event(cursor),
                )]),
                None,
                now,
            )
            .await
            .unwrap();
    }
    let notifications = control.list("p", RecordKind::Notification).await.unwrap();
    assert_eq!(notifications.len(), 3);
    assert!(notifications
        .iter()
        .all(|record| record.value.as_ref().unwrap()["transition"] == "firing"));
}

#[tokio::test]
async fn control_role_refuses_telemetry_without_changing_control_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = ServiceState::open_with_role(
        dir.path(),
        crate::node::domain::storage_role::StorageRole::Control,
    )
    .unwrap();
    let before = state.control_service().unwrap().snapshot_bytes().unwrap();
    let event = crate::EventEnvelope::for_project(
        "p",
        "prod",
        "test",
        crate::SignalKind::Log,
        json!({"message":"test"}),
    );
    assert!(state
        .append_events(vec![event])
        .await
        .unwrap_err()
        .to_string()
        .contains("role_mismatch"));
    assert_eq!(
        state.control_service().unwrap().snapshot_bytes().unwrap(),
        before
    );
}
