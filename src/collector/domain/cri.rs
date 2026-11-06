//! The CRI log format: a frame's timestamp, stream and full or partial tag, the
//! workload a pod log path names, and the resource and attributes every record
//! from that workload carries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::collector::domain::config::CriSourceConfig;
use crate::collector::domain::record::RecordEnrichment;
use crate::AttributeValue;

const MAX_WORKLOAD_ID_BYTES: usize = 253;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::collector) struct WorkloadIdentity {
    namespace: String,
    pod: String,
    pod_uid: String,
    container: String,
    restart: u32,
}

#[derive(Clone, Debug)]
pub(in crate::collector) struct DiscoveredFile {
    pub(in crate::collector) identity: String,
    pub(in crate::collector) path: PathBuf,
    pub(in crate::collector) relative_path: String,
    pub(in crate::collector) len: u64,
    pub(in crate::collector) workload: WorkloadIdentity,
    pub(in crate::collector) known_before: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::collector) enum CriStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::collector) enum CriTag {
    Full,
    Partial,
}

pub(in crate::collector) struct CriFrame {
    pub(in crate::collector) stream: CriStream,
    pub(in crate::collector) tag: CriTag,
    pub(in crate::collector) content: Vec<u8>,
}

#[derive(Debug, Error)]
pub(in crate::collector) enum CriError {
    #[error("CRI record is missing timestamp delimiter")]
    MissingTimestampDelimiter,
    #[error("CRI record is missing stream delimiter")]
    MissingStreamDelimiter,
    #[error("CRI record is missing tag delimiter")]
    MissingTagDelimiter,
    #[error("CRI timestamp must be UTF-8")]
    TimestampNotUtf8(#[source] std::str::Utf8Error),
    #[error("CRI timestamp must be RFC3339")]
    TimestampNotRfc3339(#[source] chrono::ParseError),
    #[error("CRI stream must be stdout or stderr")]
    InvalidStream,
    #[error("CRI tag must be F or P")]
    InvalidTag,
    #[error("CRI restart index must be u32")]
    InvalidRestartIndex(#[source] std::num::ParseIntError),
    #[error("invalid CRI {0}")]
    InvalidWorkloadId(String),
}

pub(in crate::collector) fn parse_cri_frame(line: &[u8]) -> Result<CriFrame, CriError> {
    let line = trim_line_ending(line);
    let mut spaces = line
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b' ').then_some(index));
    let first = spaces.next().ok_or(CriError::MissingTimestampDelimiter)?;
    let second = spaces.next().ok_or(CriError::MissingStreamDelimiter)?;
    let third = spaces.next().ok_or(CriError::MissingTagDelimiter)?;
    let timestamp = std::str::from_utf8(&line[..first]).map_err(CriError::TimestampNotUtf8)?;
    DateTime::parse_from_rfc3339(timestamp).map_err(CriError::TimestampNotRfc3339)?;
    let stream = match &line[first + 1..second] {
        b"stdout" => CriStream::Stdout,
        b"stderr" => CriStream::Stderr,
        _ => return Err(CriError::InvalidStream),
    };
    let tag = match &line[second + 1..third] {
        b"F" => CriTag::Full,
        b"P" => CriTag::Partial,
        _ => return Err(CriError::InvalidTag),
    };
    Ok(CriFrame {
        stream,
        tag,
        content: line[third + 1..].to_vec(),
    })
}

fn trim_line_ending(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n")
        .unwrap_or(line)
        .strip_suffix(b"\r")
        .unwrap_or_else(|| line.strip_suffix(b"\n").unwrap_or(line))
}

pub(in crate::collector) fn enrichment(
    config: &CriSourceConfig,
    file: &DiscoveredFile,
    stream: CriStream,
) -> RecordEnrichment {
    let mut resource = BTreeMap::from([
        ("gcp.resource.type".to_string(), "k8s_container".to_string()),
        (
            "gcp.project_id".to_string(),
            config.metadata.gcp_project.clone(),
        ),
        (
            "gcp.resource.label.project_id".to_string(),
            config.metadata.gcp_project.clone(),
        ),
        (
            "gcp.resource.label.namespace_name".to_string(),
            file.workload.namespace.clone(),
        ),
        (
            "gcp.resource.label.pod_name".to_string(),
            file.workload.pod.clone(),
        ),
        (
            "gcp.resource.label.container_name".to_string(),
            file.workload.container.clone(),
        ),
        (
            "k8s.namespace.name".to_string(),
            file.workload.namespace.clone(),
        ),
        ("k8s.pod.name".to_string(), file.workload.pod.clone()),
        ("k8s.pod.uid".to_string(), file.workload.pod_uid.clone()),
        (
            "k8s.container.name".to_string(),
            file.workload.container.clone(),
        ),
    ]);
    if let Some(cluster) = &config.metadata.cluster {
        resource.insert("k8s.cluster.name".to_string(), cluster.clone());
        resource.insert(
            "gcp.resource.label.cluster_name".to_string(),
            cluster.clone(),
        );
    }
    if let Some(location) = &config.metadata.location {
        resource.insert("cloud.region".to_string(), location.clone());
        resource.insert("gcp.resource.label.location".to_string(), location.clone());
    }
    if let Some(node) = &config.metadata.node {
        resource.insert("k8s.node.name".to_string(), node.clone());
    }
    RecordEnrichment {
        resource,
        attributes: BTreeMap::from([
            (
                "collector.stream".to_string(),
                AttributeValue::String(
                    match stream {
                        CriStream::Stdout => "stdout",
                        CriStream::Stderr => "stderr",
                    }
                    .to_string(),
                ),
            ),
            (
                "k8s.container.restart_count".to_string(),
                AttributeValue::Int(i64::from(file.workload.restart)),
            ),
        ]),
        cloud_logging_coexistence: true,
    }
}

pub(in crate::collector) fn parse_workload_path(
    relative: &Path,
) -> Result<Option<WorkloadIdentity>, CriError> {
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if components.len() != 3 {
        return Ok(None);
    }
    let mut pod = components[0].splitn(3, '_');
    let Some(namespace) = pod.next() else {
        return Ok(None);
    };
    let Some(pod_name) = pod.next() else {
        return Ok(None);
    };
    let Some(pod_uid) = pod.next() else {
        return Ok(None);
    };
    let container = &components[1];
    let Some(restart_text) = components[2].split(".log").next() else {
        return Ok(None);
    };
    if !components[2].contains(".log") || restart_text.is_empty() {
        return Ok(None);
    }
    for (name, value) in [
        ("namespace", namespace),
        ("pod", pod_name),
        ("pod uid", pod_uid),
        ("container", container.as_str()),
    ] {
        validate_workload_id(name, value)?;
    }
    let restart = restart_text
        .parse::<u32>()
        .map_err(CriError::InvalidRestartIndex)?;
    Ok(Some(WorkloadIdentity {
        namespace: namespace.to_string(),
        pod: pod_name.to_string(),
        pod_uid: pod_uid.to_string(),
        container: container.clone(),
        restart,
    }))
}

fn validate_workload_id(name: &str, value: &str) -> Result<(), CriError> {
    if value.is_empty()
        || value.len() > MAX_WORKLOAD_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(CriError::InvalidWorkloadId(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_and_partial_cri_frames_without_touching_json() {
        let full = parse_cri_frame(b"2026-07-17T10:00:00.123456789Z stdout F {\"x\":1}\n").unwrap();
        assert_eq!(full.stream, CriStream::Stdout);
        assert_eq!(full.tag, CriTag::Full);
        assert_eq!(full.content, br#"{"x":1}"#);

        let partial = parse_cri_frame(b"2026-07-17T10:00:00Z stderr P {\"x\"").unwrap();
        assert_eq!(partial.stream, CriStream::Stderr);
        assert_eq!(partial.tag, CriTag::Partial);
        assert_eq!(partial.content, br#"{"x""#);
        assert!(parse_cri_frame(b"bad stdout F {}\n").is_err());
    }

    #[test]
    fn parses_standard_and_rotated_pod_log_paths() {
        let identity = parse_workload_path(Path::new(
            "prod_checkout-7_1234-abcd/lumen/0.log.20260717-100000",
        ))
        .unwrap()
        .unwrap();
        assert_eq!(identity.namespace, "prod");
        assert_eq!(identity.pod, "checkout-7");
        assert_eq!(identity.pod_uid, "1234-abcd");
        assert_eq!(identity.container, "lumen");
        assert_eq!(identity.restart, 0);
        assert!(parse_workload_path(Path::new("escape.log"))
            .unwrap()
            .is_none());
    }
}
