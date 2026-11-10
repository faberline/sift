//! The V2 and control contracts use the same field names as their handlers.
use serde_json::json;
use utoipa::openapi::{
    path::{Operation, PathItem, PathItemType},
    OpenApi,
};
pub(super) fn extend(document: &mut OpenApi) {
    for (path, methods) in [
        ("/api/v2/query", vec!["post"]),
        ("/api/v1/recovery/data", vec!["post"]),
        ("/api/v1/alert-rules", vec!["get"]),
        ("/api/v1/alert-rules/{id}", vec!["put", "delete"]),
        ("/api/v1/alerts", vec!["get"]),
        ("/api/v1/alerts/{id}/mute", vec!["put"]),
        ("/api/v1/error-groups", vec!["get"]),
        ("/api/v1/error-groups/{id}", vec!["put"]),
        ("/api/v1/saved-queries", vec!["get"]),
        ("/api/v1/saved-queries/{id}", vec!["put", "delete"]),
    ] {
        let mut item = PathItem::default();
        for method in methods {
            let mut operation = json!({"summary":format!("{method} {path}"),"tags":["production"],
                "responses":{"200":{"description":"Complete result"},"400":{"description":"Invalid or unsupported input"},
                    "403":{"description":"Project permission is required"},"409":{"description":"Read the new revision before changing the record"},
                    "503":{"description":"Incomplete data or unavailable quorum. Retain writes and retry."}}});
            if method == "get" {
                operation["parameters"] = json!([
                    {"name":"project","in":"query","required":true,"schema":{"type":"string"}},
                    {"name":"after","in":"query","required":false,"schema":{"type":"string"}},
                    {"name":"limit","in":"query","required":false,"schema":{"type":"integer","minimum":1,"maximum":1000,"default":100}}]);
            } else {
                let mut properties = json!({"project":{"type":"string"},"request_id":{"type":"string","maxLength":256},
                    "expected_revision":{"type":"integer","minimum":0}});
                let mut required = vec!["project", "request_id", "expected_revision"];
                if path == "/api/v2/query" {
                    properties = json!({"version":{"type":"integer","enum":[2]},"project":{"type":"string"},
                        "environment":{"type":"string"},"time_range":{"type":"object","properties":{
                            "start":{"type":"string","format":"date-time"},"end":{"type":"string","format":"date-time"}}},
                        "step_seconds":{"type":"integer","minimum":1,"maximum":3600,"default":30},
                        "signal":{"oneOf":[{"type":"object","required":["kind","expression"],"properties":{
                            "kind":{"type":"string","enum":["metrics"]},"expression":{"type":"string","maxLength":16384}}},
                            {"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["logs_count"]},"filter":{"type":"object"}}}]}});
                    required = vec!["version", "project", "signal"];
                } else if path == "/api/v1/recovery/data" {
                    properties = json!({"project":{"type":"string"}});
                    required = vec!["project"];
                } else if method == "put" {
                    if path.ends_with("/mute") {
                        properties["until"] = json!({"type":"integer","nullable":true});
                    } else if path.contains("error-groups") {
                        properties["status"] =
                            json!({"type":"string","enum":["open","resolved","ignored"]});
                        required.push("status");
                    } else {
                        properties["value"] = json!({"type":"object"});
                        required.push("value");
                    }
                }
                operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{
                    "type":"object","additionalProperties":false,"required":required,"properties":properties}}}});
            }
            if path.contains("{id}") {
                operation["parameters"] =
                    json!([{ "name":"id","in":"path","required":true,"schema":{"type":"string"}}]);
            }
            let mut typed: Operation =
                serde_json::from_value(operation).expect("static production operation");
            typed.extensions = None;
            let method = match method {
                "get" => PathItemType::Get,
                "post" => PathItemType::Post,
                "put" => PathItemType::Put,
                "delete" => PathItemType::Delete,
                _ => unreachable!(),
            };
            item.operations.insert(method, typed);
        }
        document.paths.paths.insert(path.into(), item);
    }
}
