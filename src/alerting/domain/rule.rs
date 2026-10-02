//! The first rule version has a bounded query and a Secret reference.
use crate::shared_kernel::query_expression::QueryExpressionV1;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AlertRule {
    pub version: u32,
    pub project: String,
    pub environment: Option<String>,
    pub source: RuleSource,
    #[serde(default)]
    pub hold_seconds: u64,
    #[serde(default)]
    pub no_data: NoData,
    pub webhook: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub mute_until: Option<i64>,
}
fn enabled() -> bool {
    true
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleSource {
    Metrics {
        expression: String,
        comparison: Comparison,
        threshold: f64,
    },
    LogsCount {
        window_seconds: u64,
        filter: Option<Box<QueryExpressionV1>>,
        comparison: Comparison,
        threshold: f64,
    },
    NewError,
    ErrorRecurrence,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoData {
    #[default]
    Unknown,
    Firing,
    Inactive,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Equal,
    NotEqual,
}
impl Comparison {
    pub(crate) fn test(self, value: f64, threshold: f64) -> bool {
        match self {
            Self::Greater => value > threshold,
            Self::GreaterEqual => value >= threshold,
            Self::Less => value < threshold,
            Self::LessEqual => value <= threshold,
            Self::Equal => value == threshold,
            Self::NotEqual => value != threshold,
        }
    }
}
impl AlertRule {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.project.trim().is_empty()
            || self.hold_seconds > 604_800
            || self.webhook.is_empty()
            || self.webhook.len() > 128
            || !self
                .webhook
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err("invalid rule version, project, hold duration, or Secret reference".into());
        }
        match &self.source {
            RuleSource::Metrics {
                expression,
                threshold,
                ..
            } if expression.trim().is_empty()
                || expression.len() > 16384
                || !threshold.is_finite() =>
            {
                Err("invalid metric condition".into())
            }
            RuleSource::LogsCount {
                window_seconds,
                threshold,
                ..
            } if *window_seconds == 0 || *window_seconds > 86400 || !threshold.is_finite() => {
                Err("invalid log count window or threshold".into())
            }
            RuleSource::NewError | RuleSource::ErrorRecurrence if self.hold_seconds != 0 => {
                Err("error event rules require hold_seconds=0".into())
            }
            _ => Ok(()),
        }
    }
}
