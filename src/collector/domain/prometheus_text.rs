//! Prometheus text 0.0.4 samples, including quoted label values.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScrapedSample {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub value: f64,
    pub timestamp_millis: Option<i64>,
    pub counter: bool,
}
pub(crate) fn parse_text(text: &str) -> Result<Vec<ScrapedSample>, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("scrape response exceeds 8 MiB".into());
    }
    let mut kinds = BTreeMap::new();
    let mut samples = Vec::new();
    let mut identities = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if let Some(metadata) = line.strip_prefix("# TYPE ") {
                let values = metadata.split_whitespace().collect::<Vec<_>>();
                if values.len() != 2
                    || !["counter", "gauge", "histogram", "summary", "untyped"].contains(&values[1])
                    || kinds
                        .insert(values[0].to_string(), values[1].to_string())
                        .is_some()
                {
                    return Err(format!("invalid TYPE at line {}", index + 1));
                }
            }
            continue;
        }
        let mut chars = line.char_indices().peekable();
        let stop = chars
            .find(|(_, c)| *c == '{' || c.is_whitespace())
            .map_or(line.len(), |(i, _)| i);
        let name = &line[..stop];
        if name.is_empty()
            || !name.bytes().enumerate().all(|(i, b)| {
                b.is_ascii_alphabetic() || b"_:".contains(&b) || (i > 0 && b.is_ascii_digit())
            })
        {
            return Err(format!("invalid metric name at line {}", index + 1));
        }
        let (labels, rest) = if line[stop..].starts_with('{') {
            parse_labels(&line[stop + 1..])?
        } else {
            (BTreeMap::new(), &line[stop..])
        };
        let values = rest.split_whitespace().collect::<Vec<_>>();
        if values.is_empty() || values.len() > 2 {
            return Err(format!("invalid metric sample at line {}", index + 1));
        }
        let value: f64 = values[0].parse().map_err(|_| "invalid metric number")?;
        if !value.is_finite() {
            return Err("nonfinite scrape samples are not supported".into());
        }
        let timestamp_millis = values
            .get(1)
            .map(|v| v.parse::<i64>().map_err(|_| "invalid sample timestamp"))
            .transpose()?;
        if !identities.insert((name.to_string(), labels.clone())) {
            return Err("duplicate sample identity in one scrape".into());
        }
        let counter = kinds.get(name).is_some_and(|kind| kind == "counter")
            || ["_bucket", "_count", "_sum"].iter().any(|suffix| {
                name.strip_suffix(suffix)
                    .and_then(|root| kinds.get(root))
                    .is_some_and(|kind| kind == "histogram")
            });
        samples.push(ScrapedSample {
            name: name.into(),
            labels,
            value,
            timestamp_millis,
            counter,
        });
        if samples.len() > 100_000 {
            return Err("scrape exceeds 100000 samples".into());
        }
    }
    validate_histograms(&samples, &kinds)?;
    Ok(samples)
}
fn parse_labels(mut input: &str) -> Result<(BTreeMap<String, String>, &str), String> {
    let mut labels = BTreeMap::new();
    loop {
        input = input.trim_start();
        if let Some(rest) = input.strip_prefix('}') {
            return Ok((labels, rest));
        }
        let end = input.find('=').ok_or("missing label equals sign")?;
        let name = input[..end].trim();
        if name.is_empty()
            || !name
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
        {
            return Err("invalid label name".into());
        }
        input = input[end + 1..]
            .trim_start()
            .strip_prefix('"')
            .ok_or("label value must be quoted")?;
        let mut value = String::new();
        let mut escaped = false;
        let mut consumed = None;
        for (i, c) in input.char_indices() {
            if escaped {
                value.push(match c {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    _ => return Err("invalid label escape".into()),
                });
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                consumed = Some(i + 1);
                break;
            } else {
                value.push(c);
            }
        }
        input = &input[consumed.ok_or("unterminated label value")?..];
        if labels.insert(name.into(), value).is_some() {
            return Err("duplicate label name".into());
        }
        input = input.trim_start();
        if let Some(rest) = input.strip_prefix(',') {
            input = rest;
        } else if !input.starts_with('}') {
            return Err("missing label comma".into());
        }
    }
}
fn validate_histograms(
    samples: &[ScrapedSample],
    kinds: &BTreeMap<String, String>,
) -> Result<(), String> {
    for (name, _) in kinds.iter().filter(|(_, kind)| *kind == "histogram") {
        let mut groups: BTreeMap<BTreeMap<String, String>, Vec<(f64, f64)>> = BTreeMap::new();
        for sample in samples
            .iter()
            .filter(|s| s.name == format!("{name}_bucket"))
        {
            let mut labels = sample.labels.clone();
            let le = labels
                .remove("le")
                .ok_or("histogram bucket is missing le")?;
            let bound = if le == "+Inf" {
                f64::INFINITY
            } else {
                le.parse::<f64>().map_err(|_| "invalid histogram bound")?
            };
            if bound.is_nan() || sample.value < 0.0 || sample.value.fract() != 0.0 {
                return Err("invalid histogram bucket count".into());
            }
            groups
                .entry(labels)
                .or_default()
                .push((bound, sample.value));
        }
        for (labels, mut buckets) in groups {
            buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
            if buckets.last().is_none_or(|b| b.0 != f64::INFINITY)
                || buckets
                    .windows(2)
                    .any(|p| p[0].0 >= p[1].0 || p[0].1 > p[1].1)
            {
                return Err("histogram buckets are incomplete or not cumulative".into());
            }
            let count = samples
                .iter()
                .find(|s| s.name == format!("{name}_count") && s.labels == labels)
                .ok_or("histogram count is missing")?;
            if count.value != buckets.last().expect("histogram buckets").1 {
                return Err("histogram count differs from +Inf bucket".into());
            }
        }
    }
    Ok(())
}
