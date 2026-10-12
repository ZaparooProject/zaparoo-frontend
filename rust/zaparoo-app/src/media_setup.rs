// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The media-job setup forms: what the Update media database and Update
//! metadata modals ask before they start, the scope vocabulary they
//! offer, and how a scope resolves to the systems Core is given.

use crate::form_list::Role;
use crate::system_picker::{self, Row, Section, System};

/// Offer setup only after Core's status is known and no background update
/// already owns the empty database. Accepting the offer ends onboarding.
pub fn needs_first_run(
    indexed_systems: usize,
    already_shown: bool,
    status_seeded: bool,
    media_busy: bool,
) -> bool {
    indexed_systems == 0 && !already_shown && status_seeded && !media_busy
}

/// Which job the form starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Index,
    Scrape,
}

/// One form row. The ids double as the label vocabulary keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRow {
    /// Which scraper runs (Scrape only).
    Source,
    /// Which systems the job covers.
    Systems,
    /// Replace metadata that is already there (Scrape only).
    Rescrape,
    /// Start the job.
    Start,
}

impl FormRow {
    pub fn id(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Systems => "systems",
            Self::Rescrape => "rescrape",
            Self::Start => "start",
        }
    }

    /// Which control the row carries, in the settings vocabulary.
    pub fn control(self) -> crate::settings::Control {
        match self {
            Self::Source | Self::Systems => crate::settings::Control::Picker,
            Self::Rescrape => crate::settings::Control::Toggle,
            Self::Start => crate::settings::Control::Action,
        }
    }
}

/// The rows of a form, in order.
pub fn rows(kind: Kind) -> &'static [FormRow] {
    match kind {
        Kind::Index => &[FormRow::Systems, FormRow::Start],
        Kind::Scrape => &[
            FormRow::Source,
            FormRow::Systems,
            FormRow::Rescrape,
            FormRow::Start,
        ],
    }
}

/// The label the confirm button shows for the focused row
/// (`focusedActionLabel`), as a vocabulary key. `toggles` is whether the
/// picker page's focused row is one Accept checks rather than picks.
pub fn action_label_key(row: Option<FormRow>, on_picker_page: bool, toggles: bool) -> &'static str {
    if on_picker_page {
        return if toggles { "toggle" } else { "select" };
    }
    match row {
        Some(FormRow::Start) => "start",
        Some(FormRow::Rescrape) => "toggle",
        _ => "change",
    }
}

/// The form clamps at its ends rather than wrapping.
pub fn move_index(index: usize, len: usize, delta: i64) -> usize {
    if len == 0 {
        return 0;
    }
    let last = (len - 1) as i64;
    (index as i64 + delta).clamp(0, last) as usize
}

/// The one game a metadata update was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameTarget {
    /// Core's media ID, where the row carries one.
    pub media_id: Option<i64>,
    pub system: String,
    pub path: String,
    /// The title the row shows.
    pub name: String,
}

/// A scope the job can run over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every system Core knows.
    All,
    /// Every system in one category.
    Category(String),
    /// One system.
    System(String),
    /// Several systems, checked one by one in the picker.
    Systems(Vec<String>),
    /// One game (Scrape only).
    Game(GameTarget),
}

/// The systems Core is given for a scope; All answers an empty list,
/// which Core reads as "everything".
pub fn resolved_systems(
    scope: &Scope,
    category_systems: &dyn Fn(&str) -> Vec<String>,
) -> Vec<String> {
    match scope {
        Scope::All => Vec::new(),
        Scope::Category(category) => category_systems(category),
        Scope::System(id) => vec![id.clone()],
        Scope::Systems(ids) => ids.clone(),
        // A game's own system: what its source has to cover. The job
        // itself is given the game, not this list (`scrape_item`).
        Scope::Game(game) => vec![game.system.clone()],
    }
}

/// The single item Core is asked to scrape for a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrapeItem {
    /// An indexed media ID, the form Core prefers.
    Media(i64),
    /// One indexed file within a system, for a row with no ID.
    File { system: String, path: String },
}

/// How a game is named to Core. None when the row carries neither an ID
/// nor a path, which leaves nothing narrower than its system to ask for.
pub fn scrape_item(game: &GameTarget) -> Option<ScrapeItem> {
    match game.media_id {
        Some(id) => Some(ScrapeItem::Media(id)),
        None if game.path.is_empty() || game.system.is_empty() => None,
        None => Some(ScrapeItem::File {
            system: game.system.clone(),
            path: game.path.clone(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    All,
    Category,
    System,
    /// Several checked systems; the name is how many.
    Systems,
    /// The game the form was opened on; the name is its title.
    Game,
    /// A manufacturer's name over its systems; not a scope, never picked.
    Header,
    /// The header over the systems that were checked when the page opened.
    Selected,
}

impl ScopeKind {
    /// A section heading rather than a row the cursor can rest on.
    pub fn is_header(self) -> bool {
        matches!(self, Self::Header | Self::Selected)
    }
}

/// One row of the scope picker: its token, plus how the view should
/// label it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeEntry {
    pub token: String,
    /// The view translates the scope separately from its display name.
    pub kind: ScopeKind,
    /// The category id, the system's display name or the game's title;
    /// empty for All.
    pub name: String,
    /// A system row that is part of the selection being built.
    pub checked: bool,
}

/// What the picker page is showing beyond the catalog itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Picks<'a> {
    /// The system ids checked right now.
    pub checked: &'a [String],
    /// The ids that were checked when the page opened. They lead the list
    /// under their own header, and stay there while the page is open so a
    /// row never moves under the cursor.
    pub pinned: &'a [String],
    /// The game the form was opened on, offered above everything else.
    pub game: Option<&'a GameTarget>,
}

/// The scope list the picker page offers: the game the form was opened on,
/// All systems, every category that has indexable systems, the systems
/// already checked, then every system under its manufacturer, the way the
/// system picker lists them.
pub fn scope_entries(categories: &[String], systems: &[System], picks: &Picks) -> Vec<ScopeEntry> {
    let plain = |token: String, kind: ScopeKind, name: String| ScopeEntry {
        token,
        kind,
        name,
        checked: false,
    };
    let system_entry = |system: System| ScopeEntry {
        checked: picks.checked.contains(&system.id),
        token: system.id,
        kind: ScopeKind::System,
        name: system.name,
    };
    let mut entries = Vec::new();
    if let Some(game) = picks.game {
        entries.push(plain(String::new(), ScopeKind::Game, game.name.clone()));
    }
    entries.push(plain("*".to_string(), ScopeKind::All, String::new()));
    entries.extend(categories.iter().map(|category| {
        plain(
            format!("cat:{category}"),
            ScopeKind::Category,
            category.clone(),
        )
    }));
    let rows = system_picker::rows(systems, &[]);
    // A catalog short enough to read whole has no sections to lead.
    let sectioned = rows.iter().any(|row| matches!(row, Row::Header(_)));
    if sectioned {
        let pinned: Vec<System> = rows
            .iter()
            .filter_map(|row| match row {
                Row::System(system) if picks.pinned.contains(&system.id) => Some(system.clone()),
                _ => None,
            })
            .collect();
        if !pinned.is_empty() {
            entries.push(plain(String::new(), ScopeKind::Selected, String::new()));
            entries.extend(pinned.into_iter().map(system_entry));
        }
    }
    entries.extend(rows.into_iter().map(|row| match row {
        // Empty for the systems with no manufacturer, which the view
        // words.
        Row::Header(section) => plain(
            String::new(),
            ScopeKind::Header,
            match section {
                Section::Manufacturer(name) => name,
                Section::Recent | Section::Other => String::new(),
            },
        ),
        Row::System(system) => system_entry(system),
    }));
    entries
}

/// What each scope row is to the list cursor.
pub fn scope_roles(entries: &[ScopeEntry]) -> Vec<Role> {
    entries
        .iter()
        .map(|entry| {
            if entry.kind.is_header() {
                Role::Header
            } else {
                Role::Option
            }
        })
        .collect()
}

/// The row the picker page opens on: the scope's own row, or the first
/// checked system. The top of the list when the scope has no row.
pub fn scope_seat(entries: &[ScopeEntry], scope: &Scope) -> usize {
    let found = match scope {
        Scope::All => entries.iter().position(|e| e.kind == ScopeKind::All),
        Scope::Category(id) => entries
            .iter()
            .position(|e| e.kind == ScopeKind::Category && &e.name == id),
        Scope::Game(_) => entries.iter().position(|e| e.kind == ScopeKind::Game),
        Scope::System(_) | Scope::Systems(_) => entries
            .iter()
            .position(|e| e.kind == ScopeKind::System && e.checked),
    };
    found.unwrap_or(0)
}

/// The system ids a scope checks when the picker page opens.
pub fn checked_systems(scope: &Scope) -> Vec<String> {
    match scope {
        Scope::System(id) => vec![id.clone()],
        Scope::Systems(ids) => ids.clone(),
        Scope::All | Scope::Category(_) | Scope::Game(_) => Vec::new(),
    }
}

/// The scope a one-press row sets: the game, All systems or a category.
/// None for a system row, which Accept checks instead, and for a header.
pub fn picked_scope(entry: &ScopeEntry, game: Option<&GameTarget>) -> Option<Scope> {
    match entry.kind {
        ScopeKind::All => Some(Scope::All),
        ScopeKind::Category => Some(Scope::Category(entry.name.clone())),
        ScopeKind::Game => game.cloned().map(Scope::Game),
        ScopeKind::System | ScopeKind::Systems | ScopeKind::Header | ScopeKind::Selected => None,
    }
}

/// The scope the form holds once the picker page is left: the checked
/// systems in the order the list shows them, or the scope it had before
/// when nothing is checked.
pub fn scope_after_checks(entries: &[ScopeEntry], previous: &Scope) -> Scope {
    let mut ids: Vec<&str> = Vec::new();
    for entry in entries {
        // A checked system can be listed twice: under Selected and under
        // its manufacturer.
        if entry.kind == ScopeKind::System && entry.checked && !ids.contains(&entry.token.as_str())
        {
            ids.push(&entry.token);
        }
    }
    match ids.as_slice() {
        [] => previous.clone(),
        [id] => Scope::System((*id).to_string()),
        _ => Scope::Systems(ids.into_iter().map(str::to_string).collect()),
    }
}

/// The label for the scope a form currently holds, as the same
/// (kind, name) pair the picker rows carry.
pub fn scope_label(scope: &Scope, system_name: &dyn Fn(&str) -> String) -> (ScopeKind, String) {
    match scope {
        Scope::All => (ScopeKind::All, String::new()),
        Scope::Category(id) => (ScopeKind::Category, id.clone()),
        Scope::System(id) => (ScopeKind::System, system_name(id)),
        Scope::Systems(ids) => (ScopeKind::Systems, ids.len().to_string()),
        Scope::Game(game) => (ScopeKind::Game, game.name.clone()),
    }
}

/// The scraper a one-step metadata update runs with, from Core's offer as
/// `(id, supported systems)` pairs, where an empty list covers every
/// system. The saved choice wins while Core offers it and it covers every
/// requested system (an empty request means every system); otherwise the
/// first offered scraper that does. None when nothing covers them.
pub fn scraper_for<'a>(
    offered: &[(&'a str, &[String])],
    preferred: &str,
    systems: &[String],
) -> Option<&'a str> {
    let covers = |supported: &[String]| {
        supported.is_empty()
            || (!systems.is_empty() && systems.iter().all(|system| supported.contains(system)))
    };
    offered
        .iter()
        .find(|(id, supported)| *id == preferred && covers(supported))
        .or_else(|| offered.iter().find(|(_, supported)| covers(supported)))
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_waits_for_status_and_never_blocks_an_existing_update() {
        assert!(needs_first_run(0, false, true, false));
        assert!(!needs_first_run(0, false, false, false));
        assert!(!needs_first_run(0, false, true, true));
        assert!(!needs_first_run(0, true, true, false));
        assert!(!needs_first_run(1, false, true, false));
    }

    #[test]
    fn each_form_has_its_own_rows() {
        assert_eq!(rows(Kind::Index), &[FormRow::Systems, FormRow::Start]);
        assert_eq!(
            rows(Kind::Scrape),
            &[
                FormRow::Source,
                FormRow::Systems,
                FormRow::Rescrape,
                FormRow::Start
            ]
        );
        assert_eq!(FormRow::Systems.control(), crate::settings::Control::Picker);
        assert_eq!(
            FormRow::Rescrape.control(),
            crate::settings::Control::Toggle
        );
        assert_eq!(FormRow::Start.control(), crate::settings::Control::Action);
    }

    #[test]
    fn the_confirm_label_follows_the_focused_row() {
        assert_eq!(
            action_label_key(Some(FormRow::Start), false, false),
            "start"
        );
        assert_eq!(
            action_label_key(Some(FormRow::Rescrape), false, false),
            "toggle"
        );
        assert_eq!(
            action_label_key(Some(FormRow::Systems), false, false),
            "change"
        );
        assert_eq!(
            action_label_key(Some(FormRow::Source), false, false),
            "change"
        );
        // The picker page confirms with Select, or Toggle on a system row.
        assert_eq!(
            action_label_key(Some(FormRow::Start), true, false),
            "select"
        );
        assert_eq!(
            action_label_key(Some(FormRow::Systems), true, true),
            "toggle"
        );
        assert_eq!(action_label_key(None, false, false), "change");
    }

    #[test]
    fn the_form_cursor_clamps_at_both_ends() {
        assert_eq!(move_index(0, 4, -1), 0);
        assert_eq!(move_index(0, 4, 1), 1);
        assert_eq!(move_index(3, 4, 1), 3);
        assert_eq!(move_index(3, 4, -1), 2);
        assert_eq!(move_index(0, 0, 1), 0);
    }

    #[test]
    fn a_scope_resolves_to_the_systems_core_is_given() {
        let ids = |category: &str| match category {
            "Console" => vec!["NES".to_string(), "SNES".to_string()],
            _ => Vec::new(),
        };
        assert!(resolved_systems(&Scope::All, &ids).is_empty());
        assert_eq!(
            resolved_systems(&Scope::Category("Console".into()), &ids),
            vec!["NES".to_string(), "SNES".to_string()]
        );
        assert_eq!(
            resolved_systems(&Scope::System("Arcade".into()), &ids),
            vec!["Arcade".to_string()]
        );
        assert_eq!(
            resolved_systems(&Scope::Systems(vec!["NES".into(), "PSX".into()]), &ids),
            vec!["NES".to_string(), "PSX".to_string()]
        );
        // A game answers its own system, for the source-coverage check.
        assert_eq!(
            resolved_systems(&Scope::Game(game(Some(7))), &ids),
            vec!["SNES".to_string()]
        );
    }

    fn game(media_id: Option<i64>) -> GameTarget {
        GameTarget {
            media_id,
            system: "SNES".into(),
            path: "/games/SNES/Game.sfc".into(),
            name: "Game".into(),
        }
    }

    #[test]
    fn a_game_is_named_to_core_by_id_then_by_file() {
        assert_eq!(scrape_item(&game(Some(7))), Some(ScrapeItem::Media(7)));
        assert_eq!(
            scrape_item(&game(None)),
            Some(ScrapeItem::File {
                system: "SNES".into(),
                path: "/games/SNES/Game.sfc".into(),
            })
        );
        // Neither an ID nor a path: nothing narrower than the system.
        let bare = GameTarget {
            path: String::new(),
            ..game(None)
        };
        assert_eq!(scrape_item(&bare), None);
    }

    fn system(id: &str, name: &str, manufacturer: &str) -> System {
        System {
            id: id.into(),
            name: name.into(),
            manufacturer: manufacturer.into(),
            games: None,
        }
    }

    #[test]
    fn the_scope_list_leads_with_everything_then_categories() {
        let entries = scope_entries(
            &["Console".to_string()],
            &[system("NES", "Nintendo", "Nintendo")],
            &Picks::default(),
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].token, "*");
        assert_eq!(entries[0].kind, ScopeKind::All);
        assert_eq!(entries[1].token, "cat:Console");
        assert_eq!(entries[1].kind, ScopeKind::Category);
        assert_eq!(entries[1].name, "Console");
        assert_eq!(entries[2].token, "NES");
        assert_eq!(entries[2].kind, ScopeKind::System);
        assert_eq!(entries[2].name, "Nintendo");
    }

    #[test]
    fn a_long_scope_list_puts_systems_under_their_manufacturer() {
        let systems: Vec<System> = (0..6)
            .map(|n| system(&format!("N{n}"), &format!("Nintendo {n}"), "Nintendo"))
            .chain((0..4).map(|n| system(&format!("X{n}"), &format!("Other {n}"), "")))
            .collect();
        let entries = scope_entries(&[], &systems, &Picks::default());
        let kinds: Vec<ScopeKind> = entries.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds[..3],
            [ScopeKind::All, ScopeKind::Header, ScopeKind::System]
        );
        assert_eq!(entries[1].name, "Nintendo");
        // The systems with no manufacturer come last, under a header the
        // view words.
        let other = entries.iter().rposition(|e| e.kind == ScopeKind::Header);
        assert_eq!(other, Some(8));
        assert_eq!(entries[8].name, "");
        let roles = scope_roles(&entries);
        assert_eq!(roles[0], Role::Option);
        assert_eq!(roles[1], Role::Header);
        assert_eq!(roles.iter().filter(|r| **r == Role::Header).count(), 2);
    }

    #[test]
    fn the_form_labels_its_scope_the_way_the_picker_does() {
        let name = |id: &str| format!("{id} name");
        assert_eq!(
            scope_label(&Scope::All, &name),
            (ScopeKind::All, String::new())
        );
        assert_eq!(
            scope_label(&Scope::Category("Handheld".into()), &name),
            (ScopeKind::Category, "Handheld".to_string())
        );
        assert_eq!(
            scope_label(&Scope::System("NES".into()), &name),
            (ScopeKind::System, "NES name".to_string())
        );
        // Several systems are a count, a game its own title.
        assert_eq!(
            scope_label(&Scope::Systems(vec!["NES".into(), "SNES".into()]), &name),
            (ScopeKind::Systems, "2".to_string())
        );
        assert_eq!(
            scope_label(&Scope::Game(game(Some(7))), &name),
            (ScopeKind::Game, "Game".to_string())
        );
    }

    /// Ten systems under two makers: enough for the list to be sectioned.
    fn catalog() -> Vec<System> {
        (0..6)
            .map(|n| system(&format!("N{n}"), &format!("Nintendo {n}"), "Nintendo"))
            .chain((0..4).map(|n| system(&format!("S{n}"), &format!("Sega {n}"), "Sega")))
            .collect()
    }

    #[test]
    fn the_game_the_form_opened_on_leads_the_scope_list() {
        let game = game(Some(7));
        let picks = Picks {
            game: Some(&game),
            ..Picks::default()
        };
        let entries = scope_entries(&[], &catalog(), &picks);
        assert_eq!(entries[0].kind, ScopeKind::Game);
        assert_eq!(entries[0].name, "Game");
        assert_eq!(entries[1].kind, ScopeKind::All);
        assert_eq!(scope_seat(&entries, &Scope::Game(game.clone())), 0);
        assert_eq!(
            picked_scope(&entries[0], Some(&game)),
            Some(Scope::Game(game))
        );
        assert_eq!(picked_scope(&entries[1], None), Some(Scope::All));
    }

    #[test]
    fn checked_systems_lead_the_list_under_their_own_header() {
        let checked = vec!["S1".to_string(), "N2".to_string()];
        let picks = Picks {
            checked: &checked,
            pinned: &checked,
            game: None,
        };
        let entries = scope_entries(&["Console".to_string()], &catalog(), &picks);
        let kinds: Vec<ScopeKind> = entries.iter().take(5).map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                ScopeKind::All,
                ScopeKind::Category,
                ScopeKind::Selected,
                ScopeKind::System,
                ScopeKind::System
            ]
        );
        // In the list's own order, whatever order they were checked in.
        assert_eq!(entries[3].token, "N2");
        assert_eq!(entries[4].token, "S1");
        assert!(entries[3].checked && entries[4].checked);
        // They stay under their manufacturer too, checked there as well.
        assert_eq!(entries.iter().filter(|e| e.token == "N2").count(), 2);
        assert!(entries
            .iter()
            .filter(|e| e.token == "N2")
            .all(|e| e.checked));
        // The Selected heading is passed over like a manufacturer's.
        assert_eq!(scope_roles(&entries)[2], Role::Header);
        // A system row is checked, never picked.
        assert_eq!(picked_scope(&entries[3], None), None);
        // The page opens on the first checked system.
        assert_eq!(scope_seat(&entries, &Scope::Systems(checked.clone())), 3);
        assert_eq!(scope_seat(&entries, &Scope::Category("Console".into())), 1);
    }

    #[test]
    fn unchecking_a_pinned_system_leaves_its_row_where_it_was() {
        let pinned = vec!["N2".to_string()];
        let picks = Picks {
            checked: &[],
            pinned: &pinned,
            game: None,
        };
        let entries = scope_entries(&[], &catalog(), &picks);
        assert_eq!(entries[1].kind, ScopeKind::Selected);
        assert_eq!(entries[2].token, "N2");
        assert!(!entries[2].checked);
    }

    #[test]
    fn a_short_catalog_has_no_selected_section() {
        let pinned = vec!["NES".to_string()];
        let picks = Picks {
            checked: &pinned,
            pinned: &pinned,
            game: None,
        };
        let entries = scope_entries(&[], &[system("NES", "Nintendo", "Nintendo")], &picks);
        assert_eq!(entries.len(), 2);
        assert!(entries[1].checked);
        assert_eq!(scope_seat(&entries, &Scope::System("NES".into())), 1);
    }

    #[test]
    fn leaving_the_page_keeps_the_checks_as_the_scope() {
        let previous = Scope::Category("Console".into());
        let scope_for = |checked: &[String]| {
            let picks = Picks {
                checked,
                pinned: checked,
                game: None,
            };
            scope_after_checks(&scope_entries(&[], &catalog(), &picks), &previous)
        };
        // Nothing checked: the form keeps what it had.
        assert_eq!(scope_for(&[]), previous);
        assert_eq!(scope_for(&["S1".to_string()]), Scope::System("S1".into()));
        // Several, once each, in the list's order.
        assert_eq!(
            scope_for(&["S1".to_string(), "N2".to_string()]),
            Scope::Systems(vec!["N2".into(), "S1".into()])
        );
    }

    #[test]
    fn a_scope_checks_its_own_systems_when_the_page_opens() {
        assert!(checked_systems(&Scope::All).is_empty());
        assert!(checked_systems(&Scope::Game(game(Some(7)))).is_empty());
        assert_eq!(
            checked_systems(&Scope::System("NES".into())),
            vec!["NES".to_string()]
        );
        assert_eq!(
            checked_systems(&Scope::Systems(vec!["NES".into(), "SNES".into()])),
            vec!["NES".to_string(), "SNES".to_string()]
        );
    }

    #[test]
    fn scraper_for_prefers_the_saved_choice_when_it_covers_the_request() {
        let nes = vec!["NES".to_string()];
        let many = vec!["NES".to_string(), "ScummVM".to_string()];
        let apps = vec!["Android".to_string()];
        let offered: Vec<(&str, &[String])> = vec![
            ("android-apps", &apps),
            ("libretro-thumbnails", &many),
            ("media-folder", &[]),
        ];
        let scummvm = vec!["ScummVM".to_string()];
        // Saved choice offered and covering: kept.
        assert_eq!(
            scraper_for(&offered, "libretro-thumbnails", &scummvm),
            Some("libretro-thumbnails")
        );
        // Saved choice Core does not offer: first covering scraper.
        assert_eq!(
            scraper_for(&offered, "gamelist.xml", &scummvm),
            Some("libretro-thumbnails")
        );
        // Saved choice that does not cover the request is skipped.
        assert_eq!(
            scraper_for(&offered, "android-apps", &nes),
            Some("libretro-thumbnails")
        );
        // Every system: only an unrestricted scraper covers it.
        assert_eq!(
            scraper_for(&offered, "libretro-thumbnails", &[]),
            Some("media-folder")
        );
        // Nothing covers the request.
        let psx = vec!["PSX".to_string()];
        assert_eq!(scraper_for(&offered[..2], "gamelist.xml", &psx), None);
    }
}
