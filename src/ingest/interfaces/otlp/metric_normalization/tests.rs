//! The decoder, journal, and projection must keep the same histogram.
use opentelemetry_proto::tonic::metrics::v1::{
    exponential_histogram_data_point::Buckets, ExponentialHistogram, ExponentialHistogramDataPoint,
};
use serde_json::json;
use transport_otlp::proto::{self as wire, metric};

fn json_input(pod: &str, time: u64) -> serde_json::Value {
    json!({"resourceMetrics": [{"resource": {"attributes": [
    {"key":"service.name","value":{"stringValue":"checkout"}},
    {"key":"k8s.pod.name","value":{"stringValue":pod}}]},
    "scopeMetrics":[{"metrics":[{"name":"duration","unit":"s",
        "histogram":{"aggregationTemporality":2,"dataPoints":[{
            "startTimeUnixNano":(time-1_000_000_000).to_string(),
            "timeUnixNano":time.to_string(),"count":"4","sum":2.5,
            "bucketCounts":["1","2","1"],"explicitBounds":[0.1,1.0]}
        ]}}]}]}]})
}

fn proto_input(time: u64) -> wire::ExportMetricsServiceRequest {
    wire::ExportMetricsServiceRequest {
        resource_metrics: vec![wire::ResourceMetrics {
            resource: Some(wire::Resource {
                attributes: vec![
                    wire::KeyValue {
                        key: "service.name".into(),
                        value: Some(wire::AnyValue {
                            value: Some(wire::any_value::Value::StringValue("checkout".into())),
                        }),
                        ..Default::default()
                    },
                    wire::KeyValue {
                        key: "k8s.pod.name".into(),
                        value: Some(wire::AnyValue {
                            value: Some(wire::any_value::Value::StringValue("pod-a".into())),
                        }),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            scope_metrics: vec![wire::ScopeMetrics {
                metrics: vec![wire::Metric {
                    name: "duration".into(),
                    unit: "s".into(),
                    data: Some(metric::Data::Histogram(wire::Histogram {
                        aggregation_temporality: 2,
                        data_points: vec![wire::HistogramDataPoint {
                            start_time_unix_nano: time - 1_000_000_000,
                            time_unix_nano: time,
                            count: 4,
                            sum: Some(2.5),
                            bucket_counts: vec![1, 2, 1],
                            explicit_bounds: vec![0.1, 1.0],
                            ..Default::default()
                        }],
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
    }
}

#[tokio::test]
async fn json_protobuf_resource_identity_and_histogram_survive_restart() {
    let time = chrono::Utc::now().timestamp_nanos_opt().unwrap() as u64;
    let json_event = super::super::json_metrics_decoder::decode_metrics_json(
        &json_input("pod-a", time),
        "project-a",
    )
    .unwrap()
    .remove(0)
    .unwrap();
    let proto_event =
        super::super::proto_metrics_decoder::decode_metrics_proto(proto_input(time), "project-a")
            .unwrap()
            .remove(0)
            .unwrap();
    assert_eq!(json_event.event_id, proto_event.event_id);
    assert_eq!(json_event.payload, proto_event.payload);
    let other_pod = super::super::json_metrics_decoder::decode_metrics_json(
        &json_input("pod-b", time),
        "project-a",
    )
    .unwrap()
    .remove(0)
    .unwrap();
    assert_ne!(json_event.event_id, other_pod.event_id);
    let other_project = super::super::json_metrics_decoder::decode_metrics_json(
        &json_input("pod-a", time),
        "project-b",
    )
    .unwrap()
    .remove(0)
    .unwrap();
    assert_ne!(json_event.event_id, other_project.event_id);
    let dir = tempfile::tempdir().unwrap();
    {
        let state = crate::ServiceState::open(dir.path()).unwrap();
        state
            .append_events(vec![json_event, proto_event, other_pod, other_project])
            .await
            .unwrap();
        assert_eq!(state.journal().last_cursor(), 3);
        state.finish_drain().await.unwrap();
    }
    let state = crate::ServiceState::open(dir.path()).unwrap();
    state
        .projections()
        .catch_up(crate::projection::PROJECTION_METRIC_STORE)
        .unwrap();
    let query = serde_json::from_value(json!({"project":"project-a","name":"duration"})).unwrap();
    let page = state.projections().query_metrics(&query).unwrap();
    assert_eq!(page.series.len(), 2);
    for series in page.series {
        let point = &series.points[0];
        assert_eq!(point.start_time_unix_nano, Some(time - 1_000_000_000));
        let histogram = point.histogram.as_ref().unwrap();
        assert_eq!(histogram.bucket_counts, vec![1, 2, 1]);
        assert_eq!(histogram.explicit_bounds, vec![0.1, 1.0]);
        assert_eq!(histogram.count, 4);
    }
}

#[test]
fn malformed_histograms_are_rejected_before_admission() {
    let time = 1_790_000_000_000_000_000;
    for buckets in [json!([1, 2]), json!([1, 2, 9]), json!([u64::MAX, 1, 1])] {
        let mut input = json_input("pod-a", time);
        input["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]["dataPoints"]
            [0]["bucketCounts"] = buckets;
        assert!(
            super::super::json_metrics_decoder::decode_metrics_json(&input, "project-a").unwrap()
                [0]
            .is_err()
        );
    }
}

#[test]
fn invalid_temporality_and_present_histogram_fields_are_not_defaulted() {
    let time = 1_790_000_000_000_000_000;
    for temporality in [json!(0), json!(3), json!("bad")] {
        let mut input = json_input("pod-a", time);
        input["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
            ["aggregationTemporality"] = temporality;
        assert!(
            super::super::json_metrics_decoder::decode_metrics_json(&input, "p").unwrap()[0]
                .is_err()
        );
    }
    for temporality in [0, 3, -1] {
        let mut input = proto_input(time);
        if let Some(metric::Data::Histogram(histogram)) =
            &mut input.resource_metrics[0].scope_metrics[0].metrics[0].data
        {
            histogram.aggregation_temporality = temporality;
        }
        assert!(
            super::super::proto_metrics_decoder::decode_metrics_proto(input, "p").unwrap()[0]
                .is_err()
        );
    }
    for (field, value) in [
        ("sum", json!("bad")),
        ("min", json!(false)),
        ("flags", json!(-1)),
        ("explicitBounds", json!({})),
        ("bucketCounts", json!("bad")),
    ] {
        let mut input = json_input("pod-a", time);
        input["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]["dataPoints"]
            [0][field] = value;
        assert!(
            super::super::json_metrics_decoder::decode_metrics_json(&input, "p").unwrap()[0]
                .is_err(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn exponential_buckets_optional_sum_and_empty_unit_match_and_survive_restart() {
    let time = chrono::Utc::now().timestamp_nanos_opt().unwrap() as u64;
    let mut json = json_input("pod-a", time);
    let metric = &mut json["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0];
    metric.as_object_mut().unwrap().remove("histogram");
    metric["unit"] = json!("");
    metric["exponentialHistogram"] = json!({"aggregationTemporality":1,"dataPoints":[{
        "startTimeUnixNano":(time-1_000_000_000).to_string(),"timeUnixNano":time.to_string(),
        "count":"6","scale":2,"zeroCount":"1","zeroThreshold":0.01,
        "positive":{"offset":-2,"bucketCounts":["2","1"]},
        "negative":{"offset":3,"bucketCounts":["2"]},"min":-10.0,"max":10.0
    }]});
    let mut proto = proto_input(time);
    let metric = &mut proto.resource_metrics[0].scope_metrics[0].metrics[0];
    metric.unit.clear();
    metric.data = Some(metric::Data::ExponentialHistogram(ExponentialHistogram {
        aggregation_temporality: 1,
        data_points: vec![ExponentialHistogramDataPoint {
            start_time_unix_nano: time - 1_000_000_000,
            time_unix_nano: time,
            count: 6,
            scale: 2,
            zero_count: 1,
            zero_threshold: 0.01,
            positive: Some(Buckets {
                offset: -2,
                bucket_counts: vec![2, 1],
            }),
            negative: Some(Buckets {
                offset: 3,
                bucket_counts: vec![2],
            }),
            min: Some(-10.0),
            max: Some(10.0),
            ..Default::default()
        }],
    }));
    let json = super::super::json_metrics_decoder::decode_metrics_json(&json, "p")
        .unwrap()
        .remove(0)
        .unwrap();
    let proto = super::super::proto_metrics_decoder::decode_metrics_proto(proto, "p")
        .unwrap()
        .remove(0)
        .unwrap();
    assert_eq!(json.event_id, proto.event_id);
    assert_eq!(json.payload, proto.payload);
    let dir = tempfile::tempdir().unwrap();
    {
        let state = crate::ServiceState::open(dir.path()).unwrap();
        state.append_events(vec![json, proto]).await.unwrap();
        assert_eq!(state.journal().last_cursor(), 1);
        state.finish_drain().await.unwrap();
    }
    let state = crate::ServiceState::open(dir.path()).unwrap();
    state
        .projections()
        .catch_up(crate::projection::PROJECTION_METRIC_STORE)
        .unwrap();
    let query = serde_json::from_value(json!({"project":"p","name":"duration"})).unwrap();
    let page = state.projections().query_metrics(&query).unwrap();
    assert_eq!(page.series.len(), 1);
    let histogram = page.series[0].points[0].histogram.as_ref().unwrap();
    assert_eq!(histogram.positive_bucket_counts, vec![2, 1]);
    assert_eq!(histogram.negative_bucket_counts, vec![2]);
    assert_eq!(histogram.zero_threshold, 0.01);
    assert!(!histogram.sum_present);
}

#[test]
fn json_integer_points_reject_fractional_and_out_of_range_values() {
    for value in ["1.2", "9223372036854775808"] {
        let input = json!({"resourceMetrics":[{"scopeMetrics":[{"metrics":[{
            "name":"count", "gauge":{"dataPoints":[{"timeUnixNano":"100","asInt":value}]}
        }]}]}]});
        assert!(
            super::super::json_metrics_decoder::decode_metrics_json(&input, "p").unwrap()[0]
                .is_err()
        );
    }
    let input = json!({"resourceMetrics":[{"scopeMetrics":[{"metrics":[{
        "name":"count", "gauge":{"dataPoints":[{"timeUnixNano":"100","startTimeUnixNano":"bad","asInt":"1"}]}
    }]}]}]});
    assert!(
        super::super::json_metrics_decoder::decode_metrics_json(&input, "p").unwrap()[0].is_err()
    );
}
