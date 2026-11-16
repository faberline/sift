//! Actual peer transport and durable voter state, without cloud resources.
use super::*;
use crate::control::{
    application::service::ControlService, infrastructure::raft_port::RaftControlPort,
};
use crate::shared_kernel::control_record::Change;
use raft_runtime::{
    FsyncPolicy, HostConfig, Membership, NodeId, PeerTransport, RaftHost, RaftStore,
};
use rcgen::{BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair};
use std::{collections::HashMap, sync::Arc, time::Duration};

struct Node {
    host: Option<Arc<RaftHost>>,
    service: Option<Arc<ControlService>>,
    machine: Arc<ControlStateMachine>,
    listener: tokio::task::JoinHandle<anyhow::Result<()>>,
    _root: tempfile::TempDir,
    _tls: tempfile::TempDir,
}
async fn leader(nodes: &[Node]) -> usize {
    for _ in 0..400 {
        for (i, node) in nodes.iter().enumerate() {
            if let Some(host) = &node.host {
                if host.is_leader().await {
                    return i;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("control quorum did not elect a leader");
}
fn saved(revision: u64, value: &str) -> Change {
    Change {
        project: "p".into(),
        kind: RecordKind::SavedQuery,
        id: "q".into(),
        expected_revision: revision,
        value: Some(serde_json::json!({"name":value})),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_control_voters_keep_records_receipts_and_notification_ids_after_leader_loss() {
    let mut params = CertificateParams::new(Vec::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    let ca = params.self_signed(&ca_key).unwrap();
    let mut listeners = Vec::new();
    let mut urls = Vec::new();
    for _ in 0..3 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        ));
        listeners.push(listener);
    }
    let mut nodes = Vec::new();
    for (id, listener) in listeners.into_iter().enumerate() {
        let id = id as u64;
        let root = tempfile::tempdir().unwrap();
        let tls = tempfile::tempdir().unwrap();
        let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, format!("control-{id}"));
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &ca, &ca_key).unwrap();
        let config = peer_tls::PeerTlsConfig::new(
            tls.path().join("tls.crt"),
            tls.path().join("tls.key"),
            tls.path().join("ca.crt"),
            true,
        );
        std::fs::write(config.cert(), cert.pem()).unwrap();
        std::fs::write(config.key(), key.serialize_pem()).unwrap();
        std::fs::write(config.ca(), ca.pem()).unwrap();
        let transport = PeerTransport::from_config(&config).unwrap();
        let peers = urls
            .iter()
            .enumerate()
            .filter(|(peer, _)| *peer as u64 != id)
            .map(|(peer, url)| (NodeId::new(peer as u64), url.clone()))
            .collect::<HashMap<_, _>>();
        let machine = Arc::new(ControlStateMachine::open(root.path()).unwrap());
        let host = Arc::new(RaftHost::spawn_with_peer_transport(
            NodeId::new(id),
            Membership::new(
                vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)],
                Vec::new(),
            ),
            peers,
            RaftStore::open(
                root.path().to_str().unwrap(),
                NodeId::new(id),
                FsyncPolicy::Always,
            )
            .unwrap(),
            machine.clone(),
            HostConfig::default(),
            transport.clone(),
        ));
        let service = Arc::new(ControlService::new(Arc::new(RaftControlPort {
            machine: machine.clone(),
            raft: Some(host.clone()),
            local: tokio::sync::Mutex::new(()),
            gate: Arc::new(|| Ok(())),
        })));
        let router = host.router();
        let listener = tokio::spawn(async move {
            transport
                .serve(listener, router, std::future::pending::<()>())
                .await
        });
        nodes.push(Node {
            host: Some(host),
            service: Some(service),
            machine,
            listener,
            _root: root,
            _tls: tls,
        });
    }
    let first = leader(&nodes).await;
    let service = nodes[first].service.as_ref().unwrap();
    let receipt = service.transact("initial".into(), vec![saved(0, "before"), Change {
        project: "p".into(), kind: RecordKind::Notification, id: "fixed-notification-id".into(), expected_revision: 0,
        value: Some(serde_json::json!({"id":"fixed-notification-id","attempts":1,"payload":{"event":"firing"}})),
    }]).await.unwrap();
    // A quorum acknowledgement does not wait for the third voter to apply.
    for _ in 0..400 {
        if nodes
            .iter()
            .all(|n| n.machine.applied_index().get() >= receipt.index)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    nodes[first]
        .host
        .as_ref()
        .unwrap()
        .require_applied_index_on_all_voters(raft_runtime::Index::new(receipt.index))
        .await
        .unwrap();
    nodes[first]
        .host
        .as_ref()
        .unwrap()
        .snapshot_and_compact()
        .await
        .unwrap();
    nodes[first].listener.abort();
    nodes[first].service.take();
    nodes[first].host.take();
    let second = leader(&nodes).await;
    assert_ne!(first, second);
    let service = nodes[second].service.as_ref().unwrap();
    let records = service.list("p", RecordKind::Notification).await.unwrap();
    assert_eq!(records[0].id, "fixed-notification-id");
    assert_eq!(records[0].value.as_ref().unwrap()["attempts"], 1);
    let receipt = service
        .transact("after-failure".into(), vec![saved(1, "after")])
        .await
        .unwrap();
    for _ in 0..400 {
        if nodes
            .iter()
            .filter(|n| n.host.is_some())
            .all(|n| n.machine.applied_index().get() >= receipt.index)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    for node in nodes.iter().filter(|n| n.host.is_some()) {
        assert!(node.machine.applied_index().get() >= receipt.index);
        assert_eq!(
            node.machine.records(Some("p"), RecordKind::SavedQuery)[0]
                .value
                .as_ref()
                .unwrap()["name"],
            "after"
        );
        assert_eq!(
            node.machine.records(Some("p"), RecordKind::Notification)[0].id,
            "fixed-notification-id"
        );
        let reopened = ControlStateMachine::open(node._root.path()).unwrap();
        assert_eq!(
            reopened.records(Some("p"), RecordKind::SavedQuery)[0].revision,
            2
        );
    }
    let snapshot = nodes[second].machine.bytes().unwrap();
    let fresh = tempfile::tempdir().unwrap();
    ControlStateMachine::bootstrap(fresh.path(), &snapshot).unwrap();
    assert!(ControlStateMachine::bootstrap(fresh.path(), &snapshot).is_err());
    let restored = ControlStateMachine::open(fresh.path()).unwrap();
    assert_eq!(restored.applied_index().get(), 0);
    assert_eq!(
        restored.records(Some("p"), RecordKind::Notification)[0].id,
        "fixed-notification-id"
    );
    assert!(restored.receipt("after-failure").is_none());
    let command = ControlCommand {
        request_id: "fresh-group".into(),
        changes: vec![saved(2, "restored")],
    };
    restored
        .apply(
            raft_runtime::Index::new(1),
            &serde_json::to_vec(&command).unwrap(),
        )
        .unwrap();
    assert_eq!(
        restored.records(Some("p"), RecordKind::SavedQuery)[0].revision,
        3
    );
    for node in &mut nodes {
        node.service.take();
        if let Some(host) = node.host.take() {
            let _ = host.shutdown().await;
        }
        node.listener.abort();
    }
}
