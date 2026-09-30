//! The filter expression a query carries, its depth, argument and regex bounds,
//! and the fields it may name.

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_FILTER_DEPTH: usize = 32;
pub(in crate::query) const MAX_FILTER_ARGUMENTS: usize = 64;
const MAX_REGEX_BYTES: usize = 1_024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryExpressionV1 {
    And {
        args: Vec<QueryExpressionV1>,
    },
    Or {
        args: Vec<QueryExpressionV1>,
    },
    Not {
        arg: Box<QueryExpressionV1>,
    },
    Eq {
        field: String,
        value: serde_json::Value,
    },
    In {
        field: String,
        values: Vec<serde_json::Value>,
    },
    Exists {
        field: String,
    },
    Range {
        field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gt: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gte: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lt: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lte: Option<serde_json::Value>,
    },
    Text {
        field: String,
        value: String,
    },
    Regex {
        field: String,
        pattern: String,
    },
}

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("filter is nested too deeply")]
    TooDeep,
    #[error("and/or requires between 1 and {max} arguments", max = MAX_FILTER_ARGUMENTS)]
    ArgumentCount,
    #[error("in requires between 1 and {max} values", max = MAX_FILTER_ARGUMENTS)]
    ValueCount,
    #[error("range requires at least one bound")]
    NoRangeBound,
    #[error("range cannot contain both gt and gte")]
    GtAndGte,
    #[error("range cannot contain both lt and lte")]
    LtAndLte,
    #[error("text value must not be empty")]
    EmptyText,
    #[error("regex pattern must contain 1 to {max} bytes", max = MAX_REGEX_BYTES)]
    RegexLength,
    #[error("invalid regex pattern")]
    InvalidRegex(#[source] regex::Error),
    #[error("field must contain 1 to 256 bytes")]
    FieldLength,
}

impl QueryExpressionV1 {
    pub(in crate::query) fn validate(&self, depth: usize) -> Result<(), FilterError> {
        if depth > MAX_FILTER_DEPTH {
            return Err(FilterError::TooDeep);
        }
        match self {
            Self::And { args } | Self::Or { args } => {
                if args.is_empty() || args.len() > MAX_FILTER_ARGUMENTS {
                    return Err(FilterError::ArgumentCount);
                }
                for arg in args {
                    arg.validate(depth + 1)?;
                }
            }
            Self::Not { arg } => arg.validate(depth + 1)?,
            Self::Eq { field, .. } | Self::Exists { field } => validate_field(field)?,
            Self::In { field, values } => {
                validate_field(field)?;
                if values.is_empty() || values.len() > MAX_FILTER_ARGUMENTS {
                    return Err(FilterError::ValueCount);
                }
            }
            Self::Range {
                field,
                gt,
                gte,
                lt,
                lte,
            } => {
                validate_field(field)?;
                if [gt, gte, lt, lte].into_iter().all(Option::is_none) {
                    return Err(FilterError::NoRangeBound);
                }
                if gt.is_some() && gte.is_some() {
                    return Err(FilterError::GtAndGte);
                }
                if lt.is_some() && lte.is_some() {
                    return Err(FilterError::LtAndLte);
                }
            }
            Self::Text { field, value } => {
                validate_field(field)?;
                if value.trim().is_empty() {
                    return Err(FilterError::EmptyText);
                }
            }
            Self::Regex { field, pattern } => {
                validate_field(field)?;
                if pattern.is_empty() || pattern.len() > MAX_REGEX_BYTES {
                    return Err(FilterError::RegexLength);
                }
                Regex::new(pattern).map_err(FilterError::InvalidRegex)?;
            }
        }
        Ok(())
    }
}

pub(in crate::query) fn validate_field(field: &str) -> Result<(), FilterError> {
    if field.trim().is_empty() || field.len() > 256 {
        return Err(FilterError::FieldLength);
    }
    Ok(())
}
