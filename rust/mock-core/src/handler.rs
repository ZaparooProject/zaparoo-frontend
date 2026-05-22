// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// JSON-RPC 2.0 envelope parsing and response assembly. Response shapes
// mirror the upstream Core API: https://zaparoo.org/docs/core/api/

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::fixtures;

#[derive(Deserialize)]
struct RpcRequest {
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

const FALLBACK_INTERNAL_ERROR: &str = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal serialization error"}}"#;

pub fn dispatch(text: &str) -> String {
    let req: RpcRequest = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            warn!("parse error: {e}");
            return encode(&RpcResponse {
                jsonrpc: "2.0",
                id: Value::Null,
                result: None,
                error: Some(RpcError {
                    code: -32700,
                    message: format!("parse error: {e}"),
                }),
            });
        }
    };

    debug!(method = %req.method, "rpc");

    let result = match req.method.as_str() {
        "systems" => Some(fixtures::systems_response()),
        "launchers" => Some(fixtures::launchers_response()),
        "settings" => Some(fixtures::settings_response()),
        "settings.update" => Some(fixtures::settings_update_response(&req.params)),
        "media.search" => Some(fixtures::media_search_response(&req.params)),
        "media.browse" => Some(fixtures::media_browse_response(&req.params)),
        "media.history" => Some(fixtures::media_history_response(&req.params)),
        "run" => {
            let zap_script = req.params.get("text").and_then(Value::as_str).unwrap_or("");
            info!(%zap_script, "run");
            // Upstream returns null on success.
            Some(Value::Null)
        }
        "readers.write" => {
            let zap_script = req.params.get("text").and_then(Value::as_str).unwrap_or("");
            info!(%zap_script, "readers.write");
            Some(Value::Null)
        }
        "version" => Some(fixtures::version_response()),
        _ => None,
    };

    let response = match result {
        Some(r) => RpcResponse {
            jsonrpc: "2.0",
            id: req.id.unwrap_or(Value::Null),
            result: Some(r),
            error: None,
        },
        None => RpcResponse {
            jsonrpc: "2.0",
            id: req.id.unwrap_or(Value::Null),
            result: None,
            error: Some(RpcError {
                code: -32601,
                message: format!("method not found: {}", req.method),
            }),
        },
    };

    encode(&response)
}

fn encode(response: &RpcResponse) -> String {
    serde_json::to_string(response).unwrap_or_else(|_| FALLBACK_INTERNAL_ERROR.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use serde_json::Value;

    use super::dispatch;

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).expect("dispatch output must be valid JSON")
    }

    #[test]
    fn unknown_method_returns_jsonrpc_error() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"not.a.method"}"#;
        let resp = parse(&dispatch(req));
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], "1");
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn systems_returns_fixture_catalog() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"systems","params":{}}"#;
        let resp = parse(&dispatch(req));
        let systems = resp["result"]["systems"].as_array().expect("array");
        assert_eq!(systems.len(), 10);
    }

    #[test]
    fn launchers_returns_fixture_launchers() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"launchers","params":{}}"#;
        let resp = parse(&dispatch(req));
        let launchers = resp["result"]["launchers"].as_array().expect("array");
        assert!(!launchers.is_empty());
        assert!(launchers.iter().any(|l| l["systemId"] == "SNES"));
    }

    #[test]
    fn settings_update_replaces_system_defaults() {
        let update = r#"{"jsonrpc":"2.0","id":"1","method":"settings.update","params":{"systemDefaults":[{"system":"NES","launcher":"nestopia"}]}}"#;
        let resp = parse(&dispatch(update));
        assert!(resp["result"].is_null());

        let req = r#"{"jsonrpc":"2.0","id":"2","method":"settings","params":{}}"#;
        let resp = parse(&dispatch(req));
        let defaults = resp["result"]["systemDefaults"].as_array().expect("array");
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0]["system"], "NES");
        assert_eq!(defaults[0]["launcher"], "nestopia");
    }

    #[test]
    fn media_search_filters_by_system() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":["NES"],"maxResults":100}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|g| g["system"]["id"].as_str() == Some("NES")));
    }

    #[test]
    fn media_search_respects_max_results() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":[],"maxResults":3}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn media_search_emits_pagination_envelope() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":[],"maxResults":50}}"#;
        let resp = parse(&dispatch(req));
        let pagination = resp["result"]["pagination"]
            .as_object()
            .expect("pagination object");
        assert_eq!(pagination["hasNextPage"], Value::Bool(false));
        assert_eq!(pagination["pageSize"], Value::from(50));
        // Deprecated but still present for backward compatibility.
        assert_eq!(resp["result"]["total"], Value::from(-1));
    }

    #[test]
    fn media_browse_emits_media_entries_with_path_and_total() {
        let req =
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"path":"/games"}}"#;
        let resp = parse(&dispatch(req));
        assert_eq!(resp["result"]["path"], Value::from("/games"));
        let entries = resp["result"]["entries"].as_array().expect("array");
        assert!(!entries.is_empty());
        for entry in entries {
            assert_eq!(entry["type"], Value::from("media"));
            assert!(entry["systemId"].is_string());
            assert!(entry["zapScript"].is_string());
            assert!(entry["relativePath"].is_string());
        }
        assert!(resp["result"]["totalFiles"].is_number());
        assert!(resp["result"]["pagination"].is_object());
    }

    #[test]
    fn run_accepts_any_text_and_returns_null() {
        let req =
            r#"{"jsonrpc":"2.0","id":"1","method":"run","params":{"text":"**launch.system:nes"}}"#;
        let resp = parse(&dispatch(req));
        assert!(resp["result"].is_null());
    }

    #[test]
    fn readers_write_accepts_text_and_returns_null() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"readers.write","params":{"text":"**launch.system:nes"}}"#;
        let resp = parse(&dispatch(req));
        assert!(resp["result"].is_null());
    }

    #[test]
    fn parse_error_returns_null_id() {
        let resp = parse(&dispatch("this is not json"));
        assert_eq!(resp["id"], Value::Null);
        assert_eq!(resp["error"]["code"], -32700);
    }

    #[test]
    fn media_history_returns_entries_with_pagination() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.history","params":{"limit":5}}"#;
        let resp = parse(&dispatch(req));
        let entries = resp["result"]["entries"].as_array().expect("array");
        assert!(!entries.is_empty());
        for entry in entries {
            assert!(entry["mediaName"].is_string());
            assert!(entry["mediaPath"].is_string());
            assert!(entry["systemId"].is_string());
            assert!(entry["systemName"].is_string());
            assert!(entry["launcherId"].is_string());
        }
        let pagination = resp["result"]["pagination"]
            .as_object()
            .expect("pagination object");
        assert_eq!(pagination["hasNextPage"], Value::Bool(false));
    }

    #[test]
    fn media_history_omits_pagination_when_no_entries() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.history","params":{"systems":["DoesNotExist"]}}"#;
        let resp = parse(&dispatch(req));
        let entries = resp["result"]["entries"].as_array().expect("array");
        assert!(entries.is_empty());
        assert!(resp["result"].get("pagination").is_none());
    }
}
