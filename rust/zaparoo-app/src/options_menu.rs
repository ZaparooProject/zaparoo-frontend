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

/// A page of the menu, shown in place of its own rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    WriteToken,
    ManageGame,
    ManageSystem,
    ManageCategory,
}

impl Page {
    /// The Options row that opens the page.
    pub fn menu_id(self) -> &'static str {
        match self {
            Self::WriteToken => "write_token",
            Self::ManageGame => "manage_game",
            Self::ManageSystem => "manage_system",
            Self::ManageCategory => "manage_category",
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
            _ => return None,
        };
        (self.root.iter().any(|row| row.id == id) && !self.rows(page).is_empty()).then_some(page)
    }

    pub fn rows(&self, page: Page) -> &[Row] {
        match page {
            Page::WriteToken => &self.write,
            Page::ManageGame | Page::ManageSystem | Page::ManageCategory => &self.manage,
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

    fn with_manage(mut self, page: Page) -> Self {
        if !self.manage.is_empty() {
            self.root.push(row(page.menu_id()));
        }
        self
    }
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

fn favorite_row(is_favorite: bool) -> Row {
    keyed(
        "toggle_favorite",
        if is_favorite {
            "favorite:remove"
        } else {
            "favorite:add"
        },
    )
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
    /// Core reported the game's tags, so its favorite state is known.
    pub favorite_known: bool,
    pub is_favorite: bool,
    /// A folder of one game's discs.
    pub multi_disc: bool,
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
}

/// A game, or a folder or archive Core launches as one.
pub fn game(input: &GameInput) -> Menu {
    let mut menu = Menu::default();
    // Accept already launches the game, so Details leads.
    menu.root.push(row("more_info"));
    if input.favorite_known {
        menu.root.push(favorite_row(input.is_favorite));
    }
    if input.multi_disc {
        menu.root.push(row("choose_disc"));
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
            favorite_known: true,
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
            ["more_info", "favorite:add", "write_token", "manage_game"]
        );
        assert_eq!(
            keys(&menu.manage),
            ["change_launcher", "add_to_hub", "hide:hide", "scrape_game"]
        );
        assert_eq!(menu.page_for("manage_game"), Some(Page::ManageGame));
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
                "favorite:add",
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
                "favorite:add",
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
                "favorite:add",
                "choose_disc",
                "write_token",
                "discover",
                "manage_game"
            ]
        );
    }

    #[test]
    fn toggles_word_the_state_they_would_change() {
        let menu = game(&GameInput {
            is_favorite: true,
            on_hub: true,
            is_hidden: true,
            ..regular()
        });
        assert_eq!(menu.root[1], keyed("toggle_favorite", "favorite:remove"));
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

        // History Core could not resolve: no tags, so no favorite state to
        // toggle, and nothing is assumed in its place.
        let unresolved = game(&GameInput {
            favorite_known: false,
            ..regular()
        });
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
            ["more_info", "favorite:add", "manage_game"]
        );
    }

    #[test]
    fn an_empty_manage_page_is_never_offered() {
        let menu = game(&GameInput {
            favorite_known: true,
            has_payload: true,
            ..GameInput::default()
        });
        assert_eq!(
            keys(&menu.root),
            ["more_info", "favorite:add", "write_token"]
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
            ["more_info", "favorite:add", "write_token", "manage_game"]
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
            };
            assert_eq!(menu.page_for(page.menu_id()), Some(page));
        }
    }
}
