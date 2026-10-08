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
use crate::media_state::{self, Notifier};

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
/// JSON-RPC error code for the mock's domain errors (cancel-with-nothing-
/// running). Not one of the reserved `-326xx` codes; arbitrary but stable.
const DOMAIN_ERROR_CODE: i32 = -32000;

pub fn dispatch(text: &str, notifier: &Notifier) -> String {
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

    let outcome: Option<Result<Value, String>> = match req.method.as_str() {
        "systems" => Some(Ok(fixtures::systems_response(&req.params))),
        "launchers" => Some(Ok(fixtures::launchers_response())),
        "scrapers" => Some(Ok(fixtures::scrapers_response())),
        "settings" => Some(Ok(fixtures::settings_response())),
        "settings.update" => Some(Ok(fixtures::settings_update_response(&req.params))),
        "media.search" => Some(Ok(fixtures::media_search_response(&req.params))),
        "media.browse" => Some(Ok(fixtures::media_browse_response(&req.params))),
        "media.browse.index" => Some(Ok(fixtures::media_browse_index_response(&req.params))),
        "media.lookup" => Some(Ok(fixtures::media_lookup_response(&req.params))),
        "media.meta" => Some(Ok(fixtures::media_meta_response(&req.params))),
        "media.meta.update" => Some(fixtures::media_meta_update_response(&req.params)),
        "media.image" => Some(fixtures::media_image_response(&req.params)),
        "media.tags" => Some(Ok(fixtures::media_tags_response(&req.params))),
        "decks" => Some(Ok(fixtures::decks_response())),
        "media.tags.update" => Some(media_tags_update(&req.params)),
        "media.history" => Some(Ok(fixtures::media_history_response(&req.params))),
        "media.history.latest" => Some(Ok(fixtures::media_history_latest_response())),
        "media" => Some(Ok(media_state::media_response())),
        "media.scrape.status" => Some(Ok(media_state::scrape_status_response())),
        "media.generate" => Some(Ok(media_state::start_index(notifier))),
        "media.generate.cancel" => Some(media_state::cancel_index(notifier)),
        "media.scrape" => {
            let force = req
                .params
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Some(Ok(media_state::start_scrape(force, notifier)))
        }
        "media.scrape.cancel" => Some(media_state::cancel_scrape(notifier)),
        "run" => {
            let zap_script = req.params.get("text").and_then(Value::as_str).unwrap_or("");
            info!(%zap_script, "run");
            Some(Ok(media_state::start_launch(zap_script, notifier)))
        }
        "readers.write" => {
            let zap_script = req.params.get("text").and_then(Value::as_str).unwrap_or("");
            info!(%zap_script, "readers.write");
            Some(Ok(Value::Null))
        }
        "version" => Some(Ok(fixtures::version_response())),
        _ => None,
    };

    let response = match outcome {
        Some(Ok(r)) => RpcResponse {
            jsonrpc: "2.0",
            id: req.id.unwrap_or(Value::Null),
            result: Some(r),
            error: None,
        },
        Some(Err(message)) => RpcResponse {
            jsonrpc: "2.0",
            id: req.id.unwrap_or(Value::Null),
            result: None,
            error: Some(RpcError {
                code: DOMAIN_ERROR_CODE,
                message,
            }),
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

/// `media.tags.update`: validates the exclusive media ref the way Core does
/// (a `mediaId` or a `(system, path)` pair, never both) and acknowledges the
/// change. The mock's catalog is static, so the one tag it keeps is
/// `user:hidden` on a path, a file's or a folder's, which browse and
/// search then honor.
fn media_tags_update(params: &Value) -> Result<Value, String> {
    let has_id = params.get("mediaId").is_some();
    let system = params.get("system").and_then(Value::as_str).unwrap_or("");
    let path = params.get("path").and_then(Value::as_str).unwrap_or("");
    if has_id && (!system.is_empty() || !path.is_empty()) {
        return Err("invalid params: mediaId cannot be mixed with system/path".into());
    }
    info!(%params, "media.tags.update");
    let names = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|tags| tags.iter().any(|tag| tag == "user:hidden"))
    };
    let mut tags = Vec::new();
    if !path.is_empty() && (names("add") || names("remove")) {
        // Adds win over removes, as in Core.
        if fixtures::set_hidden(path, names("add")) {
            tags.push(serde_json::json!({ "type": "user", "tag": "hidden" }));
        }
    }
    Ok(serde_json::json!({ "tags": tags }))
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

    use super::dispatch as dispatch_with_notifier;
    use crate::media_state::Notifier;

    // Every existing test below predates the notifier parameter and has
    // no socket-pump task to receive pushed notifications — route them
    // through a no-op `Notifier` so none of those call sites need to
    // change.
    fn dispatch(text: &str) -> String {
        dispatch_with_notifier(text, &Notifier::noop())
    }

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
    fn media_search_matches_every_query_word_against_the_name_slug() {
        let all = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"maxResults":1000}}"#,
        ));
        let all = all["result"]["results"].as_array().expect("array").clone();
        let name = all[0]["name"].as_str().expect("name").to_string();
        let word = name.split_whitespace().next().expect("word").to_uppercase();
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": "2", "method": "media.search",
            "params": { "query": format!(" {word}! "), "maxResults": 1000 },
        });
        let resp = parse(&dispatch(&req.to_string()));
        let results = resp["result"]["results"].as_array().expect("array");
        assert!(!results.is_empty() && results.len() < all.len());
        assert!(results.iter().any(|g| g["name"] == name.as_str()));

        let none = serde_json::json!({
            "jsonrpc": "2.0", "id": "3", "method": "media.search",
            "params": { "query": format!("{word} zzzzqqqq"), "maxResults": 1000 },
        });
        let resp = parse(&dispatch(&none.to_string()));
        assert!(resp["result"]["results"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[test]
    fn media_search_limits_results_to_a_path_prefix() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"pathPrefix":"/mock/NES","maxResults":1000}}"#;
        let resp = parse(&dispatch(req));
        let results = resp["result"]["results"].as_array().expect("array");
        assert!(!results.is_empty());
        assert!(results.iter().all(|g| g["path"]
            .as_str()
            .is_some_and(|p| p.starts_with("/mock/NES/"))));
        // A sibling that merely shares the prefix's letters is not under it.
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"pathPrefix":"/mock/NE","maxResults":1000}}"#;
        let resp = parse(&dispatch(req));
        assert!(resp["result"]["results"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[test]
    fn decks_name_the_deck_whose_members_are_tagged() {
        let decks = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"decks","params":{}}"#,
        ));
        let deck = &decks["result"]["decks"][0];
        let id = deck["deckId"].as_str().expect("deck id");
        assert_eq!(deck["name"], "Couch co-op");
        let tags = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.tags","params":{}}"#,
        ));
        let wanted = format!("deck:{id}");
        assert!(tags["result"]["tags"]
            .as_array()
            .expect("array")
            .iter()
            .any(|t| t["type"] == "user" && t["tag"] == wanted.as_str()));
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
        let results = resp["result"]["results"].as_array().expect("array");
        let pagination = resp["result"]["pagination"]
            .as_object()
            .expect("pagination object");
        assert_eq!(pagination["hasNextPage"], Value::Bool(false));
        assert_eq!(pagination["pageSize"], Value::from(1000));
        assert!(!pagination.contains_key("nextCursor"));
        // Deprecated current-page count, matching real Core. It is not the
        // dataset total and must not be used to stop cursor pagination.
        assert_eq!(resp["result"]["total"], Value::from(results.len()));
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
        assert_eq!(first["result"]["total"], Value::from(5));
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
        assert_eq!(system["category"], Value::from("Console"));
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
    fn media_browse_root_contents_merges_directories_and_media() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"systems":["NES"],"rootView":"contents"}}"#;
        let resp = parse(&dispatch(req));
        assert_eq!(resp["result"]["path"], Value::from(""));
        let entries = resp["result"]["entries"].as_array().expect("array");
        // Virtual root first, then the mock directories, then media --
        // mirrors `browseSystemRootContents`'s ordering in zaparoo-core.
        assert_eq!(entries[0]["type"], Value::from("root"));
        assert!(entries[1..4]
            .iter()
            .all(|entry| entry["type"] == "directory"));
        assert!(entries[4..].iter().all(|entry| entry["type"] == "media"));
        // `totalDirs` counts the merged directories only, excluding the
        // leading virtual root.
        assert_eq!(resp["result"]["totalDirs"], Value::from(3));
        assert!(resp["result"]["totalFiles"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn a_multi_disc_folder_launches_one_disc_and_browses_to_them_all() {
        let root = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"systems":["SNES"],"rootView":"contents"}}"#,
        ));
        let entries = root["result"]["entries"].as_array().expect("array");
        let folder = entries
            .iter()
            .find(|entry| entry["multiDisc"] == true)
            .expect("multi-disc folder");
        assert_eq!(folder["type"], "directory");
        assert_eq!(folder["tags"][0]["type"], "disc");
        assert!(folder["disambiguatingTags"].is_null());
        assert!(folder["zapScript"]
            .as_str()
            .is_some_and(|s| s.contains("disc:1")));

        let path = folder["path"].as_str().expect("path");
        let discs = parse(&dispatch(&format!(
            r#"{{"jsonrpc":"2.0","id":"2","method":"media.browse","params":{{"systems":["SNES"],"path":"{path}"}}}}"#
        )));
        let discs = discs["result"]["entries"].as_array().expect("array");
        assert_eq!(discs.len(), 2);
        for (index, disc) in discs.iter().enumerate() {
            assert_eq!(disc["type"], "media");
            assert_eq!(disc["tags"][0]["tag"], (index + 1).to_string());
        }
    }

    #[test]
    fn a_hidden_folder_leaves_the_listing_until_hidden_entries_are_asked_for() {
        // GBA is this test's own system: the hidden set is process state.
        let browse = |extra: &str| {
            parse(&dispatch(&format!(
                r#"{{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{{"systems":["GBA"],"rootView":"contents"{extra}}}}}"#
            )))
        };
        let update = |verb: &str| {
            parse(&dispatch(&format!(
                r#"{{"jsonrpc":"2.0","id":"1","method":"media.tags.update","params":{{"system":"GBA","path":"/mock/games/GBA/Extras","{verb}":["user:hidden"]}}}}"#
            )))
        };
        let extras = |resp: &Value| {
            resp["result"]["entries"]
                .as_array()
                .expect("array")
                .iter()
                .find(|entry| entry["name"] == "Extras")
                .cloned()
        };

        assert_eq!(update("add")["result"]["tags"][0]["tag"], "hidden");
        let listed = browse("");
        assert!(extras(&listed).is_none());
        assert_eq!(listed["result"]["totalDirs"], Value::from(2));
        let shown = extras(&browse(r#","includeHidden":true"#)).expect("hidden folder");
        assert_eq!(shown["tags"][0]["tag"], "hidden");

        // Its own path still lists what is inside it, untagged.
        let inside = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"systems":["GBA"],"path":"/mock/games/GBA/Extras"}}"#,
        ));
        let games = inside["result"]["entries"].as_array().expect("array");
        assert!(!games.is_empty());
        assert!(games.iter().all(|game| !game["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|tag| tag["tag"] == "hidden"))));

        assert!(update("remove")["result"]["tags"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert!(extras(&browse("")).is_some());
    }

    #[test]
    fn media_browse_without_root_view_returns_route_roots() {
        let req =
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"systems":["NES"]}}"#;
        let resp = parse(&dispatch(req));
        let entries = resp["result"]["entries"].as_array().expect("array");
        assert!(entries.iter().all(|entry| entry["type"] == "root"));
        assert_eq!(resp["result"]["totalFiles"], Value::from(0));
        assert_eq!(resp["result"]["totalDirs"], Value::from(0));
        assert!(resp["result"]["pagination"].is_null());
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
    fn media_tags_lists_counted_tags_scoped_to_systems() {
        let all = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.tags"}"#,
        ));
        let all_tags = all["result"]["tags"].as_array().expect("array");
        for tag_type in ["genre", "year", "players", "developer", "user"] {
            assert!(
                all_tags.iter().any(|t| t["type"] == tag_type),
                "missing {tag_type}"
            );
        }
        assert!(all_tags
            .iter()
            .all(|t| t["count"].as_u64().unwrap_or(0) > 0));
        let nes = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.tags","params":{"systems":["NES"]}}"#,
        ));
        let nes_total: u64 = nes["result"]["tags"]
            .as_array()
            .expect("array")
            .iter()
            .filter(|t| t["type"] == "genre")
            .filter_map(|t| t["count"].as_u64())
            .sum();
        // One genre per game, so the genre counts add up to the NES games.
        assert_eq!(nes_total, 5);
    }

    #[test]
    fn media_browse_filters_on_genre_and_year_together() {
        let genre = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"path":"/games","maxResults":1000,"tags":["genre:rpg"]}}"#,
        ));
        let rpg = genre["result"]["totalFiles"].as_u64().expect("count");
        assert!(rpg > 0);
        let both = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.browse","params":{"path":"/games","maxResults":1000,"tags":["genre:rpg","year:1985"]}}"#,
        ));
        assert!(both["result"]["totalFiles"].as_u64().expect("count") < rpg);
    }

    #[test]
    fn portable_identifiers_ride_on_every_media_shape() {
        let search = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.search","params":{"systems":["NES"],"maxResults":1}}"#,
        ));
        let row = &search["result"]["results"][0];
        let relative = row["relativePath"].as_str().expect("relativePath");
        assert!(relative.starts_with("NES/"), "{relative}");

        let history = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.history","params":{"limit":1}}"#,
        ));
        let entry = &history["result"]["entries"][0];
        assert!(entry["relativePath"].is_string());
        assert!(entry["zapScript"].is_string());

        let latest = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"3","method":"media.history.latest"}"#,
        ));
        assert!(latest["result"]["entry"]["relativePath"].is_string());

        // media.meta answers a relative reference with the canonical path.
        let meta = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"4","method":"media.meta","params":{"system":"NES","path":"NES/smb.nes"}}"#,
        ));
        assert_eq!(meta["result"]["media"]["path"], "/mock/NES/smb.nes");
        assert_eq!(meta["result"]["media"]["relativePath"], "NES/smb.nes");
        assert_eq!(meta["result"]["media"]["zapScript"], "@NES/smb.nes");
    }

    #[test]
    fn media_browse_resolves_a_relative_folder_and_reports_its_own_relative_path() {
        let resp = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.browse","params":{"path":"NES/Favorites","systems":["NES"]}}"#,
        ));
        assert_eq!(resp["result"]["path"], "/mock/games/NES/Favorites");
        assert_eq!(resp["result"]["relativePath"], "NES/Favorites");

        let roots = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.browse","params":{"systems":["NES"]}}"#,
        ));
        assert_eq!(roots["result"]["entries"][0]["relativePath"], "NES");

        let contents = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"3","method":"media.browse","params":{"systems":["NES"],"rootView":"contents"}}"#,
        ));
        let dirs: Vec<&Value> = contents["result"]["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .filter(|entry| entry["type"] == "directory")
            .collect();
        assert_eq!(dirs[0]["relativePath"], "NES/Favorites");
    }

    #[test]
    fn media_lookup_matches_a_name_within_its_system_or_answers_null() {
        let found = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"1","method":"media.lookup","params":{"system":"NES","name":"super mario bros."}}"#,
        ));
        assert_eq!(found["result"]["match"]["path"], "/mock/NES/smb.nes");
        assert_eq!(found["result"]["match"]["relativePath"], "NES/smb.nes");

        let missing = parse(&dispatch(
            r#"{"jsonrpc":"2.0","id":"2","method":"media.lookup","params":{"system":"SNES","name":"Super Mario Bros."}}"#,
        ));
        assert!(missing["result"]["match"].is_null());
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
    fn media_meta_update_sets_and_media_meta_reflects_the_override() {
        let update = r#"{"jsonrpc":"2.0","id":"1","method":"media.meta.update","params":{"system":"SNES","path":"/games/set.sfc","media":{"launcherOverride":"snes9x"}}}"#;
        let resp = parse(&dispatch(update));
        assert_eq!(resp["result"]["media"]["launcherOverride"], "snes9x");

        let read = r#"{"jsonrpc":"2.0","id":"2","method":"media.meta","params":{"system":"SNES","path":"/games/set.sfc"}}"#;
        let resp = parse(&dispatch(read));
        assert_eq!(resp["result"]["media"]["launcherOverride"], "snes9x");
    }

    #[test]
    fn media_meta_update_clear_removes_the_override() {
        let set = r#"{"jsonrpc":"2.0","id":"1","method":"media.meta.update","params":{"system":"SNES","path":"/games/clear.sfc","media":{"launcherOverride":"snes9x"}}}"#;
        dispatch(set);

        let clear = r#"{"jsonrpc":"2.0","id":"2","method":"media.meta.update","params":{"system":"SNES","path":"/games/clear.sfc","media":{"launcherOverride":null}}}"#;
        let resp = parse(&dispatch(clear));
        assert!(resp["result"]["media"]["launcherOverride"].is_null());
    }

    #[test]
    fn media_meta_update_rejects_unknown_launcher() {
        let update = r#"{"jsonrpc":"2.0","id":"1","method":"media.meta.update","params":{"system":"SNES","path":"/games/unknown.sfc","media":{"launcherOverride":"not-a-real-launcher"}}}"#;
        let resp = parse(&dispatch(update));
        assert_eq!(resp["error"]["code"], -32000);
        assert!(resp["error"]["message"]
            .as_str()
            .expect("message")
            .contains("launcher not found"));
    }

    #[test]
    fn media_meta_update_rejects_launcher_from_a_different_system() {
        // "nestopia" is a real fixture launcher, but it's registered for
        // NES, not SNES.
        let update = r#"{"jsonrpc":"2.0","id":"1","method":"media.meta.update","params":{"system":"SNES","path":"/games/wrong-system.sfc","media":{"launcherOverride":"nestopia"}}}"#;
        let resp = parse(&dispatch(update));
        assert_eq!(resp["error"]["code"], -32000);
        assert!(resp["error"]["message"]
            .as_str()
            .expect("message")
            .contains("launcher not found"));
    }

    #[tokio::test]
    async fn run_accepts_any_text_and_returns_null() {
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

    // The media.generate/media.scrape sequence tests below rely on
    // `media_state`'s single process-wide `Mutex<MediaState>`. That is
    // only test-safe because `just test` runs on `cargo nextest`,
    // which gives every test its own process — a plain `cargo test`
    // invocation would run these in one process and the shared state
    // could race across tests. Mark new stateful tests `#[tokio::test]`
    // (media.generate/media.scrape call `tokio::spawn`, which panics
    // outside a runtime) and keep them independent of one another.

    #[test]
    fn media_returns_seed_response_shape() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media","params":{}}"#;
        let resp = parse(&dispatch(req));
        let database = resp["result"]["database"].as_object().expect("object");
        for key in [
            "exists",
            "indexing",
            "optimizing",
            "paused",
            "totalSteps",
            "currentStep",
            "currentStepDisplay",
            "totalFiles",
            "totalMedia",
        ] {
            assert!(database.contains_key(key), "missing {key}");
        }
        assert!(resp["result"]["active"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[tokio::test]
    async fn media_generate_starts_indexing() {
        let notifier = Notifier::noop();
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.generate","params":{}}"#;
        let resp = parse(&dispatch_with_notifier(req, &notifier));
        assert!(resp["result"].is_null());
        assert!(resp["error"].is_null());

        let seed_req = r#"{"jsonrpc":"2.0","id":"2","method":"media","params":{}}"#;
        let seed = parse(&dispatch_with_notifier(seed_req, &notifier));
        assert_eq!(seed["result"]["database"]["indexing"], Value::Bool(true));
    }

    #[tokio::test]
    async fn media_generate_cancel_without_running_index_returns_domain_error() {
        let notifier = Notifier::noop();
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.generate.cancel","params":{}}"#;
        let resp = parse(&dispatch_with_notifier(req, &notifier));
        assert!(resp["result"].is_null());
        assert_eq!(resp["error"]["code"], -32000);
    }

    #[tokio::test]
    async fn media_scrape_starts_scraping() {
        let notifier = Notifier::noop();
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.scrape","params":{"force":false}}"#;
        let resp = parse(&dispatch_with_notifier(req, &notifier));
        assert!(resp["result"].is_null());
        assert!(resp["error"].is_null());

        let status_req = r#"{"jsonrpc":"2.0","id":"2","method":"media.scrape.status","params":{}}"#;
        let status = parse(&dispatch_with_notifier(status_req, &notifier));
        assert_eq!(status["result"]["scraping"], Value::Bool(true));
        assert!(status["result"]["currentSystem"].is_object());
    }

    #[tokio::test]
    async fn media_scrape_cancel_without_running_scrape_returns_domain_error() {
        let notifier = Notifier::noop();
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.scrape.cancel","params":{}}"#;
        let resp = parse(&dispatch_with_notifier(req, &notifier));
        assert!(resp["result"].is_null());
        assert_eq!(resp["error"]["code"], -32000);
    }

    #[test]
    fn media_image_returns_base64_png() {
        use base64::Engine as _;
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.image","params":{"system":"Arcade","path":"/games/pacman.zip","maxSize":128}}"#;
        let resp = parse(&dispatch(req));
        let data = resp["result"]["data"].as_str().expect("data string");
        assert!(!data.is_empty());
        assert_eq!(resp["result"]["contentType"], "image/png");
        // PNG magic survives the base64 round trip.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("valid base64");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    // Real Core rejects a bare path; the mock mirrors that so frontend
    // parameter regressions surface in dev, not on hardware.
    #[test]
    fn media_image_rejects_bare_path_but_accepts_media_id() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.image","params":{"path":"/games/pacman.zip"}}"#;
        let resp = parse(&dispatch(req));
        assert_eq!(resp["error"]["code"], super::DOMAIN_ERROR_CODE);
        let req = r#"{"jsonrpc":"2.0","id":"2","method":"media.image","params":{"mediaId":42,"maxSize":64}}"#;
        let resp = parse(&dispatch(req));
        assert_eq!(resp["result"]["contentType"], "image/png");
    }

    #[test]
    fn tags_update_rejects_mixed_media_ref() {
        let req = r#"{"jsonrpc":"2.0","id":"1","method":"media.tags.update","params":{"mediaId":7,"system":"nes","path":"/g/x","add":["user:favorite"]}}"#;
        let resp = dispatch(req);
        assert!(resp.contains("cannot be mixed"), "{resp}");
    }

    #[test]
    fn tags_update_accepts_exclusive_refs() {
        let by_id = r#"{"jsonrpc":"2.0","id":"1","method":"media.tags.update","params":{"mediaId":7,"add":["user:favorite"]}}"#;
        assert!(dispatch(by_id).contains("\"tags\""));
        let by_pair = r#"{"jsonrpc":"2.0","id":"1","method":"media.tags.update","params":{"system":"nes","path":"/g/x","remove":["user:favorite"]}}"#;
        assert!(dispatch(by_pair).contains("\"tags\""));
    }
}
