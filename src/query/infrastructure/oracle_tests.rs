//! The same corpus runs against the pinned official Prometheus binary.
use super::promql_parser::PromqlParser;
use crate::query::domain::{
    metric_evaluator::evaluate,
    metric_expression::{ExpressionParser, Sample, Series, Value},
};
use crate::MetricTemporality;
use serde_json::Value as Json;
#[test]
fn sift_matches_the_shared_prometheus_oracle_corpus() {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../tests/fixtures/production-v1/promql-oracle.json"
    ))
    .unwrap();
    let interval = fixture["interval_seconds"].as_i64().unwrap() * 1_000_000_000;
    let series = fixture["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| Series {
            labels: serde_json::from_value(s["labels"].clone()).unwrap(),
            temporality: MetricTemporality::Cumulative,
            points: s["values"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, v)| Sample {
                    time: i as i64 * interval,
                    start_time: Some(0),
                    value: v.as_f64().unwrap(),
                    stale: false,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let end = fixture["evaluation_seconds"].as_i64().unwrap() * 1_000_000_000;
    for case in fixture["cases"].as_array().unwrap() {
        let expr = case["expression"].as_str().unwrap();
        let result = evaluate(&PromqlParser.parse(expr).unwrap(), &series, end).unwrap();
        match result {
            Value::Scalar(v) => assert!(
                (v - case["result"]["scalar"].as_f64().unwrap()).abs() < 1e-10,
                "{expr}"
            ),
            Value::Vector(mut samples) => {
                samples.sort_by(|a, b| a.0.cmp(&b.0));
                let mut expected = case["result"]["vector"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| {
                        (
                            serde_json::from_value::<std::collections::BTreeMap<String, String>>(
                                v["labels"].clone(),
                            )
                            .unwrap(),
                            v["value"].as_f64().unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                expected.sort_by(|a, b| a.0.cmp(&b.0));
                assert_eq!(samples.len(), expected.len(), "{expr}");
                for ((labels, value), (want_labels, want)) in samples.into_iter().zip(expected) {
                    assert_eq!(labels, want_labels, "{expr}");
                    assert!((value - want).abs() < 1e-10, "{expr}: {value} != {want}");
                }
            }
        }
    }
}
#[test]
fn unsupported_and_unbounded_expressions_are_explicit_errors() {
    for expr in [
        "avg_over_time(cpu[5m])",
        "cpu offset 1m",
        "cpu[5m:30s]",
        "cpu * on(job) group_left cpu",
        "cpu or cpu",
    ] {
        assert!(PromqlParser.parse(expr).is_err(), "{expr}");
    }
    assert!(PromqlParser.parse(&"1+".repeat(200)).is_err());
    let expr = PromqlParser.parse("cpu / ignoring(instance) cpu").unwrap();
    let series = ["a", "b"].map(|i| Series {
        labels: std::collections::BTreeMap::from([
            ("__name__".into(), "cpu".into()),
            ("instance".into(), i.into()),
        ]),
        temporality: MetricTemporality::Gauge,
        points: vec![Sample {
            time: 1,
            start_time: None,
            value: 1.0,
            stale: false,
        }],
    });
    assert!(evaluate(&expr, &series, 1)
        .unwrap_err()
        .contains("multiple series"));
}
