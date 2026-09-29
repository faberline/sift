//! A metric query rejects zero or overflowing chunk and series metadata before
//! it touches points or reads a chunk.

use crate::projection::domain::metric::MetricQuery;

use super::*;

#[test]
fn query_rejects_zero_resident_byte_metadata_without_touching_points() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    let mut series = test_series(&series_id, Vec::new(), vec![chunk(vec![test_point(1, 10)])]);
    series.chunks[0].materialized_bytes = 0;
    projection
        .state
        .write()
        .unwrap()
        .series
        .insert(series_id, series);
    reset_point_memory_measurements();
    let error = projection
        .query(&MetricQuery::for_project("project-a"))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("resident metric chunk has no trusted size metadata"));
    assert_eq!(point_memory_measurements(), 0);
    assert_eq!(resident_point_materializations(), 0);
}

#[test]
fn query_rejects_zero_resident_point_metadata_without_touching_points() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    let mut series = test_series(&series_id, Vec::new(), vec![chunk(vec![test_point(1, 10)])]);
    series.chunks[0].point_count = 0;
    projection
        .state
        .write()
        .unwrap()
        .series
        .insert(series_id, series);
    reset_point_memory_measurements();
    let error = projection
        .query(&MetricQuery::for_project("project-a"))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("resident metric chunk has no trusted point metadata"));
    assert_eq!(point_memory_measurements(), 0);
    assert_eq!(resident_point_materializations(), 0);
}

#[test]
fn query_rejects_zero_sealed_byte_metadata_before_reading_the_chunk() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            vec![SealedMetricChunk {
                key: format!("{series_id}/missing.json"),
                start_time_unix_nano: 10,
                end_time_unix_nano: 10,
                point_count: 1,
                encoded_bytes: 1,
                materialized_bytes: 0,
                sha256: "0".repeat(64),
            }],
            Vec::new(),
        ),
    );
    let error = projection
        .query(&MetricQuery::for_project("project-a"))
        .unwrap_err();
    assert!(
        error.to_string().contains("sealed metric chunk")
            && error.to_string().contains("has no byte metadata")
    );
    assert!(!error.to_string().contains("missing.json checksum"));
}

#[test]
fn query_rejects_source_byte_metadata_overflow() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            vec![
                SealedMetricChunk {
                    key: format!("{series_id}/first-missing.json"),
                    start_time_unix_nano: 10,
                    end_time_unix_nano: 10,
                    point_count: 1,
                    encoded_bytes: 1,
                    materialized_bytes: usize::MAX,
                    sha256: "0".repeat(64),
                },
                SealedMetricChunk {
                    key: format!("{series_id}/second-missing.json"),
                    start_time_unix_nano: 11,
                    end_time_unix_nano: 11,
                    point_count: 1,
                    encoded_bytes: 1,
                    materialized_bytes: 1,
                    sha256: "0".repeat(64),
                },
            ],
            Vec::new(),
        ),
    );
    let error = projection
        .query_with_source_limits(&MetricQuery::for_project("project-a"), 2, usize::MAX)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("metric query source byte budget overflowed"));
}

#[test]
fn query_rejects_resident_source_byte_metadata_overflow() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    let mut series = test_series(
        &series_id,
        Vec::new(),
        vec![
            chunk(vec![test_point(1, 10)]),
            chunk(vec![test_point(2, 11)]),
        ],
    );
    series.chunks[0].materialized_bytes = usize::MAX;
    series.chunks[1].materialized_bytes = 1;
    projection
        .state
        .write()
        .unwrap()
        .series
        .insert(series_id, series);
    reset_point_memory_measurements();
    let error = projection
        .query_with_source_limits(&MetricQuery::for_project("project-a"), 2, usize::MAX)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("metric query source byte budget overflowed"));
    assert_eq!(point_memory_measurements(), 0);
    assert_eq!(resident_point_materializations(), 0);
}

#[test]
fn query_rejects_zero_sealed_point_metadata_before_reading_the_chunk() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            vec![SealedMetricChunk {
                key: format!("{series_id}/missing.json"),
                start_time_unix_nano: 10,
                end_time_unix_nano: 10,
                point_count: 0,
                encoded_bytes: 1,
                materialized_bytes: 1,
                sha256: "0".repeat(64),
            }],
            Vec::new(),
        ),
    );
    let error = projection
        .query(&MetricQuery::for_project("project-a"))
        .unwrap_err();
    assert!(
        error.to_string().contains("sealed metric chunk")
            && error.to_string().contains("has no point metadata")
    );
    assert!(!error.to_string().contains("missing.json checksum"));
}

#[test]
fn query_rejects_zero_sealed_encoded_byte_metadata_before_reading_the_chunk() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            vec![SealedMetricChunk {
                key: format!("{series_id}/missing.json"),
                start_time_unix_nano: 10,
                end_time_unix_nano: 10,
                point_count: 1,
                encoded_bytes: 0,
                materialized_bytes: 1,
                sha256: "0".repeat(64),
            }],
            Vec::new(),
        ),
    );
    let error = projection
        .query(&MetricQuery::for_project("project-a"))
        .unwrap_err();
    assert!(
        error.to_string().contains("sealed metric chunk")
            && error.to_string().contains("has no byte metadata")
    );
    assert!(!error.to_string().contains("missing.json checksum"));
}

#[test]
fn query_rejects_point_metadata_overflow_within_one_series() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    let mut series = test_series(
        &series_id,
        vec![
            SealedMetricChunk {
                key: format!("{series_id}/first-missing.json"),
                start_time_unix_nano: 10,
                end_time_unix_nano: 10,
                point_count: 1,
                encoded_bytes: 1,
                materialized_bytes: 1,
                sha256: "0".repeat(64),
            },
            SealedMetricChunk {
                key: format!("{series_id}/second-missing.json"),
                start_time_unix_nano: 11,
                end_time_unix_nano: 11,
                point_count: 1,
                encoded_bytes: 1,
                materialized_bytes: 1,
                sha256: "0".repeat(64),
            },
        ],
        Vec::new(),
    );
    series.sealed_chunks[0].point_count = usize::MAX;
    projection
        .state
        .write()
        .unwrap()
        .series
        .insert(series_id, series);
    let error = projection
        .query_with_source_point_limit(&MetricQuery::for_project("project-a"), usize::MAX)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("metric query source point budget overflowed"));
}

#[test]
fn query_rejects_point_metadata_overflow_across_series() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let first_id = "a".repeat(64);
    let second_id = "b".repeat(64);
    let sealed = |series_id: &str, point_count| SealedMetricChunk {
        key: format!("{series_id}/missing.json"),
        start_time_unix_nano: 10,
        end_time_unix_nano: 10,
        point_count,
        encoded_bytes: 1,
        materialized_bytes: 1,
        sha256: "0".repeat(64),
    };
    let mut state = projection.state.write().unwrap();
    state.series.insert(
        first_id.clone(),
        test_series(&first_id, vec![sealed(&first_id, usize::MAX)], Vec::new()),
    );
    state.series.insert(
        second_id.clone(),
        test_series(&second_id, vec![sealed(&second_id, 1)], Vec::new()),
    );
    drop(state);
    let mut query = MetricQuery::for_project("project-a");
    query.limit = 2;
    let error = projection
        .query_with_source_point_limit(&query, usize::MAX)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("metric query source point budget overflowed"));
}
