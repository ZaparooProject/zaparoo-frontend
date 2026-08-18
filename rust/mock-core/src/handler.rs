// Zaparoo Frontend
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
        "systems" => Some(fixtures::systems_response(&req.params)),
        "launchers" => Some(fixtures::launchers_response()),
        "settings" => Some(fixtures::settings_response()),
        "settings.update" => Some(fixtures::settings_update_response(&req.params)),
        "media.search" => Some(fixtures::media_search_response(&req.params)),
        "media.browse" => Some(fixtures::media_browse_response(&req.params)),
        "media.browse.index" => Some(fixtures::media_browse_index_response(&req.params)),
        "media.meta" => Some(fixtures::media_meta_response(&req.params)),
        "media.history" => Some(fixtures::media_history_response(&req.params)),
        "media.history.latest" => Some(fixtures::media_history_latest_response()),
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
    fn systems_returns_fixture_catalog_with_counts() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"systems","params":{}}"#;
        let resp = parse(&dispatch(req));
        let systems = resp["result"]["systems"].as_array().expect("array");
        assert_eq!(systems.len(), 10);
        assert!(systems.iter().all(|system| system["mediaCount"].is_u64()));
    }

    #[test]
    fn systems_filters_and_counts_favorite_tag() {
        let req =
            r#"{"jsonrpc":"2.0","id":"1","method":"systems","params":{"tags":["user:favorite"]}}"#;
        let resp = parse(&dispatch(req));
        let systems = resp["result"]["systems"].as_array().expect("array");
        assert!(!systems.is_empty());
        assert!(systems
            .iter()
            .all(|system| { system["mediaCount"].as_u64().is_some_and(|count| count > 0) }));
        let total = systems
            .iter()
            .filter_map(|system| system["mediaCount"].as_u64())
            .sum::<u64>();
        assert_eq!(total, 20);
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
        assert!(results.iter().all(|g| g["hasCover"].is_boolean()));
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
        // A page that covers every row reports no next page and no cursor.
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":[],"maxResults":1000}}"#;
        let resp = parse(&dispatch(req));
        let pagination = resp["result"]["pagination"]
            .as_object()
            .expect("pagination object");
        assert_eq!(pagination["hasNextPage"], Value::Bool(false));
        assert_eq!(pagination["pageSize"], Value::from(1000));
        assert!(!pagination.contains_key("nextCursor"));
        // Deprecated but still present for backward compatibility.
        assert_eq!(resp["result"]["total"], Value::from(-1));
    }

    // Favorites uses cursor pagination. Short pages must advertise a next
    // page and return a cursor that resumes where prior page stopped.
    #[test]
    fn media_search_paginates_with_a_resumable_cursor() {
        let first = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":[],"maxResults":5}}"#,
        ));
        let page1 = first["result"]["results"].as_array().expect("array");
        assert_eq!(page1.len(), 5);
        assert_eq!(
            first["result"]["pagination"]["hasNextPage"],
            Value::Bool(true)
        );
        let cursor = first["result"]["pagination"]["nextCursor"]
            .as_str()
            .expect("cursor");
        // Offset cursor: page two must resume exactly where page one ended,
        // not skip rows or restart inside page one.
        assert_eq!(cursor, "5");

        let second = parse(&dispatch(&format!(
            r#"{{"jsonrpc":"2.0","id":"2","method":"media.search","params":{{"systems":[],"maxResults":5,"cursor":"{cursor}"}}}}"#
        )));
        let page2 = second["result"]["results"].as_array().expect("array");
        assert_eq!(page2.len(), 5);
        // The second page resumes rather than repeating the first.
        assert_ne!(page1[0]["path"], page2[0]["path"]);
    }

    // Favorites are a tag query. Without this the Favorites screen in
    // `just mock-core` lists every game and the feature cannot be exercised.
    #[test]
    fn media_search_filters_by_user_favorite_tag() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":[],"maxResults":1000,"tags":["user:favorite"]}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        assert!(!results.is_empty(), "some mock games are favorites");

        let unfiltered = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.search","params":{"systems":[],"maxResults":1000}}"#,
        ));
        let all = unfiltered["result"]["results"].as_array().expect("array");
        assert!(
            results.len() < all.len(),
            "the tag filter must narrow the set"
        );

        for game in results {
            let tags = game["tags"].as_array().expect("tags array");
            assert!(
                tags.iter()
                    .any(|tag| tag["type"] == "user" && tag["tag"] == "favorite"),
                "every returned row carries the favorite tag"
            );
        }
    }

    #[test]
    fn media_search_applies_name_sort_before_paging() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"maxResults":1000,"sort":"name-asc"}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        let names: Vec<String> = results
            .iter()
            .filter_map(|game| game["name"].as_str())
            .map(str::to_lowercase)
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    // Search rows carry real system metadata rather than bare ids.
    #[test]
    fn media_search_rows_carry_system_name_and_category() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":["NES"],"maxResults":5}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        let system = &results[0]["system"];
        assert_eq!(system["id"], Value::from("NES"));
        assert_eq!(system["name"], Value::from("Nintendo Entertainment System"));
        assert_eq!(system["category"], Value::from("Consoles"));
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
    fn media_browse_filters_and_pages_favorites() {
        let first = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"path":"/games","maxResults":2,"tags":["user:favorite"]}}"#,
        ));
        let page1 = first["result"]["entries"].as_array().expect("array");
        assert_eq!(page1.len(), 2);
        assert!(page1
            .iter()
            .all(|entry| entry["tags"].as_array().is_some_and(|tags| tags
                .iter()
                .any(|tag| tag["type"] == "user" && tag["tag"] == "favorite"))));
        assert_eq!(first["result"]["pagination"]["hasNextPage"], true);
        let cursor = first["result"]["pagination"]["nextCursor"]
            .as_str()
            .expect("cursor");
        let second = parse(&dispatch(&format!(
            r#"{{"jsonrpc":"2.0","id":"2","method":"media.browse","params":{{"path":"/games","maxResults":2,"cursor":"{cursor}","tags":["user:favorite"]}}}}"#
        )));
        let page2 = second["result"]["entries"].as_array().expect("array");
        assert!(!page2.is_empty());
        assert_ne!(page1[0]["path"], page2[0]["path"]);
    }

    #[test]
    fn media_browse_index_matches_favorite_scope() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.browse.index","params":{"path":"/games","tags":["user:favorite"]}}"#;
        let resp = parse(&dispatch(req));
        let groups = resp["result"]["groups"].as_array().expect("array");
        let indexed: u64 = groups
            .iter()
            .filter_map(|group| group["count"].as_u64())
            .sum();
        let browse = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.browse","params":{"path":"/games","maxResults":1000,"tags":["user:favorite"]}}"#,
        ));
        assert_eq!(
            indexed,
            browse["result"]["totalFiles"].as_u64().unwrap_or(0)
        );
    }

    #[test]
    fn media_meta_preserves_batch_order() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.meta","params":{"items":[{"system":"NES","path":"/games/first.nes"},{"mediaId":42}]}}"#;
        let resp = parse(&dispatch(req));
        let items = resp["result"]["items"].as_array().expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["media"]["path"], "/games/first.nes");
        assert_eq!(items[0]["media"]["title"]["system"]["id"], "NES");
        assert_eq!(items[1]["media"]["path"], "/mock/media/42");
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
    fn media_history_returns_cursor_pages() {
        let first_req = r#"{"jsonrpc":"2.0","id":"1","method":"media.history","params":{"limit":5,"distinctMedia":true}}"#;
        let first = parse(&dispatch(first_req));
        let first_entries = first["result"]["entries"].as_array().expect("array");
        assert_eq!(first_entries.len(), 5);
        for entry in first_entries {
            assert!(entry["mediaName"].is_string());
            assert!(entry["mediaPath"].is_string());
            assert!(entry["systemId"].is_string());
            assert!(entry["systemName"].is_string());
            assert!(entry["launcherId"].is_string());
            assert!(entry["hasCover"].is_boolean());
        }
        let pagination = first["result"]["pagination"]
            .as_object()
            .expect("pagination object");
        assert_eq!(pagination["hasNextPage"], Value::Bool(true));
        assert_eq!(pagination["nextCursor"], Value::String("5".into()));

        let next_req = r#"{"jsonrpc":"2.0","id":"2","method":"media.history","params":{"limit":5,"cursor":"5","distinctMedia":true}}"#;
        let next = parse(&dispatch(next_req));
        let next_entries = next["result"]["entries"].as_array().expect("array");
        assert!(!next_entries.is_empty());
        let first_paths = first_entries
            .iter()
            .map(|entry| entry["mediaPath"].as_str().expect("first path"))
            .collect::<std::collections::HashSet<_>>();
        assert!(next_entries.iter().all(|entry| {
            !first_paths.contains(entry["mediaPath"].as_str().expect("next path"))
        }));
    }

    #[test]
    fn media_history_honors_distinct_media_before_paging() {
        let repeated_req = r#"{"jsonrpc":"2.0","id":"1","method":"media.history","params":{"limit":2,"distinctMedia":false}}"#;
        let repeated = parse(&dispatch(repeated_req));
        let repeated_entries = repeated["result"]["entries"].as_array().expect("array");
        assert_eq!(
            repeated_entries[0]["mediaPath"],
            repeated_entries[1]["mediaPath"]
        );

        let distinct_req = r#"{"jsonrpc":"2.0","id":"2","method":"media.history","params":{"limit":2,"distinctMedia":true}}"#;
        let distinct = parse(&dispatch(distinct_req));
        let distinct_entries = distinct["result"]["entries"].as_array().expect("array");
        assert_ne!(
            distinct_entries[0]["mediaPath"],
            distinct_entries[1]["mediaPath"]
        );
    }

    #[test]
    fn media_history_latest_returns_latest_entry() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.history.latest"}"#;
        let resp = parse(&dispatch(req));
        let entry = resp["result"]["entry"].as_object().expect("entry object");
        assert!(entry["mediaName"].is_string());
        assert!(entry["mediaPath"].is_string());
        assert!(entry["systemId"].is_string());
        assert!(entry["systemName"].is_string());
        assert!(entry["launcherId"].is_string());
        assert!(entry["startedAt"].is_string());
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
