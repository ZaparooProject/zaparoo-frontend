// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Wire types for the user's collections: the four built-in lists Core keeps
// as per-game `user:*` flags, and decks (`decks`, `decks.get`, `decks.new`,
// `decks.update`, `decks.delete`, `decks.image`, `decks.changed`).

use serde::{Deserialize, Serialize};

/// One of the built-in lists. Each is a flag on a game, set and cleared
/// with `media.tags.update` and listed with a tag-filtered `media.search`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UserList {
    #[default]
    Favorite,
    Liked,
    Disliked,
    PlayLater,
}

impl UserList {
    /// Strip order: Favorite, Like, Dislike, Play later.
    pub const ALL: [Self; 4] = [Self::Favorite, Self::Liked, Self::Disliked, Self::PlayLater];

    /// The tag's value, `favorite` in `user:favorite`.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Favorite => "favorite",
            Self::Liked => "liked",
            Self::Disliked => "disliked",
            Self::PlayLater => "playlater",
        }
    }

    /// The full filter and mutation tag.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Favorite => "user:favorite",
            Self::Liked => "user:liked",
            Self::Disliked => "user:disliked",
            Self::PlayLater => "user:playlater",
        }
    }

    #[must_use]
    pub fn from_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|list| list.value() == value)
    }
}

/// `media.search` sort for a list in the order its games were added,
/// newest first. Core accepts it only with a list tag filter.
pub const SORT_ADDED_DESC: &str = "added-desc";

/// The local game a deck item is linked to on this device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckItemMedia {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    /// False when the linked file is no longer indexed here.
    #[serde(default)]
    pub available: bool,
    /// The file's user flags (favorite, liked, disliked, playlater,
    /// hidden). `None` when Core did not say, so the flags are unknown.
    #[serde(default)]
    pub user_tags: Option<Vec<crate::media_types::TagInfo>>,
}

/// What a deck item is. Games added from a library are `Script` items with
/// `media` set; anything Core does not recognize reads as `Script`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeckItemKind {
    Card,
    #[default]
    #[serde(other)]
    Script,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckItem {
    /// Stable across edits; how `decks.update` names an existing item.
    #[serde(default)]
    pub id: i64,
    /// 1-based.
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub kind: DeckItemKind,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub zapscript: String,
    #[serde(default)]
    pub card_id: String,
    #[serde(default)]
    pub media: Option<DeckItemMedia>,
    /// A card item's art can be fetched with `decks.image`.
    #[serde(default)]
    pub has_image: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful: each bool is one of Core's own deck fields"
)]
pub struct Deck {
    /// Also the value of the deck's membership tag, `user:deck:<id>`.
    #[serde(default)]
    pub deck_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub item_count: u32,
    /// False for a read-only copy of someone else's deck.
    #[serde(default)]
    pub owned: bool,
    /// The online account locked it.
    #[serde(default)]
    pub locked: bool,
    /// A copy of a deck the linked account saved from another account.
    #[serde(default)]
    pub saved: bool,
    #[serde(default)]
    pub owned_by: String,
    #[serde(default)]
    pub has_image: bool,
    #[serde(default)]
    pub updated_at: i64,
    /// Present only when `decks` was asked about one game: the items that
    /// are that game, empty when the deck does not hold it.
    #[serde(default)]
    pub contains_item_ids: Option<Vec<i64>>,
    /// Present on `decks.get`, `decks.new` and `decks.update`.
    #[serde(default)]
    pub items: Vec<DeckItem>,
}

/// Core keeps at most this many items in a deck.
pub const MAX_DECK_ITEMS: u32 = 120;

impl Deck {
    /// Whether this device may change the deck.
    #[must_use]
    pub const fn editable(&self) -> bool {
        self.owned && !self.locked
    }

    /// Decks the user chose to have: their own, and ones the account saved.
    /// Copies Core cached from tapped cards are not.
    #[must_use]
    pub const fn listed(&self) -> bool {
        self.owned || self.saved
    }

    #[must_use]
    pub const fn full(&self) -> bool {
        self.item_count >= MAX_DECK_ITEMS
    }

    /// Whether the game `decks` was asked about is in this deck.
    #[must_use]
    pub fn contains(&self) -> bool {
        self.contains_item_ids
            .as_ref()
            .is_some_and(|ids| !ids.is_empty())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DecksResult {
    #[serde(default)]
    pub decks: Vec<Deck>,
}

/// `decks`, optionally asking which decks hold one game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
pub struct DecksParams {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckIdParams {
    pub deck_id: String,
}

/// One item to add. The frontend adds only local games.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckItemInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

impl DeckItemInput {
    #[must_use]
    pub fn game(system: &str, path: &str) -> Self {
        Self {
            id: None,
            kind: Some("media"),
            system: system.to_owned(),
            path: path.to_owned(),
        }
    }

    /// An existing item kept in place; a list of these reorders a deck.
    #[must_use]
    pub const fn existing(id: i64) -> Self {
        Self {
            id: Some(id),
            kind: None,
            system: String::new(),
            path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckNewParams {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<DeckItemInput>,
}

/// A deck's cover, set from this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeckCover {
    /// That game's art.
    Media {
        system: String,
        path: String,
    },
    /// An image file on the Core host.
    File {
        path: String,
    },
    None,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckUpdateParams {
    pub deck_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The whole list, in order; existing items by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<DeckItemInput>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add_items: Vec<DeckItemInput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove_item_ids: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<DeckCover>,
}

/// `decks.image`: the deck's cover, or a card item's art with `item_id`.
/// The result has the `media.image` shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckImageParams {
    pub deck_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<String>,
}

/// `decks.changed`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecksChanged {
    #[serde(default)]
    pub deck_id: String,
    #[serde(default)]
    pub action: String,
}

/// `media.tags.changed`: a game's user flags after any client changed them.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MediaTagsChanged {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub tags: Vec<crate::media_types::TagInfo>,
}

impl MediaTagsChanged {
    #[must_use]
    pub fn has(&self, list: UserList) -> bool {
        self.tags
            .iter()
            .any(|tag| tag.tag_type == "user" && tag.tag == list.value())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_tags_round_trip() {
        for list in UserList::ALL {
            assert_eq!(UserList::from_value(list.value()), Some(list));
            assert_eq!(list.tag(), format!("user:{}", list.value()));
        }
        assert_eq!(UserList::from_value("hidden"), None);
    }

    #[test]
    fn deck_reads_ownership_and_membership() {
        let deck: Deck = serde_json::from_value(json!({
            "deckId": "abc", "name": "Couch", "itemCount": 120, "owned": true,
            "locked": false, "saved": false, "hasImage": true,
            "containsItemIds": [4, 9],
            "items": [{"id": 4, "position": 1, "kind": "script", "name": "Mario",
                       "media": {"system": "SNES", "path": "/m.sfc", "name": "Mario", "available": true,
                                 "userTags": [{"type": "user", "tag": "liked"}]}},
                      {"id": 5, "position": 2, "kind": "card", "cardId": "c1", "hasImage": true},
                      {"id": 6, "position": 3, "kind": "future"}]
        }))
        .unwrap();
        assert!(deck.editable() && deck.listed() && deck.full() && deck.contains());
        assert_eq!(deck.items[1].kind, DeckItemKind::Card);
        assert_eq!(deck.items[2].kind, DeckItemKind::Script);
        let media = deck.items[0].media.as_ref().unwrap();
        assert!(media.available);
        assert_eq!(media.user_tags.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn cached_copies_are_not_listed() {
        let cached = Deck {
            owned: false,
            saved: false,
            ..Deck::default()
        };
        let saved = Deck {
            owned: false,
            saved: true,
            ..Deck::default()
        };
        assert!(!cached.listed());
        assert!(saved.listed() && !saved.editable());
        let none = Deck {
            contains_item_ids: Some(Vec::new()),
            ..Deck::default()
        };
        assert!(!none.contains());
    }

    #[test]
    fn update_serializes_only_what_changes() {
        let params = DeckUpdateParams {
            deck_id: "abc".into(),
            items: Some(vec![DeckItemInput::existing(2), DeckItemInput::existing(1)]),
            cover: Some(DeckCover::Media {
                system: "SNES".into(),
                path: "/m.sfc".into(),
            }),
            ..DeckUpdateParams::default()
        };
        assert_eq!(
            serde_json::to_value(params).unwrap(),
            json!({"deckId": "abc", "items": [{"id": 2}, {"id": 1}],
                   "cover": {"kind": "media", "system": "SNES", "path": "/m.sfc"}})
        );
        assert_eq!(
            serde_json::to_value(DeckCover::None).unwrap(),
            json!({"kind": "none"})
        );
        assert_eq!(
            serde_json::to_value(DeckItemInput::game("NES", "/a.nes")).unwrap(),
            json!({"kind": "media", "system": "NES", "path": "/a.nes"})
        );
    }
}
