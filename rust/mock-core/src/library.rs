// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The user's own flags on games: favorite, liked, disliked and play later,
// keyed by the game's canonical path, each with the time it was set. Core
// keeps these in its UserDB; the mock seeds a deterministic spread over the
// fixture library and lets `media.tags.update` change it. `hidden` stays in
// `fixtures` because it also applies to folders.

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::fixtures;
use crate::media_state::Notifier;

/// The user list flags, in the order every response lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    Favorite,
    Liked,
    Disliked,
    PlayLater,
}

impl Flag {
    pub const ALL: [Flag; 4] = [Flag::Favorite, Flag::Liked, Flag::Disliked, Flag::PlayLater];

    pub fn tag(self) -> &'static str {
        match self {
            Flag::Favorite => "favorite",
            Flag::Liked => "liked",
            Flag::Disliked => "disliked",
            Flag::PlayLater => "playlater",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Flag> {
        Flag::ALL.into_iter().find(|flag| flag.tag() == tag)
    }
}

/// Set flags per path. A set flag maps to the time it was set, or `None`
/// for a flag recorded before Core kept set times.
#[derive(Debug, Default)]
struct FlagStore {
    flags: HashMap<String, HashMap<Flag, Option<u64>>>,
    clock: u64,
}

impl FlagStore {
    /// Favorite on every third game, as the mock always had; liked on every
    /// fourth from the third; play later on every seventh from the fourth;
    /// disliked on every eleventh from the sixth unless the game is already
    /// liked or a favorite. Every fourth seeded flag has no set time, and
    /// the rest get times older than anything set later.
    fn seeded() -> Self {
        let mut store = FlagStore::default();
        let mut seeded: u64 = 0;
        for (index, path) in fixtures::library_paths().enumerate() {
            let favorite = index.is_multiple_of(3);
            let liked = index % 4 == 2;
            let wanted = [
                (Flag::Favorite, favorite),
                (Flag::Liked, liked),
                (Flag::Disliked, index % 11 == 5 && !favorite && !liked),
                (Flag::PlayLater, index % 7 == 3),
            ];
            for (flag, set) in wanted {
                if !set {
                    continue;
                }
                seeded += 1;
                let at = (!seeded.is_multiple_of(4)).then_some(seeded);
                store
                    .flags
                    .entry(path.clone())
                    .or_default()
                    .insert(flag, at);
            }
        }
        store.clock = 1000;
        store
    }

    fn set(&self, path: &str) -> Vec<Flag> {
        let Some(flags) = self.flags.get(path) else {
            return Vec::new();
        };
        Flag::ALL
            .into_iter()
            .filter(|flag| flags.contains_key(flag))
            .collect()
    }

    /// Applies one request's changes with Core's exclusivity: setting
    /// disliked clears liked and favorite, and setting either of those
    /// clears disliked. A flag already set keeps its time. Returns whether
    /// anything changed.
    fn apply(&mut self, path: &str, changes: &HashMap<Flag, bool>) -> bool {
        let before = self.set(path);
        for flag in Flag::ALL {
            match changes.get(&flag) {
                Some(true) => {
                    let cleared: &[Flag] = match flag {
                        Flag::Disliked => &[Flag::Liked, Flag::Favorite],
                        Flag::Liked | Flag::Favorite => &[Flag::Disliked],
                        Flag::PlayLater => &[],
                    };
                    self.clock += 1;
                    let now = self.clock;
                    let flags = self.flags.entry(path.to_string()).or_default();
                    for other in cleared {
                        flags.remove(other);
                    }
                    flags.entry(flag).or_insert(Some(now));
                }
                Some(false) => {
                    if let Some(flags) = self.flags.get_mut(path) {
                        flags.remove(&flag);
                    }
                }
                None => {}
            }
        }
        if self.flags.get(path).is_some_and(HashMap::is_empty) {
            self.flags.remove(path);
        }
        self.set(path) != before
    }
}

/// Runs `f` on the flag store. The server shares one store across every
/// connection; unit tests get one per test thread so a test that changes a
/// flag never disturbs another test's counts.
fn with_store<R>(f: impl FnOnce(&mut FlagStore) -> R) -> R {
    #[cfg(not(test))]
    {
        use std::sync::{Mutex, OnceLock, PoisonError};
        static STORE: OnceLock<Mutex<FlagStore>> = OnceLock::new();
        let mut guard = STORE
            .get_or_init(|| Mutex::new(FlagStore::seeded()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }
    #[cfg(test)]
    {
        thread_local! {
            static STORE: std::cell::RefCell<FlagStore> =
                std::cell::RefCell::new(FlagStore::seeded());
        }
        STORE.with_borrow_mut(f)
    }
}

/// The flags set on `path`, in `Flag::ALL` order.
pub fn flags_for(path: &str) -> Vec<Flag> {
    with_store(|store| store.set(path))
}

/// When `flag` was set on `path`; None when it is not set or was set
/// without a recorded time.
pub fn set_time(path: &str, flag: Flag) -> Option<u64> {
    with_store(|store| store.flags.get(path)?.get(&flag).copied().flatten())
}

fn user_tag(tag: &str) -> Value {
    json!({ "type": "user", "tag": tag })
}

/// The item's user utility tags: its flags, then hidden.
pub fn utility_tags(path: &str) -> Vec<Value> {
    let mut tags: Vec<Value> = flags_for(path)
        .into_iter()
        .map(|flag| user_tag(flag.tag()))
        .collect();
    if fixtures::is_hidden_itself(path) {
        tags.push(user_tag("hidden"));
    }
    tags
}

/// One tag a client asked to add or remove, parsed the way Core parses
/// mutable user tags.
enum Mutable {
    Flag(Flag),
    Hidden,
}

fn parse_mutable(raw: &str) -> Result<Mutable, String> {
    if raw.trim().is_empty() {
        return Err("tag cannot be empty".into());
    }
    if raw.starts_with(['-', '~', '+']) {
        return Err(format!(
            "tag operators are not allowed for mutation: {raw:?}"
        ));
    }
    let allowed =
        "only these user tags can be mutated: favorite, liked, disliked, playlater, hidden";
    let Some(("user", value)) = raw.split_once(':') else {
        return Err(allowed.into());
    };
    if value.starts_with("deck:") {
        return Err("deck membership is managed through the decks methods".into());
    }
    if value == "hidden" {
        return Ok(Mutable::Hidden);
    }
    Flag::from_tag(value)
        .map(Mutable::Flag)
        .ok_or_else(|| allowed.into())
}

fn tag_list(params: &Value, key: &str, label: &str) -> Result<Vec<Mutable>, String> {
    params.get(key).and_then(Value::as_array).map_or_else(
        || Ok(Vec::new()),
        |tags| {
            tags.iter()
                .map(|tag| {
                    parse_mutable(tag.as_str().unwrap_or_default())
                        .map_err(|e| format!("invalid {label} tags: {e}"))
                })
                .collect()
        },
    )
}

/// `media.tags.update`: validates the exclusive media ref the way Core does
/// (a `mediaId` or a `(system, path)` pair, never both), applies the flag
/// changes with Core's exclusivity rules, and answers with the item's user
/// tags. Any change is announced with `media.tags.changed`.
pub fn tags_update(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let media_id = params.get("mediaId").and_then(Value::as_i64);
    let has_id = params.get("mediaId").is_some();
    let system = params.get("system").and_then(Value::as_str).unwrap_or("");
    let path = params.get("path").and_then(Value::as_str).unwrap_or("");
    if has_id && (!system.is_empty() || !path.is_empty()) {
        return Err("invalid params: mediaId cannot be mixed with system/path".into());
    }
    if !has_id && (system.is_empty() || path.is_empty()) {
        return Err("invalid params: mediaId or system/path is required".into());
    }
    let add = tag_list(params, "add", "add")?;
    let remove = tag_list(params, "remove", "remove")?;
    if add.is_empty() && remove.is_empty() {
        return Err("invalid params: add or remove is required".into());
    }
    tracing::info!(%params, "media.tags.update");

    // Adds win over removes, as in Core.
    let mut changes: HashMap<Flag, bool> = HashMap::new();
    let mut hidden: Option<bool> = None;
    for (list, value) in [(&remove, false), (&add, true)] {
        for tag in list {
            match tag {
                Mutable::Flag(flag) => {
                    changes.insert(*flag, value);
                }
                Mutable::Hidden => hidden = Some(value),
            }
        }
    }
    if changes.get(&Flag::Disliked) == Some(&true) {
        if changes.get(&Flag::Liked) == Some(&true) {
            return Err("invalid params: a game cannot be liked and disliked at once".into());
        }
        if changes.get(&Flag::Favorite) == Some(&true) {
            return Err("invalid params: a favorite cannot be disliked".into());
        }
    }

    // The mock has no media ids, so an id ref names a synthetic path the
    // same way `media.meta` does.
    let (system, path) = if has_id {
        let id = media_id.unwrap_or_default();
        ("Mock".to_string(), format!("/mock/media/{id}"))
    } else {
        (system.to_string(), path.to_string())
    };

    let mut changed = with_store(|store| store.apply(&path, &changes));
    if let Some(hide) = hidden {
        if fixtures::is_hidden_itself(&path) != hide {
            fixtures::set_hidden(&path, hide);
            changed = true;
        }
    }
    let tags = utility_tags(&path);
    if changed {
        notifier.send(
            "media.tags.changed",
            &json!({ "system": system, "path": path, "tags": tags }),
        );
    }
    Ok(json!({ "tags": tags }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_disliked_never_shares_a_game_with_liked_or_favorite() {
        let store = FlagStore::seeded();
        let mut disliked = 0;
        for flags in store.flags.values() {
            if flags.contains_key(&Flag::Disliked) {
                disliked += 1;
                assert!(!flags.contains_key(&Flag::Liked));
                assert!(!flags.contains_key(&Flag::Favorite));
            }
        }
        assert!(disliked > 0);
        for flag in Flag::ALL {
            let set: Vec<Option<u64>> = store
                .flags
                .values()
                .filter_map(|flags| flags.get(&flag).copied())
                .collect();
            assert!(!set.is_empty(), "{flag:?} is seeded");
        }
        let undated = store
            .flags
            .values()
            .flat_map(HashMap::values)
            .filter(|at| at.is_none())
            .count();
        assert!(undated > 0, "some seeded flags have no time");
        assert!(store
            .flags
            .values()
            .flat_map(HashMap::values)
            .flatten()
            .all(|at| *at < store.clock));
    }
}
