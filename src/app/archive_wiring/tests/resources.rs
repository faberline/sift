use crate::archive::application::object_service::ObjectService;
use crate::archive::domain::resource_ports::{ArchiveObject, CatalogEntry};
use crate::archive::infrastructure::resource_adapter::object_store_port;
use std::sync::{Arc, Mutex};
use storage_object::{Object, ObjectMeta, ObjectStore, ObjectStoreError, PutCondition};

struct RecordingStore {
    inner: storage_object::LocalObjectStore,
    calls: Mutex<Vec<String>>,
    fail_key: Option<&'static str>,
}

impl RecordingStore {
    fn open(root: &std::path::Path, fail_key: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            inner: storage_object::LocalObjectStore::open(root).unwrap(),
            calls: Mutex::new(Vec::new()),
            fail_key,
        })
    }

    fn record(&self, operation: &str, key: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{operation}:{key}"));
    }
}

impl ObjectStore for RecordingStore {
    fn put(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: &str,
        condition: PutCondition,
    ) -> Result<ObjectMeta, ObjectStoreError> {
        self.record("put", key);
        if self.fail_key == Some(key) {
            return Err(ObjectStoreError::Unavailable {
                message: "fixed upload failure".into(),
            });
        }
        self.inner.put(key, bytes, content_type, condition)
    }

    fn get(&self, key: &str) -> Result<Object, ObjectStoreError> {
        self.record("get", key);
        self.inner.get(key)
    }

    fn head(&self, key: &str) -> Result<ObjectMeta, ObjectStoreError> {
        self.record("head", key);
        self.inner.head(key)
    }

    fn list(&self, prefix: &str) -> Result<Vec<ObjectMeta>, ObjectStoreError> {
        self.record("list", prefix);
        self.inner.list(prefix)
    }

    fn delete(&self, key: &str) -> Result<(), ObjectStoreError> {
        self.record("delete", key);
        self.inner.delete(key)
    }
}

fn object(key: &str, bytes: &[u8]) -> ArchiveObject {
    ArchiveObject {
        key: key.into(),
        bytes: bytes.into(),
        content_type: "application/json".into(),
    }
}

#[test]
fn archive_catalog_and_transaction_share_the_native_store_and_commit_manifest_last() {
    let temp = tempfile::tempdir().unwrap();
    let store = RecordingStore::open(temp.path(), None);
    let service = ObjectService::new(object_store_port(store.clone()));
    let catalog = service.catalog("catalog").unwrap();
    let mut transaction = service.transaction();
    assert_eq!(Arc::strong_count(&store), 4);
    let built = catalog
        .build_sorted_observed(
            [Ok(CatalogEntry {
                key: "blob/fixed".into(),
                value: b"fixed bytes".to_vec(),
            })],
            |_| Ok(()),
        )
        .unwrap();
    let native = storage_segment::PagedCatalog::new(store.clone(), "catalog").unwrap();
    let native_root: storage_segment::CatalogRoot = (&built.root).into();
    assert_eq!(
        native
            .reader(&native_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .value,
        b"fixed bytes"
    );
    transaction
        .put(object("segment/fixed", b"segment bytes"))
        .unwrap();
    assert!(matches!(
        store.inner.head("manifest.json"),
        Err(ObjectStoreError::NotFound { .. })
    ));
    let committed = transaction.commit(object("manifest.json", b"{}")).unwrap();
    assert_eq!(committed.manifest.key, "manifest.json");
    assert_eq!(committed.manifest.size, 2);
    let calls = store.calls.lock().unwrap();
    assert_eq!(calls.last().unwrap(), "put:manifest.json");
    assert_eq!(
        calls
            .iter()
            .filter(|call| *call == "put:manifest.json")
            .count(),
        1
    );
    drop(calls);
    assert_eq!(
        service.get("segment/fixed").unwrap().bytes,
        b"segment bytes"
    );
    service.delete("segment/fixed").unwrap();
    assert!(matches!(
        store.inner.head("segment/fixed"),
        Err(ObjectStoreError::NotFound { .. })
    ));
}

#[test]
fn archive_transaction_failure_keeps_native_error_chain_and_skips_manifest() {
    let port_temp = tempfile::tempdir().unwrap();
    let native_temp = tempfile::tempdir().unwrap();
    let port_store = RecordingStore::open(port_temp.path(), Some("segment/fail"));
    let native_store = RecordingStore::open(native_temp.path(), Some("segment/fail"));
    let service = ObjectService::new(object_store_port(port_store.clone()));
    let mut port_transaction = service.transaction();
    let mut native_transaction =
        storage_segment::ArchiveCoordinator::new(native_store.clone()).begin();
    let port_error = port_transaction
        .put(object("segment/fail", b"bytes"))
        .unwrap_err();
    let native_error = native_transaction
        .put(storage_segment::ArchiveObject::new(
            "segment/fail",
            b"bytes".to_vec(),
            "application/json",
        ))
        .unwrap_err();
    let chain = |error: storage_segment::SegmentError| {
        anyhow::Error::new(error)
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(chain(port_error), chain(native_error));
    let port_error = port_transaction
        .commit(object("manifest.json", b"{}"))
        .err()
        .unwrap();
    let native_error = native_transaction
        .commit(storage_segment::ArchiveObject::new(
            "manifest.json",
            b"{}".to_vec(),
            "application/json",
        ))
        .unwrap_err();
    assert_eq!(chain(port_error), chain(native_error));
    assert_eq!(
        *port_store.calls.lock().unwrap(),
        *native_store.calls.lock().unwrap()
    );
    assert_eq!(*port_store.calls.lock().unwrap(), ["put:segment/fail"]);
    assert!(matches!(
        port_store.inner.head("manifest.json"),
        Err(ObjectStoreError::NotFound { .. })
    ));
}

#[test]
fn archive_catalog_decode_keeps_context_source_and_unknown_key_errors() {
    use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
    use crate::shared_kernel::archive_manifest::ArchiveBlob;
    let error = decode_archive_catalog_entry(CatalogEntry {
        key: "blob/fixed".into(),
        value: b"{".to_vec(),
    })
    .err()
    .unwrap();
    let source = serde_json::from_slice::<ArchiveBlob>(b"{").unwrap_err();
    assert_eq!(
        anyhow::Error::new(error)
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "decode archive blob catalog entry".to_string(),
            source.to_string()
        ]
    );
    let error = decode_archive_catalog_entry(CatalogEntry {
        key: "unknown/fixed".into(),
        value: vec![],
    })
    .err()
    .unwrap();
    assert_eq!(
        anyhow::Error::new(error)
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["archive catalog contains an unknown key unknown/fixed"]
    );
}
