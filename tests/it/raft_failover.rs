//! Three durable Sift voters retain acknowledged data across repeated leader loss.

use std::{
    collections::HashMap,
    future::pending,
    net::SocketAddr,
    sync::{Arc, Weak},
    time::Duration,
};

use raft_runtime::{
    FsyncPolicy, HostConfig, Membership, PeerTransport, RaftHost, RaftStateMachine, RaftStore,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair,
};
use sift::{
    journal::SiftStateMachine, DurableJournal, EventEnvelope, EventQuery, SignalKind, StoredEvent,
};
use tempfile::TempDir;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};

const WAIT: Duration = Duration::from_secs(10);

struct Authority {
    cert: Certificate,
    key: KeyPair,
}

impl Authority {
    fn generate() -> Self {
        let mut params = CertificateParams::new(Vec::new()).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, "Sift test peer CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let key = KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        Self { cert, key }
    }
}

struct PeerMaterial {
    dir: TempDir,
    config: peer_tls::PeerTlsConfig,
}

fn peer_material(authority: &Authority, id: u64) -> PeerMaterial {
    let mut params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    params
        .distinguished_name
        .push(DnType::CommonName, format!("sift-peer-{id}"));
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    let key = KeyPair::generate().unwrap();
    let cert = params
        .signed_by(&key, &authority.cert, &authority.key)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cert_path = dir.path().join("tls.crt");
    let key_path = dir.path().join("tls.key");
    let ca_path = dir.path().join("ca.crt");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key.serialize_pem()).unwrap();
    std::fs::write(&ca_path, authority.cert.pem()).unwrap();
    PeerMaterial {
        dir,
        config: peer_tls::PeerTlsConfig {
            cert: cert_path,
            key: key_path,
            ca: ca_path,
            required: true,
        },
    }
}

struct Node {
    running: Option<RunningNode>,
    data: TempDir,
    material: PeerMaterial,
}

struct RunningNode {
    host: RaftHost,
    journal: Arc<DurableJournal>,
    state_machine: Weak<SiftStateMachine>,
    listener: JoinHandle<anyhow::Result<()>>,
    shutdown: oneshot::Sender<()>,
}

impl Node {
    async fn start(&mut self, id: usize, peer_listener: TcpListener, urls: &[String]) {
        assert!(self.running.is_none(), "voter {id} is already running");
        let data = self.data.path();
        let journal = Arc::new(DurableJournal::open(data).unwrap());
        let state_machine = Arc::new(SiftStateMachine::open(data, journal.clone()).unwrap());
        let state_machine_released = Arc::downgrade(&state_machine);
        let transport = PeerTransport::from_config(&self.material.config).unwrap();
        let peers = urls
            .iter()
            .enumerate()
            .filter(|(peer, _)| *peer != id)
            .map(|(peer, url)| (peer as u64, url.clone()))
            .collect::<HashMap<_, _>>();
        let host = RaftHost::spawn_with_peer_transport(
            id as u64,
            Membership {
                voters: vec![0, 1, 2],
                learners: Vec::new(),
            },
            peers,
            RaftStore::open(data.to_str().unwrap(), id as u64, FsyncPolicy::Always).unwrap(),
            state_machine as Arc<dyn RaftStateMachine>,
            HostConfig::default(),
            transport.clone(),
        );
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = local.local_addr().unwrap();
        let router = host.router();
        let (shutdown, stop) = oneshot::channel();
        let listener = tokio::spawn(async move {
            tokio::select! {
                result = transport.serve(local, router, pending()) => result,
                result = peer_link(peer_listener, target, stop) => result.map_err(Into::into),
            }
        });
        self.running = Some(RunningNode {
            host,
            journal,
            state_machine: state_machine_released,
            listener,
            shutdown,
        });
    }

    fn live(&self) -> &RunningNode {
        self.running.as_ref().expect("running voter")
    }

    async fn crash(&mut self) {
        let RunningNode {
            host,
            journal,
            state_machine,
            listener,
            shutdown,
        } = self.running.take().expect("running voter to stop");
        let old_journal = Arc::downgrade(&journal);
        drop(host);
        drop(journal);
        shutdown.send(()).expect("stop the peer link");
        tokio::time::timeout(WAIT, listener)
            .await
            .expect("peer listener shutdown")
            .expect("peer listener task")
            .expect("peer listener result");
        tokio::time::timeout(WAIT, async {
            while old_journal.strong_count() != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("stopped voter released its journal");
        // Dropping RaftHost only aborts its tick and pump tasks; they release the
        // state machine (and the data root it holds) when the runtime cancels them.
        tokio::time::timeout(WAIT, async {
            while state_machine.strong_count() != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("stopped voter released its state machine");
    }
}

// Cut every live socket on failure while leaving the real mTLS peer transport
// responsible for handshakes and Raft requests.
async fn peer_link(
    listener: TcpListener,
    target: SocketAddr,
    mut stop: oneshot::Receiver<()>,
) -> std::io::Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            stream = listener.accept() => {
                let (mut peer, _) = stream?;
                connections.spawn(async move {
                    let mut backend = TcpStream::connect(target).await?;
                    tokio::io::copy_bidirectional(&mut peer, &mut backend).await
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    connections.shutdown().await;
    Ok(())
}

fn event(id: &str, observed_at: chrono::DateTime<chrono::Utc>) -> EventEnvelope {
    let mut event = EventEnvelope::for_project_at(
        "project-a",
        "prod",
        id,
        SignalKind::Log,
        serde_json::json!({"message":"replicated"}),
        observed_at,
    );
    event
        .resource
        .insert("service.name".to_string(), "sift-failover-test".to_string());
    event
}

fn command(event: EventEnvelope) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "kind":"append_events",
        "events":[event]
    }))
    .unwrap()
}

async fn wait_for_leader(nodes: &[Node]) -> usize {
    for _ in 0..400 {
        for (index, node) in nodes.iter().enumerate() {
            if let Some(running) = &node.running {
                if running.host.is_leader().await {
                    return index;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("three Sift voters did not elect a leader");
}

async fn wait_for_event(nodes: &[Node], id: &str, required: usize) {
    for _ in 0..400 {
        let present = nodes
            .iter()
            .filter_map(|node| node.running.as_ref())
            .filter(|node| has_event(&node.journal, id))
            .count();
        if present >= required {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("event {id} did not reach {required} live durable voters");
}

fn has_event(journal: &DurableJournal, id: &str) -> bool {
    events(journal)
        .iter()
        .any(|stored| stored.event.event_id == id)
}

fn events(journal: &DurableJournal) -> Vec<StoredEvent> {
    journal
        .query(EventQuery {
            limit: 100,
            ..EventQuery::default()
        })
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn acknowledged_data_survives_leader_loss_over_mutual_tls() {
    let authority = Authority::generate();
    let observed_at = chrono::Utc::now();
    let mut listeners = Vec::new();
    let mut addresses = Vec::new();
    let mut urls = Vec::new();
    for _ in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        addresses.push(address);
        urls.push(format!("https://localhost:{}", address.port()));
        listeners.push(listener);
    }

    let mut nodes = Vec::new();
    for (id, listener) in listeners.into_iter().enumerate() {
        let mut node = Node {
            running: None,
            data: tempfile::tempdir().unwrap(),
            material: peer_material(&authority, id as u64),
        };
        node.start(id, listener, &urls).await;
        nodes.push(node);
    }

    let first_leader = wait_for_leader(&nodes).await;
    nodes[first_leader]
        .live()
        .host
        .propose(command(event("before-failure", observed_at)))
        .await
        .expect("two durable voters acknowledge the first batch");
    wait_for_event(&nodes, "before-failure", 3).await;
    let first_batch = events(&nodes[first_leader].live().journal);
    assert_eq!(first_batch.len(), 1);

    nodes[first_leader].crash().await;

    let second_leader = wait_for_leader(&nodes).await;
    assert_ne!(second_leader, first_leader);
    nodes[second_leader]
        .live()
        .host
        .propose(command(event("after-failure", observed_at)))
        .await
        .expect("the surviving quorum accepts a second batch");
    wait_for_event(&nodes, "after-failure", 2).await;
    let second_batch = events(&nodes[second_leader].live().journal);
    assert_eq!(second_batch.len(), 2);
    assert_eq!(&second_batch[..1], first_batch);

    for running in nodes.iter().filter_map(|node| node.running.as_ref()) {
        assert_eq!(events(&running.journal), second_batch);
    }

    let restored = &mut nodes[first_leader];
    let stopped_journal = DurableJournal::open(restored.data.path()).unwrap();
    assert_eq!(events(&stopped_journal), first_batch);
    drop(stopped_journal);
    let listener = TcpListener::bind(addresses[first_leader]).await.unwrap();
    restored.start(first_leader, listener, &urls).await;
    wait_for_event(&nodes, "after-failure", 3).await;
    assert_eq!(events(&nodes[first_leader].live().journal), second_batch);

    let next_leader = wait_for_leader(&nodes).await;
    assert_ne!(next_leader, first_leader);
    nodes[next_leader].crash().await;
    let final_leader = wait_for_leader(&nodes).await;
    assert_ne!(final_leader, next_leader);
    nodes[final_leader]
        .live()
        .host
        .propose(command(event("after-second-failure", observed_at)))
        .await
        .expect("the quorum with the restored voter accepts a third batch");
    wait_for_event(&nodes, "after-second-failure", 2).await;

    let final_batch = events(&nodes[final_leader].live().journal);
    assert_eq!(final_batch.len(), 3);
    assert_eq!(&final_batch[..2], second_batch);
    for running in nodes.iter().filter_map(|node| node.running.as_ref()) {
        assert_eq!(events(&running.journal), final_batch);
    }
    assert_eq!(events(&nodes[first_leader].live().journal), final_batch);

    for (id, (mut node, address)) in nodes.into_iter().zip(addresses).enumerate() {
        if node.running.is_some() {
            node.crash().await;
        }
        let journal = DurableJournal::open(node.data.path()).unwrap();
        let expected = if id == next_leader {
            &second_batch
        } else {
            &final_batch
        };
        assert_eq!(&events(&journal), expected);
        drop(journal);
        TcpListener::bind(address)
            .await
            .expect("peer port released");
        node.data.close().expect("remove temporary voter data");
        node.material
            .dir
            .close()
            .expect("remove temporary voter PKI");
    }
    eprintln!(
        "stopped leaders {first_leader} and {next_leader}; final leader {final_leader}; retained three acknowledged events"
    );
}

fn event_ids(journal: &DurableJournal) -> Vec<String> {
    events(journal)
        .into_iter()
        .map(|stored| stored.event.event_id)
        .collect()
}

fn missing_acknowledged(acknowledged: &[String], stored: &[String]) -> Vec<String> {
    acknowledged
        .iter()
        .filter(|id| !stored.contains(id))
        .cloned()
        .collect()
}

async fn acknowledge(nodes: &[Node], id: &str, observed_at: chrono::DateTime<chrono::Utc>) {
    for _ in 0..40 {
        let leader = wait_for_leader(nodes).await;
        if nodes[leader]
            .live()
            .host
            .propose(command(event(id, observed_at)))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("event {id} was never acknowledged");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rolling_restart_of_three_voters_keeps_every_acknowledged_event() {
    let authority = Authority::generate();
    let observed_at = chrono::Utc::now();
    let mut listeners = Vec::new();
    let mut addresses = Vec::new();
    let mut urls = Vec::new();
    for _ in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        addresses.push(address);
        urls.push(format!("https://localhost:{}", address.port()));
        listeners.push(listener);
    }

    let mut nodes = Vec::new();
    for (id, listener) in listeners.into_iter().enumerate() {
        let mut node = Node {
            running: None,
            data: tempfile::tempdir().unwrap(),
            material: peer_material(&authority, id as u64),
        };
        node.start(id, listener, &urls).await;
        nodes.push(node);
    }

    let mut acknowledged: Vec<String> = Vec::new();
    acknowledge(&nodes, "before-restart", observed_at).await;
    acknowledged.push("before-restart".to_string());
    wait_for_event(&nodes, "before-restart", 3).await;

    for id in 0..3 {
        nodes[id].crash().await;
        let listener = TcpListener::bind(addresses[id]).await.unwrap();
        nodes[id].start(id, listener, &urls).await;
        wait_for_leader(&nodes).await;
        let next = format!("after-restart-{id}");
        acknowledge(&nodes, &next, observed_at).await;
        acknowledged.push(next.clone());
        wait_for_event(&nodes, &next, 3).await;
        for running in nodes.iter().filter_map(|node| node.running.as_ref()) {
            let stored = event_ids(&running.journal);
            let missing = missing_acknowledged(&acknowledged, &stored);
            assert!(
                missing.is_empty(),
                "voter lost acknowledged events after restarting voter {id}: {missing:?}"
            );
        }
    }

    let expected = events(&nodes[0].live().journal);
    assert_eq!(expected.len(), 4);
    for running in nodes.iter().filter_map(|node| node.running.as_ref()) {
        assert_eq!(events(&running.journal), expected);
    }

    for (mut node, address) in nodes.into_iter().zip(addresses) {
        if node.running.is_some() {
            node.crash().await;
        }
        let journal = DurableJournal::open(node.data.path()).unwrap();
        assert_eq!(events(&journal), expected);
        drop(journal);
        TcpListener::bind(address)
            .await
            .expect("peer port released");
        node.data.close().expect("remove temporary voter data");
        node.material
            .dir
            .close()
            .expect("remove temporary voter PKI");
    }
}

#[test]
fn a_missing_acknowledged_event_is_detected() {
    let acknowledged = vec!["before-restart".to_string(), "after-restart-0".to_string()];
    let stored = vec!["before-restart".to_string()];
    assert_eq!(
        missing_acknowledged(&acknowledged, &stored),
        vec!["after-restart-0".to_string()]
    );
    assert!(missing_acknowledged(&acknowledged, &acknowledged.clone()).is_empty());
}
