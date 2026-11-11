use super::*;
use crate::control::domain::port::ControlPort;
use crate::control::infrastructure::{
    raft_port::RaftControlPort, state_machine::ControlStateMachine,
};
use crate::shared_kernel::control_record::{CommitReceipt, ControlCommand, ControlRecord};
use crate::shared_kernel::port_future::PortFuture;
use std::sync::Mutex;

struct FakeWebhook {
    sent: Mutex<Vec<Notification>>,
}

impl WebhookPort for FakeWebhook {
    fn send<'a>(&'a self, notification: &'a Notification) -> PortFuture<'a, (), String> {
        let first = {
            let mut sent = self.sent.lock().unwrap();
            sent.push(notification.clone());
            sent.len() == 1
        };
        Box::pin(async move {
            if first {
                Err("boom".to_string())
            } else {
                Ok(())
            }
        })
    }
}

struct Follower;

impl ControlPort for Follower {
    fn commit(&self, _command: ControlCommand) -> PortFuture<'_, CommitReceipt, String> {
        Box::pin(async { Err::<CommitReceipt, String>("a follower must not commit".to_string()) })
    }
    fn records<'a>(
        &'a self,
        _project: Option<&'a str>,
        _kind: RecordKind,
    ) -> PortFuture<'a, Vec<ControlRecord>, String> {
        Box::pin(async {
            Err::<Vec<ControlRecord>, String>("a follower must not read".to_string())
        })
    }
    fn leader(&self) -> PortFuture<'_, bool, String> {
        Box::pin(async { Ok::<bool, String>(false) })
    }
    fn snapshot(&self) -> Result<Vec<u8>, String> {
        Err("no snapshot".into())
    }
}

fn local_service(root: &std::path::Path) -> Arc<ControlService> {
    Arc::new(ControlService::new(Arc::new(RaftControlPort {
        machine: Arc::new(ControlStateMachine::open(root).unwrap()),
        raft: None,
        local: tokio::sync::Mutex::new(()),
        gate: Arc::new(|| Ok::<(), String>(())),
    })))
}

fn notification() -> Notification {
    Notification {
        id: "n-1".into(),
        project: "project-a".into(),
        rule_id: "rule-1".into(),
        alert_id: "alert-1".into(),
        webhook: "hook".into(),
        transition: "firing".into(),
        created_at: 0,
        payload: serde_json::json!({"notification_id": "n-1", "episode": 1}),
        attempts: 0,
        next_attempt: 0,
        delivered_at: None,
        last_error: None,
    }
}

async fn seed(service: &ControlService) {
    service
        .transact(
            "seed-1".into(),
            vec![Change {
                project: "project-a".into(),
                kind: RecordKind::Notification,
                id: "n-1".into(),
                expected_revision: 0,
                value: Some(serde_json::to_value(notification()).unwrap()),
            }],
        )
        .await
        .unwrap();
}

async fn stored(service: &ControlService) -> Notification {
    let records = service
        .list("project-a", RecordKind::Notification)
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    serde_json::from_value(records[0].value.clone().unwrap()).unwrap()
}

#[tokio::test]
async fn a_failed_delivery_is_retried_with_the_same_id_and_payload() {
    let dir = tempfile::tempdir().unwrap();
    let control = local_service(dir.path());
    seed(&control).await;
    let fake = Arc::new(FakeWebhook {
        sent: Mutex::new(Vec::new()),
    });
    let service = DeliveryService {
        control: Arc::clone(&control),
        webhook: Arc::clone(&fake) as Arc<dyn WebhookPort>,
    };

    service.deliver_due(100).await.unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    let failed = stored(&control).await;
    assert_eq!(failed.attempts, 1);
    assert_eq!(failed.delivered_at, None);
    assert_eq!(failed.last_error, Some("boom".to_string()));
    assert_eq!(failed.next_attempt, 102);

    service.deliver_due(101).await.unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);

    service.deliver_due(102).await.unwrap();
    {
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].id, sent[0].id);
        assert_eq!(sent[1].id, "n-1");
        assert_eq!(sent[1].payload, sent[0].payload);
    }
    let delivered = stored(&control).await;
    assert_eq!(delivered.delivered_at, Some(102));
    assert_eq!(delivered.last_error, None);
    assert_eq!(delivered.attempts, 2);

    service.deliver_due(200).await.unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_follower_sends_nothing() {
    let fake = Arc::new(FakeWebhook {
        sent: Mutex::new(Vec::new()),
    });
    let service = DeliveryService {
        control: Arc::new(ControlService::new(Arc::new(Follower))),
        webhook: Arc::clone(&fake) as Arc<dyn WebhookPort>,
    };

    service.deliver_due(100).await.unwrap();
    assert!(fake.sent.lock().unwrap().is_empty());
}
