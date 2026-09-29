//! The PromQL subset Sift answers: its functions and how a query parses.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use regex::Regex;

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

pub fn parse_promql(input: &str) -> Result<ParsedPromQuery> {
    let input = input.trim();
    if input.is_empty() {
        bail!("query must not be empty");
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
        .context("unsupported PromQL; expected a metric selector or sum/avg/min/max/count/rate")?;
    let metric = captures[1].to_string();
    let mut labels = BTreeMap::new();
    if let Some(matchers) = captures.get(2).map(|value| value.as_str()) {
        let matcher = Regex::new(r#"^\s*([a-zA-Z_][a-zA-Z0-9_.]*)\s*=\s*\"([^\"]*)\"\s*$"#)?;
        for part in split_matchers(matchers)? {
            let captures = matcher
                .captures(part)
                .with_context(|| format!("unsupported label matcher `{part}`"))?;
            labels.insert(captures[1].to_string(), captures[2].to_string());
        }
    }
    Ok(ParsedPromQuery {
        metric,
        labels,
        function,
    })
}

fn split_matchers(input: &str) -> Result<Vec<&str>> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    if input.contains('\\') {
        bail!("escaped PromQL label values are not supported in phase one");
    }
    Ok(input.split(',').collect())
}

pub(in crate::query) fn parse_decimal_seconds_nanos(value: &str) -> Result<i64> {
    let (negative, unsigned) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    if unsigned.is_empty() {
        bail!("decimal seconds must contain digits");
    }
    let mut exponent_parts = unsigned.split(['e', 'E']);
    let mantissa = exponent_parts.next().unwrap_or_default();
    let exponent = exponent_parts
        .next()
        .map(str::parse::<i32>)
        .transpose()
        .context("decimal exponent is invalid")?
        .unwrap_or(0);
    if exponent_parts.next().is_some() {
        bail!("decimal seconds contain more than one exponent");
    }
    let mut mantissa_parts = mantissa.split('.');
    let integer = mantissa_parts.next().unwrap_or_default();
    let fraction = mantissa_parts.next().unwrap_or_default();
    if mantissa_parts.next().is_some()
        || (integer.is_empty() && fraction.is_empty())
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("decimal seconds have an invalid mantissa");
    }
    let digits = format!("{integer}{fraction}");
    let coefficient = digits
        .parse::<i128>()
        .context("decimal seconds exceed the supported precision")?;
    if coefficient == 0 {
        return Ok(0);
    }
    let fraction_digits =
        i128::try_from(fraction.len()).context("decimal seconds exceed the supported precision")?;
    let power = i128::from(exponent) - fraction_digits + 9;
    let magnitude = if power >= 0 {
        let power = u32::try_from(power).context("decimal seconds are outside nanosecond range")?;
        coefficient
            .checked_mul(
                10_i128
                    .checked_pow(power)
                    .context("decimal seconds are outside nanosecond range")?,
            )
            .context("decimal seconds are outside nanosecond range")?
    } else {
        let divisor_power =
            u32::try_from(-power).context("decimal seconds are outside nanosecond range")?;
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
            .context("decimal seconds are outside nanosecond range")?
    } else {
        magnitude
    };
    i64::try_from(nanos).context("decimal seconds are outside nanosecond range")
}
