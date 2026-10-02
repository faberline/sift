//! Load a fixed prefix and expose native histograms as classic families.
use super::{
    archive_query_status::ArchiveQueryStatus,
    prom_query_params::nanos_rfc3339,
    replay_cold_query::{replay_cold_query, ColdQueryScope},
};
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::projection::{MetricQuery, MetricSeriesResultV1, PROJECTION_METRIC_STORE};
use crate::query::domain::metric_expression::{Expr, Labels, Sample, Series};
use crate::shared_kernel::{
    event::{AttributeValue, SignalKind},
    query_v2::QueryRequestV2,
};
use std::collections::{BTreeMap, BTreeSet};

fn label_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
fn labels(series: &MetricSeriesResultV1) -> Result<Labels, ApiError> {
    let mut out = Labels::new();
    for (key, value) in series
        .resource
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .chain(series.attributes.iter().filter_map(|(k, v)| {
            Some((
                k.clone(),
                match v {
                    AttributeValue::String(v) => v.clone(),
                    AttributeValue::Int(v) => v.to_string(),
                    AttributeValue::Double(v) => v.to_string(),
                    AttributeValue::Bool(v) => v.to_string(),
                    _ => return None,
                },
            ))
        }))
    {
        let key = label_name(&key);
        if out.insert(key.clone(), value).is_some() {
            return Err(ApiError::bad_request(
                "invalid_query",
                format!("metric label normalization collision: {key}"),
            ));
        }
    }
    out.insert("__name__".into(), series.name.clone());
    Ok(out)
}

fn expand(series: &MetricSeriesResultV1, watermark: u64) -> Result<Vec<Series>, ApiError> {
    if series.overflow {
        return Err(ApiError::temporarily_unavailable(
            "metric cardinality overflow makes the result incomplete",
        ));
    }
    let base_labels = labels(series)?;
    let mut out: BTreeMap<Labels, Vec<Sample>> = BTreeMap::new();
    for point in series.points.iter().filter(|p| p.cursor <= watermark) {
        let make = |value| Sample {
            time: point.time_unix_nano,
            start_time: point.start_time_unix_nano,
            value,
            stale: point.stale,
        };
        if let Some(hist) = &point.histogram {
            if hist.kind != crate::shared_kernel::projection_metric::HistogramKind::Explicit {
                return Err(ApiError::bad_request(
                    "unsupported_query",
                    "V2 histogram queries require explicit buckets",
                ));
            }
            let mut total = 0u64;
            for (index, count) in hist.bucket_counts.iter().enumerate() {
                total = total
                    .checked_add(*count)
                    .ok_or_else(|| ApiError::internal("histogram bucket overflow"))?;
                let mut labels = base_labels.clone();
                labels.insert("__name__".into(), format!("{}_bucket", series.name));
                labels.insert(
                    "le".into(),
                    hist.explicit_bounds
                        .get(index)
                        .map(f64::to_string)
                        .unwrap_or_else(|| "+Inf".into()),
                );
                out.entry(labels).or_default().push(make(total as f64));
            }
            for (suffix, value) in [("count", hist.count as f64), ("sum", hist.sum)] {
                if suffix == "sum" && !hist.sum_present {
                    continue;
                }
                let mut labels = base_labels.clone();
                labels.insert("__name__".into(), format!("{}_{suffix}", series.name));
                out.entry(labels).or_default().push(make(value));
            }
        } else {
            out.entry(base_labels.clone())
                .or_default()
                .push(make(point.value));
        }
    }
    Ok(out
        .into_iter()
        .map(|(labels, points)| Series {
            labels,
            points,
            temporality: series.temporality,
        })
        .collect())
}

pub(crate) fn load_metrics(
    state: &ServiceState,
    request: &QueryRequestV2,
    expr: &Expr,
    start: i64,
    end: i64,
) -> Result<(Vec<Series>, u64, usize), ApiError> {
    let watermark = state.journal().last_cursor();
    state
        .projections()
        .catch_up(PROJECTION_METRIC_STORE)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if state
        .projections()
        .current_cursor(PROJECTION_METRIC_STORE)
        .map_err(|e| ApiError::internal(e.to_string()))?
        < watermark
    {
        return Err(ApiError::temporarily_unavailable(
            "metric projection has not reached the query watermark",
        ));
    }
    let mut selectors = Vec::new();
    expr.selectors(&mut selectors);
    let lookback = selectors
        .iter()
        .map(|s| s.window_nanos.unwrap_or(300_000_000_000))
        .max()
        .unwrap_or(0);
    let start = start
        .checked_sub(lookback)
        .ok_or_else(|| ApiError::bad_request("invalid_query", "metric lookback overflow"))?;
    let start_time =
        nanos_rfc3339(start).map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?;
    let end_time = nanos_rfc3339(
        end.checked_add(1)
            .ok_or_else(|| ApiError::bad_request("invalid_query", "metric end overflow"))?,
    )
    .map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?;
    let replay = if start
        < (chrono::Utc::now() - chrono::Duration::minutes(15))
            .timestamp_nanos_opt()
            .unwrap_or(i64::MIN)
    {
        let replay = state.projections().new_metric_replay();
        match replay_cold_query(
            state,
            &ColdQueryScope {
                project: &request.project,
                environment: request.environment.as_deref(),
                start: Some(&start_time),
                end: Some(&end_time),
            },
            SignalKind::Metric,
            &replay,
        ) {
            ArchiveQueryStatus::Ready(source) if source.replayed <= 250_000 => Some(replay),
            ArchiveQueryStatus::Ready(_) => {
                return Err(ApiError::bad_request(
                    "query_limit",
                    "archive metric replay exceeds 250000 source points",
                ))
            }
            ArchiveQueryStatus::Unavailable(message) => {
                return Err(ApiError::temporarily_unavailable(message))
            }
            ArchiveQueryStatus::NotRequired => None,
        }
    } else {
        None
    };
    let mut names = BTreeSet::new();
    for selector in &selectors {
        names.insert(selector.name.clone());
        if let Some(name) = &selector.name {
            for suffix in ["_bucket", "_count", "_sum"] {
                if let Some(root) = name.strip_suffix(suffix) {
                    names.insert(Some(root.into()));
                }
            }
        }
    }
    if names.contains(&None) {
        names = BTreeSet::from([None]);
    }
    let (mut out, mut scanned, mut bytes) = (Vec::new(), 0usize, 0usize);
    for name in names {
        let query: MetricQuery = serde_json::from_value(serde_json::json!({
            "project":request.project,"environment":request.environment,"name":name,
            "start_time":start_time,"end_time":end_time,"limit":1000
        }))
        .map_err(|e| ApiError::internal(e.to_string()))?;
        let page = if let Some(replay) = &replay {
            replay.query(&query)
        } else {
            state.projections().query_metrics(&query)
        }
        .map_err(|e| ApiError::bad_request("query_limit", e.to_string()))?;
        if page.incomplete || page.overflowed_points > 0 {
            return Err(ApiError::temporarily_unavailable(
                "metric retention or cardinality makes the source incomplete",
            ));
        }
        if page.has_more {
            return Err(ApiError::bad_request(
                "query_limit",
                "metric source exceeds 1000 series",
            ));
        }
        for series in page.series {
            scanned = scanned.saturating_add(series.points.len());
            bytes = bytes.saturating_add(
                serde_json::to_vec(&series)
                    .map_err(|e| ApiError::internal(e.to_string()))?
                    .len(),
            );
            out.extend(expand(&series, watermark)?);
        }
        if scanned > 250_000
            || bytes > 64 * 1024 * 1024
            || out.len() > 1000
            || out.iter().map(|s| s.points.len()).sum::<usize>() > 250_000
        {
            return Err(ApiError::bad_request(
                "query_limit",
                "metric source or histogram expansion exceeds the query budget",
            ));
        }
    }
    // A selector can load the same family through more than one expression.
    let mut merged: BTreeMap<Labels, Series> = BTreeMap::new();
    for series in out {
        if let Some(existing) = merged.get(&series.labels) {
            if existing.temporality != series.temporality
                || existing.points.len() != series.points.len()
                || existing.points.iter().zip(&series.points).any(|(a, b)| {
                    a.time != b.time
                        || a.value != b.value
                        || a.stale != b.stale
                        || a.start_time != b.start_time
                })
            {
                return Err(ApiError::bad_request(
                    "invalid_query",
                    "conflicting series share the same normalized labels",
                ));
            }
        } else {
            merged.insert(series.labels.clone(), series);
        }
    }
    Ok((merged.into_values().collect(), watermark, scanned))
}
