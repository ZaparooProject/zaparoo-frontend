// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Port of `src/ui/screens/HubScreen.qml`'s rules: how the persisted
// `[[hub.items]]` layout becomes the flat list of tiles one paged grid
// renders, which tile the persisted state restores focus to, what a focus
// move persists, and the board-model helpers a Move session needs. The
// layout records intent (this tile exists, here); each resolver folds in
// the live rule its kind needs, and a tile whose precondition is not met
// stays in place with `disabled` set instead of vanishing.
//
// Toolkit-free and Core-free: the caller hands in the visible layout
// items, the live state, and a resolver for system names and art.

use crate::paged_grid::{self, Fit, Grid, Insets};
use crate::sizing::{Derived, Inputs, Tier};

/// The built-in action ids in their seed order (`hub_layout.rs`).
pub const BUILT_IN_ACTIONS: [&str; 5] = ["resume", "favorites", "recents", "update", "settings"];

/// One visible `[[hub.items]]` entry, as the layout stores it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LayoutItem {
    pub kind: String,
    pub id: String,
    pub path: String,
    pub script: String,
    pub name: String,
    pub icon: String,
    pub system: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Category,
    Action,
    System,
    Folder,
    ZapScript,
    /// A blank cell: a persisted spacer or the last page's tail padding.
    Empty,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Action => "action",
            Self::System => "system",
            Self::Folder => "folder",
            Self::ZapScript => "zapscript",
            Self::Empty => "empty",
        }
    }
}

/// Why a tile is disabled; the view words it (docs/style.md, "Hidden and
/// disabled tiles").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reason {
    #[default]
    None,
    NoRecentGames,
    NoInternet,
    NotAvailable,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::NoRecentGames => "no-recent-games",
            Self::NoInternet => "no-internet",
            Self::NotAvailable => "not-available",
        }
    }
}

/// A grid-ready tile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub kind: Option<Kind>,
    pub id: String,
    pub path: String,
    pub script: String,
    /// Owning system of a folder shortcut, carried through to Accept.
    pub system: String,
    /// Translatable label id (`category:Arcade`, `action:favorites`), or
    /// empty when `name` is free text.
    pub label_key: String,
    /// Free-text name, or the resumable game's name for `action:resume`.
    pub name: String,
    /// Resource path of the art: `categories/...`, `icons/...`,
    /// `systems/<id>`, or a media cover key the host resolves.
    pub cover_key: String,
    pub disabled: bool,
    pub reason: Reason,
    /// The entry's real position in the layout, or -1 for the bootstrap
    /// placeholders and the tail padding.
    pub hub_index: i32,
}

impl Entry {
    pub fn is_empty(&self) -> bool {
        self.kind == Some(Kind::Empty)
    }

    fn blank(hub_index: i32) -> Self {
        Self {
            kind: Some(Kind::Empty),
            hub_index,
            ..Self::default()
        }
    }
}

/// Live state the resolvers read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per live visibility rule the QML resolvers read"
)]
pub struct Live<'a> {
    /// Core has answered with a category list this launch.
    pub categories_loaded: bool,
    /// Categories Core currently lists (canonical ids).
    pub confirmed_categories: &'a [String],
    /// Resume may read Core history (false on the first-paint path).
    pub resume_enabled: bool,
    pub resume_name: &'a str,
    /// Resolved cover key for the resumable game; empty for the glyph.
    pub resume_cover_key: &'a str,
    /// Core history confirmed there is nothing to resume.
    pub resume_known_unavailable: bool,
    /// This build carries the Update screen.
    pub update_enabled: bool,
    pub internet_available: bool,
}

/// Name and art lookups the caller owns.
pub trait Resolver {
    fn system_name(&self, id: &str) -> String;
    fn system_cover_key(&self, id: &str) -> String;
    fn media_cover_key(&self, system: &str, path: &str) -> String;
    /// A user override from the `custom/hub/` folder for `id`, if any.
    fn hub_override(&self, _id: &str) -> Option<String> {
        None
    }
}

fn hub_cover_key(resolver: &dyn Resolver, id: &str, fallback: &str) -> String {
    resolver
        .hub_override(id)
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/// `CategoryIds.canonicalize`: the plural spellings older configs used.
pub fn canonical_category(id: &str) -> &str {
    match id {
        "Computers" => "Computer",
        "Consoles" => "Console",
        "Handhelds" => "Handheld",
        other => other,
    }
}

/// `CategoryIds.coverKey`.
pub fn category_cover_key(id: &str) -> String {
    format!("categories/{}", canonical_category(id))
}

fn action_entry(
    resolver: &dyn Resolver,
    id: &str,
    fallback_cover: &str,
    disabled: bool,
    reason: Reason,
) -> Entry {
    Entry {
        kind: Some(Kind::Action),
        id: id.to_string(),
        label_key: format!("action:{id}"),
        cover_key: hub_cover_key(resolver, id, fallback_cover),
        disabled,
        reason,
        hub_index: -1,
        ..Entry::default()
    }
}

fn resolve_action(live: &Live, resolver: &dyn Resolver, id: &str) -> Option<Entry> {
    match id {
        "resume" => {
            let name = if live.resume_enabled {
                live.resume_name
            } else {
                ""
            };
            let fallback = if live.resume_enabled && !live.resume_cover_key.is_empty() {
                live.resume_cover_key
            } else {
                "icons/PlayOutline"
            };
            let mut entry = action_entry(
                resolver,
                id,
                fallback,
                live.resume_known_unavailable,
                if live.resume_known_unavailable {
                    Reason::NoRecentGames
                } else {
                    Reason::None
                },
            );
            entry.name = name.to_string();
            Some(entry)
        }
        "favorites" => Some(action_entry(
            resolver,
            id,
            "icons/HeartOutline",
            false,
            Reason::None,
        )),
        "recents" => Some(action_entry(
            resolver,
            id,
            "icons/History",
            false,
            Reason::None,
        )),
        "update" => {
            if !live.update_enabled {
                return None;
            }
            Some(action_entry(
                resolver,
                id,
                "icons/RefreshCw",
                !live.internet_available,
                if live.internet_available {
                    Reason::None
                } else {
                    Reason::NoInternet
                },
            ))
        }
        "settings" => Some(action_entry(
            resolver,
            id,
            "icons/Tools",
            false,
            Reason::None,
        )),
        _ => None,
    }
}

fn resolve_category(live: &Live, resolver: &dyn Resolver, id: &str) -> Entry {
    // The layout's id is canonicalized (an older build may have written
    // a plural), then compared against Core's list as Core spells it -
    // `HubScreen.qml`'s `index_for_category(canonicalId)`. Core sends the
    // canonical singular ids (`Console`, `Handheld`, ...).
    let canonical = canonical_category(id);
    let unconfirmed =
        live.categories_loaded && !live.confirmed_categories.iter().any(|c| c == canonical);
    Entry {
        kind: Some(Kind::Category),
        id: canonical.to_string(),
        // The id doubles as the free-text fallback for a category the
        // catalogs have no word for.
        name: canonical.to_string(),
        label_key: format!("category:{canonical}"),
        cover_key: hub_cover_key(resolver, canonical, &category_cover_key(canonical)),
        disabled: unconfirmed,
        reason: if unconfirmed {
            Reason::NotAvailable
        } else {
            Reason::None
        },
        hub_index: -1,
        ..Entry::default()
    }
}

fn resolve_system(resolver: &dyn Resolver, item: &LayoutItem) -> Option<Entry> {
    if item.id.is_empty() {
        return None;
    }
    Some(Entry {
        kind: Some(Kind::System),
        id: item.id.clone(),
        name: if item.name.is_empty() {
            resolver.system_name(&item.id)
        } else {
            item.name.clone()
        },
        cover_key: if item.icon.is_empty() {
            resolver.system_cover_key(&item.id)
        } else {
            hub_cover_key(resolver, &item.icon, "icons/File")
        },
        hub_index: -1,
        ..Entry::default()
    })
}

/// The path's final segment, the way `GamesScreen` names a folder.
pub fn folder_name_for_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_string()
}

fn resolve_folder(resolver: &dyn Resolver, item: &LayoutItem) -> Option<Entry> {
    if item.path.is_empty() {
        return None;
    }
    Some(Entry {
        kind: Some(Kind::Folder),
        id: item.path.clone(),
        path: item.path.clone(),
        system: item.system.clone(),
        name: if item.name.is_empty() {
            folder_name_for_path(&item.path)
        } else {
            item.name.clone()
        },
        cover_key: if item.icon.is_empty() {
            hub_cover_key(resolver, &item.path, "icons/Folder")
        } else {
            hub_cover_key(resolver, &item.icon, "icons/Folder")
        },
        hub_index: -1,
        ..Entry::default()
    })
}

fn resolve_zapscript(resolver: &dyn Resolver, item: &LayoutItem) -> Option<Entry> {
    if item.script.is_empty() {
        return None;
    }
    let cover_key = if !item.icon.is_empty() {
        hub_cover_key(resolver, &item.icon, "icons/File")
    } else if !item.system.is_empty() && !item.path.is_empty() {
        resolver.media_cover_key(&item.system, &item.path)
    } else if !item.system.is_empty() {
        resolver.system_cover_key(&item.system)
    } else {
        "icons/File".to_string()
    };
    Some(Entry {
        kind: Some(Kind::ZapScript),
        id: item.script.clone(),
        script: item.script.clone(),
        name: if item.name.is_empty() {
            item.script.clone()
        } else {
            item.name.clone()
        },
        cover_key,
        hub_index: -1,
        ..Entry::default()
    })
}

fn resolve_item(
    live: &Live,
    resolver: &dyn Resolver,
    index: usize,
    item: &LayoutItem,
) -> Option<Entry> {
    let hub_index = i32::try_from(index).unwrap_or(-1);
    if item.kind == "blank" {
        return Some(Entry::blank(hub_index));
    }
    let mut entry = match item.kind.as_str() {
        "category" => Some(resolve_category(live, resolver, &item.id)),
        "action" => resolve_action(live, resolver, &item.id),
        "system" => resolve_system(resolver, item),
        "folder" => resolve_folder(resolver, item),
        "zapscript" => resolve_zapscript(resolver, item),
        _ => None,
    }?;
    entry.hub_index = hub_index;
    Some(entry)
}

/// The placeholder categories the bootstrap window shows before Core has
/// ever answered.
pub const PLACEHOLDER_CATEGORIES: [&str; 5] =
    ["Arcade", "Computer", "Console", "Handheld", "Other"];

/// Pad the end of the list up to a full page so the last page's remainder
/// renders as deliberate empty slots. `min_pages` is a floor on the page
/// count (a Move session reserves one extra page).
pub fn pad_to_page_size(mut list: Vec<Entry>, page_size: usize, min_pages: usize) -> Vec<Entry> {
    if page_size == 0 {
        return list;
    }
    let natural_pages = if list.is_empty() {
        1
    } else {
        list.len().div_ceil(page_size)
    };
    let target = natural_pages.max(min_pages) * page_size;
    while list.len() < target {
        list.push(Entry::blank(-1));
    }
    list
}

/// The flat item list the grid renders: the layout in the user's own
/// order once seeded, or the bootstrap placeholders (Resume, the five
/// categories, the remaining actions) while it is not.
pub fn entries(
    items: &[LayoutItem],
    unseeded: bool,
    live: &Live,
    resolver: &dyn Resolver,
    page_size: usize,
    min_pages: usize,
) -> Vec<Entry> {
    let mut list = Vec::new();
    if unseeded {
        if let Some(resume) = resolve_action(live, resolver, BUILT_IN_ACTIONS[0]) {
            list.push(resume);
        }
        for id in PLACEHOLDER_CATEGORIES {
            let mut entry = resolve_category(live, resolver, id);
            entry.disabled = false;
            entry.reason = Reason::None;
            list.push(entry);
        }
        for id in &BUILT_IN_ACTIONS[1..] {
            if let Some(entry) = resolve_action(live, resolver, id) {
                list.push(entry);
            }
        }
    } else {
        for (i, item) in items.iter().enumerate() {
            if let Some(entry) = resolve_item(live, resolver, i, item) {
                list.push(entry);
            }
        }
    }
    pad_to_page_size(list, page_size, min_pages)
}

/// Flat index of the first entry of `kind`.
pub fn first_index_of_kind(entries: &[Entry], kind: Kind) -> Option<usize> {
    entries.iter().position(|e| e.kind == Some(kind))
}

/// Flat index of the first entry matching (`kind`, `id`).
pub fn index_for(entries: &[Entry], kind: Kind, id: &str) -> Option<usize> {
    entries
        .iter()
        .position(|e| e.kind == Some(kind) && e.id == id)
}

/// The action with `id`, else the first action, else 0.
pub fn action_index_for_id(entries: &[Entry], id: &str) -> usize {
    index_for(entries, Kind::Action, id)
        .or_else(|| first_index_of_kind(entries, Kind::Action))
        .unwrap_or(0)
}

/// The category tile with `id`, else whichever category tile is on screen.
pub fn flat_index_for_category(entries: &[Entry], id: &str) -> Option<usize> {
    index_for(entries, Kind::Category, id).or_else(|| first_index_of_kind(entries, Kind::Category))
}

/// Flat index of the entry backed by layout position `hub_index`.
pub fn flat_index_for_hub_index(entries: &[Entry], hub_index: i32) -> Option<usize> {
    if hub_index < 0 {
        return None;
    }
    entries.iter().position(|e| e.hub_index == hub_index)
}

/// The persisted Hub state the restore reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Saved<'a> {
    pub category: &'a str,
    pub selected_row: u32,
    pub selected_action: &'a str,
    /// `"<kind>:<id>"`, authoritative when non-empty.
    pub selected_item: &'a str,
}

/// `restoreFromCategoriesReset`: which flat entry the persisted state
/// seats focus on. `category_ids` is Core's current list (canonical),
/// `fallback_action` the action an empty catalog lands on (`update` when
/// available with internet, else `settings`).
pub fn restore_index(
    entries: &[Entry],
    saved: &Saved,
    category_ids: &[String],
    resume_visible: bool,
    fallback_action: &str,
) -> usize {
    let saved_category = canonical_category(saved.category);
    let idx = if saved_category.is_empty() {
        None
    } else {
        category_ids.iter().position(|c| c == saved_category)
    };
    let chosen_category = if idx.is_some() {
        saved_category.to_string()
    } else {
        category_ids.first().cloned().unwrap_or_default()
    };
    let category_seat = || flat_index_for_category(entries, &chosen_category).unwrap_or(0);

    if !saved.selected_item.is_empty() {
        // Split on the first ':' only: a folder path or script body can
        // itself contain one.
        let (kind, id) = saved.selected_item.split_once(':').unwrap_or(("", ""));
        let kind = match kind {
            "category" => Some(Kind::Category),
            "action" => Some(Kind::Action),
            "system" => Some(Kind::System),
            "folder" => Some(Kind::Folder),
            "zapscript" => Some(Kind::ZapScript),
            _ => None,
        };
        if let Some(restored) = kind.and_then(|k| index_for(entries, k, id)) {
            return restored;
        }
        if idx.is_some() {
            return category_seat();
        }
        if resume_visible {
            return action_index_for_id(entries, "resume");
        }
        if category_ids.is_empty() {
            return action_index_for_id(entries, fallback_action);
        }
        return category_seat();
    }
    if saved.selected_row == 1 && !saved.selected_action.is_empty() {
        return action_index_for_id(entries, saved.selected_action);
    }
    if idx.is_some() {
        return category_seat();
    }
    if resume_visible {
        return action_index_for_id(entries, "resume");
    }
    if category_ids.is_empty() {
        return action_index_for_id(entries, fallback_action);
    }
    category_seat()
}

/// What a focus move persists (`_commitCurrent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub selected_item: String,
    pub selected_row: u32,
    /// Set for a category tile with an id.
    pub category: Option<String>,
    /// Set for every non-category tile.
    pub selected_action: Option<String>,
}

pub fn commit(entry: &Entry) -> Option<Commit> {
    let kind = entry.kind.filter(|k| *k != Kind::Empty)?;
    let selected_item = format!("{}:{}", kind.as_str(), entry.id);
    if kind == Kind::Category {
        Some(Commit {
            selected_item,
            selected_row: 0,
            category: (!entry.id.is_empty()).then(|| entry.id.clone()),
            selected_action: None,
        })
    } else {
        Some(Commit {
            selected_item,
            selected_row: 1,
            category: None,
            selected_action: Some(entry.id.clone()),
        })
    }
}

/// Flat index of the first synthetic tail-padding entry, or the length.
pub fn first_pad_flat_index(entries: &[Entry]) -> usize {
    entries
        .iter()
        .position(|e| e.hub_index < 0)
        .unwrap_or(entries.len())
}

/// The layout position placing a tile at flat index `flat` means: the
/// entry's own position when it is backed by the layout, else the
/// position the tail padding would create, offset from `item_count`.
pub fn real_index_for_flat(entries: &[Entry], flat: usize, item_count: usize) -> Option<usize> {
    let entry = entries.get(flat)?;
    if entry.hub_index >= 0 {
        return usize::try_from(entry.hub_index).ok();
    }
    let first_pad = first_pad_flat_index(entries);
    if first_pad >= entries.len() {
        return None;
    }
    Some(item_count + (flat - first_pad))
}

/// A held tile must never wrap around the grid's edges.
pub fn would_wrap_vertically(grid: &Grid, d_row: i32) -> bool {
    match d_row.signum() {
        1 => {
            grid.current_row() == grid.rows() - 1
                && grid.current_page() == grid.total_page_count() - 1
        }
        -1 => grid.current_row() == 0 && grid.current_page() == 0,
        _ => false,
    }
}

pub fn would_wrap_column(grid: &Grid, d_col: i32) -> bool {
    match d_col.signum() {
        1 => grid.current_column() == grid.columns() - 1,
        -1 => grid.current_column() == 0,
        _ => false,
    }
}

pub fn would_wrap_page(grid: &Grid, delta: i32) -> bool {
    match delta.signum() {
        -1 => grid.current_page() == 0,
        1 => grid.current_page() == grid.total_page_count() - 1,
        _ => false,
    }
}

/// Page floor for a Move session: the current real content's pages plus
/// one reserve page.
pub fn move_armed_total_pages(item_count: usize, page_size: usize) -> usize {
    item_count.div_ceil(page_size.max(1)).max(1) + 1
}

/// One row of the "Add item" picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddEntry {
    /// `"<kind>:<id>"`.
    pub id: String,
    pub label_key: String,
}

/// Entries for View > Add item: every known category or action not in
/// the layout. Resume always offers its plain label.
pub fn add_entries(
    available: &[(String, String)],
    live: &Live,
    resolver: &dyn Resolver,
) -> Vec<AddEntry> {
    available
        .iter()
        .filter_map(|(kind, id)| {
            let label_key = match kind.as_str() {
                "action" if id == "resume" => "action:resume".to_string(),
                "category" => resolve_category(live, resolver, id).label_key,
                "action" => resolve_action(live, resolver, id)?.label_key,
                _ => return None,
            };
            Some(AddEntry {
                id: format!("{kind}:{id}"),
                label_key,
            })
        })
        .collect()
}

/// The Hub's vertical composition: the grid band between the header and
/// the help bar, the caption row under it, and the equal gaps that
/// separate them (or the compact 240p footer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub compact_footer: bool,
    pub columns: i32,
    pub rows: i32,
    pub insets: Insets,
    pub fit: Fit,
    pub grid_y: i32,
    pub grid_height: i32,
    pub label_y: i32,
    pub label_height: i32,
    pub vertical_gap: i32,
}

pub fn geometry(inputs: &Inputs, d: &Derived) -> Geometry {
    let compact = d.tier == Tier::T240;
    let insets = Insets {
        left: d.hub_grid_side_inset,
        right: d.hub_grid_side_inset,
        top: d.hub_grid_top_inset,
        bottom: d.hub_grid_bottom_inset,
        column_gap: d.hub_grid_column_gap,
        row_gap: d.hub_grid_row_gap,
    };
    let height = inputs.screen_height as i32;
    let width = inputs.screen_width as i32;
    let mut fit = paged_grid::fit(
        d.hub_grid_columns,
        d.hub_grid_rows,
        width,
        height,
        Some(d.hub_grid_height_budget),
        !compact,
        &insets,
    );
    let grid_height = insets.top
        + insets.bottom
        + d.hub_grid_rows * fit.cell_height
        + (d.hub_grid_rows - 1) * insets.row_gap;
    // HubScreen.qml sizes the grid item to exactly its own content plus
    // insets, so PagedGrid's vertical centering has no slack to hand out:
    // the rows start at the top of the grid and the band's leftover room
    // is spent on `vertical_gap` above and below instead. The screen
    // height above is only the reference for the width fit; centering the
    // block against it would push every row down by half the band.
    fit.available_height = (grid_height - insets.top - insets.bottom).max(0);
    fit.block_offset_y = 0;
    let vertical_band = (height - d.header_bottom - d.help_bar_height).max(0);
    let vertical_gap = ((f64::from(vertical_band - grid_height - d.hub_active_label_height) / 3.0)
        .round() as i32)
        .max(0);
    let grid_y = d.header_bottom
        + if compact {
            inputs.pct_h(1.0)
        } else {
            vertical_gap
        };
    let footer_y = height - d.help_bar_height - d.hub_active_label_height;
    Geometry {
        compact_footer: compact,
        columns: d.hub_grid_columns,
        rows: d.hub_grid_rows,
        insets,
        fit,
        grid_y,
        grid_height,
        label_y: if compact {
            footer_y
        } else {
            grid_y + grid_height + vertical_gap
        },
        label_height: d.hub_active_label_height,
        vertical_gap,
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on an unexpected None"
)]
mod tests {
    use super::*;

    struct Names;
    impl Resolver for Names {
        fn system_name(&self, id: &str) -> String {
            format!("Name of {id}")
        }
        fn system_cover_key(&self, id: &str) -> String {
            format!("systems/{id}")
        }
        fn media_cover_key(&self, system: &str, path: &str) -> String {
            format!("media-image/{system}/{path}")
        }
    }

    fn item(kind: &str, id: &str) -> LayoutItem {
        LayoutItem {
            kind: kind.into(),
            id: id.into(),
            ..LayoutItem::default()
        }
    }

    fn live(confirmed: &[String]) -> Live<'_> {
        Live {
            categories_loaded: true,
            confirmed_categories: confirmed,
            resume_enabled: true,
            resume_name: "Sonic",
            resume_cover_key: "",
            resume_known_unavailable: false,
            update_enabled: true,
            internet_available: false,
        }
    }

    #[test]
    fn seeded_layout_resolves_in_order_and_pads_the_last_page() {
        let confirmed = vec!["Arcade".to_string()];
        let items = vec![
            item("action", "resume"),
            item("category", "Arcade"),
            item("category", "Consoles"),
            LayoutItem {
                kind: "blank".into(),
                ..LayoutItem::default()
            },
            LayoutItem {
                kind: "folder".into(),
                path: "/media/fat/games/SNES/Homebrew/".into(),
                system: "SNES".into(),
                ..LayoutItem::default()
            },
            item("collection", "x"),
            item("action", "update"),
        ];
        let out = entries(&items, false, &live(&confirmed), &Names, 4, 0);
        assert_eq!(out.len(), 8);
        assert_eq!(out[0].label_key, "action:resume");
        assert_eq!(out[0].name, "Sonic");
        assert_eq!(out[0].cover_key, "icons/PlayOutline");
        assert_eq!((out[1].id.as_str(), out[1].disabled), ("Arcade", false));
        assert_eq!(out[1].cover_key, "categories/Arcade");
        assert_eq!(
            (out[2].id.as_str(), out[2].disabled, out[2].reason),
            ("Console", true, Reason::NotAvailable)
        );
        assert!(out[3].is_empty());
        assert_eq!(out[3].hub_index, 3);
        assert_eq!(
            (out[4].kind, out[4].name.as_str(), out[4].system.as_str()),
            (Some(Kind::Folder), "Homebrew", "SNES")
        );
        assert_eq!(out[4].hub_index, 4);
        assert_eq!(
            (out[5].id.as_str(), out[5].disabled, out[5].reason),
            ("update", true, Reason::NoInternet)
        );
        assert_eq!(out[5].hub_index, 6);
        assert!(out[6].is_empty() && out[6].hub_index == -1);
        assert!(out[7].is_empty());
    }

    #[test]
    fn bootstrap_window_seeds_resume_categories_then_actions() {
        let confirmed: Vec<String> = Vec::new();
        let mut l = live(&confirmed);
        l.update_enabled = false;
        let out = entries(&[], true, &l, &Names, 10, 0);
        let keys: Vec<&str> = out.iter().map(|e| e.label_key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "action:resume",
                "category:Arcade",
                "category:Computer",
                "category:Console",
                "category:Handheld",
                "category:Other",
                "action:favorites",
                "action:recents",
                "action:settings",
                ""
            ]
        );
        assert!(out.iter().all(|e| e.hub_index == -1));
        assert!(!out[1].disabled);
    }

    #[test]
    fn resume_and_update_stay_present_but_disabled() {
        let confirmed: Vec<String> = Vec::new();
        let mut l = live(&confirmed);
        l.resume_known_unavailable = true;
        l.resume_cover_key = "media-image/x";
        let out = entries(
            &[item("action", "resume"), item("action", "update")],
            false,
            &l,
            &Names,
            2,
            0,
        );
        assert_eq!(
            (out[0].disabled, out[0].reason),
            (true, Reason::NoRecentGames)
        );
        assert_eq!(out[0].cover_key, "media-image/x");
        assert_eq!((out[1].disabled, out[1].reason), (true, Reason::NoInternet));
        l.update_enabled = false;
        let out = entries(&[item("action", "update")], false, &l, &Names, 2, 0);
        assert!(out.iter().all(Entry::is_empty));
    }

    #[test]
    fn padding_honours_the_page_floor() {
        let list: Vec<Entry> = (0..3)
            .map(|i| Entry {
                hub_index: i,
                ..Entry::default()
            })
            .collect();
        assert_eq!(pad_to_page_size(list.clone(), 4, 0).len(), 4);
        assert_eq!(pad_to_page_size(list.clone(), 4, 3).len(), 12);
        assert_eq!(pad_to_page_size(Vec::new(), 4, 0).len(), 4);
        assert_eq!(pad_to_page_size(list, 0, 0).len(), 3);
    }

    fn board() -> Vec<Entry> {
        let confirmed = vec!["Arcade".to_string(), "Console".to_string()];
        entries(
            &[
                item("action", "resume"),
                item("category", "Arcade"),
                item("category", "Console"),
                item("action", "favorites"),
                item("action", "settings"),
            ],
            false,
            &live(&confirmed),
            &Names,
            4,
            0,
        )
    }

    #[test]
    fn restore_prefers_the_selected_item_then_the_fallbacks() {
        let b = board();
        let cats = vec!["Arcade".to_string(), "Console".to_string()];
        let saved = Saved {
            category: "Arcade",
            selected_row: 1,
            selected_action: "settings",
            selected_item: "category:Console",
        };
        assert_eq!(restore_index(&b, &saved, &cats, true, "settings"), 2);

        let saved = Saved {
            selected_item: "system:Missing",
            category: "Consoles",
            ..Saved::default()
        };
        assert_eq!(restore_index(&b, &saved, &cats, true, "settings"), 2);

        let saved = Saved {
            selected_item: "system:Missing",
            category: "",
            ..Saved::default()
        };
        assert_eq!(restore_index(&b, &saved, &cats, true, "settings"), 0);
        assert_eq!(restore_index(&b, &saved, &cats, false, "settings"), 1);
        assert_eq!(restore_index(&b, &saved, &[], false, "settings"), 4);

        let saved = Saved {
            selected_row: 1,
            selected_action: "favorites",
            ..Saved::default()
        };
        assert_eq!(restore_index(&b, &saved, &cats, true, "settings"), 3);

        let saved = Saved::default();
        assert_eq!(restore_index(&b, &saved, &cats, false, "settings"), 1);
        assert_eq!(restore_index(&b, &saved, &cats, true, "settings"), 0);
    }

    #[test]
    fn commit_records_the_kind_and_the_fallback_fields() {
        let b = board();
        let c = commit(&b[1]).expect("category commits");
        assert_eq!(c.selected_item, "category:Arcade");
        assert_eq!((c.selected_row, c.category.as_deref()), (0, Some("Arcade")));
        let c = commit(&b[3]).expect("action commits");
        assert_eq!(c.selected_item, "action:favorites");
        assert_eq!(
            (c.selected_row, c.selected_action.as_deref()),
            (1, Some("favorites"))
        );
        assert!(commit(&b[7]).is_none());
    }

    #[test]
    fn real_index_maps_padding_past_the_layout_end() {
        let b = board();
        assert_eq!(real_index_for_flat(&b, 2, 5), Some(2));
        assert_eq!(real_index_for_flat(&b, 5, 5), Some(5));
        assert_eq!(real_index_for_flat(&b, 7, 5), Some(7));
        assert_eq!(real_index_for_flat(&b, 8, 5), None);
        assert_eq!(first_pad_flat_index(&b), 5);
        assert_eq!(move_armed_total_pages(5, 4), 3);
        assert_eq!(move_armed_total_pages(0, 4), 2);
    }

    #[test]
    fn move_wrap_guards_read_the_grid_edges() {
        let mut g = Grid::new(4, 2);
        g.set_item_count(16);
        assert!(would_wrap_vertically(&g, -1));
        assert!(!would_wrap_vertically(&g, 1));
        assert!(would_wrap_column(&g, -1));
        assert!(would_wrap_page(&g, -1));
        g.set_current_index_immediate(15);
        assert!(would_wrap_vertically(&g, 1));
        assert!(would_wrap_column(&g, 1));
        assert!(would_wrap_page(&g, 1));
    }

    #[test]
    fn add_entries_resolve_labels_and_keep_resume_plain() {
        let confirmed: Vec<String> = Vec::new();
        let available = vec![
            ("action".to_string(), "resume".to_string()),
            ("category".to_string(), "Handhelds".to_string()),
            ("action".to_string(), "update".to_string()),
            ("blank".to_string(), String::new()),
        ];
        let mut l = live(&confirmed);
        l.update_enabled = false;
        let out = add_entries(&available, &l, &Names);
        assert_eq!(out.len(), 2);
        assert_eq!(
            (out[0].id.as_str(), out[0].label_key.as_str()),
            ("action:resume", "action:resume")
        );
        assert_eq!(
            (out[1].id.as_str(), out[1].label_key.as_str()),
            ("category:Handhelds", "category:Handheld")
        );
    }

    #[test]
    fn folder_names_use_the_last_segment() {
        assert_eq!(
            folder_name_for_path("/media/fat/games/SNES/Homebrew/"),
            "Homebrew"
        );
        assert_eq!(folder_name_for_path("Homebrew"), "Homebrew");
    }

    #[test]
    fn a_layout_id_confirms_against_the_ids_core_sends() {
        // Core sends the canonical singular ids; a layout entry written
        // by an older build may be plural. The plural is canonicalized
        // before the comparison, so both confirm - an unconfirmed
        // category mutes the tile and empties the screen behind it.
        let resolver = Names;
        for (layout_id, core_id) in [
            ("Consoles", "Console"),
            ("Console", "Console"),
            ("Handhelds", "Handheld"),
            ("Arcade", "Arcade"),
        ] {
            let live = Live {
                categories_loaded: true,
                confirmed_categories: &[core_id.to_string()],
                resume_enabled: false,
                resume_name: "",
                resume_cover_key: "",
                resume_known_unavailable: false,
                update_enabled: false,
                internet_available: true,
            };
            let entry = resolve_category(&live, &resolver, layout_id);
            assert!(
                !entry.disabled,
                "{layout_id} against {core_id} read as unavailable"
            );
        }
        // A category Core really does not list still reads unavailable.
        let live = Live {
            categories_loaded: true,
            confirmed_categories: &["Console".to_string()],
            resume_enabled: false,
            resume_name: "",
            resume_cover_key: "",
            resume_known_unavailable: false,
            update_enabled: false,
            internet_available: true,
        };
        assert!(resolve_category(&live, &resolver, "Arcade").disabled);
    }

    #[test]
    fn the_grid_block_starts_at_the_top_of_its_own_band() {
        // HubScreen.qml sizes the grid item to its content plus insets,
        // so PagedGrid has no slack to center within: row 0 sits at the
        // top of the grid, and the band's spare room becomes the gaps
        // above the grid and around the label. Centering the block
        // against the screen instead drops every row half a band down.
        for (w, h) in [(1280.0, 720.0), (1920.0, 1080.0), (960.0, 540.0)] {
            let inputs = Inputs {
                screen_width: w,
                screen_height: h,
                ..Inputs::default()
            };
            let d = crate::sizing::derive(&inputs);
            let g = geometry(&inputs, &d);
            assert_eq!(g.fit.block_offset_y, 0, "{w}x{h}");
            let first_row = paged_grid::cell_rect(&g.fit, &g.insets, 0, 0);
            assert_eq!(first_row.y, g.insets.top, "{w}x{h}");
            let last_row_bottom =
                paged_grid::cell_rect(&g.fit, &g.insets, g.rows - 1, 0).y + g.fit.cell_height;
            assert!(
                last_row_bottom + g.insets.bottom <= g.grid_height,
                "{w}x{h}: rows overflow the grid item"
            );
        }
    }

    #[test]
    fn geometry_matches_the_sizing_tile_size() {
        let inputs = Inputs {
            screen_width: 1280.0,
            screen_height: 720.0,
            ..Inputs::default()
        };
        let d = crate::sizing::derive(&inputs);
        let g = geometry(&inputs, &d);
        assert_eq!(g.fit.cell_width, d.hub_tile_width);
        assert_eq!(g.fit.cell_height, d.hub_tile_height);
        assert_eq!(g.columns * g.rows, d.hub_grid_columns * d.hub_grid_rows);
        assert!(g.grid_y >= d.header_bottom);
        assert!(g.label_y + g.label_height <= 720 - d.help_bar_height + 1);
        assert!(!g.compact_footer);

        let crt = Inputs {
            screen_width: 320.0,
            screen_height: 216.0,
            crt_native_path: true,
            bitmap_type: true,
            ..Inputs::default()
        };
        let d = crate::sizing::derive(&crt);
        let g = geometry(&crt, &d);
        assert!(g.compact_footer);
        assert_eq!(g.fit.cell_width, d.hub_tile_width);
        assert_eq!(g.fit.cell_height, d.hub_tile_height);
    }
}
