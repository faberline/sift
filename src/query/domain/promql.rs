//! The PromQL subset Sift answers: its functions and how a query parses.

use std::collections::BTreeMap;

use regex::Regex;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromFunction {
    Raw,
    Sum,
    Avg,
    Min,
    Max,
    Count,
    Rate,
}

#[derive(Clone, Debug)]
pub struct ParsedPromQuery {
    pub metric: String,
    pub labels: BTreeMap<String, String>,
    pub function: PromFunction,
}

#[derive(Debug, Error)]
pub enum PromQlError {
    #[error("query must not be empty")]
    EmptyQuery,
    #[error("unsupported PromQL; expected a metric selector or sum/avg/min/max/count/rate")]
    UnsupportedQuery,
    #[error("unsupported label matcher `{0}`")]
    UnsupportedLabelMatcher(String),
    #[error("escaped PromQL label values are not supported in phase one")]
    EscapedLabelValue,
    #[error("decimal seconds must contain digits")]
    NoDigits,
    #[error("decimal exponent is invalid")]
    InvalidExponent(#[source] std::num::ParseIntError),
    #[error("decimal seconds contain more than one exponent")]
    SeveralExponents,
    #[error("decimal seconds have an invalid mantissa")]
    InvalidMantissa,
    #[error("decimal seconds exceed the supported precision")]
    PrecisionParse(#[source] std::num::ParseIntError),
    #[error("decimal seconds exceed the supported precision")]
    PrecisionConversion(#[source] std::num::TryFromIntError),
    #[error("decimal seconds are outside nanosecond range")]
    NanosecondRange,
    #[error("decimal seconds are outside nanosecond range")]
    NanosecondRangeConversion(#[source] std::num::TryFromIntError),
    #[error(transparent)]
    Regex(#[from] regex::Error),
}

pub fn parse_promql(input: &str) -> Result<ParsedPromQuery, PromQlError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(PromQlError::EmptyQuery);
    }
    let function = Regex::new(r"^(sum|avg|min|max|count|rate)\((.*)\)$")?;
    let (function, selector) = match function.captures(input) {
        Some(captures) => {
            let function = match &captures[1] {
                "sum" => PromFunction::Sum,
                "avg" => PromFunction::Avg,
                "min" => PromFunction::Min,
                "max" => PromFunction::Max,
                "count" => PromFunction::Count,
                "rate" => PromFunction::Rate,
                _ => unreachable!(),
            };
            (function, captures[2].trim().to_string())
        }
        None => (PromFunction::Raw, input.to_string()),
    };
    let selector_pattern = Regex::new(r#"^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})?$"#)?;
    let captures = selector_pattern
        .captures(&selector)
        .ok_or(PromQlError::UnsupportedQuery)?;
    let metric = captures[1].to_string();
    let mut labels = BTreeMap::new();
    if let Some(matchers) = captures.get(2).map(|value| value.as_str()) {
        let matcher = Regex::new(r#"^\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*=\s*\"([^\"]*)\"\s*$"#)?;
        for part in split_matchers(matchers)? {
            let captures = matcher
                .captures(part)
                .ok_or_else(|| PromQlError::UnsupportedLabelMatcher(part.to_string()))?;
            labels.insert(captures[1].to_string(), captures[2].to_string());
        }
    }
    Ok(ParsedPromQuery {
        metric,
        labels,
        function,
    })
}

fn split_matchers(input: &str) -> Result<Vec<&str>, PromQlError> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    if input.contains('\\') {
        return Err(PromQlError::EscapedLabelValue);
    }
    Ok(input.split(',').collect())
}

pub(in crate::query) fn parse_decimal_seconds_nanos(value: &str) -> Result<i64, PromQlError> {
    let (negative, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    if unsigned.is_empty() {
        return Err(PromQlError::NoDigits);
    }
    let mut exponent_parts = unsigned.split(['e', 'E']);
    let mantissa = exponent_parts.next().unwrap_or_default();
    let exponent = exponent_parts
        .next()
        .map(str::parse::<i32>)
        .transpose()
        .map_err(PromQlError::InvalidExponent)?
        .unwrap_or(0);
    if exponent_parts.next().is_some() {
        return Err(PromQlError::SeveralExponents);
    }
    let mut mantissa_parts = mantissa.split('.');
    let integer = mantissa_parts.next().unwrap_or_default();
    let fraction = mantissa_parts.next().unwrap_or_default();
    if mantissa_parts.next().is_some()
        || (integer.is_empty() && fraction.is_empty())
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(PromQlError::InvalidMantissa);
    }
    let digits = format!("{integer}{fraction}");
    let coefficient = digits
        .parse::<i128>()
        .map_err(PromQlError::PrecisionParse)?;
    if coefficient == 0 {
        return Ok(0);
    }
    let fraction_digits =
        i128::try_from(fraction.len()).map_err(PromQlError::PrecisionConversion)?;
    let power = i128::from(exponent) - fraction_digits + 9;
    let magnitude = if power >= 0 {
        let power = u32::try_from(power).map_err(PromQlError::NanosecondRangeConversion)?;
        coefficient
            .checked_mul(
                10_i128
                    .checked_pow(power)
                    .ok_or(PromQlError::NanosecondRange)?,
            )
            .ok_or(PromQlError::NanosecondRange)?
    } else {
        let divisor_power =
            u32::try_from(-power).map_err(PromQlError::NanosecondRangeConversion)?;
        let Some(divisor) = 10_i128.checked_pow(divisor_power) else {
            return Ok(0);
        };
        let quotient = coefficient / divisor;
        let remainder = coefficient % divisor;
        quotient + i128::from(remainder >= (divisor + 1) / 2)
    };
    let nanos = if negative {
        magnitude
            .checked_neg()
            .ok_or(PromQlError::NanosecondRange)?
    } else {
        magnitude
    };
    i64::try_from(nanos).map_err(PromQlError::NanosecondRangeConversion)
}
