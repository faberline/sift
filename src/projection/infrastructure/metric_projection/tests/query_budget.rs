//! A metric query stops at its source point and byte budgets before it
//! materializes history, reads a sealed chunk or loads an excess series.

use crate::projection::domain::metric::{HistogramKind, MetricHistogramV1, MetricQuery};

use super::*;

#[test]
fn query_rejects_source_history_before_materializing_it() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            Vec::new(),
            vec![chunk(vec![
                test_point(1, 1),
                test_point(2, 2),
                test_point(3, 3),
            ])],
        ),
    );
    let mut query = MetricQuery::for_project("project-a");
    query.limit = 1;
    reset_point_memory_measurements();
    let error = projection
        .query_with_source_point_limit(&query, 2)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("would scan 3 source points; the limit is 2"));
    assert_eq!(point_memory_measurements(), 0);
    assert_eq!(resident_point_materializations(), 0);
}

#[test]
fn query_skips_sealed_chunks_outside_the_time_window() {
    let projection = MetricProjection::with_limits(10, 2_000).unwrap();
    let series_id = "a".repeat(64);
    let old_sealed = SealedMetricChunk {
        key: format!("{series_id}/missing.json"),
        start_time_unix_nano: -20_001_000_000_000,
        end_time_unix_nano: -20_000_000_000_000,
        point_count: 1_000,
        encoded_bytes: 1_000,
        materialized_bytes: 1_000,
        sha256: "0".repeat(64),
    };
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(
            &series_id,
            vec![old_sealed],
            vec![chunk(vec![test_point(1, 10_000_000_000)])],
        ),
    );
    let mut query = MetricQuery::for_project("project-a");
    query.start_time = Some("1970-01-01T00:00:10Z".into());
    query.end_time = Some("1970-01-01T00:00:11Z".into());
    query.limit = 1;
    let resident_bytes =
        projection.state.read().unwrap().series[&series_id].chunks[0].materialized_bytes;
    let page = projection
        .query_with_source_limits(&query, 1, resident_bytes)
        .unwrap();
    assert_eq!(page.series.len(), 1);
    assert_eq!(page.series[0].points.len(), 1);
}

#[test]
fn query_marks_excess_cardinality_without_loading_the_extra_series() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let first_id = "a".repeat(64);
    let second_id = "b".repeat(64);
    let mut state = projection.state.write().unwrap();
    state.series.insert(
        first_id.clone(),
        test_series(
            &first_id,
            Vec::new(),
            vec![chunk(vec![test_point(1, 10_000_000_000)])],
        ),
    );
    state.series.insert(
        second_id.clone(),
        test_series(
            &second_id,
            vec![SealedMetricChunk {
                key: format!("{second_id}/missing.json"),
                start_time_unix_nano: 10_000_000_000,
                end_time_unix_nano: 10_000_000_000,
                point_count: 1,
                encoded_bytes: 1,
                materialized_bytes: 1,
                sha256: "0".repeat(64),
            }],
            Vec::new(),
        ),
    );
    drop(state);

    let mut query = MetricQuery::for_project("project-a");
    query.limit = 1;
    let page = projection.query_with_source_point_limit(&query, 1).unwrap();
    assert!(page.has_more);
    assert_eq!(page.series.len(), 1);
    assert_eq!(page.series[0].series_id, first_id);
}

#[test]
fn query_rejects_sealed_bytes_before_reading_the_chunk() {
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
                encoded_bytes: 2_048,
                materialized_bytes: 4_096,
                sha256: "0".repeat(64),
            }],
            Vec::new(),
        ),
    );
    let mut query = MetricQuery::for_project("project-a");
    query.limit = 1;
    let error = projection
        .query_with_source_limits(&query, 1, 1_024)
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("would materialize 4096 source bytes; the limit is 1024"));
    assert!(!error.to_string().contains("missing.json"));
}

#[test]
fn query_rejects_a_large_resident_histogram_by_byte_budget() {
    let projection = MetricProjection::with_limits(10, 100).unwrap();
    let series_id = "a".repeat(64);
    let mut point = test_point(1, 10);
    point.histogram = Some(MetricHistogramV1 {
        kind: HistogramKind::Explicit,
        count: 2_049,
        sum: 2_049.0,
        explicit_bounds: (0..2_048).map(|value| value as f64).collect(),
        bucket_counts: vec![1; 2_049],
        scale: None,
        zero_count: 0,
        positive_offset: 0,
        positive_bucket_counts: Vec::new(),
        negative_offset: 0,
        negative_bucket_counts: Vec::new(),
        min: Some(0.0),
        max: Some(2_048.0),
    });
    projection.state.write().unwrap().series.insert(
        series_id.clone(),
        test_series(&series_id, Vec::new(), vec![chunk(vec![point])]),
    );
    let mut query = MetricQuery::for_project("project-a");
    query.limit = 1;
    reset_point_memory_measurements();
    let error = projection
        .query_with_source_limits(&query, 1, 4_096)
        .unwrap_err();
    assert!(error.to_string().contains("would materialize"));
    assert!(error.to_string().contains("the limit is 4096"));
    assert_eq!(point_memory_measurements(), 0);
    assert_eq!(resident_point_materializations(), 0);
}
