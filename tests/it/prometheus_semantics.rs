use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use metrics_remote_write::proto::{Label, Sample, TimeSeries, WriteRequest};
use prost::Message;
use sift::{router, ServiceState};
use tower::ServiceExt;

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn write_remote_series(app: &axum::Router, timeseries: Vec<TimeSeries>) {
    let request = WriteRequest {
        timeseries,
        metadata: Vec::new(),
    };
    let compressed = metrics_remote_write::encode_snappy(&request.encode_to_vec()).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/prometheus/api/v1/write")
                .header("content-type", "application/x-protobuf")
                .header("content-encoding", "snappy")
                .header("x-prometheus-remote-write-version", "0.1.0")
                .body(Body::from(compressed))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

/// Builds one series. `samples` are `(timestamp_ms, value)` pairs.
fn series(name: &str, extra: &[(&str, &str)], samples: &[(i64, f64)]) -> TimeSeries {
    let mut labels = vec![
        Label {
            name: "__name__".into(),
            value: name.into(),
        },
        Label {
            name: "project".into(),
            value: "project-a".into(),
        },
    ];
    for (key, value) in extra {
        labels.push(Label {
            name: (*key).into(),
            value: (*value).into(),
        });
    }
    // Remote write requires labels sorted by name.
    labels.sort_by(|a, b| a.name.cmp(&b.name));
    TimeSeries {
        labels,
        samples: samples
            .iter()
            .map(|&(timestamp, value)| Sample { value, timestamp })
            .collect(),
        exemplars: Vec::new(),
    }
}

/// Runs an instant query. `query` must already be percent-encoded by the caller.
async fn instant(
    app: &axum::Router,
    query: &str,
    time_seconds: i64,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/prometheus/api/v1/query?project=project-a&query={query}&time={time_seconds}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    (status, body_json(response).await)
}

fn open_app() -> (tempfile::TempDir, axum::Router) {
    let temp = tempfile::tempdir().unwrap();
    let app = router(Arc::new(ServiceState::open(temp.path()).unwrap()));
    (temp, app)
}

#[tokio::test]
async fn aggregates_sum_avg_min_max_count_over_three_series() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;
    let ms = base * 1_000;
    write_remote_series(
        &app,
        vec![
            series("agg_gauge", &[("instance", "a")], &[(ms, 1.0)]),
            series("agg_gauge", &[("instance", "b")], &[(ms, 2.0)]),
            series("agg_gauge", &[("instance", "c")], &[(ms, 6.0)]),
        ],
    )
    .await;

    let cases = [
        ("sum%28agg_gauge%29", "9"),
        ("avg%28agg_gauge%29", "3"),
        ("min%28agg_gauge%29", "1"),
        ("max%28agg_gauge%29", "6"),
        ("count%28agg_gauge%29", "3"),
    ];
    for (query, expected) in cases {
        let (status, body) = instant(&app, query, base).await;
        assert_eq!(status, StatusCode::OK, "query {query}");
        assert_eq!(body["status"], "success", "query {query}");
        assert_eq!(body["data"]["resultType"], "vector", "query {query}");
        let result = body["data"]["result"].as_array().unwrap();
        assert_eq!(result.len(), 1, "query {query}");
        assert_eq!(result[0]["metric"], serde_json::json!({}), "query {query}");
        assert_eq!(result[0]["value"][1], expected, "query {query}");
    }
}

#[tokio::test]
async fn rate_is_increase_per_second_over_the_sampled_window() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;
    let ms = base * 1_000;
    write_remote_series(
        &app,
        vec![series(
            "rate_counter",
            &[],
            &[(ms, 0.0), (ms + 10_000, 10.0), (ms + 20_000, 20.0)],
        )],
    )
    .await;

    let (status, body) = instant(&app, "rate%28rate_counter%29", base + 20).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    let result = body["data"]["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["value"][1], "1");
}

#[tokio::test]
async fn rate_treats_a_counter_reset_as_a_new_start() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;
    let ms = base * 1_000;
    write_remote_series(
        &app,
        vec![series(
            "reset_counter",
            &[],
            &[(ms, 10.0), (ms + 10_000, 20.0), (ms + 20_000, 5.0)],
        )],
    )
    .await;

    let (status, body) = instant(&app, "rate%28reset_counter%29", base + 20).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    let result = body["data"]["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["value"][1], "0.75");
}

#[tokio::test]
async fn label_matcher_selects_only_matching_series() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;
    let ms = base * 1_000;
    write_remote_series(
        &app,
        vec![
            series("matcher_gauge", &[("job", "a")], &[(ms, 3.0)]),
            series("matcher_gauge", &[("job", "b")], &[(ms, 4.0)]),
        ],
    )
    .await;

    let (status, body) = instant(&app, "matcher_gauge%7Bjob%3D%22a%22%7D", base).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    let result = body["data"]["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["metric"]["job"], "a");
    assert_eq!(result[0]["value"][1], "3");

    let (status, body) = instant(&app, "sum%28matcher_gauge%7Bjob%3D%22b%22%7D%29", base).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    let result = body["data"]["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["value"][1], "4");
}

#[tokio::test]
async fn stale_sample_is_hidden_after_the_five_minute_lookback() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;
    let ms = base * 1_000;
    write_remote_series(&app, vec![series("lookback_gauge", &[], &[(ms, 7.0)])]).await;

    // 299 s after the sample: still inside the 300 s lookback.
    let (status, body) = instant(&app, "lookback_gauge", base + 299).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    let result = body["data"]["result"].as_array().unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["value"][1], "7");

    // 301 s after the sample: outside the lookback, so nothing is returned.
    let (status, body) = instant(&app, "lookback_gauge", base + 301).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "success");
    assert_eq!(body["data"]["result"], serde_json::json!([]));
}

#[tokio::test]
async fn unsupported_query_is_rejected_with_bad_data() {
    let (_temp, app) = open_app();
    let base = chrono::Utc::now().timestamp() - 1;

    let queries = [
        "histogram_quantile%280.9%2C%20x%29",
        "sum%20by%20%28job%29%20%28x%29",
        "x%7Bjob%3D~%22a.*%22%7D",
    ];
    for query in queries {
        let (status, body) = instant(&app, query, base).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "query {query}");
        assert_eq!(body["error"], "bad_data", "query {query}");
    }
}
