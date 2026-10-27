use super::archive_services;
use crate::archive::domain::control_values::{
    ArchiveGcPending, ArchiveUploadIntent, LocalBlobGcPending, RestoreState,
};
use serde::{de::DeserializeOwned, Serialize};

mod resources;

fn assert_round_trip<T: DeserializeOwned + Serialize>(bytes: &[u8]) {
    let value: T = serde_json::from_slice(bytes).unwrap();
    assert_eq!(serde_json::to_vec(&value).unwrap(), bytes);
    let mut unknown: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    unknown["unknown_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<T>(unknown).is_err());
}

fn root_bytes() -> String {
    format!(
        r#"{{"format_version":1,"height":1,"entry_count":2,"page_bytes_limit":65536,"root":{{"key":"catalog/root.json","sha256":"{}","bytes":256,"entry_count":2,"first_key":"gc/a","last_key":"gc/z"}}}}"#,
        "2".repeat(64)
    )
}

#[test]
fn archive_control_records_keep_field_order_and_native_catalog_bytes() {
    let root = root_bytes();
    let native: storage_segment::CatalogRoot = serde_json::from_str(&root).unwrap();
    assert_eq!(serde_json::to_string(&native).unwrap(), root);
    let published: crate::shared_kernel::catalog_manifest::CatalogRoot = native.into();
    assert_eq!(serde_json::to_string(&published).unwrap(), root);
    let gc = format!(
        r#"{{"format_version":3,"replacement_manifest_uri":"gs://bucket/backup/manifest.json","replacement_manifest_sha256":"{}","gc_plan_uri":"gs://bucket/backup/gc","gc_plan_root":{},"cursor":"gc/a"}}"#,
        "1".repeat(64),
        root
    );
    assert_round_trip::<ArchiveGcPending>(gc.as_bytes());
    let local_gc = format!(
        r#"{{"format_version":3,"replacement_manifest_uri":"gs://bucket/backup/manifest.json","replacement_manifest_sha256":"{}","gc_plan_uri":"gs://bucket/backup/gc","gc_plan_root":{},"plan_cursor":null,"plan_exhausted":false,"candidates":[],"scan_start_cursor":12,"scanned_through_cursor":15}}"#,
        "1".repeat(64),
        root
    );
    assert_round_trip::<LocalBlobGcPending>(local_gc.as_bytes());
    let upload = br#"{"format_version":1,"destination_uri":"gs://bucket/backup","archive_prefix":"backup/archives/fixed","manifest_uri":"gs://bucket/backup/archives/fixed/manifest.json","generated_at":"2026-09-01T00:00:00Z","captured_cursor":24,"source_cluster_id":"cluster-fixed","source_manifest_uri":null,"source_manifest_sha256":null}"#;
    assert_round_trip::<ArchiveUploadIntent>(upload);
    let restore = br#"{"format_version":1,"manifest_uri":"gs://bucket/backup/manifest.json","phase":"ready"}"#;
    assert_round_trip::<RestoreState>(restore);

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("gc.json");
    let value = serde_json::from_slice::<ArchiveGcPending>(gc.as_bytes()).unwrap();
    let services = archive_services();
    services.persist_archive_gc_pending(&path, &value).unwrap();
    let reopened = services.read_archive_gc_pending(&path).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), gc.as_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn control_ports_keep_decode_error_chains_and_stop_before_new_intents() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("control/archive-commit.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{").unwrap();
    let services = archive_services();
    let error = services.read_commit_state(temp.path()).unwrap_err();
    let source =
        serde_json::from_slice::<crate::archive::domain::control_values::ArchiveCommitState>(b"{")
            .unwrap_err();
    assert_eq!(
        error.chain().map(ToString::to_string).collect::<Vec<_>>(),
        vec![
            format!("decode archive commit receipt {}", path.display()),
            source.to_string()
        ]
    );
    assert!(!temp
        .path()
        .join("control/archive-upload-intent.json")
        .exists());
}
