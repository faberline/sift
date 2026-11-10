//! Incomplete query results produce unknown observations for every alert.
use crate::alerting::application::service::{AlertRule, Observation, RuleSource};
use crate::control::application::service::{ControlService, RecordKind};
use crate::error_reporting::application::service::{ErrorGroup, ErrorStatus};
use crate::query::application::sift_api_service::SiftApiClient;
use crate::shared_kernel::{
    query_request::TimeRangeV1,
    query_v2::{QueryRequestV2, QuerySignalV2},
};
use std::collections::BTreeMap;
type Observations = BTreeMap<BTreeMap<String, String>, Observation>;

pub(super) async fn observe(
    api: &SiftApiClient,
    control: &ControlService,
    rule: &AlertRule,
    now: i64,
) -> (Observations, Option<u64>) {
    match complete_observation(api, control, rule, now).await {
        Ok(result) => result,
        Err(reason) => (
            BTreeMap::from([(BTreeMap::new(), Observation::Unknown(reason))]),
            None,
        ),
    }
}
async fn complete_observation(
    api: &SiftApiClient,
    control: &ControlService,
    rule: &AlertRule,
    now: i64,
) -> Result<(Observations, Option<u64>), String> {
    let mut request = QueryRequestV2 {
        version: 2,
        project: rule.project.clone(),
        environment: rule.environment.clone(),
        time_range: TimeRangeV1 {
            start: None,
            end: Some(
                chrono::DateTime::from_timestamp(now, 0)
                    .ok_or("invalid alert clock")?
                    .to_rfc3339(),
            ),
        },
        signal: QuerySignalV2::LogsCount { filter: None },
        step_seconds: None,
    };
    let (comparison, threshold) = match &rule.source {
        RuleSource::Metrics {
            expression,
            comparison,
            threshold,
        } => {
            request.signal = QuerySignalV2::Metrics {
                expression: expression.clone(),
            };
            (*comparison, *threshold)
        }
        RuleSource::LogsCount {
            window_seconds,
            filter,
            comparison,
            threshold,
        } => {
            request.time_range.start = Some(
                chrono::DateTime::from_timestamp(
                    now.checked_sub(*window_seconds as i64)
                        .ok_or("alert time underflow")?,
                    0,
                )
                .ok_or("invalid alert window")?
                .to_rfc3339(),
            );
            request.signal = QuerySignalV2::LogsCount {
                filter: filter.clone(),
            };
            (*comparison, *threshold)
        }
        RuleSource::NewError | RuleSource::ErrorRecurrence => {
            let groups = control.list(&rule.project, RecordKind::ErrorGroup).await?;
            let mut observations = Observations::new();
            let mut cursor = 0;
            for record in groups {
                let Some(value) = record.value else {
                    continue;
                };
                let group: ErrorGroup = serde_json::from_value(value).map_err(|e| e.to_string())?;
                if group.status == ErrorStatus::Ignored
                    || rule
                        .environment
                        .as_ref()
                        .is_some_and(|e| *e != group.environment)
                {
                    continue;
                }
                let event = if matches!(rule.source, RuleSource::NewError) {
                    group.first_cursor
                } else {
                    group.recurrence_cursor.unwrap_or(0)
                };
                cursor = cursor.max(event);
                observations.insert(
                    BTreeMap::from([("error_group".into(), group.id)]),
                    Observation::Event(event),
                );
            }
            let _ = cursor;
            return Ok((observations, None));
        }
    };
    let response = api
        .query_v2(&request)
        .await
        .map_err(|e| e.to_string())?
        .response;
    if response.partial || response.next_cursor.is_some() {
        return Err("alert query is incomplete".into());
    }
    let mut observations = Observations::new();
    let mut insert = |labels, value: f64| -> Result<(), String> {
        if !value.is_finite() {
            return Err("alert result is not finite".into());
        }
        if observations
            .insert(
                labels,
                Observation::Condition(comparison.test(value, threshold)),
            )
            .is_some()
        {
            return Err("alert result has duplicate labels".into());
        }
        Ok(())
    };
    match &rule.source {
        RuleSource::LogsCount { .. } => {
            if response
                .data
                .get("exact")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                return Err("log count is not exact".into());
            }
            insert(
                BTreeMap::new(),
                response
                    .data
                    .get("count")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("log count is missing")? as f64,
            )?;
        }
        _ => match response
            .data
            .get("resultType")
            .and_then(serde_json::Value::as_str)
        {
            Some("scalar") => insert(BTreeMap::new(), number(&response.data["result"][1])?)?,
            Some("vector") => {
                for item in response.data["result"]
                    .as_array()
                    .ok_or("metric vector is missing")?
                {
                    insert(
                        serde_json::from_value(item["metric"].clone())
                            .map_err(|e| e.to_string())?,
                        number(&item["value"][1])?,
                    )?;
                }
            }
            _ => return Err("alert requires an instant result".into()),
        },
    }
    Ok((observations, None))
}
fn number(value: &serde_json::Value) -> Result<f64, String> {
    value
        .as_str()
        .ok_or("metric number is missing")?
        .parse::<f64>()
        .map_err(|e| e.to_string())
}
