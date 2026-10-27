//! The existing data-root manifest, with unchanged field order and serde rules.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutManifest {
    pub format_version: u32,
    pub cluster_id: String,
    pub node_id: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::LayoutManifest;

    #[test]
    fn layout_manifest_keeps_bytes_optional_field_and_unknown_field_rejection() {
        for bytes in [
            br#"{"format_version":1,"cluster_id":"cluster-fixed","node_id":"node-fixed","role":"all"}"#.as_slice(),
            br#"{"format_version":1,"cluster_id":"cluster-fixed","node_id":"node-fixed","role":"all","restored_from":"gs://bucket/fixed/manifest.json"}"#.as_slice(),
        ] {
            let value: LayoutManifest = serde_json::from_slice(bytes).unwrap();
            assert_eq!(serde_json::to_vec(&value).unwrap(), bytes);
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["unknown_field"] = serde_json::json!(true);
            assert!(serde_json::from_value::<LayoutManifest>(value).is_err());
        }
    }
}
