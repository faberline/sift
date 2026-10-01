use super::*;
use crate::journal::domain::{
    dedupe_store::DedupeStore,
    journal_head::JournalHead,
    recovery_store::{JournalRoot, RecoveryReader, RecoveryStore},
    wal_store::WalStore,
};
use crate::journal::infrastructure::recovery_store_adapter::StorageRecoveryAdapter;
use crate::shared_kernel::{dedupe_stats::DedupeStats, event::EventEnvelope};
use std::sync::Mutex;

struct Stores {
    inner: StorageRecoveryAdapter,
    steps: Arc<Mutex<Vec<&'static str>>>,
    failure: Option<&'static str>,
}
impl Stores {
    fn enter(&self, step: &'static str) -> Result<()> {
        self.steps.lock().unwrap().push(step);
        if self.failure == Some(step) {
            return Err(anyhow::anyhow!("storage fault")).map_err(|error| error.context(step));
        }
        Ok(())
    }
}
impl RecoveryStore for Stores {
    type Error = anyhow::Error;
    fn initialize_root(&self, root: &Path) -> Result<Arc<dyn JournalRoot>> {
        self.enter("root")?;
        self.inner.initialize_root(root)
    }
    fn open_wal(&self, root: &Path) -> Result<Arc<dyn WalStore<Error = Self::Error>>> {
        self.enter("wal")?;
        self.inner.open_wal(root)
    }
    fn open_events(&self, root: &Path) -> Result<Arc<dyn RawEventStore<Error = Self::Error>>> {
        self.enter("events")?;
        self.inner.open_events(root)
    }
    fn load_head(&self, root: &Path) -> Result<Option<JournalHead>> {
        self.enter("load head")?;
        self.inner.load_head(root)
    }
    fn persist_head(&self, root: &Path, head: &JournalHead) -> Result<()> {
        self.enter("persist head")?;
        self.inner.persist_head(root, head)
    }
    fn open_dedupe(
        &self,
        root: &Path,
    ) -> Result<(Arc<dyn DedupeStore<Error = Self::Error>>, DedupeStats)> {
        self.enter("dedupe")?;
        self.inner.open_dedupe(root)
    }
    fn reader<'a>(
        &self,
        storage: &'a dyn RawEventStore<Error = Self::Error>,
        wal: &dyn WalStore<Error = Self::Error>,
        archived: ArchiveWatermarks,
        after: u64,
        repair: bool,
    ) -> Result<Box<dyn RecoveryReader<Error = Self::Error> + 'a>> {
        self.enter("reader")?;
        self.inner.reader(storage, wal, archived, after, repair)
    }
}
struct Policy(bool);
impl JournalGovernance for Policy {
    type Error = anyhow::Error;
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.0, "policy rejected");
        Ok(())
    }
    fn govern(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        Ok(event)
    }
}
fn recorded_dependencies(
    failure: Option<&'static str>,
) -> (JournalDependencies, Arc<Mutex<Vec<&'static str>>>) {
    let steps = Arc::new(Mutex::new(Vec::new()));
    let mut ports = dependencies(StorageRole::All);
    ports.resources = Arc::new(Stores {
        inner: StorageRecoveryAdapter {
            role: StorageRole::All,
        },
        steps: steps.clone(),
        failure,
    });
    (ports, steps)
}

#[test]
fn recovery_failure_stops_at_each_store_port_and_keeps_error_chain() {
    let expected = [
        "root",
        "wal",
        "events",
        "load head",
        "dedupe",
        "reader",
        "reader",
        "persist head",
    ];
    for failure in [
        "root",
        "wal",
        "events",
        "load head",
        "dedupe",
        "reader",
        "persist head",
    ] {
        let position = expected.iter().position(|step| *step == failure).unwrap();
        let root = tempfile::tempdir().unwrap();
        let (ports, steps) = recorded_dependencies(Some(failure));
        let result =
            JournalRuntime::open_configured(root.path(), Arc::new(Policy(true)), 128, ports);
        let error = match result {
            Ok(_) => panic!("fault must stop recovery"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), failure);
        assert_eq!(error.root_cause().to_string(), "storage fault");
        assert_eq!(*steps.lock().unwrap(), expected[..=position]);
    }
}

#[test]
fn recovery_policy_and_limit_fail_before_opening_storage() {
    for (policy, limit, message) in [
        (false, 128, "policy rejected"),
        (
            true,
            0,
            "resident journal event limit must be greater than zero",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (ports, steps) = recorded_dependencies(None);
        let result =
            JournalRuntime::open_configured(root.path(), Arc::new(Policy(policy)), limit, ports);
        let error = match result {
            Ok(_) => panic!("invalid configuration must fail"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), message);
        assert!(steps.lock().unwrap().is_empty());
    }
}

#[test]
fn recovered_head_is_persisted_after_replay() {
    let root = tempfile::tempdir().unwrap();
    let (ports, steps) = recorded_dependencies(None);
    let journal =
        JournalRuntime::open_configured(root.path(), Arc::new(Policy(true)), 128, ports).unwrap();
    assert_eq!(
        *steps.lock().unwrap(),
        [
            "root",
            "wal",
            "events",
            "load head",
            "dedupe",
            "reader",
            "reader",
            "persist head"
        ]
    );
    assert_eq!(journal.total_event_count(), 0);
    assert_eq!(journal.last_cursor(), 0);
}

#[test]
fn raft_and_read_session_share_runtime_without_archive_ownership_cycle() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(
        JournalRuntime::open_configured(
            root.path(),
            Arc::new(Policy(true)),
            128,
            dependencies(StorageRole::All),
        )
        .unwrap(),
    );
    let weak = Arc::downgrade(&runtime);
    let journal = Arc::new(JournalService::owned(runtime.clone()));
    let raft = crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine::new(
        journal.clone(),
    );
    let mut session = JournalService::projection_read_session(&journal, 0).unwrap();
    assert_eq!(Arc::strong_count(&runtime), 4);
    assert!(session.read_next(10).unwrap().is_empty());
    drop(session);
    drop(raft);
    drop(journal);
    drop(runtime);
    assert!(weak.upgrade().is_none());
}
