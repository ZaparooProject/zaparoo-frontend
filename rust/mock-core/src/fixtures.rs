// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Canned fixture data for mock-core. Response shapes mirror the
// upstream Core API: https://zaparoo.org/docs/core/api/methods/
// 3 categories x 10 systems x 5 games each = 50 games total,
// distributed so every system has content when the frontend drills
// into it.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static SYSTEM_DEFAULTS: OnceLock<Mutex<Vec<SystemDefaultFixture>>> = OnceLock::new();
// Per-media launcher overrides set via `media.meta.update`, keyed by the
// same `(system, path)` pair `media_ref_key` derives for `media.meta`.
// Mirrors `SYSTEM_DEFAULTS`'s in-memory, process-lifetime pattern -- this
// is dev-server state, not a real database.
static LAUNCHER_OVERRIDES: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();

pub(crate) const MOCK_SYSTEMS: &[(&str, &str, &str)] = &[
    ("NES", "Nintendo Entertainment System", "Consoles"),
    ("SNES", "Super Nintendo", "Consoles"),
    ("Genesis", "Sega Genesis", "Consoles"),
    ("Nintendo64", "Nintendo 64", "Consoles"),
    ("Gameboy", "Game Boy", "Handhelds"),
    ("GameboyColor", "Game Boy Color", "Handhelds"),
    ("GBA", "Game Boy Advance", "Handhelds"),
    ("NDS", "Nintendo DS", "Handhelds"),
    ("MAME", "MAME", "Arcade"),
    ("NeoGeo", "Neo Geo", "Arcade"),
];

#[derive(Clone)]
struct SystemDefaultFixture {
    system: String,
    launcher: String,
    before_exit: String,
}

pub fn version_response() -> Value {
    json!({
        "version": "mock-0.1.0",
        "platform": "mock",
    })
}

pub fn launchers_response() -> Value {
    json!({
        "launchers": [
            { "id": "nestopia",          "systemId": "NES",        "systemName": "Nintendo Entertainment System", "groups": ["libretro"] },
            { "id": "fceumm",           "systemId": "NES",        "systemName": "Nintendo Entertainment System", "groups": ["libretro"] },
            { "id": "snes9x",           "systemId": "SNES",       "systemName": "Super Nintendo",                "groups": ["libretro"] },
            { "id": "bsnes",            "systemId": "SNES",       "systemName": "Super Nintendo",                "groups": ["libretro"] },
            { "id": "genesis-plus-gx",  "systemId": "Genesis",    "systemName": "Sega Genesis",                  "groups": ["libretro"] },
            { "id": "mupen64plus-next", "systemId": "Nintendo64", "systemName": "Nintendo 64",                   "groups": ["libretro"] },
            { "id": "gambatte",         "systemId": "Gameboy",    "systemName": "Game Boy",                      "groups": ["libretro"] },
            { "id": "mgba",             "systemId": "GBA",        "systemName": "Game Boy Advance",              "groups": ["libretro"] },
            { "id": "mame",             "systemId": "MAME",       "systemName": "MAME",                          "groups": ["arcade"] },
            { "id": "fbneo",            "systemId": "NeoGeo",     "systemName": "Neo Geo",                       "groups": ["arcade"] }
        ]
    })
}

/// Mirrors Core's `scrapers` result (`ScrapersResult` / `ScraperInfo`).
/// Without this the Source picker in the Get metadata modal is always
/// empty against mock-core and `refresh_scrapers` errors into the
/// "Source list unavailable" modal, so the whole metadata flow is
/// untestable off-device.
///
/// `supportedSystems` is left empty, which Core uses to mean "no
/// restriction" -- the frontend only reads `id` and `name` today.
pub fn scrapers_response() -> Value {
    json!({
        "scrapers": [
            { "id": "gamelist.xml", "name": "gamelist.xml", "supportedSystems": [] },
            { "id": "screenscraper", "name": "ScreenScraper", "supportedSystems": [] }
        ]
    })
}

pub fn settings_response() -> Value {
    let defaults = system_defaults()
        .lock()
        .map(|defaults| defaults.clone())
        .unwrap_or_default();
    let system_defaults: Vec<Value> = defaults
        .into_iter()
        .map(|default| {
            json!({
                "system": default.system,
                "launcher": default.launcher,
                "beforeExit": default.before_exit,
            })
        })
        .collect();
    json!({
        "runZapScript": true,
        "debugLogging": false,
        "audioScanFeedback": true,
        "readersAutoDetect": true,
        "readersScanMode": "tap",
        "readersScanExitDelay": 0.0,
        "readersScanIgnoreSystems": [],
        "errorReporting": false,
        "readersConnect": [],
        "systemDefaults": system_defaults,
    })
}

pub fn settings_update_response(params: &Value) -> Value {
    if let Some(items) = params.get("systemDefaults").and_then(Value::as_array) {
        let next = items
            .iter()
            .filter_map(|item| {
                let system = item.get("system").and_then(Value::as_str)?;
                Some(SystemDefaultFixture {
                    system: system.to_string(),
                    launcher: item
                        .get("launcher")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    before_exit: item
                        .get("beforeExit")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect();
        if let Ok(mut defaults) = system_defaults().lock() {
            *defaults = next;
        }
    }
    Value::Null
}

fn system_defaults() -> &'static Mutex<Vec<SystemDefaultFixture>> {
    SYSTEM_DEFAULTS.get_or_init(|| {
        Mutex::new(vec![SystemDefaultFixture {
            system: "SNES".into(),
            launcher: "snes9x".into(),
            before_exit: String::new(),
        }])
    })
}

fn launcher_overrides() -> &'static Mutex<HashMap<(String, String), String>> {
    LAUNCHER_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Derive the `(system, path)` identity `media.meta`/`media.meta.update`
/// share for one media ref -- a `mediaId`-only ref falls back to the same
/// synthetic `/mock/media/<id>` path `media_for` already uses, and an
/// absent `system` falls back to `"Mock"`, so both call sites agree on the
/// same key regardless of which ref shape the caller sent.
fn media_ref_key(reference: &Value) -> (String, String) {
    let media_id = reference.get("mediaId").and_then(Value::as_i64);
    let system = reference
        .get("system")
        .and_then(Value::as_str)
        .unwrap_or("Mock")
        .to_string();
    let path = reference.get("path").and_then(Value::as_str).map_or_else(
        || format!("/mock/media/{}", media_id.unwrap_or_default()),
        str::to_string,
    );
    (system, path)
}

pub fn systems_response(params: &Value) -> Value {
    let tags = params
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let systems = MOCK_SYSTEMS
        .iter()
        .copied()
        .filter_map(|(id, name, category)| {
            let media_count = ALL_GAMES
                .iter()
                .enumerate()
                .filter(|(index, (_, file, system))| {
                    *system == id
                        && tags.iter().all(|tag| {
                            let game = json!({ "tags": tags_for(file, *index) });
                            game_has_tag(&game, tag)
                        })
                })
                .count();
            if !tags.is_empty() && media_count == 0 {
                return None;
            }
            Some(json!({
                "id": id,
                "name": name,
                "category": category,
                "mediaCount": media_count,
            }))
        })
        .collect::<Vec<_>>();
    json!({ "systems": systems })
}

pub fn media_search_response(params: &Value) -> Value {
    let systems = params
        .get("systems")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let tags = params
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let max = params
        .get("maxResults")
        .and_then(Value::as_u64)
        .unwrap_or(100) as usize;
    // Cursor is opaque to clients, so the mock uses the row offset as its
    // own cursor format. Real Core's cursors are keyset tokens.
    let offset = params
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0);

    let mut matching: Vec<Value> = games_for_systems(&systems)
        .filter(|game| tags.iter().all(|tag| game_has_tag(game, tag)))
        .collect();
    match params.get("sort").and_then(Value::as_str) {
        Some("name-asc") => matching.sort_by_key(|game| {
            game.get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase()
        }),
        Some("name-desc") => matching.sort_by_key(|game| {
            std::cmp::Reverse(
                game.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_lowercase(),
            )
        }),
        _ => {}
    }
    let total = matching.len();
    let results: Vec<Value> = matching.into_iter().skip(offset).take(max).collect();
    let next_offset = offset.saturating_add(results.len());
    // A zero-length page would hand back the same cursor with hasNextPage
    // still true, so a client draining pages would spin forever. Real Core
    // rejects maxResults=0 outright; the mock just terminates the chain.
    let has_next_page = !results.is_empty() && next_offset < total;

    // Real Core keeps this deprecated field for compatibility, but it is only
    // the current page length. Clients must use the pagination envelope.
    let page_total = results.len();
    let mut pagination = json!({
        "hasNextPage": has_next_page,
        "pageSize": max,
    });
    if has_next_page {
        pagination["nextCursor"] = json!(next_offset.to_string());
    }
    json!({
        "results": results,
        "total": page_total,
        "pagination": pagination,
    })
}

/// Match one `type:value` filter against a search row's `tags` array, the
/// same shape Core returns. Only the AND form is modelled; the frontend
/// never sends the `-`/`~` operators.
fn game_has_tag(game: &Value, filter: &str) -> bool {
    let Some((want_type, want_value)) = filter.split_once(':') else {
        return false;
    };
    game.get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| {
            tags.iter().any(|tag| {
                tag.get("type").and_then(Value::as_str) == Some(want_type)
                    && tag.get("tag").and_then(Value::as_str) == Some(want_value)
            })
        })
}

/// Media rows for `system` under `path_prefix`, filtered by `filters` and
/// sorted by display name -- the shared core of every `media.browse`
/// shape below.
fn matching_media_rows(path_prefix: &str, systems: &[&str], filters: &[&str]) -> Vec<Value> {
    let mut matching: Vec<Value> = ALL_GAMES
        .iter()
        .enumerate()
        .filter(|(_, (_, _, system))| systems.is_empty() || systems.contains(system))
        .filter_map(|(index, (name, file, system))| {
            let row = json!({
                "name": name,
                "path": format!("{path_prefix}/{file}"),
                "type": "media",
                "systemId": system,
                "zapScript": format!("@{system}/{file}"),
                "relativePath": file,
                "tags": tags_for(file, index),
                "disambiguatingTags": disambiguating_tags_for(file),
                "hasCover": true,
            });
            filters
                .iter()
                .all(|filter| game_has_tag(&row, filter))
                .then_some(row)
        })
        .collect();
    matching.sort_by_key(|entry| {
        entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase()
    });
    matching
}

fn mock_directory_entry(name: &str, path: &str, file_count: u64) -> Value {
    json!({
        "name": name,
        "path": path,
        "type": "directory",
        "fileCount": file_count,
        "hasCover": false,
    })
}

/// A virtual-scheme route. Core never merges these into a system's
/// contents view (`rootView: "contents"`) -- it prepends them ahead of
/// the merged directories, and only on the page a cursor is absent from.
fn mock_virtual_root_entry(system: &str) -> Value {
    json!({
        "name": "Mock Arcade",
        "path": "mock-arcade://",
        "type": "root",
        "group": "mock",
        "systemId": system,
        "systemIds": [system],
        "hasCover": false,
    })
}

fn mock_route_root_entry(path: &str, system: &str, file_count: u64) -> Value {
    json!({
        // Real launcher routes are commonly named after the system
        // itself, so two configured folders collide on name -- this is
        // the shape `root_distinguishers`' fallback exists to label.
        "name": system,
        "path": path,
        "type": "root",
        "systemId": system,
        "systemIds": [system],
        "fileCount": file_count,
        "hasCover": false,
    })
}

/// Core's merged system-root view (`rootView: "contents"`): a virtual
/// route (first page only), two mock directories, then the system's
/// media, all under one pathless response. Mirrors
/// `browseSystemRootContents` in zaparoo-core's `media_browse.go`.
fn media_browse_root_contents_response(
    system: &str,
    filters: &[&str],
    max: usize,
    offset: usize,
    cursor_present: bool,
) -> Value {
    let path_prefix = format!("/mock/games/{system}");
    let dirs = if cursor_present {
        Vec::new()
    } else {
        vec![
            mock_directory_entry("Favorites", &format!("{path_prefix}/Favorites"), 1),
            mock_directory_entry("Extras", &format!("{path_prefix}/Extras"), 1),
        ]
    };
    let remaining = max.saturating_sub(dirs.len());
    let matching = matching_media_rows(&path_prefix, &[system], filters);
    let total_files = matching.len();
    let media_page: Vec<Value> = matching.into_iter().skip(offset).take(remaining).collect();
    let next_offset = offset.saturating_add(media_page.len());
    let has_next_page = !media_page.is_empty() && next_offset < total_files;
    let mut entries = Vec::new();
    if !cursor_present {
        entries.push(mock_virtual_root_entry(system));
    }
    entries.extend(dirs);
    entries.extend(media_page);
    let mut pagination = json!({
        "hasNextPage": has_next_page,
        "pageSize": max,
    });
    if has_next_page {
        pagination["nextCursor"] = json!(next_offset.to_string());
    }
    json!({
        "path": "",
        "entries": entries,
        // Virtual roots are excluded, mirroring Core: `totalDirs` counts
        // merged physical directories only.
        "totalDirs": 2,
        "totalFiles": total_files,
        "pagination": pagination,
    })
}

/// Core's default, unmerged system-root view: one `root` entry per
/// configured launcher route. Reachable in dev by omitting `rootView` on
/// a pathless, system-scoped request -- the older-Core fallback path the
/// frontend's `dedup_roots_drop_ancestors` / `root_distinguishers` still
/// cover. The live frontend never sends this shape itself once
/// `rootView: "contents"` is wired up, since a pathless single-system
/// browse always includes it.
fn media_browse_route_roots_response(systems: &[&str]) -> Value {
    let entries: Vec<Value> = systems
        .iter()
        .flat_map(|system| {
            let count = ALL_GAMES.iter().filter(|(_, _, s)| s == system).count() as u64;
            vec![
                mock_route_root_entry(&format!("/mock/games/{system}"), system, count),
                mock_route_root_entry(&format!("/mock/usb0/{system}"), system, 0),
            ]
        })
        .collect();
    json!({
        "path": "",
        "entries": entries,
        "totalFiles": 0,
        "totalDirs": 0,
    })
}

pub fn media_browse_response(params: &Value) -> Value {
    let path = params.get("path").and_then(Value::as_str).unwrap_or("");
    let systems = params
        .get("systems")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let filters = params
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let max = params
        .get("maxResults")
        .and_then(Value::as_u64)
        .unwrap_or(100) as usize;
    let cursor = params.get("cursor").and_then(Value::as_str);
    let offset = cursor
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);
    if path.is_empty() {
        let root_view = params.get("rootView").and_then(Value::as_str);
        if root_view == Some("contents") {
            if systems.len() == 1 {
                return media_browse_root_contents_response(
                    systems[0],
                    &filters,
                    max,
                    offset,
                    cursor.is_some(),
                );
            }
        } else if !systems.is_empty() {
            return media_browse_route_roots_response(&systems);
        }
    }
    let matching = matching_media_rows(path, &systems, &filters);
    let total_files = matching.len();
    let entries: Vec<Value> = matching.into_iter().skip(offset).take(max).collect();
    let next_offset = offset.saturating_add(entries.len());
    let has_next_page = !entries.is_empty() && next_offset < total_files;
    let mut pagination = json!({
        "hasNextPage": has_next_page,
        "pageSize": max,
    });
    if has_next_page {
        pagination["nextCursor"] = json!(next_offset.to_string());
    }
    json!({
        "path": path,
        "entries": entries,
        "totalFiles": total_files,
        // Mock browse returns only media entries (no subdirectories) for
        // an ordinary path browse.
        "totalDirs": 0,
        "pagination": pagination,
    })
}

pub fn media_meta_response(params: &Value) -> Value {
    fn media_for(reference: &Value) -> Value {
        let (system, path) = media_ref_key(reference);
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or("Mock Game")
            .split('.')
            .next()
            .unwrap_or("Mock Game")
            .to_string();
        let launcher_override = launcher_overrides()
            .lock()
            .ok()
            .and_then(|overrides| overrides.get(&(system.clone(), path.clone())).cloned());
        let mut media = json!({
            "path": path,
            "parentDir": "/mock",
            "isMissing": false,
            "tags": [{"type": "region", "tag": "world"}],
            "properties": {},
            "availableImageTypes": [],
            "title": {
                "slug": name.to_lowercase(),
                "name": name,
                "slugLength": name.len(),
                "slugWordCount": name.split_whitespace().count(),
                "system": {"id": &system, "name": system_display_for(&system)},
                "tags": [
                    {"type": "year", "tag": "1994"},
                    {"type": "developer", "tag": "Mock Studio"}
                ],
                "properties": {
                    "property:description": {
                        "text": format!("Mock metadata for {name}."),
                        "contentType": ""
                    }
                },
                "availableImageTypes": []
            }
        });
        if let Some(launcher_override) = launcher_override {
            media["launcherOverride"] = json!(launcher_override);
        }
        media
    }

    if let Some(items) = params.get("items").and_then(Value::as_array) {
        return json!({
            "items": items
                .iter()
                .map(|item| json!({"media": media_for(item)}))
                .collect::<Vec<_>>()
        });
    }
    json!({"media": media_for(params)})
}

/// Mirrors Core's `media.meta.update`: sets or clears the per-media
/// launcher override, validated against `launchers_response()`'s known
/// launchers for the row's system, then returns the same shape
/// `media.meta` would (via `media_meta_response`) so the caller sees its
/// own write reflected immediately.
pub fn media_meta_update_response(params: &Value) -> Result<Value, String> {
    let key = media_ref_key(params);
    let Some(field) = params
        .get("media")
        .and_then(|media| media.get("launcherOverride"))
    else {
        return Err("no supported media updates provided".to_string());
    };
    if field.is_null() {
        if let Ok(mut overrides) = launcher_overrides().lock() {
            overrides.remove(&key);
        }
    } else if let Some(launcher_id) = field.as_str() {
        let (system, _path) = &key;
        let known = launchers_response()["launchers"]
            .as_array()
            .is_some_and(|launchers| {
                launchers.iter().any(|launcher| {
                    launcher
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id.eq_ignore_ascii_case(launcher_id))
                        && launcher
                            .get("systemId")
                            .and_then(Value::as_str)
                            .is_some_and(|system_id| system_id.eq_ignore_ascii_case(system))
                })
            });
        if !known {
            return Err(format!("launcher not found: {launcher_id}"));
        }
        if let Ok(mut overrides) = launcher_overrides().lock() {
            overrides.insert(key.clone(), launcher_id.to_string());
        }
    } else {
        return Err("media.launcherOverride must be a string or null".to_string());
    }
    Ok(media_meta_response(params))
}

pub fn media_browse_index_response(params: &Value) -> Value {
    let mut browse_params = params.clone();
    browse_params["maxResults"] = json!(1000);
    browse_params
        .as_object_mut()
        .map(|object| object.remove("cursor"));
    let browse = media_browse_response(&browse_params);
    let entries = browse["entries"].as_array().cloned().unwrap_or_default();
    // Bucket by first character over media entries only. A merged-root
    // page's leading `directory`/`root` entries aren't part of the
    // ordered file list Core's `offset` describes ("excludes any
    // directory entries the listing shows before files" -- see
    // `media.browse.index`'s documented `offset` semantics), so they
    // must not shift the file-position math below.
    let media_entries: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry["type"] == "media")
        .collect();
    let mut groups: Vec<Value> = Vec::new();
    for (offset, entry) in media_entries.iter().enumerate() {
        let first = entry["name"]
            .as_str()
            .and_then(|name| name.chars().next())
            .unwrap_or('#');
        let key = if first.is_ascii_alphabetic() {
            first.to_ascii_uppercase().to_string()
        } else if first.is_ascii_digit() {
            "0-9".to_string()
        } else {
            "#".to_string()
        };
        if let Some(group) = groups.last_mut().filter(|group| group["key"] == key) {
            let count = group["count"].as_u64().unwrap_or(0) + 1;
            group["count"] = json!(count);
        } else {
            groups.push(json!({
                "key": key.clone(),
                "label": key,
                "count": 1,
                "cursor": if offset == 0 { String::new() } else { offset.to_string() },
                "offset": offset,
            }));
        }
    }
    json!({ "scheme": "latin", "groups": groups })
}

pub fn media_history_latest_response() -> Value {
    let (name, file, system) = ALL_GAMES[0];
    json!({
        "entry": {
            "systemId": system,
            "systemName": system_display_for(system),
            "mediaName": name,
            "mediaPath": format!("/mock/{system}/{file}"),
            "launcherId": system,
            "startedAt": "2026-04-29T23:00:00Z",
        }
    })
}

pub fn media_history_response(params: &Value) -> Value {
    let systems = params
        .get("systems")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .min(100) as usize;
    let offset = params
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);
    let distinct_media = params
        .get("distinctMedia")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Synthesize newest-first play sessions. Non-distinct history repeats the
    // newest game once so tests can prove `distinctMedia` changes semantics;
    // distinct history exposes one row per `(systemId, mediaPath)` before the
    // cursor is applied, matching Core's pagination contract.
    let mut sessions: Vec<_> = ALL_GAMES
        .iter()
        .filter(|(_, _, system)| systems.is_empty() || systems.contains(system))
        .collect();
    if !distinct_media {
        if let Some(newest) = sessions.first().copied() {
            sessions.insert(1, newest);
        }
    }
    let total = sessions.len();
    let entries: Vec<Value> = sessions
        .into_iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(i, (name, file, system))| {
            let started = format!("2026-04-29T{:02}:00:00Z", 23 - i.min(23));
            let ended = format!("2026-04-29T{:02}:30:00Z", 23 - i.min(23));
            json!({
                "systemId": system,
                "systemName": system_display_for(system),
                "mediaName": name,
                "mediaPath": format!("/mock/{system}/{file}"),
                "launcherId": system,
                "hasCover": true,
                "startedAt": started,
                "endedAt": ended,
                "playTime": 1800,
            })
        })
        .collect();
    // Core's docs say `pagination` is only present when entries are
    // returned; mirror that so the frontend's MediaHistoryResult
    // deserialiser hits the same edges in mock as on real Core.
    let has_entries = !entries.is_empty();
    let next_offset = offset.saturating_add(entries.len());
    let mut response = json!({ "entries": entries });
    if has_entries {
        let has_next_page = next_offset < total;
        response["pagination"] = json!({
            "hasNextPage": has_next_page,
            "pageSize": limit,
        });
        if has_next_page {
            response["pagination"]["nextCursor"] = json!(next_offset.to_string());
        }
    }
    response
}

// Mirrors the display names in `systems_response`. The history fixture
// only has the system *id* in scope (via `ALL_GAMES`), so this lookup
// surfaces the same human-readable label Core would return.
fn system_display_for(id: &str) -> &str {
    match id {
        "NES" => "Nintendo Entertainment System",
        "SNES" => "Super Nintendo",
        "Genesis" => "Sega Genesis",
        "Nintendo64" => "Nintendo 64",
        "Gameboy" => "Game Boy",
        "GameboyColor" => "Game Boy Color",
        "GBA" => "Game Boy Advance",
        "NDS" => "Nintendo DS",
        "MAME" => "MAME",
        "NeoGeo" => "Neo Geo",
        _ => id,
    }
}

fn games_for_systems<'a>(systems: &'a [&'a str]) -> impl Iterator<Item = Value> + 'a {
    ALL_GAMES
        .iter()
        .enumerate()
        .filter_map(move |(index, (name, file, system))| {
            if !systems.is_empty() && !systems.contains(system) {
                return None;
            }
            let (system_name, category) = system_meta(system);
            Some(json!({
                "name": name,
                "path": format!("/mock/{system}/{file}"),
                "zapScript": format!("@{system}/{file}"),
                "system": { "id": system, "name": system_name, "category": category },
                "tags": tags_for(file, index),
                "disambiguatingTags": disambiguating_tags_for(file),
                "hasCover": true,
            }))
        })
}

/// Display name and category for a mock system id. Unknown ids use an
/// `Unknown` display name and an empty category.
fn system_meta(system: &str) -> (&'static str, &'static str) {
    MOCK_SYSTEMS
        .iter()
        .find_map(|(id, name, category)| (*id == system).then_some((*name, *category)))
        .unwrap_or(("Unknown", ""))
}

/// Full tag list for a search row: the disambiguation tags plus, for every
/// third entry, the `user:favorite` tag. A deterministic spread means the
/// mock's Favorites screen holds entries from several systems, which is what
/// the favorites sort and system-filter paths need to exercise.
fn tags_for(file: &str, index: usize) -> Value {
    let mut tags = disambiguating_tags_for(file)
        .as_array()
        .cloned()
        .unwrap_or_default();
    if index.is_multiple_of(3) {
        tags.push(json!({ "tag": "favorite", "type": "user" }));
    }
    Value::Array(tags)
}

// Synthesize disambiguating tags for a handful of mock entries so the
// variant-badge UI has something to render in `just run-dev`. Keyed on the
// filename so the same game always gets the same badges. Real Core derives
// these at index time from filename metadata across same-named siblings.
fn disambiguating_tags_for(file: &str) -> Value {
    match file {
        "smb.nes" | "pang_u.zip" => json!([{ "tag": "us", "type": "region" }]),
        "zelda.nes" => json!([{ "tag": "eu", "type": "region" }, { "tag": "1", "type": "rev" }]),
        "metroid.nes" => json!([{ "tag": "us,eu", "type": "region" }]),
        "sonic1.md" => json!([{ "tag": "1", "type": "disc" }]),
        "sonic2.md" => json!([{ "tag": "2", "type": "disc" }, { "tag": "ja", "type": "region" }]),
        // Arcade variants. `edition` is Core's catch-all for unrecognized
        // qualifiers, so the messy normalized values arrive there.
        "crossbow_joy.zip" => json!([{ "tag": "atari-joystick", "type": "edition" }]),
        "crossbow_gun.zip" => json!([{ "tag": "atari-lightgun", "type": "edition" }]),
        "arkanoid_uls.zip" => json!([{ "tag": "unl-lives-slow", "type": "edition" }]),
        "arkanoid_ul.zip" => json!([{ "tag": "unl-lives", "type": "edition" }]),
        "pang_w.zip" => json!([{ "tag": "world", "type": "region" }]),
        "aliensyn_s4.zip" => json!([{ "tag": "set-4-system-1", "type": "edition" }]),
        "aliensyn_s2.zip" => json!([{ "tag": "set-2-system-1", "type": "edition" }]),
        _ => json!([]),
    }
}

// (display name, filename, system id)
const ALL_GAMES: &[(&str, &str, &str)] = &[
    // NES
    ("Super Mario Bros.", "smb.nes", "NES"),
    ("The Legend of Zelda", "zelda.nes", "NES"),
    ("Metroid", "metroid.nes", "NES"),
    ("Mega Man 2", "mm2.nes", "NES"),
    ("Castlevania", "castlevania.nes", "NES"),
    // SNES
    ("Super Mario World", "smw.sfc", "SNES"),
    ("A Link to the Past", "alttp.sfc", "SNES"),
    ("Super Metroid", "sm.sfc", "SNES"),
    ("Chrono Trigger", "ct.sfc", "SNES"),
    ("F-Zero", "fzero.sfc", "SNES"),
    // Genesis
    ("Sonic the Hedgehog", "sonic1.md", "Genesis"),
    ("Sonic the Hedgehog 2", "sonic2.md", "Genesis"),
    ("Streets of Rage 2", "sor2.md", "Genesis"),
    ("Gunstar Heroes", "gunstar.md", "Genesis"),
    ("Ecco the Dolphin", "ecco.md", "Genesis"),
    // Nintendo 64
    ("Super Mario 64", "sm64.z64", "Nintendo64"),
    ("Ocarina of Time", "oot.z64", "Nintendo64"),
    ("GoldenEye 007", "goldeneye.z64", "Nintendo64"),
    ("Mario Kart 64", "mk64.z64", "Nintendo64"),
    ("Perfect Dark", "pd.z64", "Nintendo64"),
    // Game Boy
    ("Tetris", "tetris.gb", "Gameboy"),
    ("Pokemon Red", "pokered.gb", "Gameboy"),
    ("Link's Awakening", "la.gb", "Gameboy"),
    ("Super Mario Land", "sml.gb", "Gameboy"),
    ("Metroid II", "metroid2.gb", "Gameboy"),
    // Game Boy Color
    ("Pokemon Crystal", "pokecrystal.gbc", "GameboyColor"),
    (
        "Zelda: Oracle of Ages",
        "oracle_of_ages.gbc",
        "GameboyColor",
    ),
    ("Wario Land 3", "wl3.gbc", "GameboyColor"),
    ("Dragon Warrior III", "dw3.gbc", "GameboyColor"),
    ("Shantae", "shantae.gbc", "GameboyColor"),
    // Game Boy Advance
    ("Metroid Fusion", "fusion.gba", "GBA"),
    ("Castlevania: Aria of Sorrow", "aos.gba", "GBA"),
    ("Pokemon Emerald", "emerald.gba", "GBA"),
    ("Advance Wars", "aw.gba", "GBA"),
    ("Golden Sun", "gs.gba", "GBA"),
    // Nintendo DS
    ("Super Mario 64 DS", "sm64ds.nds", "NDS"),
    ("Mario Kart DS", "mkds.nds", "NDS"),
    ("Phoenix Wright", "pw.nds", "NDS"),
    ("Pokemon Diamond", "diamond.nds", "NDS"),
    ("The World Ends With You", "twewy.nds", "NDS"),
    // MAME
    ("Pac-Man", "pacman.zip", "MAME"),
    ("Donkey Kong", "dkong.zip", "MAME"),
    ("Galaga", "galaga.zip", "MAME"),
    ("Street Fighter II", "sf2.zip", "MAME"),
    ("Ms. Pac-Man", "mspacman.zip", "MAME"),
    // Same-named arcade variants (kept adjacent) to exercise the sibling-diff +
    // width policy: shared-prefix values, length-difference values, region
    // world->w, and a long value that must not hide the title.
    ("Crossbow", "crossbow_joy.zip", "MAME"),
    ("Crossbow", "crossbow_gun.zip", "MAME"),
    ("Arkanoid", "arkanoid_uls.zip", "MAME"),
    ("Arkanoid", "arkanoid_ul.zip", "MAME"),
    ("Pang", "pang_w.zip", "MAME"),
    ("Pang", "pang_u.zip", "MAME"),
    ("Alien Syndrome", "aliensyn_s4.zip", "MAME"),
    ("Alien Syndrome", "aliensyn_s2.zip", "MAME"),
    // Neo Geo
    ("Metal Slug", "mslug.neo", "NeoGeo"),
    ("The King of Fighters '98", "kof98.neo", "NeoGeo"),
    ("Samurai Shodown", "samsho.neo", "NeoGeo"),
    ("Fatal Fury", "fatfury.neo", "NeoGeo"),
    ("Garou: Mark of the Wolves", "garou.neo", "NeoGeo"),
];
