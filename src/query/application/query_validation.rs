//! Query request validation keeps its original order and error chains.

use crate::query::domain::filter_expression::{validate_field, MAX_FILTER_ARGUMENTS};
use crate::shared_kernel::query_request::{
    QueryRequestV1, QuerySignalV1, MAX_QUERY_LIMIT, QUERY_AST_VERSION,
};
use anyhow::{bail, Context, Result};

impl QueryRequestV1 {
    pub fn validate(&self) -> Result<()> {
        if self.version != QUERY_AST_VERSION {
            bail!(
                "unsupported query version {}; expected {}",
                self.version,
                QUERY_AST_VERSION
            );
        }
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self
            .environment
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("environment must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_QUERY_LIMIT}");
        }
        if let (Some(start), Some(end)) = (&self.time_range.start, &self.time_range.end) {
            let start = chrono::DateTime::parse_from_rfc3339(start)
                .context("time_range.start must be RFC 3339")?;
            let end = chrono::DateTime::parse_from_rfc3339(end)
                .context("time_range.end must be RFC 3339")?;
            if start >= end {
                bail!("time_range.start must be earlier than time_range.end");
            }
        } else {
            for (name, value) in [
                ("time_range.start", self.time_range.start.as_ref()),
                ("time_range.end", self.time_range.end.as_ref()),
            ] {
                if let Some(value) = value {
                    chrono::DateTime::parse_from_rfc3339(value)
                        .with_context(|| format!("{name} must be RFC 3339"))?;
                }
            }
        }
        match &self.signal {
            QuerySignalV1::Logs { filter }
            | QuerySignalV1::Metrics { filter, .. }
            | QuerySignalV1::Traces { filter, .. } => {
                if let Some(filter) = filter {
                    filter.validate(0)?;
                }
            }
        }
        if let QuerySignalV1::Metrics {
            step_seconds,
            group_by,
            ..
        } = &self.signal
        {
            if step_seconds.is_some_and(|step| step == 0) {
                bail!("step_seconds must be greater than zero");
            }
            if group_by.len() > MAX_FILTER_ARGUMENTS {
                bail!("group_by has too many fields");
            }
            for field in group_by {
                validate_field(field)?;
            }
        }
        if let QuerySignalV1::Traces {
            min_duration_ms,
            max_duration_ms,
            attributes,
            ..
        } = &self.signal
        {
            if min_duration_ms
                .zip(*max_duration_ms)
                .is_some_and(|(min, max)| min > max)
            {
                bail!("min_duration_ms must not exceed max_duration_ms");
            }
            if attributes.len() > MAX_FILTER_ARGUMENTS {
                bail!("attributes has too many fields");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::shared_kernel::query_request::QueryRequestV1;

    #[test]
    fn published_request_validation_keeps_parse_error_chains() {
        let request: QueryRequestV1 = serde_json::from_value(serde_json::json!({
            "version": 1,
            "project": "compatibility",
            "time_range": {"start": "invalid"},
            "signal": {"kind": "logs"}
        }))
        .unwrap();
        let error = request.validate().unwrap_err();
        assert_eq!(error.to_string(), "time_range.start must be RFC 3339");
        assert!(error
            .root_cause()
            .downcast_ref::<chrono::ParseError>()
            .is_some());

        let request: QueryRequestV1 = serde_json::from_value(serde_json::json!({
            "version": 1,
            "project": "compatibility",
            "signal": {"kind": "logs", "filter": {"op": "regex", "field": "body_text", "pattern": "["}}
        })).unwrap();
        let error = request.validate().unwrap_err();
        assert_eq!(error.to_string(), "invalid regex pattern");
        assert!(error.root_cause().downcast_ref::<regex::Error>().is_some());
    }
}
