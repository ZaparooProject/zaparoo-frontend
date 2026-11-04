// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Stateful decks: `decks`, `decks.get`, `decks.new`, `decks.update`,
// `decks.delete` and `decks.image`, with a `decks.changed` notification
// after every change. Shapes and error wording follow Core's deck methods.
// Search rows carry `user:deck:<id>` for every deck holding the game, read
// from this store, so membership tags always match deck contents.

use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fixtures;
use crate::media_state::Notifier;

/// The owned deck the mock always had; its id is kept so persisted
/// frontend state that names it still resolves.
pub const COUCH_DECK_ID: &str = "0k3v9x2rq7bm";
pub const WEEKEND_DECK_ID: &str = "w3k8e7nd2p4x";
pub const CLUB_DECK_ID: &str = "c1ub9n7ght4k";
pub const ARCADE_DECK_ID: &str = "a7c4d3e2x9q1";
pub const TAPPED_DECK_ID: &str = "t4p9d3ck8m2v";

/// Core's per-deck item limit.
const MAX_ITEMS: usize = 120;

const NOT_FOUND: &str = "deck not found";
const READ_ONLY: &str = "deck is read-only";
const ITEM_LIMIT: &str = "deck item limit reached";
const ITEM_NOT_FOUND: &str = "deck item not found";
const IMAGE_NOT_FOUND: &str = "image not found";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Cover {
    None,
    /// A game's art, as `decks.update` `cover` `media` sets it.
    Media {
        system: String,
        path: String,
    },
    /// A file on the Core host.
    File {
        path: String,
    },
    /// An image Online sent for a saved deck.
    Online,
}

#[derive(Clone, Debug)]
struct ItemMedia {
    system: String,
    path: String,
    name: String,
}

#[derive(Clone, Debug)]
struct Item {
    id: i64,
    /// `script` or `card`. A game is a script item that also names its
    /// media.
    kind: &'static str,
    name: String,
    zapscript: Option<String>,
    card_id: Option<String>,
    media: Option<ItemMedia>,
    has_image: bool,
}

impl Item {
    fn json(&self, position: usize) -> Value {
        let mut item = json!({
            "id": self.id,
            "position": position,
            "kind": self.kind,
            "name": self.name,
            "hasImage": self.has_image,
        });
        if let Some(zapscript) = &self.zapscript {
            item["zapscript"] = json!(zapscript);
        }
        if let Some(card_id) = &self.card_id {
            item["cardId"] = json!(card_id);
        }
        if let Some(media) = &self.media {
            item["media"] = json!({
                "system": media.system,
                "path": media.path,
                "name": media.name,
                "available": fixtures::library_game(&media.system, &media.path).is_some(),
                "userTags": crate::library::utility_tags(&media.path),
            });
        }
        item
    }
}

#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags mirroring Core's deck object"
)]
struct Deck {
    id: String,
    name: String,
    description: String,
    owned: bool,
    locked: bool,
    saved: bool,
    owned_by: Option<String>,
    source_url: Option<String>,
    cover: Cover,
    created_at: i64,
    updated_at: i64,
    items: Vec<Item>,
}

impl Deck {
    fn summary(&self) -> Value {
        let mut deck = json!({
            "deckId": self.id,
            "name": self.name,
            "description": self.description,
            "itemCount": self.items.len(),
            "owned": self.owned,
            "locked": self.locked,
            "saved": self.saved,
            "hasImage": self.cover != Cover::None,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        });
        if let Some(owned_by) = &self.owned_by {
            deck["ownedBy"] = json!(owned_by);
        }
        if let Some(source_url) = &self.source_url {
            deck["sourceUrl"] = json!(source_url);
        }
        deck
    }

    fn full(&self) -> Value {
        let mut deck = self.summary();
        deck["items"] = Value::Array(
            self.items
                .iter()
                .enumerate()
                .map(|(index, item)| item.json(index + 1))
                .collect(),
        );
        deck
    }

    fn writable(&self) -> bool {
        self.owned && !self.locked
    }
}

#[derive(Debug)]
struct DeckStore {
    decks: Vec<Deck>,
    next_item_id: i64,
    next_deck: u64,
}

impl DeckStore {
    fn seeded() -> Self {
        let mut store = DeckStore {
            decks: Vec::new(),
            next_item_id: 1,
            next_deck: 1,
        };
        let library = |store: &mut DeckStore, indices: &[usize]| -> Vec<Item> {
            indices
                .iter()
                .filter_map(|index| fixtures::library_entry(*index))
                .map(|(name, system, path)| store.game_item(system, &path, name))
                .collect()
        };

        // Eight library games, one game the library does not have, a card
        // with art and a plain script.
        let mut couch = library(&mut store, &[1, 5, 9, 13, 17, 21, 25, 29]);
        couch.push(store.game_item("SNES", "/mock/games/SNES/earthbound.sfc", "EarthBound"));
        let id = store.item_id();
        couch.push(Item {
            id,
            kind: "card",
            name: "Party night".into(),
            zapscript: Some("**launch.random:SNES".into()),
            card_id: Some("c8x2kq4m7v1d".into()),
            media: None,
            has_image: true,
        });
        let id = store.item_id();
        couch.push(Item {
            id,
            kind: "script",
            name: "Random Genesis game".into(),
            zapscript: Some("**launch.random:Genesis".into()),
            card_id: None,
            media: None,
            has_image: false,
        });
        let club = library(&mut store, &[43, 54, 56, 57]);
        let arcade = library(&mut store, &[40, 41, 42, 44]);
        let tapped = library(&mut store, &[0, 15]);

        let deck = |id: &str, name: &str, updated_at: i64, items: Vec<Item>| Deck {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            owned: true,
            locked: false,
            saved: false,
            owned_by: None,
            source_url: None,
            cover: Cover::None,
            created_at: updated_at - 86_400,
            updated_at,
            items,
        };
        store.decks = vec![
            deck(COUCH_DECK_ID, "Couch co-op", 1_793_000_000, couch),
            deck(WEEKEND_DECK_ID, "Weekend", 1_792_900_000, Vec::new()),
            Deck {
                locked: true,
                ..deck(CLUB_DECK_ID, "Club night", 1_792_800_000, club)
            },
            Deck {
                owned: false,
                saved: true,
                owned_by: Some("wizzo".into()),
                source_url: Some(format!("https://zaparoo.com/decks/{ARCADE_DECK_ID}")),
                cover: Cover::Online,
                ..deck(ARCADE_DECK_ID, "Arcade classics", 1_792_700_000, arcade)
            },
            Deck {
                owned: false,
                source_url: Some(format!("https://zaparoo.com/decks/{TAPPED_DECK_ID}")),
                ..deck(TAPPED_DECK_ID, "Tapped deck", 1_792_600_000, tapped)
            },
        ];
        store
    }

    fn item_id(&mut self) -> i64 {
        let id = self.next_item_id;
        self.next_item_id += 1;
        id
    }

    fn game_item(&mut self, system: &str, path: &str, name: &str) -> Item {
        let file = path.rsplit('/').next().unwrap_or_default();
        Item {
            id: self.item_id(),
            kind: "script",
            name: name.into(),
            zapscript: Some(format!("@{system}/{file}")),
            card_id: None,
            media: Some(ItemMedia {
                system: system.into(),
                path: path.into(),
                name: name.into(),
            }),
            has_image: false,
        }
    }

    fn find(&self, id: &str) -> Option<&Deck> {
        self.decks
            .iter()
            .find(|deck| deck.id.eq_ignore_ascii_case(id))
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut Deck> {
        self.decks
            .iter_mut()
            .find(|deck| deck.id.eq_ignore_ascii_case(id))
    }

    /// A change time newer than every deck's, so a changed deck heads the
    /// list even when the host clock is behind the seeded times.
    fn change_time(&self) -> i64 {
        let latest = self
            .decks
            .iter()
            .map(|deck| deck.updated_at)
            .max()
            .unwrap_or_default();
        now_secs().max(latest + 1)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Runs `f` on the deck store. The server shares one store across every
/// connection; unit tests get one per test thread so tests that create or
/// delete decks never disturb one another.
fn with_store<R>(f: impl FnOnce(&mut DeckStore) -> R) -> R {
    #[cfg(not(test))]
    {
        use std::sync::{Mutex, OnceLock, PoisonError};
        static STORE: OnceLock<Mutex<DeckStore>> = OnceLock::new();
        let mut guard = STORE
            .get_or_init(|| Mutex::new(DeckStore::seeded()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }
    #[cfg(test)]
    {
        thread_local! {
            static STORE: std::cell::RefCell<DeckStore> =
                std::cell::RefCell::new(DeckStore::seeded());
        }
        STORE.with_borrow_mut(f)
    }
}

/// Ids of the decks with an item linked to `system`/`path`, for the
/// `user:deck:<id>` membership tags on search and browse rows.
pub fn deck_ids_containing(system: &str, path: &str) -> Vec<String> {
    with_store(|store| {
        store
            .decks
            .iter()
            .filter(|deck| {
                deck.items.iter().any(|item| {
                    item.media
                        .as_ref()
                        .is_some_and(|media| media.system == system && media.path == path)
                })
            })
            .map(|deck| deck.id.clone())
            .collect()
    })
}

/// One playable entry of a deck, for the playlist picker.
pub struct PickerEntry {
    pub id: i64,
    pub label: String,
    pub zapscript: Option<String>,
    /// `(system, path, name)` of a game the library has.
    pub media: Option<(String, String, String)>,
}

/// A deck's name and items, for `**playlist.open:deck://<id>`.
pub fn picker_source(deck_id: &str) -> Option<(String, Vec<PickerEntry>)> {
    with_store(|store| {
        let deck = store.find(deck_id)?;
        let entries = deck
            .items
            .iter()
            .map(|item| PickerEntry {
                id: item.id,
                label: item.name.clone(),
                zapscript: item.zapscript.clone(),
                media: item
                    .media
                    .as_ref()
                    .filter(|media| fixtures::library_game(&media.system, &media.path).is_some())
                    .map(|media| (media.system.clone(), media.path.clone(), media.name.clone())),
            })
            .collect();
        Some((deck.name.clone(), entries))
    })
}

fn deck_id_param(params: &Value) -> Result<&str, String> {
    params
        .get("deckId")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "invalid params: deckId is required".to_string())
}

fn changed(notifier: &Notifier, deck_id: &str, action: &str) {
    notifier.send(
        "decks.changed",
        &json!({ "deckId": deck_id, "action": action }),
    );
}

/// `decks`: every deck without items, most recently changed first. With a
/// `system` and `path`, every deck also lists `containsItemIds`.
pub fn list(params: &Value) -> Result<Value, String> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let game = match (text("system"), text("path")) {
        (Some(system), Some(path)) => Some((system, path)),
        (None, None) => None,
        _ => return Err("invalid params: system and path must be given together".into()),
    };
    with_store(|store| {
        let mut decks: Vec<&Deck> = store.decks.iter().collect();
        decks.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.name.cmp(&b.name))
        });
        let decks: Vec<Value> = decks
            .into_iter()
            .map(|deck| {
                let mut summary = deck.summary();
                if let Some((system, path)) = game {
                    let ids: Vec<i64> = deck
                        .items
                        .iter()
                        .filter(|item| {
                            item.media
                                .as_ref()
                                .is_some_and(|media| media.system == system && media.path == path)
                        })
                        .map(|item| item.id)
                        .collect();
                    summary["containsItemIds"] = json!(ids);
                }
                summary
            })
            .collect();
        Ok(json!({ "decks": decks }))
    })
}

/// `decks.get`: one deck with its items.
pub fn get(params: &Value) -> Result<Value, String> {
    let id = deck_id_param(params)?;
    with_store(|store| {
        store
            .find(id)
            .map(Deck::full)
            .ok_or_else(|| NOT_FOUND.into())
    })
}

/// A client's item input before it is given an id.
enum Input {
    /// Keeps an existing item, in `decks.update` `items`.
    Keep(i64),
    New(Item),
}

fn parse_item(input: &Value, allow_keep: bool) -> Result<Input, String> {
    let fields = input.as_object().cloned().unwrap_or_else(Map::new);
    let text = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    if let Some(id) = fields.get("id") {
        if !allow_keep {
            return Err("only the items of decks.update can name an existing item by id".into());
        }
        if fields.len() > 1 {
            return Err("an item named by id takes no other fields".into());
        }
        return id
            .as_i64()
            .map(Input::Keep)
            .ok_or_else(|| "item id must be a number".into());
    }
    let kind = text("kind").unwrap_or_default();
    let item = match kind.as_str() {
        "media" => {
            let (Some(system), Some(path)) = (text("system"), text("path")) else {
                return Err("media not found".into());
            };
            let name = fixtures::library_game(&system, &path).ok_or("media not found")?;
            let file = path.rsplit('/').next().unwrap_or_default();
            Item {
                id: 0,
                kind: "script",
                name: name.into(),
                zapscript: Some(format!("@{system}/{file}")),
                card_id: None,
                media: Some(ItemMedia {
                    system,
                    path: path.clone(),
                    name: name.into(),
                }),
                has_image: false,
            }
        }
        "script" => {
            let zapscript = text("zapscript").ok_or("a script item needs a zapscript")?;
            let name = text("name").ok_or("a script item needs a name")?;
            Item {
                id: 0,
                kind: "script",
                name,
                zapscript: Some(zapscript),
                card_id: None,
                media: None,
                has_image: false,
            }
        }
        "card" => {
            let card_id = text("cardId").ok_or("a card item needs a cardId")?;
            Item {
                id: 0,
                kind: "card",
                name: text("name").unwrap_or_else(|| card_id.clone()),
                zapscript: text("zapscript"),
                card_id: Some(card_id),
                media: None,
                has_image: false,
            }
        }
        "" => return Err("an item needs a kind, or an id to keep an existing item".into()),
        other => return Err(format!("unknown item kind {other:?}")),
    };
    Ok(Input::New(item))
}

fn parse_items(params: &Value, key: &str, allow_keep: bool) -> Result<Option<Vec<Input>>, String> {
    let Some(list) = params.get(key).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let list = list
        .as_array()
        .ok_or_else(|| format!("invalid params: {key} must be a list"))?;
    if list.len() > MAX_ITEMS {
        return Err(ITEM_LIMIT.into());
    }
    list.iter()
        .enumerate()
        .map(|(index, input)| {
            parse_item(input, allow_keep).map_err(|e| format!("invalid item {}: {e}", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// `decks.new`: an owned deck from a name and items, answered with its
/// items.
pub fn new(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or("invalid params: name is required")?
        .to_string();
    let description = params
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let inputs = parse_items(params, "items", false)?.unwrap_or_default();
    let deck = with_store(|store| {
        let items = inputs
            .into_iter()
            .filter_map(|input| match input {
                Input::New(item) => Some(item),
                Input::Keep(_) => None,
            })
            .map(|item| Item {
                id: store.item_id(),
                ..item
            })
            .collect();
        let id = format!("mk{:010}", store.next_deck);
        store.next_deck += 1;
        let now = store.change_time();
        let deck = Deck {
            id,
            name,
            description,
            owned: true,
            locked: false,
            saved: false,
            owned_by: None,
            source_url: None,
            cover: Cover::None,
            created_at: now,
            updated_at: now,
            items,
        };
        let full = deck.full();
        store.decks.push(deck);
        full
    });
    if let Some(id) = deck["deckId"].as_str() {
        changed(notifier, id, "created");
    }
    Ok(deck)
}

fn parse_cover(params: &Value) -> Result<Option<Cover>, String> {
    let Some(cover) = params.get("cover").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let text = |key: &str| {
        cover
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match cover.get("kind").and_then(Value::as_str) {
        Some("media") => {
            let (Some(system), Some(path)) = (text("system"), text("path")) else {
                return Err("invalid params: a media cover needs system and path".into());
            };
            if fixtures::library_game(&system, &path).is_none() {
                return Err("media not found".into());
            }
            Ok(Some(Cover::Media { system, path }))
        }
        Some("file") => {
            let path = text("path")
                .filter(|path| path.starts_with('/'))
                .ok_or("invalid params: a file cover needs an absolute path")?;
            Ok(Some(Cover::File { path }))
        }
        Some("none") => Ok(Some(Cover::None)),
        _ => Err("invalid params: cover kind must be media, file or none".into()),
    }
}

/// `decks.update`: renames, reorders (a full `items` list that may keep
/// items by id), appends (`addItems`), removes (`removeItemIds`) and sets
/// the cover of an owned, unlocked deck, applied as remove, replace, then
/// append. A request that asks for no change answers with the deck as it
/// is.
pub fn update(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let id = deck_id_param(params)?.to_string();
    let name = params
        .get("name")
        .filter(|value| !value.is_null())
        .map(|name| name.as_str().map(str::trim).unwrap_or_default().to_string());
    let description = params
        .get("description")
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string());
    let replaced = parse_items(params, "items", true)?;
    let added = parse_items(params, "addItems", false)?.unwrap_or_default();
    let removed: Vec<i64> = params
        .get("removeItemIds")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let cover = parse_cover(params)?;

    let deck = with_store(|store| -> Result<Value, String> {
        let deck = store.find(&id).ok_or(NOT_FOUND)?;
        if name.is_none()
            && description.is_none()
            && replaced.is_none()
            && added.is_empty()
            && removed.is_empty()
            && cover.is_none()
        {
            return Ok(deck.full());
        }
        if !deck.writable() {
            return Err(READ_ONLY.into());
        }
        if name.as_deref() == Some("") {
            return Err("invalid params: name cannot be empty".into());
        }

        let current = deck.items.clone();
        let mut items: Vec<Item> = if let Some(replaced) = replaced {
            let mut kept: Vec<i64> = Vec::new();
            let mut out = Vec::with_capacity(replaced.len());
            for input in replaced {
                match input {
                    Input::New(item) => out.push(item),
                    Input::Keep(item_id) => {
                        let item = current
                            .iter()
                            .find(|item| item.id == item_id)
                            .ok_or_else(|| format!("{ITEM_NOT_FOUND}: item {item_id}"))?;
                        if kept.contains(&item_id) {
                            return Err(format!("deck item listed more than once: item {item_id}"));
                        }
                        kept.push(item_id);
                        out.push(item.clone());
                    }
                }
            }
            out
        } else {
            current
                .into_iter()
                .filter(|item| !removed.contains(&item.id))
                .collect()
        };
        items.extend(added.into_iter().filter_map(|input| match input {
            Input::New(item) => Some(item),
            Input::Keep(_) => None,
        }));
        if items.len() > MAX_ITEMS {
            return Err(ITEM_LIMIT.into());
        }
        for item in &mut items {
            if item.id == 0 {
                item.id = store.item_id();
            }
        }
        let now = store.change_time();
        let deck = store.find_mut(&id).ok_or(NOT_FOUND)?;
        if let Some(name) = name {
            deck.name = name;
        }
        if let Some(description) = description {
            deck.description = description;
        }
        if let Some(cover) = cover {
            deck.cover = cover;
        }
        deck.items = items;
        deck.updated_at = now;
        Ok(deck.full())
    })?;
    if let Some(id) = deck["deckId"].as_str() {
        changed(notifier, id, "updated");
    }
    Ok(deck)
}

/// `decks.delete`: removes a deck, owned or cached.
pub fn delete(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let id = deck_id_param(params)?;
    let removed = with_store(|store| {
        let index = store
            .decks
            .iter()
            .position(|deck| deck.id.eq_ignore_ascii_case(id))?;
        Some(store.decks.remove(index).id)
    })
    .ok_or(NOT_FOUND)?;
    changed(notifier, &removed, "deleted");
    Ok(Value::Null)
}

/// `decks.image`: the deck's cover, or with `itemId` a card item's art, in
/// the `media.image` result shape. The art is the mock's placeholder cover,
/// so a media cover draws the same image `media.image` gives that game.
pub fn image(params: &Value) -> Result<Value, String> {
    let id = deck_id_param(params)?;
    let item_id = params.get("itemId").and_then(Value::as_i64);
    let (system, path, type_tag) = with_store(|store| -> Result<_, String> {
        let deck = store.find(id).ok_or(NOT_FOUND)?;
        if let Some(item_id) = item_id {
            let item = deck
                .items
                .iter()
                .find(|item| item.id == item_id)
                .ok_or(ITEM_NOT_FOUND)?;
            if item.kind != "card" || !item.has_image {
                return Err(IMAGE_NOT_FOUND.into());
            }
            return Ok((
                "Deck".to_string(),
                format!("deck://{}/items/{item_id}", deck.id),
                "deck:card",
            ));
        }
        let (system, path) = match &deck.cover {
            Cover::None => return Err(IMAGE_NOT_FOUND.into()),
            Cover::Media { system, path } => (system.clone(), path.clone()),
            Cover::File { path } => ("Deck".to_string(), path.clone()),
            Cover::Online => ("Deck".to_string(), format!("deck://{}", deck.id)),
        };
        Ok((system, path, "deck:cover"))
    })?;
    let mut request = json!({ "system": system, "path": path });
    if let Some(max_size) = params.get("maxSize") {
        request["maxSize"] = max_size.clone();
    }
    let mut result = fixtures::media_image_response(&request)?;
    result["delivery"] = json!("inline");
    result["typeTag"] = json!(type_tag);
    Ok(result)
}
