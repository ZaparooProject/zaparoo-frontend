// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// What the Options menu offers for an item, and in what order. Options is a
// quick menu: the frequent actions sit on its own rows, and configuration,
// organization and maintenance move to one Manage page. A row is offered
// because the item can do it, never because of the screen showing the item,
// so the same game reads the same in Games, Favorites, Search results,
// Recently played and on the Hub.

use crate::user_flags::Flags;

/// What an Options row opens in the menu's place, for Back to return from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// The Update metadata setup panel.
    Setup,
    /// The launcher picker.
    Picker,
    QrCode,
    /// Waiting for a token to write.
    TokenWrite,
    /// Game Info.
    Details,
    /// The Rename screen for a Hub tile.
    Rename,
    /// The file browser picking a Hub tile's icon.
    IconBrowser,
    /// Moving a Hub tile.
    Move,
}

/// A page of the menu, shown in place of its own rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    WriteToken,
    ManageGame,
    ManageSystem,
    ManageCategory,
    ManageDeck,
}

impl Page {
    /// The Options row that opens the page.
    pub fn menu_id(self) -> &'static str {
        match self {
            Self::WriteToken => "write_token",
            Self::ManageGame => "manage_game",
            Self::ManageSystem => "manage_system",
            Self::ManageCategory => "manage_category",
            Self::ManageDeck => "manage_deck",
        }
    }
}

/// One row: the action it runs and the `Labels.menu` key that words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: &'static str,
    pub key: &'static str,
}

const fn row(id: &'static str) -> Row {
    Row { id, key: id }
}

const fn keyed(id: &'static str, key: &'static str) -> Row {
    Row { id, key }
}

/// An item's whole menu: its own rows and the pages they open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Menu {
    /// A game's four collections, shown as a strip of toggles above the
    /// rows. `None` when the item is not a game or its flags are unknown.
    pub strip: Option<Flags>,
    pub root: Vec<Row>,
    /// The Write to token page: the ways to write. Empty when only one way
    /// exists, and the root row then runs it directly.
    pub write: Vec<Row>,
    /// The Manage page of whichever kind the root names.
    pub manage: Vec<Row>,
}

impl Menu {
    /// Nothing to offer: the item gets no menu and no Options hint.
    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// The page the root row `id` opens, or `None` for a row that acts.
    pub fn page_for(&self, id: &str) -> Option<Page> {
        let page = match id {
            "write_token" => Page::WriteToken,
            "manage_game" => Page::ManageGame,
            "manage_system" => Page::ManageSystem,
            "manage_category" => Page::ManageCategory,
            "manage_deck" => Page::ManageDeck,
            _ => return None,
        };
        (self.root.iter().any(|row| row.id == id) && !self.rows(page).is_empty()).then_some(page)
    }

    pub fn rows(&self, page: Page) -> &[Row] {
        match page {
            Page::WriteToken => &self.write,
            Page::ManageGame | Page::ManageSystem | Page::ManageCategory | Page::ManageDeck => {
                &self.manage
            }
        }
    }

    /// The action Write to token runs when there is no page to choose on.
    pub fn direct_action(id: &str) -> &str {
        if id == "write_token" {
            "qr_code"
        } else {
            id
        }
    }

    /// What the row `id` opens in the menu's place, when Back there returns
    /// to the menu: on the page it was on, with the row focused. Finishing
    /// what it opened (Start, Save, a pick) goes to the screen instead.
    pub fn opens(id: &str) -> Option<Opens> {
        Some(match id {
            "scrape_game" | "scrape_system" | "scrape_category" => Opens::Setup,
            "change_launcher" => Opens::Picker,
            "qr_code" => Opens::QrCode,
            "write_card" => Opens::TokenWrite,
            "more_info" => Opens::Details,
            "hub_rename" => Opens::Rename,
            "hub_icon" => Opens::IconBrowser,
            "hub_move" => Opens::Move,
            _ => return None,
        })
    }

    /// Whether the row `id` opens something that may not be ready at once:
    /// a screen of its own, or a picker Core has to be asked about first.
    /// The menu stays up, its row held down, until that is ready, and
    /// leaves in the same frame it appears: closing it first would show the
    /// screen underneath on its own for a moment.
    pub fn holds_open(id: &str) -> bool {
        matches!(
            id,
            "hub_rename"
                | "hub_icon"
                | "change_launcher"
                | "more_info"
                | "deck_rename"
                | "deck_cover_choose"
        )
    }

    /// A Hub tile's own Rename and Change icon rows, ahead of its layout
    /// rows (Remove, Move) wherever those sit: configuration before
    /// organization. A menu with no layout rows is not a tile's.
    #[must_use]
    pub fn with_tile_edits(mut self, can_change_icon: bool) -> Self {
        let mut edits = vec![keyed("hub_rename", "hub_rename:tile")];
        if can_change_icon {
            edits.push(keyed("hub_icon", "hub_icon:tile"));
        }
        let is_layout = |row: &Row| row.id == "hub_move" || row.id == "hub_remove";
        for list in [&mut self.root, &mut self.manage] {
            if let Some(at) = list.iter().position(is_layout) {
                list.splice(at..at, edits);
                break;
            }
        }
        self
    }

    /// An item of a deck the user can change, on the Deck screen: its deck
    /// rows join the item's own. They are the item's place in the deck, so
    /// they lead its organization, on its Manage page when it has one.
    #[must_use]
    pub fn with_deck_item(mut self, item: DeckItem) -> Self {
        let mut rows = Vec::new();
        if item.can_be_cover {
            rows.push(row("deck_use_cover"));
        }
        rows.push(row("deck_move"));
        rows.push(row("deck_remove"));
        let manage = self
            .root
            .iter()
            .position(|row| row.id.starts_with("manage_"));
        if manage.is_some() {
            self.manage.splice(0..0, rows);
        } else {
            self.root.extend(rows);
        }
        self
    }

    fn with_manage(mut self, page: Page) -> Self {
        if !self.manage.is_empty() {
            self.root.push(row(page.menu_id()));
        }
        self
    }
}

/// An item of an editable deck, for its deck rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeckItem {
    /// A game this device has, whose art can be the deck's cover.
    pub can_be_cover: bool,
}

/// A deck, on the Decks screen or as a Hub tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent capability of the deck"
)]
pub struct DeckInput {
    /// The user's own and not locked: it can be renamed, changed, deleted.
    pub editable: bool,
    pub has_items: bool,
    pub has_cover: bool,
    /// An image file can be chosen for the cover: Core runs here and can
    /// read what the file browser shows.
    pub can_choose_cover: bool,
    pub on_hub: bool,
    /// Opened on the deck's Hub tile, whose own rows join.
    pub tile: bool,
}

/// A deck's menu. Accept opens the deck, so playing it leads; then its
/// place on the Hub, and for a deck the user can change, the edits on a
/// Manage page.
pub fn deck(input: &DeckInput) -> Menu {
    let mut menu = Menu::default();
    if input.has_items {
        menu.root.push(row("play_deck"));
    }
    if input.tile {
        menu.root.push(keyed("hub_move", "hub_move:tile"));
        menu.root.push(keyed("hub_remove", "hub:remove"));
    } else {
        menu.root.push(keyed(
            "deck_hub",
            if input.on_hub {
                "hub:remove"
            } else {
                "add_to_hub"
            },
        ));
    }
    if input.editable {
        menu.manage.push(row("deck_rename"));
        if input.can_choose_cover {
            menu.manage.push(row("deck_cover_choose"));
        }
        if input.has_cover {
            menu.manage.push(row("deck_cover_remove"));
        }
        menu.manage.push(row("deck_delete"));
    }
    menu.with_manage(Page::ManageDeck)
}

/// The Hub shortcut a game or system menu was opened on. Its layout actions
/// join the item's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    /// The built-in Resume tile, standing for the game last played.
    Resume,
    /// A shortcut pinned to the Hub.
    Pinned,
}

fn pin_row(on_hub: bool) -> Row {
    keyed(
        "add_to_hub",
        if on_hub { "hub:remove" } else { "add_to_hub" },
    )
}

fn hide_row(id: &'static str, hidden: bool, hide: &'static str, unhide: &'static str) -> Row {
    keyed(id, if hidden { unhide } else { hide })
}

/// Write to token and its page: both ways when a reader is present, the App
/// alone otherwise.
fn push_write(menu: &mut Menu, has_nfc: bool) {
    menu.root.push(row("write_token"));
    if has_nfc {
        menu.write = vec![row("write_card"), row("qr_code")];
    }
}

/// What a game can do. Every flag is the game's own data or a shared
/// operational gate; none is the screen it is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per menu gate, each independent of the others"
)]
pub struct GameInput {
    /// The game's collections, when Core reported its tags and can address
    /// it to change them.
    pub flags: Option<Flags>,
    /// Core can add the game to a deck: it has a system and a path.
    pub can_add_to_deck: bool,
    /// A folder of one game's discs.
    pub multi_disc: bool,
    /// A search result standing for several versions of one title.
    pub versions: bool,
    /// There is a script or path to write to a token.
    pub has_payload: bool,
    pub has_nfc: bool,
    /// An arcade game Core can look up other versions of.
    pub can_discover: bool,
    /// A launcher exists for the game's system.
    pub has_launchers: bool,
    /// There is something to pin to the Hub.
    pub pinnable: bool,
    pub on_hub: bool,
    /// Core can address the game to hide it, and its visibility is known.
    pub hideable: bool,
    pub is_hidden: bool,
    pub has_system: bool,
    pub media_busy: bool,
    pub tile: Option<Tile>,
    /// Core has nothing to show in Game Info: a file a live folder read
    /// from disk that Core has not indexed.
    pub details_unknown: bool,
}

/// A game, or a folder or archive Core launches as one.
pub fn game(input: &GameInput) -> Menu {
    // The collections sit above every row, in the strip; Accept already
    // launches the game, so Details leads the rows.
    let mut menu = Menu {
        strip: input.flags,
        ..Menu::default()
    };
    if !input.details_unknown {
        menu.root.push(row("more_info"));
    }
    if input.can_add_to_deck {
        menu.root.push(row("add_to_deck"));
    }
    if input.multi_disc {
        menu.root.push(row("choose_disc"));
    }
    if input.versions {
        menu.root.push(row("choose_version"));
    }
    if input.has_payload {
        push_write(&mut menu, input.has_nfc);
    }
    if input.can_discover {
        menu.root.push(row("discover"));
    }
    if input.has_launchers && input.has_system {
        menu.manage.push(row("change_launcher"));
    }
    match input.tile {
        // The shortcut itself is the pin: removing it is this row.
        Some(Tile::Pinned) => menu.manage.push(keyed("hub_remove", "hub:remove")),
        Some(Tile::Resume) | None => {
            if input.pinnable {
                menu.manage.push(pin_row(input.on_hub));
            }
        }
    }
    if input.hideable {
        menu.manage.push(if input.tile.is_some() {
            hide_row("toggle_hidden", input.is_hidden, "hide:game", "unhide:game")
        } else {
            hide_row("toggle_hidden", input.is_hidden, "hide:hide", "hide:unhide")
        });
    }
    if input.tile.is_some() {
        menu.manage.push(keyed("hub_move", "hub_move:tile"));
    }
    if input.tile == Some(Tile::Resume) {
        menu.manage.push(keyed("hub_remove", "hub_remove:resume"));
    }
    if input.has_system && !input.media_busy {
        menu.manage.push(row("scrape_game"));
    }
    menu.with_manage(Page::ManageGame)
}

/// A row Accept browses into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    /// An ordinary directory.
    Directory,
    /// One of a system's own game directories. Core cannot hide it.
    FilesystemRoot,
    /// A root with no filesystem identity to pin.
    VirtualRoot,
}

/// A folder's two actions stay on Options: too few for a page.
pub fn folder(kind: Folder, on_hub: bool, is_hidden: bool) -> Menu {
    let root = match kind {
        Folder::Directory => vec![
            pin_row(on_hub),
            hide_row("toggle_hidden", is_hidden, "hide:hide", "hide:unhide"),
        ],
        Folder::FilesystemRoot => vec![pin_row(on_hub)],
        Folder::VirtualRoot => Vec::new(),
    };
    Menu {
        root,
        ..Menu::default()
    }
}

/// A folder inside a live folder: Core reads it from disk and cannot hide
/// it, so pinning it is all there is.
pub fn live_folder(on_hub: bool) -> Menu {
    Menu {
        root: vec![pin_row(on_hub)],
        ..Menu::default()
    }
}

/// What a system can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per menu gate, each independent of the others"
)]
pub struct SystemInput {
    /// The system launches by its own script and has no indexed media.
    pub launch_only: bool,
    /// Shown as a collection of its favorites: random play stays in it.
    pub favorites: bool,
    pub has_launchers: bool,
    pub on_hub: bool,
    pub is_hidden: bool,
    pub media_busy: bool,
    /// Opened on the system's pinned Hub shortcut.
    pub tile: bool,
}

pub fn system(input: &SystemInput) -> Menu {
    let hide = if input.tile {
        hide_row(
            "toggle_hide_system",
            input.is_hidden,
            "hide:system",
            "unhide:system",
        )
    } else {
        hide_row(
            "toggle_hide_system",
            input.is_hidden,
            "hide:hide",
            "hide:unhide",
        )
    };
    let pin = if input.tile {
        keyed("hub_remove", "hub:remove")
    } else {
        pin_row(input.on_hub)
    };
    if input.launch_only {
        // Accept launches it, and nothing here is indexed: the few rows
        // left need no page.
        let mut root = vec![pin, hide];
        if input.tile {
            root.push(keyed("hub_move", "hub_move:tile"));
        }
        return Menu {
            root,
            ..Menu::default()
        };
    }
    let mut menu = Menu::default();
    // Accept opens the system's games, so launching it is not a duplicate.
    menu.root.push(row("launch_system"));
    menu.root.push(if input.favorites {
        keyed("launch_random_favorite", "random_favorite")
    } else {
        row("launch_random_system")
    });
    if input.has_launchers {
        menu.manage.push(row("change_launcher"));
    }
    menu.manage.push(pin);
    menu.manage.push(hide);
    if input.tile {
        menu.manage.push(keyed("hub_move", "hub_move:tile"));
    }
    if !input.media_busy {
        menu.manage.push(row("index_system"));
        menu.manage.push(row("scrape_system"));
    }
    menu.with_manage(Page::ManageSystem)
}

/// A Hub category tile. `maintainable` is false while a media job runs or
/// when the category holds nothing Core indexes.
pub fn category(maintainable: bool) -> Menu {
    let mut menu = Menu {
        root: vec![row("hub_move"), keyed("hub_remove", "hub_remove:hide")],
        ..Menu::default()
    };
    if maintainable {
        menu.manage = vec![row("index_category"), row("scrape_category")];
    }
    menu.with_manage(Page::ManageCategory)
}

/// A built-in Hub action, or Resume with no game to act on.
pub fn built_in() -> Menu {
    Menu {
        root: vec![row("hub_move"), keyed("hub_remove", "hub_remove:hide")],
        ..Menu::default()
    }
}

/// A pinned shortcut whose game, system or folder could not be resolved:
/// its layout actions remain.
pub fn unresolved_shortcut() -> Menu {
    Menu {
        root: vec![row("hub_move"), keyed("hub_remove", "hub:remove")],
        ..Menu::default()
    }
}

/// A shortcut that is not one game: a script, or a saved search. A script
/// can be written to a token exactly as stored.
pub fn shortcut(has_script: bool, has_nfc: bool) -> Menu {
    let mut menu = Menu::default();
    if has_script {
        push_write(&mut menu, has_nfc);
    }
    menu.root.push(keyed("hub_remove", "hub:remove"));
    menu.root.push(keyed("hub_move", "hub_move:tile"));
    menu
}

#[cfg(test)]
mod deck_tests {
    use super::*;

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|row| row.id).collect()
    }

    #[test]
    fn an_editable_deck_plays_first_and_keeps_its_edits_on_manage() {
        let menu = deck(&DeckInput {
            editable: true,
            has_items: true,
            has_cover: true,
            can_choose_cover: true,
            ..DeckInput::default()
        });
        assert_eq!(ids(&menu.root), ["play_deck", "deck_hub", "manage_deck"]);
        assert_eq!(
            ids(&menu.manage),
            [
                "deck_rename",
                "deck_cover_choose",
                "deck_cover_remove",
                "deck_delete"
            ]
        );
        assert_eq!(menu.page_for("manage_deck"), Some(Page::ManageDeck));
    }

    #[test]
    fn a_saved_deck_only_plays_and_pins() {
        let menu = deck(&DeckInput {
            has_items: true,
            on_hub: true,
            ..DeckInput::default()
        });
        assert_eq!(ids(&menu.root), ["play_deck", "deck_hub"]);
        assert_eq!(menu.root[1].key, "hub:remove");
        assert!(menu.manage.is_empty());
    }

    #[test]
    fn an_empty_deck_has_nothing_to_play() {
        let menu = deck(&DeckInput::default());
        assert_eq!(ids(&menu.root), ["deck_hub"]);
    }

    #[test]
    fn a_deck_tile_has_the_tiles_rows() {
        let menu = deck(&DeckInput {
            has_items: true,
            tile: true,
            ..DeckInput::default()
        })
        .with_tile_edits(true);
        assert_eq!(
            ids(&menu.root),
            [
                "play_deck",
                "hub_rename",
                "hub_icon",
                "hub_move",
                "hub_remove"
            ]
        );
    }

    #[test]
    fn deck_rows_lead_a_games_manage_page_and_sit_on_options_otherwise() {
        let game = Menu {
            root: vec![row("more_info"), row("manage_game")],
            manage: vec![row("scrape_game")],
            ..Menu::default()
        }
        .with_deck_item(DeckItem { can_be_cover: true });
        assert_eq!(
            ids(&game.manage),
            ["deck_use_cover", "deck_move", "deck_remove", "scrape_game"]
        );
        let card = Menu::default().with_deck_item(DeckItem {
            can_be_cover: false,
        });
        assert_eq!(ids(&card.root), ["deck_move", "deck_remove"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|row| row.key).collect()
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|row| row.id).collect()
    }

    /// A regular game with everything Core can report for it.
    fn regular() -> GameInput {
        GameInput {
            flags: Some(Flags::default()),
            can_add_to_deck: true,
            has_payload: true,
            has_launchers: true,
            pinnable: true,
            hideable: true,
            has_system: true,
            ..GameInput::default()
        }
    }

    #[test]
    fn a_regular_game_keeps_four_quick_rows_and_manages_the_rest() {
        let menu = game(&regular());
        assert_eq!(
            keys(&menu.root),
            ["more_info", "add_to_deck", "write_token", "manage_game"]
        );
        assert_eq!(
            keys(&menu.manage),
            ["change_launcher", "add_to_hub", "hide:hide", "scrape_game"]
        );
        assert_eq!(menu.page_for("manage_game"), Some(Page::ManageGame));
    }

    #[test]
    fn a_file_in_a_live_folder_offers_what_its_data_supports() {
        // Not indexed: no tags, no system, nothing for Game Info.
        let unmatched = game(&GameInput {
            has_payload: true,
            pinnable: true,
            details_unknown: true,
            ..GameInput::default()
        });
        assert_eq!(keys(&unmatched.root), ["write_token", "manage_game"]);
        assert_eq!(keys(&unmatched.manage), ["add_to_hub"]);
        // Matched to Core's index: Game Info and the system's rows return.
        let matched = game(&GameInput {
            has_payload: true,
            pinnable: true,
            has_system: true,
            has_launchers: true,
            ..GameInput::default()
        });
        assert_eq!(keys(&matched.root)[0], "more_info");
        assert!(!keys(&matched.root).contains(&"favorite:add"));
        assert!(!keys(&matched.manage).contains(&"hide:hide"));
    }

    #[test]
    fn a_folder_in_a_live_folder_only_pins() {
        assert_eq!(keys(&live_folder(false).root), ["add_to_hub"]);
        assert_eq!(keys(&live_folder(true).root), ["hub:remove"]);
    }

    #[test]
    fn write_to_token_is_a_page_only_when_there_are_two_ways() {
        let app_only = game(&regular());
        assert!(app_only.write.is_empty());
        assert_eq!(app_only.page_for("write_token"), None);
        assert_eq!(Menu::direct_action("write_token"), "qr_code");
        assert_eq!(Menu::direct_action("more_info"), "more_info");

        let both = game(&GameInput {
            has_nfc: true,
            ..regular()
        });
        assert_eq!(both.page_for("write_token"), Some(Page::WriteToken));
        assert_eq!(keys(&both.write), ["write_card", "qr_code"]);
        // The root reads the same either way.
        assert_eq!(keys(&both.root), keys(&app_only.root));
    }

    #[test]
    fn discs_and_alternates_stay_on_options_in_their_own_places() {
        let arcade = game(&GameInput {
            can_discover: true,
            ..regular()
        });
        assert_eq!(
            keys(&arcade.root),
            [
                "more_info",
                "add_to_deck",
                "write_token",
                "discover",
                "manage_game"
            ]
        );
        let discs = game(&GameInput {
            multi_disc: true,
            has_launchers: false,
            ..regular()
        });
        assert_eq!(
            keys(&discs.root),
            [
                "more_info",
                "add_to_deck",
                "choose_disc",
                "write_token",
                "manage_game"
            ]
        );
        let both = game(&GameInput {
            multi_disc: true,
            can_discover: true,
            ..regular()
        });
        assert_eq!(
            keys(&both.root),
            [
                "more_info",
                "add_to_deck",
                "choose_disc",
                "write_token",
                "discover",
                "manage_game"
            ]
        );
    }

    #[test]
    fn a_title_with_versions_offers_them_where_discs_sit() {
        let stacked = game(&GameInput {
            versions: true,
            ..regular()
        });
        assert_eq!(
            keys(&stacked.root),
            [
                "more_info",
                "favorite:add",
                "choose_version",
                "write_token",
                "manage_game"
            ]
        );
        assert_eq!(stacked.page_for("choose_version"), None);
    }

    #[test]
    fn toggles_word_the_state_they_would_change() {
        let flags = Flags {
            favorite: true,
            ..Flags::default()
        };
        let menu = game(&GameInput {
            flags: Some(flags),
            on_hub: true,
            is_hidden: true,
            ..regular()
        });
        assert_eq!(menu.strip, Some(flags));
        assert_eq!(
            keys(&menu.manage),
            [
                "change_launcher",
                "hub:remove",
                "hide:unhide",
                "scrape_game"
            ]
        );
    }

    #[test]
    fn a_gate_removes_only_its_own_row() {
        let busy = game(&GameInput {
            media_busy: true,
            ..regular()
        });
        assert_eq!(
            keys(&busy.manage),
            ["change_launcher", "add_to_hub", "hide:hide"]
        );
        assert_eq!(keys(&busy.root), keys(&game(&regular()).root));

        // History Core could not resolve: no tags, so no collections to
        // toggle and no deck to add to, and nothing is assumed in their
        // place.
        let unresolved = game(&GameInput {
            flags: None,
            can_add_to_deck: false,
            ..regular()
        });
        assert_eq!(unresolved.strip, None);
        assert_eq!(
            keys(&unresolved.root),
            ["more_info", "write_token", "manage_game"]
        );

        let no_system = game(&GameInput {
            has_system: false,
            ..regular()
        });
        assert_eq!(keys(&no_system.manage), ["add_to_hub", "hide:hide"]);

        let no_payload = game(&GameInput {
            has_payload: false,
            ..regular()
        });
        assert_eq!(
            keys(&no_payload.root),
            ["more_info", "add_to_deck", "manage_game"]
        );
    }

    #[test]
    fn an_empty_manage_page_is_never_offered() {
        let menu = game(&GameInput {
            can_add_to_deck: true,
            has_payload: true,
            ..GameInput::default()
        });
        assert_eq!(
            keys(&menu.root),
            ["more_info", "add_to_deck", "write_token"]
        );
        assert_eq!(menu.page_for("manage_game"), None);
    }

    #[test]
    fn the_resume_tile_adds_its_layout_actions_to_the_games_own() {
        let menu = game(&GameInput {
            tile: Some(Tile::Resume),
            ..regular()
        });
        assert_eq!(
            keys(&menu.root),
            ["more_info", "add_to_deck", "write_token", "manage_game"]
        );
        assert_eq!(
            menu.manage,
            [
                row("change_launcher"),
                row("add_to_hub"),
                keyed("toggle_hidden", "hide:game"),
                keyed("hub_move", "hub_move:tile"),
                keyed("hub_remove", "hub_remove:resume"),
                row("scrape_game"),
            ]
        );
    }

    #[test]
    fn a_pinned_game_removes_its_shortcut_where_the_pin_toggle_sits() {
        let menu = game(&GameInput {
            tile: Some(Tile::Pinned),
            is_hidden: true,
            on_hub: true,
            ..regular()
        });
        assert_eq!(
            menu.manage,
            [
                row("change_launcher"),
                keyed("hub_remove", "hub:remove"),
                keyed("toggle_hidden", "unhide:game"),
                keyed("hub_move", "hub_move:tile"),
                row("scrape_game"),
            ]
        );
        assert!(!ids(&menu.manage).contains(&"add_to_hub"));
    }

    #[test]
    fn folders_keep_their_actions_on_options() {
        assert_eq!(
            keys(&folder(Folder::Directory, false, false).root),
            ["add_to_hub", "hide:hide"]
        );
        assert_eq!(
            keys(&folder(Folder::Directory, true, true).root),
            ["hub:remove", "hide:unhide"]
        );
        assert_eq!(
            keys(&folder(Folder::FilesystemRoot, false, false).root),
            ["add_to_hub"]
        );
        assert!(folder(Folder::VirtualRoot, false, false).is_empty());
        assert!(folder(Folder::Directory, false, false).manage.is_empty());
    }

    #[test]
    fn a_browsable_system_launches_quickly_and_manages_the_rest() {
        let menu = system(&SystemInput {
            has_launchers: true,
            ..SystemInput::default()
        });
        assert_eq!(
            keys(&menu.root),
            ["launch_system", "launch_random_system", "manage_system"]
        );
        assert_eq!(
            keys(&menu.manage),
            [
                "change_launcher",
                "add_to_hub",
                "hide:hide",
                "index_system",
                "scrape_system"
            ]
        );
        assert_eq!(menu.page_for("manage_system"), Some(Page::ManageSystem));

        let busy = system(&SystemInput {
            media_busy: true,
            ..SystemInput::default()
        });
        assert_eq!(keys(&busy.manage), ["add_to_hub", "hide:hide"]);
    }

    #[test]
    fn favorite_systems_scopes_random_play_and_keeps_the_rest() {
        let favorites = system(&SystemInput {
            favorites: true,
            has_launchers: true,
            ..SystemInput::default()
        });
        assert_eq!(
            favorites.root,
            [
                row("launch_system"),
                keyed("launch_random_favorite", "random_favorite"),
                row("manage_system"),
            ]
        );
        let systems = system(&SystemInput {
            has_launchers: true,
            ..SystemInput::default()
        });
        assert_eq!(favorites.manage, systems.manage);
    }

    #[test]
    fn a_launch_only_system_has_no_page_and_no_duplicate_launch() {
        let menu = system(&SystemInput {
            launch_only: true,
            has_launchers: true,
            ..SystemInput::default()
        });
        assert_eq!(keys(&menu.root), ["add_to_hub", "hide:hide"]);
        assert!(menu.manage.is_empty());

        let pinned = system(&SystemInput {
            launch_only: true,
            tile: true,
            ..SystemInput::default()
        });
        assert_eq!(
            keys(&pinned.root),
            ["hub:remove", "hide:system", "hub_move:tile"]
        );
    }

    #[test]
    fn a_pinned_system_manages_its_shortcut_with_the_system() {
        let menu = system(&SystemInput {
            tile: true,
            has_launchers: true,
            is_hidden: true,
            ..SystemInput::default()
        });
        assert_eq!(
            keys(&menu.root),
            ["launch_system", "launch_random_system", "manage_system"]
        );
        assert_eq!(
            menu.manage,
            [
                row("change_launcher"),
                keyed("hub_remove", "hub:remove"),
                keyed("toggle_hide_system", "unhide:system"),
                keyed("hub_move", "hub_move:tile"),
                row("index_system"),
                row("scrape_system"),
            ]
        );
    }

    #[test]
    fn a_category_manages_its_maintenance_only_when_there_is_some() {
        let menu = category(true);
        assert_eq!(
            keys(&menu.root),
            ["hub_move", "hub_remove:hide", "manage_category"]
        );
        assert_eq!(keys(&menu.manage), ["index_category", "scrape_category"]);
        assert_eq!(menu.page_for("manage_category"), Some(Page::ManageCategory));

        let idle = category(false);
        assert_eq!(keys(&idle.root), ["hub_move", "hub_remove:hide"]);
        assert_eq!(idle.page_for("manage_category"), None);
    }

    #[test]
    fn small_hub_menus_stay_small() {
        assert_eq!(keys(&built_in().root), ["hub_move", "hub_remove:hide"]);
        assert_eq!(
            keys(&unresolved_shortcut().root),
            ["hub_move", "hub:remove"]
        );
        // A saved search is not a script to write.
        assert_eq!(
            keys(&shortcut(false, true).root),
            ["hub:remove", "hub_move:tile"]
        );
        let script = shortcut(true, true);
        assert_eq!(
            keys(&script.root),
            ["write_token", "hub:remove", "hub_move:tile"]
        );
        assert_eq!(script.page_for("write_token"), Some(Page::WriteToken));
        assert_eq!(shortcut(true, false).page_for("write_token"), None);
    }

    #[test]
    fn a_tile_names_and_draws_itself_ahead_of_its_layout_rows() {
        assert_eq!(
            keys(&built_in().with_tile_edits(true).root),
            [
                "hub_rename:tile",
                "hub_icon:tile",
                "hub_move",
                "hub_remove:hide"
            ]
        );
        assert_eq!(
            keys(&category(true).with_tile_edits(false).root),
            [
                "hub_rename:tile",
                "hub_move",
                "hub_remove:hide",
                "manage_category"
            ]
        );
        assert_eq!(
            keys(&shortcut(true, false).with_tile_edits(true).root),
            [
                "write_token",
                "hub_rename:tile",
                "hub_icon:tile",
                "hub:remove",
                "hub_move:tile"
            ]
        );
        // A game tile keeps its layout rows on Manage, and the edits join
        // them there.
        let pinned = game(&GameInput {
            tile: Some(Tile::Pinned),
            ..regular()
        })
        .with_tile_edits(true);
        assert!(!ids(&pinned.root).contains(&"hub_rename"));
        let manage = ids(&pinned.manage);
        let rename = manage.iter().position(|id| *id == "hub_rename");
        let remove = manage.iter().position(|id| *id == "hub_remove");
        assert_eq!(rename.map(|at| at + 2), remove);
        // A menu with no layout rows gains nothing.
        assert_eq!(
            folder(Folder::Directory, false, false).with_tile_edits(true),
            folder(Folder::Directory, false, false)
        );
    }

    #[test]
    fn rows_that_open_a_surface_say_which() {
        assert_eq!(Menu::opens("scrape_system"), Some(Opens::Setup));
        assert_eq!(Menu::opens("change_launcher"), Some(Opens::Picker));
        assert_eq!(Menu::opens("qr_code"), Some(Opens::QrCode));
        assert_eq!(Menu::opens("write_card"), Some(Opens::TokenWrite));
        assert_eq!(Menu::opens("more_info"), Some(Opens::Details));
        assert_eq!(Menu::opens("hub_rename"), Some(Opens::Rename));
        assert_eq!(Menu::opens("hub_icon"), Some(Opens::IconBrowser));
        assert_eq!(Menu::opens("hub_move"), Some(Opens::Move));
        // Rows that act at once, and rows that turn a page, open nothing.
        assert_eq!(Menu::opens("toggle_favorite"), None);
        assert_eq!(Menu::opens("index_system"), None);
        assert_eq!(Menu::opens("manage_game"), None);
    }

    #[test]
    fn only_rows_that_open_something_slow_keep_the_menu_up() {
        assert!(Menu::holds_open("hub_rename"));
        assert!(Menu::holds_open("hub_icon"));
        assert!(Menu::holds_open("change_launcher"));
        assert!(!Menu::holds_open("hub_move"));
        assert!(Menu::holds_open("more_info"));
    }

    #[test]
    fn every_page_is_opened_by_the_row_it_names() {
        for page in [
            Page::WriteToken,
            Page::ManageGame,
            Page::ManageSystem,
            Page::ManageCategory,
        ] {
            let menu = Menu {
                root: vec![row(page.menu_id())],
                write: vec![row("write_card")],
                manage: vec![row("scrape_game")],
                ..Menu::default()
            };
            assert_eq!(menu.page_for(page.menu_id()), Some(page));
        }
    }
}
