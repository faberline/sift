//! A v7 snapshot keeps its verified disk chunks and gets current memory sizes.
use super::*;
use crate::projection::domain::projection::Projection;
#[test]
fn v7_disk_chunks_migrate_size_metadata_without_weakening_integrity() {
    let dir = tempfile::tempdir().unwrap();
    let projection = MetricProjection::open_with_limits(dir.path(), 10, 100, 1_000_000).unwrap();
    let id = "a".repeat(64);
    let resident = resident_metric_chunk(chunk(vec![test_point(1, 10)])).unwrap();
    let sealed = projection
        .chunk_store
        .as_ref()
        .unwrap()
        .write(&id, &resident)
        .unwrap();
    let correct = sealed.materialized_bytes;
    let mut old = sealed.clone();
    old.materialized_bytes = correct - 8;
    projection
        .state
        .write()
        .unwrap()
        .series
        .insert(id.clone(), test_series(&id, vec![old], vec![]));
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&projection.snapshot().unwrap()).unwrap();
    snapshot["schema_version"] = 7.into();
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    projection.restore(&bytes).unwrap();
    assert_eq!(
        projection.state.read().unwrap().series[&id].sealed_chunks[0].materialized_bytes,
        correct
    );
    assert_eq!(
        projection
            .query(&crate::projection::MetricQuery::for_project("project-a"))
            .unwrap()
            .series[0]
            .points
            .len(),
        1
    );
    snapshot["state"]["series"][&id]["sealed_chunks"][0]["sha256"] = "0".repeat(64).into();
    assert!(projection
        .restore(&serde_json::to_vec(&snapshot).unwrap())
        .is_err());
    assert_eq!(
        projection
            .query(&crate::projection::MetricQuery::for_project("project-a"))
            .unwrap()
            .series[0]
            .points
            .len(),
        1
    );
}
