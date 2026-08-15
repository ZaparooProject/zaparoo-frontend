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
use std::sync::{Mutex, OnceLock};

static SYSTEM_DEFAULTS: OnceLock<Mutex<Vec<SystemDefaultFixture>>> = OnceLock::new();

const MOCK_SYSTEMS: &[(&str, &str, &str)] = &[
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

    // `total` is deprecated upstream and always returns -1; pagination
    // info travels under the `pagination` envelope.
    let mut pagination = json!({
        "hasNextPage": has_next_page,
        "pageSize": max,
    });
    if has_next_page {
        pagination["nextCursor"] = json!(next_offset.to_string());
    }
    json!({
        "results": results,
        "total": -1,
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
    let offset = params
        .get("cursor")
        .and_then(Value::as_str)
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);
    let mut matching: Vec<Value> = ALL_GAMES
        .iter()
        .enumerate()
        .filter(|(_, (_, _, system))| systems.is_empty() || systems.contains(system))
        .filter_map(|(index, (name, file, system))| {
            let row = json!({
                "name": name,
                "path": format!("{path}/{file}"),
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
        // Mock browse returns only media entries (no subdirectories).
        "totalDirs": 0,
        "pagination": pagination,
    })
}

pub fn media_meta_response(params: &Value) -> Value {
    fn media_for(reference: &Value) -> Value {
        let media_id = reference.get("mediaId").and_then(Value::as_i64);
        let system = reference
            .get("system")
            .and_then(Value::as_str)
            .unwrap_or("Mock");
        let path = reference.get("path").and_then(Value::as_str).map_or_else(
            || format!("/mock/media/{}", media_id.unwrap_or_default()),
            str::to_string,
        );
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or("Mock Game")
            .split('.')
            .next()
            .unwrap_or("Mock Game")
            .to_string();
        json!({
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
                "system": {"id": system, "name": system_display_for(system)},
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
        })
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

pub fn media_browse_index_response(params: &Value) -> Value {
    let mut browse_params = params.clone();
    browse_params["maxResults"] = json!(1000);
    browse_params
        .as_object_mut()
        .map(|object| object.remove("cursor"));
    let browse = media_browse_response(&browse_params);
    let entries = browse["entries"].as_array().cloned().unwrap_or_default();
    let mut groups: Vec<Value> = Vec::new();
    for (offset, entry) in entries.iter().enumerate() {
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

    // Synthesize a history list from the first ten games in `ALL_GAMES`,
    // newest first. Real Core sorts by `endedAt` descending; the mock
    // just walks the array and stamps backward-counting timestamps so
    // the order is stable across runs.
    let entries: Vec<Value> = ALL_GAMES
        .iter()
        .filter(|(_, _, system)| systems.is_empty() || systems.contains(system))
        .take(limit)
        .enumerate()
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
    let mut response = json!({ "entries": entries });
    if has_entries {
        response["pagination"] = json!({
            "hasNextPage": false,
            "pageSize": limit,
        });
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
