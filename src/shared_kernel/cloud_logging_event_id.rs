//! The event id of a Cloud Logging entry that has no insertId: a digest of the
//! project, the monitored resource, the timestamp and the canonical JSON
//! payload. The GCP normalizer and CRI collection derive the same id for the
//! same line.

use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn stable_id(
    project: &str,
    resource_type: &str,
    timestamp: &str,
    resource: &BTreeMap<String, String>,
    payload: &Value,
) -> String {
    let mut digest = Sha256::new();
    digest.update(project.as_bytes());
    digest.update([0]);
    digest.update(resource_type.as_bytes());
    digest.update([0]);
    digest.update(timestamp.as_bytes());
    digest.update([0]);
    for key in [
        "gcp.resource.label.project_id",
        "gcp.resource.label.location",
        "gcp.resource.label.cluster_name",
        "gcp.resource.label.namespace_name",
        "gcp.resource.label.pod_name",
        "gcp.resource.label.container_name",
    ] {
        digest.update(key.as_bytes());
        digest.update([0]);
        digest.update(resource.get(key).map(String::as_bytes).unwrap_or_default());
        digest.update([0]);
    }
    digest.update(canonical_json(payload));
    format!("gcp-log-{}", hex::encode(&digest.finalize()[..16]))
}

fn canonical_json(value: &Value) -> Vec<u8> {
    fn write(value: &Value, output: &mut Vec<u8>) {
        match value {
            Value::Object(object) => {
                output.push(b'{');
                let mut keys = object.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key)
                        .expect("JSON object key serialization");
                    output.push(b':');
                    write(&object[key], output);
                }
                output.push(b'}');
            }
            Value::Array(values) => {
                output.push(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    write(value, output);
                }
                output.push(b']');
            }
            _ => serde_json::to_writer(output, value).expect("JSON scalar serialization"),
        }
    }

    let mut output = Vec::new();
    write(value, &mut output);
    output
}
