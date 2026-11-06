// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The games-style screen driver (Games, Favorites, Recently played,
// Search results):
// fills the rows from Core, drives the cursor through `paged_grid`, runs
// the page swoop, the folder stack, the selection persist debounce, the
// focused-detail debounce and the Options menu, and paints the current
// page into the `GamesView` global. The rules live in
// `zaparoo_app::media_list`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::runtime::Handle;
use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::media_list::{
    self as rules, CoverSettle, CoverSettleStep, CoverState, DetailStep, EntryType, FocusedDetail,
    LinearMove, SelectionPersist, State,
};
use zaparoo_app::options_menu as menu;
use zaparoo_app::paged_grid::{self, Grid, Insets};
use zaparoo_app::user_flags::{Flag, Flags};
use zaparoo_core::collections::DecksParams;
use zaparoo_core::endpoints::decks::{DeckEndpoint, DecksEndpoint};
use zaparoo_core::endpoints::media_browse::{BrowseArgs, MediaBrowseEndpoint};
use zaparoo_core::endpoints::media_history::{HistoryArgs, MediaHistoryEndpoint};
use zaparoo_core::endpoints::media_history_top::{MediaHistoryTopEndpoint, TopArgs};
use zaparoo_core::endpoints::media_search::MediaSearchEndpoint;
use zaparoo_core::endpoints::media_user_list::{MediaUserListEndpoint, UserListArgs};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    merged_root_view, BrowseEntry, MediaBrowseParams, MediaHistoryEntry, MediaHistoryParams,
    MediaHistoryTopEntry, MediaItem, MediaMeta, MediaSearchParams, MediaSearchResult,
    MediaTagsUpdateParams, MediaTagsUpdateResult, SystemInfo, TagInfo,
};
use zaparoo_core::persist::RecentsState;
use zaparoo_core::remote_resource::ResourceStatus;
use zaparoo_core::store::{Endpoint, Tag};

use crate::media_cache::MediaKey;
use crate::navigation::EntryMode;
use crate::router::{lock, Ctx, Shared};
use crate::{App, GamesInput, GamesView, GridCell};

const HIDDEN_TAG: &str = "user:hidden";
const FILE_GLYPH: &str = "icons/File";
const FOLDER_GLYPH: &str = "icons/Folder";
const DECK_GLYPH: &str = "icons/BookMarked";
const ARCADE_SYSTEM_ID: &str = "Arcade";

/// Which list the screen shows; the mode picks the fill source, the
/// copy, the persisted selection slot and the back route.
pub use crate::GamesMode;

impl GamesMode {
    pub fn screen(self) -> crate::Screen {
        match self {
            Self::Browse => crate::Screen::Games,
            Self::Favorites => crate::Screen::Favorites,
            Self::Recents => crate::Screen::Recents,
            Self::Search => crate::Screen::SearchResults,
            Self::Deck => crate::Screen::Deck,
            Self::Decks => crate::Screen::Decks,
            Self::TopPlayed => crate::Screen::TopPlayed,
        }
    }
}

/// One row as the screen and its menus see it.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Core row capabilities and user preference flags are independent"
)]
pub struct GameRow {
    pub media_id: Option<i64>,
    /// Core's cleaned title.
    pub name: String,
    pub path: String,
    pub entry_type: EntryType,
    pub file_count: u32,
    pub system_id: String,
    /// Core's system name (the flat lists' tile top label).
    pub system_name: String,
    pub zap_script: String,
    /// Core's launcher-relative path, empty when Core derived none.
    pub relative_path: String,
    /// Compact disambiguation token labels (pre sibling diff).
    pub tag_labels: Vec<String>,
    /// `false` only when Core confirmed no cover exists.
    pub has_cover: bool,
    /// Core's average cover colour, painted in the art slot until the
    /// art itself lands.
    pub cover_color: Option<[u8; 3]>,
    /// Its Favorite, Like, Dislike and Play later flags.
    pub flags: Flags,
    pub is_hidden: bool,
    /// Core reported the row's tags, so `flags` and `is_hidden` are its
    /// real state. False for history Core could no longer resolve.
    pub tags_known: bool,
    pub media_capable: bool,
    /// A folder of one game's discs: Accept launches the disc Core chose
    /// and the menu lists the rest.
    pub multi_disc: bool,
    /// The title a grouped search result stands for, which its versions
    /// page asks Core for.
    pub title_id: Option<i64>,
    /// How many versions of the title the search matched. Zero outside a
    /// grouped search.
    pub version_count: u32,
    /// The roots page distinguisher, when siblings share a name.
    pub root_distinguisher: String,
    /// The metadata rows Core sent with the row (the detail pane's
    /// immediate peek before media.meta answers).
    pub detail_rows: Vec<(&'static str, String)>,
    /// Display fields, recomputed when the naming settings change.
    pub display: String,
    pub suffix: String,
    /// Set on the Deck screen: which of the deck's items the row is.
    pub deck_item: Option<DeckItemRow>,
    /// Set on the Decks screen: the deck the row is.
    pub deck_entry: Option<DeckEntryRow>,
    /// Set on Top played: the game's total play time, in seconds.
    pub play_seconds: Option<u64>,
}

/// A row that is one deck on the Decks screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckEntryRow {
    pub deck_id: String,
    /// Who a saved deck is from; empty for the user's own.
    pub owner: String,
    /// The user cannot change it: a saved copy or a locked deck.
    pub locked: bool,
    pub item_count: u32,
    /// The deck's `updatedAt`, which its cover is asked for by.
    pub revision: i64,
}

/// A row that is one item of the deck on the Deck screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckItemRow {
    pub id: i64,
    /// 1-based, what `start` names when the deck plays from here.
    pub position: u32,
    pub kind: zaparoo_core::collections::DeckItemKind,
    /// A game item whose file this device has: it launches, has art and
    /// can be the deck's cover.
    pub game: bool,
    /// A card item with art Core serves through `decks.image`.
    pub card_art: bool,
    /// The deck and its revision, which a card's art is asked for by.
    pub deck_id: String,
    pub revision: i64,
}

fn tag_pairs(tags: &[TagInfo]) -> Vec<(String, String)> {
    tags.iter()
        .map(|tag| {
            (
                tag.tag_type.clone(),
                crate::tag_utils::tag_display_value(tag),
            )
        })
        .collect()
}

impl GameRow {
    /// Accept browses into this row.
    pub fn is_dir(&self) -> bool {
        rules::browses(self.entry_type, self.media_id.is_some(), &self.zap_script)
    }

    /// How many versions the row stands for, for the info line: zero
    /// unless it stands for more than one.
    pub(crate) fn versions(&self) -> i32 {
        if zaparoo_app::title_versions::has_versions(self.title_id, self.version_count) {
            i32::try_from(self.version_count).unwrap_or(i32::MAX)
        } else {
            0
        }
    }

    /// What the row's selection is persisted and restored by: its path, or
    /// on the Deck screen the item, since one game can be in a deck twice.
    pub fn selection_key(&self) -> String {
        if let Some(deck) = &self.deck_entry {
            return deck.deck_id.clone();
        }
        self.deck_item
            .as_ref()
            .map_or_else(|| self.path.clone(), |item| format!("#{}", item.id))
    }

    /// The run text (`run_text_for_entry`): a media-capable directory
    /// launches by its zapscript, everything else by path then script.
    pub fn launch_text(&self) -> Option<String> {
        let non_empty = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
        if self.entry_type == EntryType::Directory && self.media_capable {
            return non_empty(&self.zap_script).or_else(|| non_empty(&self.path));
        }
        non_empty(&self.path).or_else(|| non_empty(&self.zap_script))
    }

    /// The run text for a row of a live folder: Core's `zapScript`
    /// first, which it quotes when the file name needs it, so a name with
    /// `ZapScript` syntax in it still launches that file. Elsewhere, as
    /// `launch_text`.
    pub fn launch_text_from(&self, live: bool) -> Option<String> {
        if live && !self.zap_script.trim().is_empty() {
            return Some(self.zap_script.clone());
        }
        self.launch_text()
    }

    fn identity(&self, fallback_system: &str) -> String {
        if !self.media_capable {
            return String::new();
        }
        let system = self.system_or(fallback_system);
        if system.is_empty() || self.path.is_empty() {
            return String::new();
        }
        // A multi-disc folder's details are its launch disc's.
        match self
            .media_id
            .filter(|_| self.entry_type == EntryType::Directory)
        {
            Some(disc) => format!("{system}\n{}\n{disc}", self.path),
            None => format!("{system}\n{}", self.path),
        }
    }

    /// The `media.meta` request for the detail pane: a game by its id, a
    /// multi-disc folder by its path with the disc it launches.
    fn meta_params(&self, system: String) -> zaparoo_core::media_types::MediaMetaParams {
        use zaparoo_core::media_types::MediaMetaParams;
        match self.media_id {
            Some(disc) if self.entry_type == EntryType::Directory => {
                MediaMetaParams::for_folder_disc(disc, system, self.path.clone())
            }
            _ => MediaMetaParams::for_row(self.media_id, system, self.path.clone()),
        }
    }

    /// Core has artwork to ask for: a game's, a folder's own, or a deck
    /// card's. A system root has none.
    fn wants_cover(&self) -> bool {
        let card_art = self.deck_item.as_ref().is_some_and(|item| item.card_art);
        self.has_cover
            && (self.media_capable
                || self.entry_type == EntryType::Directory
                || card_art
                || self.deck_entry.is_some())
    }

    fn system_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.system_id.is_empty() {
            fallback
        } else {
            &self.system_id
        }
    }
}

/// Whether a browse of the list asks Core for hidden entries too.
pub(crate) fn include_hidden(shared: &Shared) -> bool {
    shared.show_hidden
}

fn cover_color(value: Option<&str>) -> Option<[u8; 3]> {
    value.and_then(zaparoo_app::covers::parse_cover_color)
}

fn has_user_tag(tags: &[TagInfo], name: &str) -> bool {
    tags.iter()
        .any(|tag| tag.tag_type == "user" && tag.tag == name)
}

/// A game's collections, from its `user` tags.
pub(crate) fn user_flags(tags: &[TagInfo]) -> Flags {
    Flags::from_tag_values(
        tags.iter()
            .filter(|tag| tag.tag_type == "user")
            .map(|tag| tag.tag.as_str()),
    )
}

impl From<&BrowseEntry> for GameRow {
    fn from(e: &BrowseEntry) -> Self {
        let entry_type = EntryType::parse(&e.entry_type);
        Self {
            media_id: e.media_id,
            name: e.name.clone(),
            path: e.path.clone(),
            entry_type,
            file_count: e.file_count,
            system_id: e.system_id.clone(),
            system_name: String::new(),
            zap_script: e.zap_script.clone(),
            relative_path: e.relative_path.clone(),
            tag_labels: if e.multi_disc {
                crate::tag_utils::multi_disc_tag_labels(&e.tags, &e.disambiguating_tags)
            } else {
                crate::tag_utils::disambiguating_tag_labels(&e.disambiguating_tags)
            },
            has_cover: e.has_cover,
            cover_color: cover_color(e.cover_color.as_deref()),
            flags: user_flags(&e.tags),
            is_hidden: has_user_tag(&e.tags, "hidden"),
            tags_known: true,
            media_capable: rules::is_media_capable(entry_type, e.media_id.is_some(), &e.zap_script),
            multi_disc: e.multi_disc,
            title_id: None,
            version_count: 0,
            root_distinguisher: String::new(),
            detail_rows: rules::detail_rows_from_tags(&tag_pairs(&e.tags)),
            display: String::new(),
            suffix: String::new(),
            deck_item: None,
            deck_entry: None,
            play_seconds: None,
        }
    }
}

impl From<&MediaItem> for GameRow {
    fn from(item: &MediaItem) -> Self {
        Self {
            media_id: item.media_id,
            name: item.name.clone(),
            path: item.path.clone(),
            entry_type: EntryType::Media,
            file_count: 0,
            system_id: item.system.id.clone(),
            system_name: item.system.name.clone(),
            zap_script: item.zap_script.clone(),
            relative_path: item.relative_path.clone().unwrap_or_default(),
            tag_labels: crate::tag_utils::disambiguating_tag_labels(&item.disambiguating_tags),
            has_cover: item.has_cover,
            cover_color: cover_color(item.cover_color.as_deref()),
            flags: user_flags(&item.tags),
            is_hidden: has_user_tag(&item.tags, "hidden"),
            tags_known: true,
            media_capable: true,
            multi_disc: false,
            title_id: item.title_id,
            version_count: item.version_count.unwrap_or(0),
            root_distinguisher: String::new(),
            detail_rows: rules::detail_rows_from_tags(&tag_pairs(&item.tags)),
            display: String::new(),
            suffix: String::new(),
            deck_item: None,
            deck_entry: None,
            play_seconds: None,
        }
    }
}

impl GameRow {
    /// A game known only by where it is (a Hub tile, the Resume tile),
    /// filled from what Core's `media.meta` says about it now.
    pub(crate) fn from_meta(system: &str, name: &str, meta: &MediaMeta) -> Self {
        Self {
            media_id: None,
            name: name.to_string(),
            path: meta.path.clone(),
            entry_type: EntryType::Media,
            file_count: 0,
            system_id: system.to_string(),
            system_name: String::new(),
            zap_script: meta.zap_script.clone(),
            relative_path: meta.relative_path.clone().unwrap_or_default(),
            tag_labels: Vec::new(),
            has_cover: true,
            cover_color: None,
            flags: user_flags(&meta.tags),
            is_hidden: has_user_tag(&meta.tags, "hidden"),
            tags_known: true,
            media_capable: true,
            // `media.meta` does not say whether a game spans several discs.
            multi_disc: false,
            title_id: None,
            version_count: 0,
            root_distinguisher: String::new(),
            detail_rows: Vec::new(),
            display: name.to_string(),
            suffix: String::new(),
            deck_item: None,
            deck_entry: None,
            play_seconds: None,
        }
    }

    /// A script that is not one game: only its text is known, which is
    /// all writing it to a token needs.
    pub(crate) fn from_script(name: &str, script: &str) -> Self {
        Self {
            zap_script: script.to_string(),
            media_capable: false,
            tags_known: false,
            ..Self::from_meta("", name, &MediaMeta::default())
        }
    }
}

impl GameRow {
    /// One deck on the Decks screen: its name, its cover when it has one.
    pub(crate) fn from_deck(deck: &zaparoo_core::collections::Deck) -> Self {
        let mut row = Self::from_script(&deck.name, "");
        row.has_cover = deck.has_image;
        row.display.clone_from(&deck.name);
        row.deck_entry = Some(DeckEntryRow {
            deck_id: deck.deck_id.clone(),
            owner: if deck.saved {
                deck.owned_by.clone()
            } else {
                String::new()
            },
            locked: !deck.editable(),
            item_count: deck.item_count,
            revision: deck.updated_at,
        });
        row
    }
}

impl From<&MediaHistoryTopEntry> for GameRow {
    fn from(e: &MediaHistoryTopEntry) -> Self {
        Self {
            media_id: e.media_id,
            name: e.media_name.clone(),
            path: e.media_path.clone(),
            entry_type: EntryType::Media,
            file_count: 0,
            system_id: e.system_id.clone(),
            system_name: e.system_name.clone(),
            zap_script: e.zap_script.clone(),
            relative_path: e.relative_path.clone().unwrap_or_default(),
            tag_labels: Vec::new(),
            has_cover: true,
            cover_color: None,
            flags: user_flags(&e.tags),
            is_hidden: has_user_tag(&e.tags, "hidden"),
            // Core sends tags only for a game it resolved to indexed
            // media, which is also when it sends the media id.
            tags_known: e.media_id.is_some(),
            media_capable: true,
            multi_disc: false,
            root_distinguisher: String::new(),
            detail_rows: Vec::new(),
            display: String::new(),
            suffix: String::new(),
            deck_item: None,
            deck_entry: None,
            play_seconds: Some(e.total_play_time),
            title_id: None,
            version_count: 0,
        }
    }
}

impl From<&MediaHistoryEntry> for GameRow {
    fn from(e: &MediaHistoryEntry) -> Self {
        Self {
            media_id: e.media_id,
            name: e.media_name.clone(),
            path: e.media_path.clone(),
            entry_type: EntryType::Media,
            file_count: 0,
            system_id: e.system_id.clone(),
            system_name: e.system_name.clone(),
            zap_script: e.zap_script.clone(),
            relative_path: e.relative_path.clone().unwrap_or_default(),
            tag_labels: Vec::new(),
            has_cover: e.has_cover,
            cover_color: cover_color(e.cover_color.as_deref()),
            flags: user_flags(&e.tags),
            is_hidden: has_user_tag(&e.tags, "hidden"),
            // Core sends tags only for history it resolved to indexed
            // media, which is also when it sends the media id.
            tags_known: e.media_id.is_some(),
            media_capable: true,
            multi_disc: false,
            title_id: None,
            version_count: 0,
            root_distinguisher: String::new(),
            detail_rows: Vec::new(),
            display: String::new(),
            suffix: String::new(),
            deck_item: None,
            deck_entry: None,
            play_seconds: None,
        }
    }
}

/// The screen's model: rows, cursor, fetch state and the transient cues.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per MediaListScreen property the shell binds"
)]
pub struct GamesModel {
    pub mode: GamesMode,
    pub rows: Vec<GameRow>,
    /// The Show tags setting: off, a row's tags never reach its suffix.
    pub show_tags: bool,
    /// Left and Right step the detail pane's images instead of turning
    /// the page (the "Left and Right in lists" setting).
    pub list_images: bool,
    /// The detail pane's image: the row's identity and its index in the
    /// row's image order. Another row starts again at its cover.
    pub detail_image: (String, usize),
    pub grid: Grid,
    pub next_cursor: Option<String>,
    /// The initial fill is in flight (the in-screen loading cue).
    pub loading: bool,
    pub loading_more: bool,
    /// The header cue for the follow-up fetch in flight (`sync_page_wait`).
    pub page_wait: Option<crate::cue::Wait>,
    /// A page just landed and the next fetch of the same walk follows in
    /// this turn: the header cue must not end and restart between them.
    pub fetch_follows: bool,
    /// The header cue for a reload in place that keeps its rows on screen.
    pub refill_wait: Option<crate::cue::Wait>,
    /// A page arrived with no cursor to continue from: the loaded rows are
    /// the whole list, and `known_total` reports them over Core's count.
    pub tail_reached: bool,
    /// A failed page already restarted this fill, and no page has loaded
    /// since: a second failure is shown instead of restarting again.
    pub page_restarted: bool,
    pub error: String,
    /// Core's browse totals (Games only; the flat lists have none).
    pub total_files: u32,
    pub total_dirs: u32,
    pub total_known: bool,
    pub system_id: String,
    pub system_name: String,
    /// Browse reads its folders straight from disk (`media.browse` `live`),
    /// with no system: a live folder tile's. Mirrors `GamesState::live`.
    pub live: bool,
    /// A list's scope: the system its systems screen drilled into, empty
    /// for the whole list.
    pub list_system: String,
    /// The browse path behind the rows: every follow-up fetch repeats it.
    pub browse_path: String,
    /// A fill applies only while its ticket is current.
    pub ticket: u64,
    pub fill_task: crate::scoped_task::ScopedTask,
    pub focus_armed: bool,
    pub restore_done: bool,
    /// A saved selection deeper than the loaded rows: keep fetching
    /// until it lands, unless the user moves first.
    pub pending_restore_path: String,
    pub persist: SelectionPersist,
    pub persist_seq: u64,
    pub detail: FocusedDetail,
    pub detail_seq: u64,
    /// The fast scroll and its rail.
    pub rapid: crate::browse_motion::Rapid,
    /// Holds the detailed list's artwork demand until the selection rests.
    pub cover_settle: CoverSettle,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// The page turn in flight (`page_motion`); input waits for it.
    pub page: crate::page_motion::PageMotion,
    pub folder_direction: i32,
    pub folder_sliding: bool,
    /// A jump walk is loading its target pages.
    pub jump_loading: bool,
    pub press_seq: u64,
    /// Bulk appends pause cover fetches until they land.
    pub covers_paused: bool,
    /// The fill in flight restores a saved folder; if that folder is
    /// gone, the system root replaces it.
    pub focus_recalled: bool,
    /// The deck on the Deck screen, as Core last answered.
    pub deck: Option<zaparoo_core::collections::Deck>,
    /// An item picked up by Move: the order to put back on Back.
    pub moving: Option<Vec<GameRow>>,
}

impl GamesModel {
    pub fn new() -> Self {
        Self {
            mode: GamesMode::Browse,
            rows: Vec::new(),
            show_tags: true,
            list_images: false,
            detail_image: (String::new(), 0),
            grid: Grid::new(4, 3),
            next_cursor: None,
            loading: false,
            loading_more: false,
            page_wait: None,
            fetch_follows: false,
            refill_wait: None,
            tail_reached: false,
            page_restarted: false,
            error: String::new(),
            total_files: 0,
            total_dirs: 0,
            total_known: true,
            system_id: String::new(),
            system_name: String::new(),
            live: false,
            list_system: String::new(),
            browse_path: String::new(),
            ticket: 0,
            fill_task: crate::scoped_task::ScopedTask::default(),
            focus_armed: false,
            restore_done: false,
            pending_restore_path: String::new(),
            persist: SelectionPersist::new(),
            persist_seq: 0,
            detail: FocusedDetail::new(),
            detail_seq: 0,
            rapid: crate::browse_motion::Rapid::default(),
            cover_settle: CoverSettle::default(),
            activate_pulse: 0,
            release_pulse: 0,
            page: crate::page_motion::PageMotion::default(),
            folder_direction: 0,
            folder_sliding: false,
            jump_loading: false,
            press_seq: 0,
            covers_paused: false,
            focus_recalled: false,
            deck: None,
            moving: None,
        }
    }

    pub fn current(&self) -> Option<&GameRow> {
        self.rows.get(self.grid.current_index())
    }

    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    /// Root rows Core places before its counted directories. `totalDirs`
    /// excludes these virtual entries, including while only part of the
    /// directory prefix has loaded.
    fn root_count(&self) -> usize {
        self.rows
            .iter()
            .take_while(|row| row.entry_type != EntryType::Media)
            .filter(|row| row.entry_type == EntryType::Root)
            .count()
    }

    fn non_media_total(&self) -> usize {
        self.total_dirs as usize + self.root_count()
    }

    fn known_total(&self) -> Option<usize> {
        self.total_known.then(|| {
            if self.tail_reached {
                self.rows.len()
            } else {
                self.total_files as usize + self.non_media_total()
            }
        })
    }

    fn state(&self) -> State {
        rules::screen_state(self.loading, false, !self.error.is_empty(), self.rows.len())
    }

    /// Naming/language changes and replacement fills reproject the full model.
    fn refresh_display(&mut self, show_original_filenames: bool, language: &str) {
        self.refresh_display_from(0, show_original_filenames, language);
    }

    /// Ordinary appends touch new rows and only the old sibling run they join.
    /// Recompute that run from original tokens, not its already-trimmed suffix.
    fn refresh_display_from(
        &mut self,
        appended_from: usize,
        show_original: bool,
        language: &str,
    ) -> usize {
        for row in self.rows.iter_mut().skip(appended_from) {
            row.display = rules::display_name(
                &row.name,
                &row.path,
                row.entry_type.is_folder(),
                show_original,
            );
        }
        let start = rules::sibling_group_start(&self.rows, appended_from, |row| &row.display);
        let names: Vec<_> = self.rows[start..]
            .iter()
            .map(|row| (row.display.clone(), row.tag_labels.clone()))
            .collect();
        let displays = crate::tag_utils::sibling_disambiguation_displays(&names);
        let count = |n: u32| zaparoo_app::format::count(i64::from(n), language);
        for (row, tags) in self.rows[start..].iter_mut().zip(displays) {
            // Two same-named roots keep what tells them apart: that is not
            // a tag, and hiding it would leave two identical rows.
            let tags = if !row.root_distinguisher.is_empty() {
                row.root_distinguisher.clone()
            } else if self.show_tags {
                tags
            } else {
                String::default()
            };
            row.suffix = rules::row_suffix(
                row.entry_type,
                &tags,
                rules::effective_file_count(row.media_capable, row.file_count),
                &count,
            );
        }
        self.rows.len() - start
    }
}

impl Default for GamesModel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Shared state helpers ----------

fn list_layout(shared: &Shared) -> bool {
    matches!(
        shared.persist.settings.games_browse_layout.as_str(),
        "list" | "list-only"
    )
}

fn list_only_layout(shared: &Shared) -> bool {
    shared.persist.settings.games_browse_layout == "list-only"
}

pub(crate) fn rotated(shared: &Shared) -> bool {
    matches!(shared.persist.settings.orientation.as_str(), "cw" | "ccw")
}

/// The row count the list's profile is fitted to where its row height
/// follows the card.
fn list_target_rows(ctx: &Ctx, shared: &Shared) -> usize {
    rules::list_visible_rows(ctx.crt_enabled, rotated(shared))
}

/// The list's page size: the rows the painted card holds. One answer for
/// the row window, paging and the fetch size, from the same geometry the
/// card is laid out with.
fn list_rows_visible(ctx: &Ctx, app: &App, shared: &Shared) -> usize {
    list_geometry(ctx, app, shared).visible_rows.max(1)
}

/// The fetch page size: the grid page, or a screenful of list rows.
fn page_size(ctx: &Ctx, app: &App, shared: &Shared) -> u32 {
    if list_layout(shared) {
        return u32::try_from(list_rows_visible(ctx, app, shared)).unwrap_or(10);
    }
    let view = app.global::<GamesView>();
    (view.get_columns().max(1) * view.get_rows().max(1)) as u32
}

/// The list card's geometry from the games list profile (the TATE table
/// on a rotated scene), with the screen's target row count.
fn list_geometry(ctx: &Ctx, app: &App, shared: &Shared) -> rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    list_geometry_for(
        &inputs,
        rotated(shared),
        crate::page_bar::games_row_block(shared.games.mode, &derived),
        list_target_rows(ctx, shared),
    )
}

/// The games list profile's card for `inputs`, below a Menu chip band of
/// `row_block` (zero for a screen without chips). The file browser's list
/// shares it.
pub(crate) fn list_geometry_for(
    inputs: &zaparoo_app::sizing::Inputs,
    rotated: bool,
    row_block: i32,
    target_rows: usize,
) -> rules::ListGeometry {
    rules::scene_list_geometry(inputs, rotated, row_block, target_rows)
}

/// The Core sort for the list on screen.
fn list_sort(shared: &Shared) -> Option<String> {
    crate::user_lists::core_sort(shared)
}

/// The list's system scope, as Core's argument list.
fn list_scope(shared: &Shared) -> Vec<String> {
    let system = shared.games.list_system.trim();
    if system.is_empty() {
        Vec::new()
    } else {
        vec![system.to_string()]
    }
}

/// The persisted selection for the active mode and folder level.
fn saved_path(shared: &Shared) -> String {
    match shared.games.mode {
        GamesMode::Browse => shared
            .persist
            .games
            .selected_at_level
            .last()
            .cloned()
            .unwrap_or_default(),
        GamesMode::Favorites => {
            crate::user_lists::games_focus(&shared.persist, crate::user_lists::active(shared)).0
        }
        GamesMode::Recents => shared.persist.recents.selected_path.clone(),
        GamesMode::Search => shared.persist.search.selected_path.clone(),
        GamesMode::Deck => shared.persist.deck.selected_item.clone(),
        GamesMode::Decks => shared.persist.decks.selected_deck.clone(),
        GamesMode::TopPlayed => shared.persist.top_played.selected_path.clone(),
    }
}

fn saved_list_top(shared: &Shared) -> Option<usize> {
    match shared.games.mode {
        GamesMode::Browse => shared
            .persist
            .games
            .list_top_at_level
            .get(shared.persist.games.path_stack.len().saturating_sub(1))
            .copied(),
        GamesMode::Favorites => {
            crate::user_lists::games_focus(&shared.persist, crate::user_lists::active(shared)).1
        }
        GamesMode::Recents => shared.persist.recents.list_top,
        GamesMode::Search => shared.persist.search.list_top,
        GamesMode::Deck => shared.persist.deck.list_top,
        GamesMode::Decks => shared.persist.decks.list_top,
        GamesMode::TopPlayed => shared.persist.top_played.list_top,
    }
}

fn list_restore_needs_rows(ctx: &Ctx, app: &App, shared: &Shared) -> bool {
    if !list_layout(shared) || !shared.games.has_more() {
        return false;
    }
    let visible = list_rows_visible(ctx, app, shared).max(1);
    let selected = shared.games.grid.current_index();
    let top = saved_list_top(shared)
        .unwrap_or_else(|| selected.saturating_sub(visible / 2))
        .min(selected)
        .max(selected.saturating_sub(visible - 1));
    shared.games.rows.len() < top.saturating_add(visible)
}

fn remember_list_top(shared: &mut Shared, top: usize) {
    match shared.games.mode {
        GamesMode::Browse => {
            let levels = shared.persist.games.path_stack.len();
            shared.persist.games.list_top_at_level.resize(levels, 0);
            if let Some(slot) = shared.persist.games.list_top_at_level.last_mut() {
                *slot = top;
            }
        }
        GamesMode::Favorites => {
            let list = crate::user_lists::active(shared);
            crate::user_lists::set_games_top(&mut shared.persist, list, Some(top));
        }
        GamesMode::Recents => shared.persist.recents.list_top = Some(top),
        GamesMode::Search => shared.persist.search.list_top = Some(top),
        GamesMode::Deck => shared.persist.deck.list_top = Some(top),
        GamesMode::Decks => shared.persist.decks.list_top = Some(top),
        GamesMode::TopPlayed => shared.persist.top_played.list_top = Some(top),
    }
}

fn write_saved_path(shared: &mut Shared, path: String) {
    match shared.games.mode {
        GamesMode::Browse => {
            if let Some(top) = shared.persist.games.selected_at_level.last_mut() {
                *top = path;
            }
        }
        GamesMode::Favorites => {
            let list = crate::user_lists::active(shared);
            crate::user_lists::set_games_path(&mut shared.persist, list, path);
        }
        GamesMode::Recents => shared.persist.recents.selected_path = path,
        GamesMode::Search => shared.persist.search.selected_path = path,
        GamesMode::Deck => shared.persist.deck.selected_item = path,
        GamesMode::Decks => shared.persist.decks.selected_deck = path,
        GamesMode::TopPlayed => shared.persist.top_played.selected_path = path,
    }
}

fn refresh_display(shared: &mut Shared) {
    shared.games.show_tags = shared.persist.settings.show_tags;
    shared.games.list_images = shared.persist.settings.list_left_right == "image";
    let show_original = shared.persist.settings.show_original_filenames;
    let language = crate::effective_language(&shared.persist.settings.language);
    shared.games.refresh_display(show_original, &language);
}

fn detail_ctx() -> Option<(Arc<zaparoo_core::client::Client>, Handle)> {
    DETAIL_CTX.get().cloned()
}

/// Client and runtime handle for the detail pane's media.meta fetch.
static DETAIL_CTX: OnceLock<(Arc<zaparoo_core::client::Client>, Handle)> = OnceLock::new();

/// The detail pane's rows from Core (`rules::DetailCache`).
static DETAIL_CACHE: std::sync::Mutex<rules::DetailCache> =
    std::sync::Mutex::new(rules::DetailCache::new(rules::DETAIL_CACHE_BYTES));

/// Core's metadata may have changed: ask again as each row is shown.
pub fn clear_detail_cache() {
    detail_cache().clear();
}

fn detail_cache() -> std::sync::MutexGuard<'static, rules::DetailCache> {
    DETAIL_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn seed_detail_ctx(client: Arc<zaparoo_core::client::Client>, handle: Handle) {
    let _ = DETAIL_CTX.set((client, handle));
}

// ---------- Entry points ----------

/// Systems Accept (and the Hub's system shortcut): a launch-only system
/// runs its script; every fresh browse starts at the system root. Only
/// cold-start restoration reuses the saved folder, game and viewport.
pub fn enter(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    if !sys.zap_script.is_empty() {
        crate::router::launch(ctx, app, sys.zap_script.clone(), &sys.name);
        return;
    }
    crate::perf::open_pressed("system");
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let games = &mut shared.persist.games;
        games.system_id.clone_from(&sys.id);
        games.path_stack = vec![String::new()];
        games.selected_at_level = vec![String::new()];
        games.list_top_at_level.clear();
        begin_browse_mode(&mut shared, sys);
    }
    browse(ctx, app, "", true);
}

/// A recalled browse position whose folder is gone (Core answered it
/// empty): forget it and browse the system root instead. True when the
/// fallback took over the fill.
fn fall_back_from_recalled_focus(ctx: &Ctx, app: &App, flip: bool) -> bool {
    let (root, direction) = {
        let mut shared = lock(&ctx.shared);
        if !std::mem::take(&mut shared.games.focus_recalled) {
            return false;
        }
        let system_id = shared.games.system_id.clone();
        shared.persist.games.forget_system_focus(&system_id);
        (
            shared
                .persist
                .games
                .path_stack
                .last()
                .cloned()
                .unwrap_or_default(),
            shared.games.folder_direction,
        )
    };
    tracing::info!("remembered folder is gone; browsing the system root");
    browse_with_motion(ctx, app, &root, flip, direction);
    true
}

/// Cold-start re-entry: browse the persisted stack's top level so a kill
/// inside a folder resumes inside that folder.
pub fn enter_restored(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    if !sys.zap_script.is_empty() {
        return;
    }
    let top = {
        let mut shared = lock(&ctx.shared);
        begin_browse_mode(&mut shared, sys);
        let top = shared
            .persist
            .games
            .path_stack
            .last()
            .cloned()
            .unwrap_or_default();
        shared.games.focus_recalled = !top.is_empty();
        top
    };
    browse(ctx, app, &top, true);
}

/// A Hub `system` shortcut lands on Games having skipped Systems; Back
/// then returns to the Hub.
pub fn enter_from_hub(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.games.entered_from_hub = true;
        shared.persist.systems.system_id.clone_from(&sys.id);
    }
    enter(ctx, app, sys);
}

/// A Hub `folder` shortcut: establish the system, then browse the folder
/// as one pushed level so Back climbs to the system root.
pub fn enter_folder_from_hub(ctx: &Ctx, app: &App, system_id: &str, path: &str) {
    if system_id.is_empty() || path.is_empty() {
        return;
    }
    let system = lock(&ctx.shared)
        .systems
        .iter()
        .find(|s| s.id == system_id)
        .cloned();
    let Some(system) = system else {
        return;
    };
    crate::perf::open_pressed("folder");
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.games.system_id.clone_from(&system.id);
        shared.persist.games.path_stack = vec![String::new(), path.to_string()];
        shared.persist.games.selected_at_level = vec![String::new(), String::new()];
        shared.persist.games.list_top_at_level.clear();
        shared.persist.games.entered_from_hub = true;
        shared.persist.systems.system_id.clone_from(&system.id);
        begin_browse_mode(&mut shared, &system);
    }
    browse(ctx, app, path, true);
}

/// The browse filter changed: reload the folder on screen from its top.
/// Core filters the games directly inside a folder and leaves its
/// sub-folders alone, so the user stays where they are.
pub(crate) fn refilter(ctx: &Ctx, app: &App) {
    let path = {
        let mut shared = lock(&ctx.shared);
        if shared.games.mode != GamesMode::Browse {
            return;
        }
        write_saved_path(&mut shared, String::new());
        remember_list_top(&mut shared, 0);
        // The letters belong to the list that was filtered. Bumping the
        // ticket retires a fetch still in flight with the old filter.
        shared.letter_seq += 1;
        shared.letter_buckets.clear();
        shared.letter_scope = None;
        shared.games.browse_path.clone()
    };
    browse(ctx, app, &path, false);
}

fn begin_browse_mode(shared: &mut Shared, sys: &SystemInfo) {
    let name = crate::systems::display_name(shared, &sys.id);
    let model = &mut shared.games;
    model.mode = GamesMode::Browse;
    model.system_id.clone_from(&sys.id);
    model.system_name = name;
    model.focus_armed = false;
    model.restore_done = false;
    model.focus_recalled = false;
    set_live(shared, false);
}

/// Whether Browse reads from disk; kept in the model, the persisted state
/// and the chip row's surface together.
fn set_live(shared: &mut Shared, live: bool) {
    shared.games.live = live;
    shared.persist.games.live = live;
    crate::page_bar::set_live_browse(live);
}

/// Browse a live folder: no system, titled by the folder it starts from.
fn begin_live_mode(shared: &mut Shared, root: &str) {
    let model = &mut shared.games;
    model.mode = GamesMode::Browse;
    model.system_id.clear();
    model.system_name = zaparoo_app::hub::folder_name_for_path(root);
    model.focus_armed = false;
    model.restore_done = false;
    model.focus_recalled = false;
    set_live(shared, true);
}

/// A Hub live folder tile: browse `path` straight from disk. The folder is
/// the stack's base, so Back climbs to it and then returns to the Hub.
pub fn enter_live_folder_from_hub(ctx: &Ctx, app: &App, path: &str) {
    if path.is_empty() {
        return;
    }
    crate::perf::open_pressed("folder");
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let games = &mut shared.persist.games;
        games.system_id.clear();
        games.path_stack = vec![path.to_string()];
        games.selected_at_level = vec![String::new()];
        games.list_top_at_level.clear();
        games.entered_from_hub = true;
        begin_live_mode(&mut shared, path);
    }
    browse(ctx, app, path, true);
}

/// Cold-start re-entry into a live folder: the persisted stack's top
/// level, as `enter_restored` does for a system.
pub fn enter_live_restored(ctx: &Ctx, app: &App) {
    let top = {
        let mut shared = lock(&ctx.shared);
        let root = shared
            .persist
            .games
            .path_stack
            .first()
            .cloned()
            .unwrap_or_default();
        if root.is_empty() {
            return;
        }
        begin_live_mode(&mut shared, &root);
        shared
            .persist
            .games
            .path_stack
            .last()
            .cloned()
            .unwrap_or_default()
    };
    browse(ctx, app, &top, true);
}

/// A built-in list (a Hub action): the games with its flag, all of them,
/// or, on a restore of a grouped list, the system it was left in.
pub fn enter_list(ctx: &Ctx, app: &App, list: Flag, entry: EntryMode) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        crate::user_lists::set_active(&mut shared, app, list);
        if entry == EntryMode::Fresh {
            crate::user_lists::reset_games(&mut shared.persist, list);
        }
        shared.games.list_system =
            if entry == EntryMode::Restore && crate::user_lists::grouped(&shared, list) {
                crate::user_lists::systems_focus(&shared.persist, list).0
            } else {
                String::new()
            };
    }
    enter_flat(ctx, app, GamesMode::Favorites, true);
}

/// One system's games in the list on screen, from the list's systems.
pub fn enter_list_for_system(ctx: &Ctx, app: &App, system_id: &str) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let list = crate::user_lists::active(&shared);
        shared.games.list_system = system_id.to_string();
        crate::user_lists::set_systems_path(&mut shared.persist, list, system_id.to_string());
        crate::user_lists::reset_games(&mut shared.persist, list);
    }
    enter_flat(ctx, app, GamesMode::Favorites, true);
}

/// The sort or the grouping changed: refill the list in place.
pub fn refresh_list(ctx: &Ctx, app: &App) {
    enter_flat(ctx, app, GamesMode::Favorites, false);
}

/// A game's collections changed in Core, from here or another client:
/// the rows that are that game take its flags, and so does the Options
/// strip when the open menu is about it.
pub(crate) fn apply_tags_changed(
    ctx: &Ctx,
    app: &App,
    changed: &zaparoo_core::collections::MediaTagsChanged,
) {
    let flags = user_flags(&changed.tags);
    let same = |row: &GameRow, fallback: &str| {
        row.path == changed.path
            && (changed.system.is_empty() || row.system_or(fallback) == changed.system)
    };
    let (repaint, menu_row) = {
        let mut shared = lock(&ctx.shared);
        let fallback = shared.games.system_id.clone();
        let mut repaint = false;
        for row in shared
            .games
            .rows
            .iter_mut()
            .filter(|row| same(row, &fallback))
        {
            repaint |= row.flags != flags;
            row.flags = flags;
        }
        let menu_row = match shared.context_owner {
            crate::router::ContextOwner::Games => shared
                .games
                .rows
                .get(shared.context_target)
                .is_some_and(|row| same(row, &fallback)),
            crate::router::ContextOwner::Hub => {
                if let crate::hub::MenuTarget::Game { row, system } = &mut shared.hub.menu_target {
                    let hit = row.path == changed.path
                        && (changed.system.is_empty() || *system == changed.system);
                    if hit {
                        row.flags = flags;
                    }
                    hit
                } else {
                    false
                }
            }
            crate::router::ContextOwner::Systems => false,
        };
        (repaint, menu_row)
    };
    if menu_row {
        let seq = crate::router::context_seq(ctx);
        crate::router::show_strip_flags(ctx, app, seq, flags);
    }
    let shell = app.global::<crate::Shell>();
    let mode = lock(&ctx.shared).games.mode;
    if repaint && shell.get_active_screen() == mode.screen() && !shell.get_transitioning() {
        render(ctx, app);
    }
}

/// Visibility changes retire letter offsets and cursor chains. Settings can
/// change visibility while another screen is active; only refill a visible list.
pub fn refresh_visibility(ctx: &Ctx, app: &App) {
    let (mode, path) = {
        let mut shared = lock(&ctx.shared);
        shared.letter_seq += 1;
        shared.letter_scope = None;
        shared.letter_buckets.clear();
        (shared.games.mode, shared.games.browse_path.clone())
    };
    let shell = app.global::<crate::Shell>();
    if shell.get_active_screen() != mode.screen() || shell.get_transitioning() {
        return;
    }
    let hidden_selection = {
        let shared = lock(&ctx.shared);
        mode == GamesMode::Browse
            && !shared.show_hidden
            && !crate::browse_filter::shows_hidden(&shared)
            && shared.games.current().is_some_and(|row| row.is_hidden)
    };
    if hidden_selection {
        write_saved_path(&mut lock(&ctx.shared), String::new());
    } else {
        persist_now(ctx);
    }
    match mode {
        GamesMode::Browse => browse(ctx, app, &path, false),
        GamesMode::Favorites
        | GamesMode::Recents
        | GamesMode::Search
        | GamesMode::Deck
        | GamesMode::Decks
        | GamesMode::TopPlayed => {
            enter_flat(ctx, app, mode, false);
        }
    }
}

/// Search results (the Search screen's submit): media matching the search
/// held in `persist.search`.
pub fn enter_search(ctx: &Ctx, app: &App, entry: EntryMode) {
    crate::navigation::stage(ctx, app);
    if entry == EntryMode::Fresh {
        let mut shared = lock(&ctx.shared);
        shared.persist.search.selected_path.clear();
        shared.persist.search.list_top = None;
    }
    enter_flat(ctx, app, GamesMode::Search, true);
}

/// The same, focused on a result the Search screen's preview named.
pub fn enter_search_at(ctx: &Ctx, app: &App, path: &str) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.search.selected_path = path.to_string();
        shared.persist.search.list_top = None;
    }
    enter_flat(ctx, app, GamesMode::Search, true);
}

/// The Deck screen, on the deck `persist.deck` names. Fresh starts on its
/// first item; a restore or a return lands on the saved one.
pub fn enter_deck(ctx: &Ctx, app: &App, entry: EntryMode) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        if entry == EntryMode::Fresh {
            shared.persist.deck.selected_item.clear();
            shared.persist.deck.list_top = None;
        }
        shared.games.moving = None;
        shared.games.list_system.clear();
    }
    enter_flat(ctx, app, GamesMode::Deck, true);
}

/// Refill the Deck screen in place after the deck changed in Core.
pub fn refresh_deck(ctx: &Ctx, app: &App) {
    persist_now(ctx);
    enter_flat(ctx, app, GamesMode::Deck, false);
}

/// The Decks screen: the user's own decks and the ones the account saved.
/// Fresh starts on the first; a restore or a return lands on the saved one.
pub fn enter_decks(ctx: &Ctx, app: &App, entry: EntryMode) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        if entry == EntryMode::Fresh {
            shared.persist.decks = zaparoo_core::persist::DecksState::default();
        }
        shared.games.list_system.clear();
    }
    enter_flat(ctx, app, GamesMode::Decks, true);
}

/// Refill the Decks screen in place from the deck list.
pub fn refresh_decks(ctx: &Ctx, app: &App) {
    persist_now(ctx);
    enter_flat(ctx, app, GamesMode::Decks, false);
}

/// Top played (Hub action): Core's most played games, or their systems
/// when the list is grouped.
pub fn enter_top_played(ctx: &Ctx, app: &App, entry: EntryMode) {
    let grouped = zaparoo_app::top_played::grouped(&lock(&ctx.shared).persist.top_played.grouping);
    if grouped && entry == EntryMode::Fresh {
        crate::systems::enter_top_played(ctx, app, entry);
        return;
    }
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        if entry == EntryMode::Fresh {
            shared.persist.top_played.selected_path.clear();
            shared.persist.top_played.list_top = None;
        }
        shared.games.list_system = if grouped {
            shared.persist.top_played.system_id.clone()
        } else {
            String::new()
        };
    }
    enter_flat(ctx, app, GamesMode::TopPlayed, true);
}

/// One system's most played games, from Top played's systems.
pub fn enter_top_played_for_system(ctx: &Ctx, app: &App, system_id: &str) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.games.list_system = system_id.to_string();
        shared.persist.top_played.system_id = system_id.to_string();
        shared.persist.top_played.selected_path.clear();
        shared.persist.top_played.list_top = None;
    }
    enter_flat(ctx, app, GamesMode::TopPlayed, true);
}

/// Refill a Top played screen on show after play history changed.
pub fn refresh_top_played(ctx: &Ctx, app: &App) {
    let visible = lock(&ctx.shared).games.mode == GamesMode::TopPlayed
        && app.global::<crate::Shell>().get_active_screen() == crate::Screen::TopPlayed;
    if visible {
        enter_flat(ctx, app, GamesMode::TopPlayed, false);
    }
}

/// Recently played (Hub action): Core's play history.
pub fn enter_recents(ctx: &Ctx, app: &App, entry: EntryMode) {
    crate::navigation::stage(ctx, app);
    if entry == EntryMode::Fresh {
        lock(&ctx.shared).persist.recents = RecentsState::default();
    }
    enter_flat(ctx, app, GamesMode::Recents, true);
}

/// Refill an already-visible Recently Played screen after Core records
/// a completed launch. Other game-style screens keep their current data.
pub fn refresh_recents(ctx: &Ctx, app: &App) {
    let visible = {
        let shared = lock(&ctx.shared);
        shared.games.mode == GamesMode::Recents
    };
    if visible {
        enter_flat(ctx, app, GamesMode::Recents, false);
    }
}

/// Refill a visible browse list in place after a game was launched, when
/// it holds a multi-disc folder: Core points each folder at the disc last
/// played from it, so the row's launch target and art may have changed.
pub fn refresh_after_launch(ctx: &Ctx, app: &App) {
    let path = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        (model.mode == GamesMode::Browse && model.rows.iter().any(|row| row.multi_disc))
            .then(|| model.browse_path.clone())
    };
    let Some(path) = path else {
        return;
    };
    let shell = app.global::<crate::Shell>();
    if shell.get_active_screen() != GamesMode::Browse.screen() || shell.get_transitioning() {
        return;
    }
    persist_now(ctx);
    browse(ctx, app, &path, false);
}

fn enter_flat(ctx: &Ctx, app: &App, mode: GamesMode, flip: bool) {
    let (ticket, page_size, sort, scope, search, list, top_system) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared);
        let sort = list_sort(&shared);
        let scope = list_scope(&shared);
        let top_system = shared.games.list_system.clone();
        let list = crate::user_lists::active(&shared);
        let search = crate::search::args(&shared);
        let model = &mut shared.games;
        model.mode = mode;
        model.system_id.clear();
        model.system_name.clear();
        model.browse_path.clear();
        model.total_known = false;
        model.total_files = 0;
        model.total_dirs = 0;
        model.focus_armed = false;
        model.restore_done = false;
        model.focus_recalled = false;
        (
            begin_fill(model),
            size,
            sort,
            scope,
            search,
            list,
            top_system,
        )
    };
    if flip {
        crate::router::begin_pending(app, mode.screen());
    } else {
        render(ctx, app);
    }
    let search_rows = |result: &MediaSearchResult| {
        (
            result.results.iter().map(GameRow::from).collect(),
            next_cursor(result.pagination.as_ref()),
        )
    };
    match mode {
        GamesMode::Favorites => fill_from::<MediaUserListEndpoint>(
            ctx,
            app,
            UserListArgs::new(crate::user_lists::user_list(list), page_size, sort, scope),
            (ticket, flip),
            search_rows,
        ),
        GamesMode::Search => {
            fill_from::<MediaSearchEndpoint>(ctx, app, search, (ticket, flip), search_rows);
        }
        GamesMode::Recents => fill_from::<MediaHistoryEndpoint>(
            ctx,
            app,
            HistoryArgs::new(Vec::new(), page_size),
            (ticket, flip),
            |result| {
                (
                    result.entries.iter().map(GameRow::from).collect(),
                    next_cursor(result.pagination.as_ref()),
                )
            },
        ),
        GamesMode::Deck => fill_deck(ctx, app, (ticket, flip)),
        GamesMode::Decks => {
            fill_from::<DecksEndpoint>(
                ctx,
                app,
                DecksParams::default(),
                (ticket, flip),
                |result| (crate::decks::rows_of(&result.decks), None),
            );
        }
        GamesMode::TopPlayed => fill_from::<MediaHistoryTopEndpoint>(
            ctx,
            app,
            TopArgs::new(&top_system),
            (ticket, flip),
            |result| (result.entries.iter().map(GameRow::from).collect(), None),
        ),
        GamesMode::Browse => {}
    }
}

/// Fill the Deck screen from the open deck: every item at once, in the
/// deck's order (a deck holds at most 120). The deck itself is kept for
/// the title, the menus and the cover keys.
fn fill_deck(ctx: &Ctx, app: &App, fill: (u64, bool)) {
    let (ticket, flip) = fill;
    let deck_id = lock(&ctx.shared).persist.deck.deck_id.clone();
    let resource = ctx.store.subscribe::<DeckEndpoint>(deck_id);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(deck) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        {
                            let mut shared = lock(&ctx2.shared);
                            if shared.games.ticket != ticket {
                                return;
                            }
                            shared.games.deck = Some(deck.clone());
                        }
                        crate::decks::publish_open(&ctx2, &app);
                        apply_fill(
                            &ctx2,
                            &app,
                            ticket,
                            crate::decks::rows(&deck),
                            None,
                            None,
                            flip,
                        );
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_error(&ctx2, &app, ticket, &message, flip);
                    });
                    return;
                }
                ResourceStatus::Idle | ResourceStatus::Loading => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    lock(&ctx.shared).games.fill_task.replace(task);
}

/// One page of rows and the cursor for the next, if any.
type Page = (Vec<GameRow>, Option<String>);

/// Fill a flat list from an endpoint's first page: wait for the shared
/// resource to settle, then apply its rows or show its error. `fill` is
/// the fill's ticket and whether it flips to the screen when it lands.
fn fill_from<E: Endpoint>(
    ctx: &Ctx,
    app: &App,
    args: E::Args,
    fill: (u64, bool),
    rows: fn(&E::Output) -> Page,
) {
    let (ticket, flip) = fill;
    let resource = ctx.store.subscribe::<E>(args);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        let (rows, cursor) = rows(&result);
                        apply_fill(&ctx2, &app, ticket, rows, cursor, None, flip);
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_error(&ctx2, &app, ticket, &message, flip);
                    });
                    return;
                }
                ResourceStatus::Idle | ResourceStatus::Loading => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    lock(&ctx.shared).games.fill_task.replace(task);
}

fn next_cursor(pagination: Option<&zaparoo_core::media_types::Pagination>) -> Option<String> {
    pagination
        .filter(|p| p.has_next_page)
        .and_then(|p| p.next_cursor.clone())
}

/// Arm a fresh fill: a new ticket, the loading cue, and the model
/// replacement rules (the snap to row 0 must not persist, pending
/// targets drop, the detail pane resets).
fn begin_fill(model: &mut GamesModel) -> u64 {
    model.fill_task.cancel();
    model.ticket += 1;
    model.page.sliding = false;
    model.folder_direction = 0;
    model.folder_sliding = false;
    model.loading = true;
    model.error.clear();
    model.persist.begin_replacement();
    model.grid.prepare_for_model_replacement();
    model.next_cursor = None;
    model.loading_more = false;
    model.tail_reached = false;
    model.page_restarted = false;
    model.fetch_follows = false;
    model.cover_settle.reset();
    model.jump_loading = false;
    model.pending_restore_path.clear();
    model.detail_seq += 1;
    let _ = model.detail.disable(true);
    model.ticket
}

/// Browse (`system_id`, `path`). `flip` selects the deferred route entry
/// (input gate plus screen push on Ready). Folder entry uses
/// `browse_with_motion` to retain the outgoing grid during the fill.
fn browse(ctx: &Ctx, app: &App, path: &str, flip: bool) {
    browse_with_motion(ctx, app, path, flip, 0);
}

fn browse_with_motion(ctx: &Ctx, app: &App, path: &str, flip: bool, direction: i32) {
    crate::folder_motion::capture(app, direction);
    let (ticket, args, live) = {
        let mut shared = lock(&ctx.shared);
        // One round trip fills the lookahead the grid keeps loaded (or two
        // list screens), instead of a page and then follow-up fetches.
        let size = rules::first_fill_limit(
            page_size(ctx, app, &shared),
            list_layout(&shared),
            shared.games.grid.load_ahead_pages,
        );
        let tags = crate::browse_filter::active_tags(&shared);
        let include_hidden = include_hidden(&shared);
        let model = &mut shared.games;
        model.mode = GamesMode::Browse;
        model.browse_path = path.to_string();
        model.total_known = true;
        let ticket = begin_fill(model);
        model.folder_direction = direction;
        let args = if model.live {
            BrowseArgs::live(path.to_string(), size)
        } else {
            BrowseArgs::new(path.to_string(), vec![model.system_id.clone()], size, tags)
                .with_hidden(include_hidden)
        };
        (ticket, args, model.live)
    };
    crate::router::save_persist(&ctx.shared);
    if flip {
        crate::router::begin_pending(app, crate::Screen::Games);
    } else {
        render(ctx, app);
    }

    let resource = ctx.store.subscribe::<MediaBrowseEndpoint>(args);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let at_root = path.is_empty();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        on_browse_ready(&ctx2, &app, ticket, &result, at_root, flip, live);
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_error(&ctx2, &app, ticket, &message, flip);
                    });
                    return;
                }
                ResourceStatus::Idle | ResourceStatus::Loading => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    lock(&ctx.shared).games.fill_task.replace(task);
}

/// A browse page's rows. A live page carries no tags, so its rows' favorite
/// and hidden state is not known.
fn browse_rows(entries: &[BrowseEntry], live: bool) -> Vec<GameRow> {
    let roots: Vec<Option<&str>> = entries
        .iter()
        .map(|e| (e.entry_type == "root" && !e.path.is_empty()).then_some(e.path.as_str()))
        .collect();
    let keep = rules::dedup_roots_drop_ancestors(&roots);
    let kept: Vec<&BrowseEntry> = entries
        .iter()
        .zip(keep)
        .filter_map(|(e, keep)| keep.then_some(e))
        .collect();
    let kept_roots: Vec<Option<&str>> = kept
        .iter()
        .map(|e| (e.entry_type == "root" && !e.path.is_empty()).then_some(e.path.as_str()))
        .collect();
    let distinguishers = rules::root_distinguishers(&kept_roots);
    kept.iter()
        .zip(distinguishers)
        .map(|(e, distinguisher)| {
            let mut row = GameRow::from(*e);
            row.root_distinguisher = distinguisher;
            row.tags_known = !live;
            row
        })
        .collect()
}

pub(crate) fn on_browse_ready(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    result: &zaparoo_core::media_types::MediaBrowseResult,
    at_root: bool,
    flip: bool,
    live: bool,
) {
    if lock(&ctx.shared).games.ticket != ticket {
        return;
    }
    let rows = browse_rows(&result.entries, live);
    if rows.is_empty() && !at_root && fall_back_from_recalled_focus(ctx, app, flip) {
        return;
    }
    // Single-root auto-nav: a system whose scoped roots collapse to one
    // folder skips the pointless one-entry level. The root REPLACES the
    // stack's base so Back still exits the screen.
    let types: Vec<EntryType> = rows.iter().map(|r| r.entry_type).collect();
    if rules::single_root_auto_nav(at_root, &types) {
        let root_path = rows[0].path.clone();
        {
            let mut shared = lock(&ctx.shared);
            shared.persist.games.path_stack = vec![root_path.clone()];
            shared.persist.games.selected_at_level = vec![String::new()];
            shared.persist.games.list_top_at_level.clear();
        }
        let direction = lock(&ctx.shared).games.folder_direction;
        browse_with_motion(ctx, app, &root_path, flip, direction);
        return;
    }
    let total_dirs = result.total_dirs.unwrap_or(0);
    apply_fill(
        ctx,
        app,
        ticket,
        rows,
        next_cursor(result.pagination.as_ref()),
        Some((result.total_files, total_dirs)),
        flip,
    );
    // The fast-scroll rail and letter steps need the scope's letters
    // before the first long hold, not only once the picker opens. A live
    // folder has no letter index.
    if live {
        return;
    }
    let fetched = {
        let shared = lock(&ctx.shared);
        shared.letter_scope.as_ref().is_some_and(|(system, path)| {
            *system == shared.games.system_id && *path == shared.games.browse_path
        })
    };
    if !fetched {
        crate::router::fetch_letter_index(ctx, app);
    }
}

/// Ready-side fill: store the rows, seat the persisted selection, persist
/// the screen token, flip (or clear the cue), paint.
pub(crate) fn apply_fill(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    rows: Vec<GameRow>,
    cursor: Option<String>,
    totals: Option<(u32, u32)>,
    flip: bool,
) {
    let (token, restore_fetch, fill_list) = {
        let mut shared = lock(&ctx.shared);
        if shared.games.ticket != ticket {
            return;
        }
        let list = list_layout(&shared);
        let visible = list_rows_visible(ctx, app, &shared);
        let saved = saved_path(&shared);
        let model = &mut shared.games;
        model.focus_recalled = false;
        model.rows = rows;
        model.next_cursor = cursor;
        model.tail_reached = model.next_cursor.is_none();
        model.loading = false;
        model.loading_more = false;
        model.covers_paused = false;
        model.error.clear();
        if let Some((files, dirs)) = totals {
            model.total_files = files;
            model.total_dirs = dirs;
        }
        model.grid.set_loading_more(false);
        model.grid.pagination_total_known = model.total_known;
        model.grid.total_items_override = model.known_total();
        model.grid.set_item_count(model.rows.len());
        model.grid.set_has_more_pages(model.next_cursor.is_some());
        model.persist.end_replacement();
        // Restore: the saved path when loaded; otherwise row 0 while the
        // restore walk keeps fetching for a deeper saved row.
        let found = (!saved.is_empty())
            .then(|| model.rows.iter().position(|r| r.selection_key() == saved))
            .flatten();
        let restore_fetch = match found {
            Some(index) => {
                model.grid.set_current_index_immediate(index);
                model.pending_restore_path.clear();
                false
            }
            None if !saved.is_empty() && model.has_more() => {
                model.pending_restore_path = saved;
                model.grid.set_current_index_immediate(0);
                true
            }
            None => {
                model.pending_restore_path.clear();
                model.grid.set_current_index_immediate(0);
                false
            }
        };
        if !restore_fetch {
            // Nothing left to walk: the restart landed, so a later failed
            // page may restart again.
            model.page_restarted = false;
        }
        model.restore_done = true;
        let fill_list = list && rules::list_fill_page(model.rows.len(), visible, model.has_more());
        let token = model.mode.screen();
        drop(shared);
        let mut shared = lock(&ctx.shared);
        refresh_display(&mut shared);
        shared.persist.active_screen = token.token().to_string();
        (token, restore_fetch, fill_list)
    };
    if app.global::<crate::Shell>().get_transitioning()
        && (restore_fetch || list_restore_needs_rows(ctx, app, &lock(&ctx.shared)))
    {
        fetch_more(ctx, app, rules::RESTORE_FETCH_CHUNK, true);
        return;
    }
    crate::navigation::finish(app);
    crate::router::save_persist(&ctx.shared);
    if flip {
        crate::router::transition_to_screen(app, token, 1);
    }
    render(ctx, app);
    crate::folder_motion::start(ctx, app);
    schedule_detail(ctx, app, false);
    if restore_fetch {
        fetch_more(ctx, app, rules::RESTORE_FETCH_CHUNK, true);
    } else if fill_list {
        let size = {
            let shared = lock(&ctx.shared);
            page_size(ctx, app, &shared)
        };
        fetch_more(ctx, app, size, false);
    } else {
        drain_load_requests(ctx, app);
    }
}

/// Terminal in-screen error (`ScreenStateOverlay`'s Error state): flip to
/// the destination and paint "Failed to load" plus the message.
pub(crate) fn show_error(ctx: &Ctx, app: &App, ticket: u64, message: &str, flip: bool) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.games.ticket != ticket {
            return;
        }
        // An error does not prove a recalled folder is gone: it may be the
        // link or Core being busy. Keep the remembered position for the
        // next visit; only an empty answer drops it.
        shared.games.focus_recalled = false;
    }
    if crate::navigation::fail(ctx, app, message) {
        return;
    }
    let token = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        if model.ticket != ticket {
            return;
        }
        model.rows.clear();
        model.grid.set_item_count(0);
        model.loading = false;
        model.loading_more = false;
        model.error = message.to_string();
        model.folder_direction = 0;
        model.persist.end_replacement();
        model.mode.screen()
    };
    if flip {
        crate::router::transition_to_screen(app, token, 1);
    }
    render(ctx, app);
    if !flip {
        crate::router::clear_pending(app);
    }
}

/// Fetch the next chunk with the stored cursor, bypassing the endpoint
/// cache (each follow-up has a different cursor). `bulk` pauses cover
/// fetches until the chunk lands (jumps and restores).
/// The most rows one request for this list may ask for: Core validates
/// `media.browse`/`media.search` `maxResults` up to 1000 and `media.history`
/// `limit` up to 100, and rejects anything larger.
fn fetch_cap(mode: GamesMode) -> u32 {
    match mode {
        GamesMode::Recents | GamesMode::TopPlayed => rules::HISTORY_FETCH_CAP,
        GamesMode::Browse
        | GamesMode::Favorites
        | GamesMode::Search
        | GamesMode::Deck
        | GamesMode::Decks => rules::JUMP_FETCH_CEILING,
    }
}

/// A cursor page of the folder on screen. A live folder sends only its
/// path; Core rejects systems, tags and the root view there.
fn browse_page_params(
    model: &GamesModel,
    tags: Vec<String>,
    include_hidden: bool,
    cursor: &str,
) -> MediaBrowseParams {
    let path = model.browse_path.clone();
    if model.live {
        return MediaBrowseParams {
            path,
            cursor: Some(cursor.to_string()),
            live: true,
            ..MediaBrowseParams::default()
        };
    }
    MediaBrowseParams {
        root_view: merged_root_view(&path, std::slice::from_ref(&model.system_id)),
        path,
        systems: vec![model.system_id.clone()],
        include_hidden: Some(include_hidden),
        max_results: None,
        cursor: Some(cursor.to_string()),
        tags,
        letter: None,
        sort: None,
        live: false,
    }
}

fn fetch_more(ctx: &Ctx, app: &App, limit: u32, bulk: bool) {
    let list_tag = crate::user_lists::active(&lock(&ctx.shared)).tag();
    let (mode, cursor, browse, ticket, sort, scope, search, live) = {
        let mut shared = lock(&ctx.shared);
        let search = crate::search::args(&shared);
        let include_hidden = include_hidden(&shared);
        let tags = crate::browse_filter::active_tags(&shared);
        let sort = list_sort(&shared);
        let scope = list_scope(&shared);
        let model = &mut shared.games;
        if model.loading_more || model.loading {
            return;
        }
        let Some(cursor) = model.next_cursor.clone() else {
            return;
        };
        model.loading_more = true;
        model.grid.set_loading_more(true);
        if bulk {
            model.covers_paused = true;
        }
        let browse = browse_page_params(model, tags, include_hidden, &cursor);
        (
            model.mode,
            cursor,
            browse,
            model.ticket,
            sort,
            scope,
            search,
            model.live,
        )
    };
    render(ctx, app);
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let limit = limit.clamp(1, fetch_cap(mode));
    ctx.handle.spawn(async move {
        let outcome: Result<(Vec<GameRow>, Option<String>), PageError> = match mode {
            GamesMode::Browse => client
                .media_browse(MediaBrowseParams {
                    max_results: Some(limit),
                    ..browse
                })
                .await
                .map(|r| {
                    let cursor = next_cursor(r.pagination.as_ref());
                    (browse_rows(&r.entries, live), cursor)
                })
                .map_err(PageError::from),
            GamesMode::Favorites | GamesMode::Search => client
                .media_search(if mode == GamesMode::Search {
                    MediaSearchParams {
                        max_results: Some(limit),
                        ..search.params(Some(cursor))
                    }
                } else {
                    MediaSearchParams {
                        systems: scope,
                        max_results: Some(limit),
                        cursor: Some(cursor),
                        tags: vec![list_tag.to_string()],
                        sort,
                        ..MediaSearchParams::default()
                    }
                })
                .await
                .map(|r| {
                    let cursor = next_cursor(r.pagination.as_ref());
                    (r.results.iter().map(GameRow::from).collect(), cursor)
                })
                .map_err(PageError::from),
            GamesMode::Recents => client
                .media_history(MediaHistoryParams {
                    limit: Some(limit),
                    cursor: Some(cursor),
                    systems: Vec::new(),
                    distinct_media: Some(true),
                })
                .await
                .map(|r| {
                    let cursor = next_cursor(r.pagination.as_ref());
                    (r.entries.iter().map(GameRow::from).collect(), cursor)
                })
                .map_err(PageError::from),
            // A deck, the deck list and Top played fill whole: there is
            // never a page to follow.
            GamesMode::Deck | GamesMode::Decks | GamesMode::TopPlayed => Ok((Vec::new(), None)),
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            on_append(&ctx2, &app, ticket, outcome);
        });
    });
}

/// A follow-up page did not load. A link failure leaves the rows on screen
/// and the cursor good: the next move retries. An answer from Core is
/// different. Core accepts a cursor only while the list it came from is
/// unchanged, so the same request would fail again: reload from the first
/// page and walk back to the selection (`restart`), and show a reload that
/// fails too.
fn page_failed(ctx: &Ctx, app: &App, ticket: u64, error: &PageError, restart: bool) {
    tracing::warn!("page fetch failed: {}", error.message);
    if app.global::<crate::Shell>().get_transitioning() {
        show_error(ctx, app, ticket, &error.message, true);
    } else if error.transport {
        render(ctx, app);
    } else if restart {
        restart_after_failed_page(ctx, app);
    } else {
        show_error(ctx, app, ticket, &error.message, false);
    }
}

/// A follow-up page that did not load. `transport` says the link failed
/// before Core answered, which leaves the cursor good for another try; an
/// answer from Core means it will not serve that cursor again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageError {
    pub message: String,
    pub transport: bool,
}

// The motion tests, which only build under the MiSTer feature set, are
// the callers that make one by hand.
#[cfg(all(test, feature = "mister"))]
impl PageError {
    pub(crate) fn transport(message: &str) -> Self {
        Self {
            message: message.to_string(),
            transport: true,
        }
    }

    pub(crate) fn refused(message: &str) -> Self {
        Self {
            message: message.to_string(),
            transport: false,
        }
    }
}

impl From<zaparoo_core::client::ClientError> for PageError {
    fn from(error: zaparoo_core::client::ClientError) -> Self {
        Self {
            transport: error.is_transport(),
            message: error.message,
        }
    }
}

pub(crate) fn on_append(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    outcome: Result<(Vec<GameRow>, Option<String>), PageError>,
) {
    let (restore_again, landed_restore, changed_page) = {
        let mut shared = lock(&ctx.shared);
        if shared.games.ticket != ticket {
            return;
        }
        let saved = saved_path(&shared);
        let list = list_layout(&shared);
        let show_original = shared.persist.settings.show_original_filenames;
        let language = crate::effective_language(&shared.persist.settings.language);
        shared.games.show_tags = shared.persist.settings.show_tags;
        shared.games.list_images = shared.persist.settings.list_left_right == "image";
        let model = &mut shared.games;
        let from_page = model.grid.current_page();
        model.loading_more = false;
        model.covers_paused = false;
        let (rows, cursor) = match outcome {
            Ok(page) => page,
            Err(error) => {
                model.grid.set_loading_more(false);
                model.jump_loading = false;
                let restart = !model.page_restarted;
                drop(shared);
                page_failed(ctx, app, ticket, &error, restart);
                return;
            }
        };
        let empty = rows.is_empty();
        let appended_from = model.rows.len();
        model.rows.extend(rows);
        model.next_cursor = if empty { None } else { cursor };
        model.tail_reached = model.next_cursor.is_none();
        model.page_restarted = false;
        model.grid.total_items_override = model.known_total();
        model.grid.set_item_count(model.rows.len());
        model.grid.set_has_more_pages(model.next_cursor.is_some());
        model.grid.set_loading_more(false);
        if !model.grid.has_pending_target() {
            model.jump_loading = false;
        }
        // Restore continuation: a user move diverges the saved path from
        // the pending one and ends the walk.
        let mut restore_again = false;
        let mut landed = false;
        if !model.pending_restore_path.is_empty() {
            if saved != model.pending_restore_path {
                model.pending_restore_path.clear();
            } else if let Some(index) =
                model
                    .rows
                    .iter()
                    .enumerate()
                    .skip(appended_from)
                    .find_map(|(index, row)| {
                        (row.selection_key() == model.pending_restore_path).then_some(index)
                    })
            {
                model.grid.set_current_index_immediate(index);
                model.pending_restore_path.clear();
                landed = true;
            } else if model.has_more() {
                restore_again = true;
            } else {
                model.pending_restore_path.clear();
            }
        }
        let changed_page = !list && model.grid.current_page() != from_page;
        model.refresh_display_from(appended_from, show_original, &language);
        model.fetch_follows =
            restore_again || (model.grid.has_pending_target() && model.has_more());
        (restore_again, landed, changed_page)
    };
    if app.global::<crate::Shell>().get_transitioning() {
        if restore_again || list_restore_needs_rows(ctx, app, &lock(&ctx.shared)) {
            fetch_more(ctx, app, rules::RESTORE_FETCH_CHUNK, true);
            return;
        }
        let token = lock(&ctx.shared).games.mode.screen();
        crate::navigation::finish(app);
        crate::router::save_persist(&ctx.shared);
        crate::router::transition_to_screen(app, token, 1);
        crate::folder_motion::start(ctx, app);
    }
    if landed_restore {
        persist_now(ctx);
    }
    if changed_page && !landed_restore {
        persist_current(ctx);
        slide_to_current_page(ctx, app);
    } else {
        render(ctx, app);
    }
    schedule_detail(ctx, app, false);
    if restore_again {
        fetch_more(ctx, app, rules::RESTORE_FETCH_CHUNK, true);
    } else {
        drain_load_requests(ctx, app);
    }
    // The walk's next fetch has started, or there was none after all.
    lock(&ctx.shared).games.fetch_follows = false;
    sync_waits(ctx, app);
}

/// Reload the list from its first page after a page could not be loaded,
/// keeping the selection: the fill restores the saved row, walking back to
/// it when it sits past the first page. The cached first page carries the
/// same dead cursor, so it is dropped first.
fn restart_after_failed_page(ctx: &Ctx, app: &App) {
    persist_now(ctx);
    for endpoint in [
        MediaBrowseEndpoint::NAME,
        MediaUserListEndpoint::NAME,
        MediaHistoryEndpoint::NAME,
        MediaSearchEndpoint::NAME,
    ] {
        ctx.store.invalidate(&Tag::any(endpoint));
    }
    let (mode, path) = {
        let shared = lock(&ctx.shared);
        (shared.games.mode, shared.games.browse_path.clone())
    };
    match mode {
        GamesMode::Browse => browse(ctx, app, &path, false),
        GamesMode::Favorites
        | GamesMode::Recents
        | GamesMode::Search
        | GamesMode::Deck
        | GamesMode::Decks
        | GamesMode::TopPlayed => {
            enter_flat(ctx, app, mode, false);
        }
    }
    lock(&ctx.shared).games.page_restarted = true;
}

/// Restart source work retired by a canceled navigation's generation.
pub(crate) fn resume_after_cancel(ctx: &Ctx, app: &App) {
    schedule_detail(ctx, app, true);
    drain_load_requests(ctx, app);
}

/// Run the fetches the grid asked for: a jump loads up to its target in
/// bulk, held rapid scrolling takes bigger chunks, everything else one page.
pub(crate) fn drain_load_requests(ctx: &Ctx, app: &App) {
    let (limit, bulk) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared) as usize;
        let list = list_layout(&shared);
        let model = &mut shared.games;
        if model.loading || model.loading_more {
            return;
        }
        let urgent = model
            .grid
            .take_load_requests()
            .iter()
            .any(|request| request.urgent);
        // Prime the current page plus a bounded lookahead, even before the
        // first keypress. Cover prefetch can then queue that page's artwork.
        let needed = (model.grid.current_page() + 1 + model.grid.load_ahead_pages)
            .saturating_mul(model.grid.page_size());
        if !urgent && (list || !model.has_more() || model.rows.len() >= needed) {
            return;
        }
        if let Some(target) = model.grid.pending_jump_index() {
            (
                rules::jump_fetch_limit(target, model.rows.len(), size),
                true,
            )
        } else if model.rapid.active {
            let limit = if urgent {
                rules::RAPID_FETCH_CHUNK
            } else {
                rules::RAPID_FETCH_CHUNK
                    .min(u32::try_from(needed.saturating_sub(model.rows.len())).unwrap_or(u32::MAX))
            };
            (limit, false)
        } else {
            (u32::try_from(size).unwrap_or(12), false)
        }
    };
    fetch_more(ctx, app, limit, bulk);
}

// ---------- Geometry and rendering ----------

/// The grid band, cell fit and footer slot for the scene, resolved
/// through the games browse layout profile.
pub struct Geometry {
    pub columns: i32,
    pub rows: i32,
    pub grid_y: i32,
    pub grid_height: i32,
    pub label_y: i32,
    pub label_height: i32,
    pub insets: Insets,
    pub fit: paged_grid::Fit,
}

/// Resolve the geometry for `inputs`. Games keeps its caption at the
/// screen bottom above the help bar (`activeLabelAtBottom`); the flat
/// lists hang it directly under the grid with the shell's default gutter.
pub fn geometry_for(inputs: &zaparoo_app::sizing::Inputs, mode: GamesMode) -> Geometry {
    let derived = zaparoo_app::sizing::derive(inputs);
    grid_geometry_for(
        inputs,
        mode,
        crate::page_bar::games_row_block(mode, &derived),
    )
}

/// The games grid under a Menu chip band of `row_block`; zero for a screen
/// without chips, such as the file browser.
pub(crate) fn grid_geometry_for(
    inputs: &zaparoo_app::sizing::Inputs,
    mode: GamesMode,
    row_block: i32,
) -> Geometry {
    let derived = zaparoo_app::sizing::derive(inputs);
    let profile = layouts::profile(ThemeId::current(inputs), View::GamesGrid, inputs);
    let (grid, footer) = match profile.body {
        Body::Grid { grid, footer } => (grid, footer),
        Body::List { .. } => unreachable!("the games grid view resolves to a grid body"),
    };
    let screen_h = inputs.screen_height as i32;
    let grid_y =
        derived.header_bottom + profile.status.top_margin + profile.status.strip_height + row_block;
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let (bottom, label_y, label_height) = match mode {
        GamesMode::Browse | GamesMode::Decks => {
            let label_height = footer.active_label_height;
            let bottom = if t240 {
                derived.help_bar_height + label_height
            } else {
                footer.grid_bottom_margin
            };
            let label_y = screen_h
                - if t240 {
                    derived.help_bar_height
                } else {
                    footer.active_label_bottom_margin
                }
                - label_height;
            (bottom, label_y, label_height)
        }
        GamesMode::Favorites
        | GamesMode::Recents
        | GamesMode::Search
        | GamesMode::Deck
        | GamesMode::TopPlayed => {
            let label_height = inputs.pct_h(7.0);
            let bottom = if t240 {
                derived.help_bar_height + label_height
            } else {
                zaparoo_app::sizing::less_hidden_bar(inputs, &derived, inputs.pct_h(15.0))
            };
            let grid_height = (screen_h - grid_y - bottom).max(0);
            (bottom, grid_y + grid_height, label_height)
        }
    };
    let grid_height = (screen_h - grid_y - bottom).max(0);
    let insets = Insets {
        left: grid.left_inset,
        right: grid.right_inset,
        top: grid.top_inset,
        bottom: grid.bottom_inset,
        column_gap: grid.column_gap,
        row_gap: grid.row_gap,
    };
    let columns = derived.games_grid_columns;
    let rows = derived.games_grid_rows;
    Geometry {
        columns,
        rows,
        grid_y,
        grid_height,
        label_y,
        label_height,
        insets,
        fit: paged_grid::fit(
            columns,
            rows,
            inputs.screen_width as i32,
            grid_height,
            None,
            false,
            &insets,
        ),
    }
}

fn geometry(app: &App, mode: GamesMode) -> Geometry {
    geometry_for(&crate::router::output_scene(app).inputs(), mode)
}

/// How a cover is asked for and prepared: Core's size tier, and the
/// painted box the decoded image is resized to. The software renderer
/// samples a bitmap nearest-neighbor, so the decoded size must be the
/// painted one.
#[derive(Clone, Copy)]
struct CoverSize {
    tier: u32,
    fit: zaparoo_app::covers::Fit,
}

impl CoverSize {
    /// The detail pane's cover, fitted to the art box the view reported.
    /// Until it has, the image keeps its decoded size. The pane shows one
    /// image at a time, so a small screenshot or title screen is enlarged.
    fn detail(app: &App) -> Self {
        let view = app.global::<GamesView>();
        let fit = zaparoo_app::covers::Fit::new(
            view.get_detail_art_width(),
            view.get_detail_art_height(),
        )
        .crisp();
        Self {
            tier: crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
            fit,
        }
    }
}

/// The grid's cover size at the current output geometry: the decode tier
/// and the one art box every tile of `mode` paints in.
fn cover_tier(app: &App, mode: GamesMode) -> CoverSize {
    let scene = crate::router::output_scene(app);
    cover_size(
        &scene.inputs(),
        mode,
        crate::sizing::games_grid_cover_source_size(scene),
    )
}

fn cover_size(inputs: &zaparoo_app::sizing::Inputs, mode: GamesMode, tier: u32) -> CoverSize {
    let geometry = geometry_for(inputs, mode);
    // Every tile paints its art in the same box: a system label rides over
    // the art on its scrim instead of taking a band from it.
    let (width, height) = zaparoo_app::sizing::tile_art_box(
        inputs,
        geometry.fit.cell_width,
        geometry.fit.cell_height,
        zaparoo_app::sizing::TileArt {
            compact_padding: false,
            caption: true,
        },
    );
    CoverSize {
        tier,
        fit: zaparoo_app::covers::Fit::new(width, height),
    }
}

/// The flat lists name each tile's system over its art; Games and the
/// Decks screen do not.
fn labelled_mode(mode: GamesMode) -> bool {
    !matches!(mode, GamesMode::Browse | GamesMode::Decks)
}

/// The system name a flat list's tile shows over its art, empty for a row
/// with no system (a deck, a card or a script item).
fn top_label(row: &GameRow) -> &str {
    let name = row.system_name.trim();
    if name.is_empty() {
        row.system_id.as_str()
    } else {
        name
    }
}

/// The grid tile art box, for tests that seed covers by key.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn grid_cover_fit(app: &App, mode: GamesMode) -> zaparoo_app::covers::Fit {
    cover_tier(app, mode).fit
}

/// A folder's art is asked for by path, with the disc it launches when it
/// has one: Core then answers with the folder's own artwork before that
/// disc's, and each disc caches under its own key.
fn media_key(row: &GameRow, fallback_system: &str, tier: CoverSize) -> MediaKey {
    if let Some(deck) = &row.deck_entry {
        return MediaKey::deck_art(&deck.deck_id, None, deck.revision, tier.tier, tier.fit);
    }
    if let Some(item) = row.deck_item.as_ref().filter(|item| item.card_art) {
        return MediaKey::deck_art(
            &item.deck_id,
            Some(item.id),
            item.revision,
            tier.tier,
            tier.fit,
        );
    }
    MediaKey {
        media_id: row.media_id,
        system: row.system_or(fallback_system).to_string(),
        path: row.path.clone(),
        max_size: tier.tier,
        fit: tier.fit,
        folder: row.entry_type == EntryType::Directory,
        image_type: None,
    }
}

/// List rows never resolve artwork: only their separate detail pane paints it.
fn text_cell(row: &GameRow) -> GridCell {
    GridCell {
        name: SharedString::from(row.display.as_str()),
        tags: SharedString::from(row.suffix.as_str()),
        favorite: row.flags.favorite,
        hidden: row.is_hidden,
        opens: row.is_dir(),
        ..Default::default()
    }
}

/// One tile: the caption with its dim suffix, the favorite heart, the
/// flat lists' system label, and the cover slot under its `MediaKey`.
fn cell_for(
    ctx: &Ctx,
    model: &GamesModel,
    logos: crate::systems::LogoPrefs,
    row: &GameRow,
    tier: CoverSize,
) -> GridCell {
    let mut cell = text_cell(row);
    if labelled_mode(model.mode) {
        cell.top_label = SharedString::from(top_label(row));
    }
    let key = media_key(row, &model.system_id, tier);
    let cached = ctx.media.get(&key);
    let state = rules::cover_state(
        row.entry_type,
        row.media_capable,
        row.wants_cover() && !key.system.is_empty(),
        cached.is_some(),
        ctx.media.is_negative(&key),
    );
    if matches!(state, CoverState::Art | CoverState::Pending) {
        cell.fetched = true;
        if let Some(color) = placeholder_color(ctx, row, &key) {
            cell.placeholder = color;
            cell.has_placeholder = true;
        }
    }
    match state {
        CoverState::Folder => cell.glyph_key = SharedString::from(FOLDER_GLYPH),
        CoverState::Art => {
            if let Some(image) = cached {
                cell.cover = slint::Image::from_rgba8(image.buffer);
                cell.has_cover = true;
            }
        }
        CoverState::Absent if row.deck_entry.is_some() => {
            cell.glyph_key = SharedString::from(DECK_GLYPH);
        }
        CoverState::Absent if row.deck_item.is_some() && row.system_id.is_empty() => {
            // A card or a script: no system to show the logo of.
            cell.glyph_key = SharedString::from(FILE_GLYPH);
        }
        CoverState::Absent => {
            // The flat lists fall back to the system logo, a friendlier
            // "no cover" cue than the file chip; Games keeps the chip.
            let bounds = zaparoo_app::logo_cache::Bounds::new(tier.tier, tier.tier);
            let logo = (model.mode != GamesMode::Browse && !key.system.is_empty())
                .then(|| logos.key(ctx, &key.system, bounds))
                .flatten()
                .and_then(|key| ctx.logos.get(&key));
            if let Some(pair) = logo {
                (cell.cover, cell.cover_focus) = pair.images();
                cell.has_cover = true;
                cell.has_cover_focus = true;
            } else {
                cell.glyph_key = SharedString::from(FILE_GLYPH);
            }
        }
        CoverState::Pending => {}
    }
    cell
}

/// The art slot's stand-in colour while a row's cover loads.
fn placeholder_color(ctx: &Ctx, row: &GameRow, key: &MediaKey) -> Option<slint::Color> {
    row.cover_color
        .or_else(|| ctx.media.average_color(key))
        .or_else(|| ctx.media.average_color(&key.color_preview()))
        .map(|[r, g, b]| slint::Color::from_rgb_u8(r, g, b))
}

/// A row that will show art but has none cached yet.
/// Whether the grid page on screen still waits on any of its covers, for
/// `zaparoo_app::cover_landing`. A list's one detail cover never waits.
#[cfg(feature = "mister")]
pub fn page_covers_waiting(ctx: &Ctx, app: &App) -> bool {
    if !matches!(
        app.global::<crate::Shell>().get_active_screen(),
        crate::Screen::Games
            | crate::Screen::Favorites
            | crate::Screen::Recents
            | crate::Screen::SearchResults
            | crate::Screen::Deck
            | crate::Screen::Decks
            | crate::Screen::TopPlayed
    ) {
        return false;
    }
    let shared = lock(&ctx.shared);
    if list_layout(&shared) {
        return false;
    }
    let model = &shared.games;
    let tier = cover_tier(app, model.mode);
    let page_size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(model.grid.current_page() * page_size)
        .take(page_size)
        .any(|row| cover_waiting(ctx, model, row, tier))
}

fn cover_waiting(ctx: &Ctx, model: &GamesModel, row: &GameRow, tier: CoverSize) -> bool {
    let key = media_key(row, &model.system_id, tier);
    let state = rules::cover_state(
        row.entry_type,
        row.media_capable,
        row.wants_cover() && !key.system.is_empty(),
        ctx.media.get(&key).is_some(),
        ctx.media.is_negative(&key),
    );
    matches!(state, CoverState::Pending)
}

fn page_cells(
    ctx: &Ctx,
    model: &GamesModel,
    logos: crate::systems::LogoPrefs,
    page: usize,
    tier: CoverSize,
) -> Vec<GridCell> {
    let page_size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|row| cell_for(ctx, model, logos, row, tier))
        .collect()
}

/// Replace this view's demand, including obsolete detail images. Other owners
/// (Hub and modals) keep their independently enqueued requests.
fn request_covers(ctx: &Ctx, model: &GamesModel, mut keys: Vec<MediaKey>) {
    // The focused game's background art rides the same cancellable
    // demand, after the covers on screen.
    if let Some(key) = background_game_key(model) {
        keys.push(key);
    }
    // Even a tiny thumbnail makes Core decode its original on a cold miss.
    // Fast navigation reuses prepared colors but starts no artwork work.
    ctx.media
        .request_wanted(if model.covers_paused || model.rapid.active {
            Vec::new()
        } else {
            keys
        });
}

/// The focused row's art for Game background when it is a game: exactly
/// the chosen type, at a size the blur needs. Folders, decks and deck
/// cards show their own art instead (`report_background`).
fn background_game_key(model: &GamesModel) -> Option<MediaKey> {
    let image_type = crate::background::game_type()?;
    let row = model.current().filter(|row| row.wants_cover())?;
    if shows_own_art(row) || !row.media_capable {
        return None;
    }
    Some(MediaKey {
        media_id: row.media_id,
        system: row.system_or(&model.system_id).to_string(),
        path: row.path.clone(),
        folder: row.entry_type == EntryType::Directory,
        max_size: crate::background::art_request_size(),
        fit: zaparoo_app::covers::Fit::SOURCE,
        image_type: Some(image_type.to_string()),
    })
}

/// A folder, a deck or a deck card: its own art is its background,
/// whatever the Game background type.
fn shows_own_art(row: &GameRow) -> bool {
    row.deck_entry.is_some()
        || row.deck_item.as_ref().is_some_and(|item| item.card_art)
        || (row.entry_type == EntryType::Directory && !row.media_capable)
}

/// Tell the background what the focused row puts behind the screen.
fn report_background(ctx: &Ctx, model: &GamesModel, art_tier: CoverSize) {
    use crate::background::Focus;
    if crate::background::game_type().is_none() {
        return;
    }
    if model.rapid.active {
        crate::background::focus(Focus::Moving);
        return;
    }
    let key = model
        .current()
        .filter(|row| row.wants_cover())
        .and_then(|row| {
            if shows_own_art(row) {
                Some(media_key(row, &model.system_id, art_tier))
            } else {
                background_game_key(model)
            }
        });
    let Some(key) = key else {
        crate::background::focus(Focus::None);
        return;
    };
    let id = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    };
    if let Some(image) = ctx.media.get(&key) {
        crate::background::focus(Focus::Art(id, image.buffer));
    } else if ctx.media.is_negative(&key) {
        crate::background::focus(Focus::None);
    } else {
        crate::background::focus(Focus::Waiting(id));
    }
}

/// The cover keys around the visible window, in fetch priority order.
fn wanted_covers(
    ctx: &Ctx,
    model: &GamesModel,
    first_visible: usize,
    page_size: usize,
    tier: CoverSize,
) -> Vec<MediaKey> {
    let indices = rules::prefetch_rows(model.rows.len(), page_size, first_visible);
    let mut wanted = Vec::new();
    for window in indices.chunks(page_size.max(1)) {
        let rows: Vec<_> = window
            .iter()
            .filter_map(|index| model.rows.get(*index))
            .filter(|row| row.wants_cover())
            .map(|row| (row, media_key(row, &model.system_id, tier)))
            .filter(|(_, key)| !key.system.is_empty() && !key.path.is_empty())
            .collect();
        // Prepare real colors before this window's full-size art; replacing
        // the wanted window drops work for pages that scrolled past.
        for (row, key) in &rows {
            if row.cover_color.is_none()
                && tier.tier > zaparoo_app::covers::COLOR_PREVIEW_MAX_SIZE
                && !ctx.media.is_cached(key)
                && !ctx.media.is_negative(key)
            {
                wanted.push(key.color_preview());
            }
        }
        wanted.extend(rows.into_iter().map(|(_, key)| key));
    }
    wanted
}

/// What the header says while follow-up rows load: how far a walk to a
/// distant row has got, when the list's size is known, and otherwise only
/// that more is coming.
fn page_wait_text(shared: &Shared) -> (crate::AppCue, String, String) {
    let model = &shared.games;
    let walking =
        model.grid.pending_jump_index().is_some() || !model.pending_restore_path.is_empty();
    match model.known_total() {
        Some(total) if walking => {
            let language = crate::effective_language(&shared.persist.settings.language);
            let count = |n: usize| {
                zaparoo_app::format::count(i64::try_from(n).unwrap_or(i64::MAX), &language)
            };
            (
                crate::AppCue::LoadingProgress,
                count(model.rows.len().min(total)),
                count(total),
            )
        }
        _ => (crate::AppCue::LoadingMore, String::new(), String::new()),
    }
}

/// Keep the header's loading cue in step with what the list is waiting
/// on. A reload in place holds it for the whole reload. Otherwise a
/// follow-up fetch does: the cue starts with the fetch, follows a walk's
/// progress from chunk to chunk (one wait, so its delay is not restarted
/// by each chunk), and ends when nothing more is in flight. The rows and
/// the selection stay as they are throughout.
fn sync_waits(ctx: &Ctx, app: &App) {
    // A reload in place: the old rows stay up, so the cue belongs in the
    // header, worded from the screen it reloads. A route has its own cue.
    let (refilling, refill_wait) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        (
            model.loading
                && !model.rows.is_empty()
                && model.folder_direction == 0
                && !app.global::<crate::Shell>().get_transitioning(),
            model.refill_wait,
        )
    };
    match (refilling, refill_wait) {
        (true, None) => {
            let wait = crate::cue::begin(ctx, app, crate::AppCue::LoadingList, "", "");
            lock(&ctx.shared).games.refill_wait = Some(wait);
        }
        (false, Some(wait)) => {
            lock(&ctx.shared).games.refill_wait = None;
            crate::cue::end(ctx, app, wait);
        }
        _ => {}
    }
    if refilling {
        return;
    }
    let (loading, wait, text) = {
        let shared = lock(&ctx.shared);
        (
            shared.games.loading_more || shared.games.fetch_follows,
            shared.games.page_wait,
            page_wait_text(&shared),
        )
    };
    let (cue, arg, arg2) = text;
    match (loading, wait) {
        (true, None) => {
            let wait = crate::cue::begin(ctx, app, cue, &arg, &arg2);
            lock(&ctx.shared).games.page_wait = Some(wait);
        }
        (true, Some(wait)) => crate::cue::retext(ctx, app, wait, cue, &arg, &arg2),
        (false, Some(wait)) => {
            lock(&ctx.shared).games.page_wait = None;
            crate::cue::end(ctx, app, wait);
        }
        (false, None) => {}
    }
}

/// Whether the detailed list must hold its artwork demand for this draw:
/// the selection moved and has not rested yet. Starts the timer that
/// redraws with the demand once it has. A fast scroll asks for nothing
/// either way and ends already rested (`set_rapid`).
fn hold_list_covers(ctx: &Ctx, app: &App) -> bool {
    let step = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        if model.rapid.active || model.rows.is_empty() {
            return false;
        }
        let index = model.grid.current_index();
        model.cover_settle.observe(index)
    };
    match step {
        CoverSettleStep::Request => false,
        CoverSettleStep::Hold => true,
        CoverSettleStep::Arm(seq) => {
            let ctx = ctx.clone();
            let weak = app.as_weak();
            slint::Timer::single_shot(
                Duration::from_millis(rules::DETAIL_DEBOUNCE_MS),
                move || {
                    let rested = lock(&ctx.shared).games.cover_settle.fire(seq);
                    if rested {
                        if let Some(app) = weak.upgrade() {
                            render(&ctx, &app);
                        }
                    }
                },
            );
            true
        }
    }
}

fn wanted_detail_covers(ctx: &Ctx, model: &GamesModel, tier: CoverSize) -> Vec<MediaKey> {
    let mut keys = cover_keys(
        model,
        rules::detail_prefetch_rows(model.rows.len(), model.grid.current_index()),
        tier,
    );
    // A stepped-to image leads; the row's cover stays wanted for the step
    // back to it.
    if let Some(key) = model
        .current()
        .map(|row| detail_key(ctx, model, row, tier))
        .filter(|key| key.image_type.is_some())
    {
        keys.insert(0, key);
    }
    keys
}

/// The selected row's image order and the pane's place in it. Empty
/// unless Left and Right step images and Core's metadata for the row has
/// arrived.
fn detail_images(ctx: &Ctx, model: &GamesModel) -> (usize, Vec<String>) {
    if !model.list_images {
        return (0, Vec::new());
    }
    let Some(row) = model.current() else {
        return (0, Vec::new());
    };
    let identity = row.identity(&model.system_id);
    let types = detail_cache()
        .image_types(&identity)
        .map(|types| rules::detail_image_order(types, &ctx.media.preferred_image_type()))
        .unwrap_or_default();
    let index = if model.detail_image.0 == identity {
        model.detail_image.1.min(types.len().saturating_sub(1))
    } else {
        0
    };
    (index, types)
}

/// The key the detail pane shows for `row`: its cover, or the image type
/// the pane was stepped to. The first image is the cover itself, asked for
/// as browsing asks, so it is never fetched twice, unless a plain cover
/// request cannot return it (a game with only a map and fanart).
fn detail_key(ctx: &Ctx, model: &GamesModel, row: &GameRow, tier: CoverSize) -> MediaKey {
    let key = media_key(row, &model.system_id, tier);
    let (index, types) = detail_images(ctx, model);
    match types.get(index) {
        Some(kind)
            if index > 0 || !rules::plain_cover_serves(kind, &ctx.media.preferred_image_type()) =>
        {
            MediaKey {
                image_type: Some(kind.clone()),
                ..key
            }
        }
        _ => key,
    }
}

/// Left or Right in the list: a step through the pane's images when set
/// to, else a page as L and R turn. Text only has no pane.
fn list_left_right(ctx: &Ctx, app: &App, dir: i64, ready: bool, list_page: i64) {
    let images = {
        let shared = lock(&ctx.shared);
        shared.games.list_images && !list_only_layout(&shared)
    };
    if images {
        step_detail_image(ctx, app, dir);
    } else if ready {
        list_move(ctx, app, dir * list_page);
    }
}

/// Steps the detail pane's image. False when there is nowhere to go, so
/// the press does nothing.
fn step_detail_image(ctx: &Ctx, app: &App, delta: i64) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let (index, types) = detail_images(ctx, &shared.games);
        let next = (index as i64 + delta).clamp(0, types.len().saturating_sub(1) as i64) as usize;
        if types.len() < 2 || next == index {
            return false;
        }
        let model = &mut shared.games;
        let Some(identity) = model.current().map(|row| row.identity(&model.system_id)) else {
            return false;
        };
        model.detail_image = (identity, next);
    }
    render(ctx, app);
    true
}

fn cover_keys(model: &GamesModel, indices: Vec<usize>, tier: CoverSize) -> Vec<MediaKey> {
    indices
        .into_iter()
        .filter_map(|index| model.rows.get(index))
        .filter(|row| row.wants_cover())
        .map(|row| media_key(row, &model.system_id, tier))
        .filter(|key| !key.system.is_empty() && !key.path.is_empty())
        .collect()
}

#[cfg(test)]
thread_local! {
    /// Full renders on this thread, for tests that bound repaint counts.
    pub(crate) static RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Paint the current page (or the list window), the cursor, the caption,
/// the cues and the geometry.
#[allow(
    clippy::too_many_lines,
    reason = "one setter per GamesView property keeps the inventory reviewable"
)]
pub fn render(ctx: &Ctx, app: &App) {
    #[cfg(test)]
    RENDERS.with(|count| count.set(count.get() + 1));
    // Before the early returns: a fetch retired by a route must still
    // give the header line back.
    sync_waits(ctx, app);
    // Even an offscreen render updates saved viewport positions. Wait for the
    // complete destination so a partial restore cannot overwrite its target.
    if app.global::<crate::Shell>().get_transitioning() {
        return;
    }
    {
        let shared = lock(&ctx.shared);
        // Keep the complete source, including list details, while a folder
        // fills, and the rows already on screen while their list reloads
        // in place: the header carries the cue (`sync_waits`), and the
        // reload replaces them in one step. Only a list with nothing to
        // keep falls through to the centered loading cue.
        if shared.games.loading
            && (shared.games.folder_direction != 0 || !shared.games.rows.is_empty())
        {
            return;
        }
    }
    let mode = lock(&ctx.shared).games.mode;
    let geometry = geometry(app, mode);
    let tier = cover_tier(app, mode);
    let view = app.global::<GamesView>();
    let shared = lock(&ctx.shared);
    let list = list_layout(&shared);
    let visible = list_rows_visible(ctx, app, &shared);
    let path_stack_len = shared.persist.games.path_stack.len();
    let logos = crate::systems::LogoPrefs::of(&shared);
    let model = &shared.games;
    // The grid shape follows the scene; the fetch page size follows the
    // grid (or the list window).
    let shape_changed = model.grid.columns() != geometry.columns.max(1) as usize
        || model.grid.rows() != geometry.rows.max(1) as usize;
    drop(shared);
    if shape_changed {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let current = model.grid.current_index();
        model.grid.set_shape(
            geometry.columns.max(1) as usize,
            geometry.rows.max(1) as usize,
        );
        if current < model.rows.len() {
            model.grid.set_current_index_immediate(current);
        }
    }
    let covers_held = list && hold_list_covers(ctx, app);
    let shared = lock(&ctx.shared);
    let model = &shared.games;
    let page = model.grid.current_page();
    let start = page * model.grid.page_size();
    let count = model.rows.len();

    view.set_mode(mode);
    if mode == GamesMode::Search {
        crate::search::publish_results_title(&view, &shared);
    } else if mode == GamesMode::Deck {
        let name = model.deck.as_ref().map_or("", |deck| deck.name.as_str());
        view.set_title(SharedString::from(name));
    } else {
        view.set_title(SharedString::from(rules::screen_title(
            path_stack_len,
            &model.browse_path,
            &model.system_name,
        )));
    }
    view.set_options_available(options_available(app, &shared));
    view.set_page_menu(mode != GamesMode::Recents && !(mode == GamesMode::Browse && model.live));
    view.set_loading(model.loading);
    view.set_error(SharedString::from(model.error.as_str()));
    view.set_count(i32::try_from(count).unwrap_or(0));
    view.set_total_items(i32::try_from(model.grid.total_items()).unwrap_or(0));
    view.set_total_known(model.total_known);
    view.set_total_files(i32::try_from(model.total_files).unwrap_or(0));
    view.set_filter_text(SharedString::from(
        if mode == GamesMode::Browse && !model.live {
            crate::browse_filter::summary(&shared)
        } else {
            None
        }
        .unwrap_or_default(),
    ));
    view.set_has_more(model.has_more());
    view.set_focus_ready(model.focus_armed || model.restore_done);
    view.set_rapid_active(model.rapid.active);
    publish_rail(app, &shared);
    crate::page_bar::publish_games(app, &shared, mode);

    if !model.folder_sliding {
        if view.get_folder_slide() {
            crate::folder_motion::clear(app);
        }
        if model.folder_direction == 0 {
            view.set_folder_from_cells(ModelRc::default());
        }
    }
    let strip_sliding = model.page.sliding && !view.get_cached_transition();
    if strip_sliding {
        crate::view_model::publish_cells(
            &view.get_next_cells(),
            page_cells(ctx, model, logos, page, tier),
            |rows| view.set_next_cells(rows),
        );
    } else if list {
        view.set_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    } else {
        let landed = crate::view_model::publish_cells(
            &view.get_cells(),
            page_cells(ctx, model, logos, page, tier),
            |rows| view.set_cells(rows),
        );
        let grid = crate::cover_fade::Grid {
            fit: &geometry.fit,
            insets: &geometry.insets,
            top: geometry.grid_y,
            columns: geometry.columns,
        };
        let selected = model.grid.current_index().checked_sub(start);
        let screens = [
            crate::Screen::Games,
            crate::Screen::Favorites,
            crate::Screen::Recents,
            crate::Screen::SearchResults,
            crate::Screen::Deck,
            crate::Screen::Decks,
            crate::Screen::TopPlayed,
        ];
        crate::cover_fade::request(app, &screens, &grid, &landed, selected);
    }
    if !strip_sliding {
        view.set_next_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    }
    view.set_selected_local(if strip_sliding {
        -1
    } else {
        i32::try_from(model.grid.current_index().saturating_sub(start)).unwrap_or(0)
    });
    view.set_current_index(i32::try_from(model.grid.current_index()).unwrap_or(0));
    view.set_columns(geometry.columns);
    view.set_rows(geometry.rows);
    view.set_cell_width(geometry.fit.cell_width as f32);
    view.set_cell_height(geometry.fit.cell_height as f32);
    view.set_block_offset_x(geometry.fit.block_offset_x as f32);
    view.set_block_offset_y(geometry.fit.block_offset_y as f32);
    view.set_grid_y(geometry.grid_y as f32);
    view.set_grid_height(geometry.grid_height as f32);
    view.set_label_y(geometry.label_y as f32);
    view.set_label_height(geometry.label_height as f32);
    view.set_page(i32::try_from(page).unwrap_or(0));
    view.set_total_pages(i32::try_from(model.grid.total_page_count()).unwrap_or(1));
    view.set_has_pages_above(model.grid.has_pages_above());
    view.set_has_pages_below(model.grid.has_pages_below());
    view.set_activate_pulse(model.activate_pulse);
    view.set_release_pulse(model.release_pulse);
    view.set_info(info_data(&info_line(model.current())));

    // Detailed list: the window around the centered slot.
    let paging = rules::list_paging(
        model.grid.current_index(),
        count,
        model.known_total(),
        model.total_known,
        visible,
        model.has_more(),
    );
    view.set_list_visible(i32::try_from(visible).unwrap_or(10));
    view.set_list_page(i32::try_from(paging.current_page).unwrap_or(0));
    view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
    view.set_has_items_above(paging.has_items_above);
    view.set_has_items_below(paging.has_items_below);
    if list {
        let list_geometry = list_geometry(ctx, app, &shared);
        view.set_list_row_height(list_geometry.row_height as f32);
        let previous = saved_list_top(&shared).unwrap_or_else(|| {
            rules::list_view_top(model.grid.current_index(), count, visible, None)
        });
        crate::browse_motion::publish_list(
            &view,
            &shared.input,
            model.grid.current_index(),
            count,
            visible,
            previous,
            &model.rows,
            text_cell,
        );
    } else {
        view.set_list_rows(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    }

    let first_visible = if list {
        view.get_list_scroll_top().max(0) as usize
    } else {
        start
    };
    let window = if list {
        visible
    } else {
        model.grid.page_size()
    };
    let (art_start, art_window, art_tier) = if list {
        let detail_tier = CoverSize::detail(app);
        let wanted = if covers_held {
            Vec::new()
        } else {
            wanted_detail_covers(ctx, model, detail_tier)
        };
        request_covers(ctx, model, wanted);
        refresh_detail_cover(ctx, app, model);
        (model.grid.current_index(), 1, detail_tier)
    } else {
        request_covers(
            ctx,
            model,
            wanted_covers(ctx, model, first_visible, window, tier),
        );
        (first_visible, window, tier)
    };
    report_background(ctx, model, art_tier);
    if matches!(
        app.global::<crate::Shell>().get_active_screen(),
        crate::Screen::Games
            | crate::Screen::Favorites
            | crate::Screen::Recents
            | crate::Screen::SearchResults
            | crate::Screen::Deck
            | crate::Screen::Decks
            | crate::Screen::TopPlayed
    ) {
        let bounds = zaparoo_app::logo_cache::Bounds::new(art_tier.tier, art_tier.tier);
        let key = |row: &GameRow| {
            if model.mode == GamesMode::Browse
                || model.rapid.active
                || (row.has_cover
                    && !ctx
                        .media
                        .is_negative(&media_key(row, &model.system_id, art_tier)))
            {
                return None;
            }
            let id = row.system_or(&model.system_id);
            logos.key(ctx, id, bounds)
        };
        let visible = model
            .rows
            .iter()
            .skip(art_start)
            .take(art_window)
            .filter_map(key);
        let neighbors = [
            art_start.saturating_add(art_window),
            art_start.saturating_sub(art_window),
        ]
        .into_iter()
        .flat_map(|first| model.rows.iter().skip(first).take(art_window))
        .filter_map(key);
        ctx.logos.request_window(visible, neighbors);
    } else {
        crate::system_logos::defer_refresh(ctx, app);
    }
    if crate::perf::enabled() {
        let visible_rows = model.rows.iter().skip(art_start).take(art_window);
        let (visible, pending) = visible_rows.fold((0, 0), |(visible, pending), row| {
            let waiting = cover_waiting(ctx, model, row, art_tier);
            (visible + 1, pending + usize::from(waiting))
        });
        crate::perf::games_rendered(count, model.loading, visible, pending);
    }
    // A replacement temporarily seats row zero while old rows may still be
    // painted. That transient viewport must not overwrite the restore target.
    let stable_viewport = !model.loading && model.pending_restore_path.is_empty();
    drop(shared);
    if list && stable_viewport {
        remember_list_top(
            &mut lock(&ctx.shared),
            view.get_list_scroll_top().max(0) as usize,
        );
    }
}

/// The detail pane's identity fields for the focused row: title, path and
/// the cached cover (misses stream in through `covers_landed`).
fn detail_row(key: &str, value: &str) -> crate::DetailRow {
    crate::DetailRow {
        key: SharedString::from(key),
        value: SharedString::from(value),
    }
}

/// The metadata rows for a row: the flat lists lead with the system.
fn detail_rows_for(
    model: &GamesModel,
    row: &GameRow,
    rows: &[(&str, String)],
) -> Vec<crate::DetailRow> {
    let mut out = Vec::with_capacity(rows.len() + 1);
    if model.mode != GamesMode::Browse {
        let name = row.system_name.trim();
        let system = if name.is_empty() {
            row.system_id.as_str()
        } else {
            name
        };
        if !system.is_empty() {
            out.push(detail_row("system", system));
        }
    }
    out.extend(rows.iter().map(|(key, value)| detail_row(key, value)));
    out
}

fn refresh_detail_cover(ctx: &Ctx, app: &App, model: &GamesModel) {
    let view = app.global::<GamesView>();
    let Some(row) = model.current() else {
        view.set_detail_title(SharedString::default());
        view.set_detail_path(SharedString::default());
        view.set_detail_image_count(0);
        view.set_detail_image_index(0);
        view.set_detail_image_kind(SharedString::default());
        view.set_detail_has_cover(false);
        view.set_detail_cover_absent(false);
        view.set_detail_has_placeholder(false);
        return;
    };
    view.set_detail_title(SharedString::from(row.display.as_str()));
    let (index, types) = detail_images(ctx, model);
    let count = if types.len() > 1 { types.len() } else { 0 };
    view.set_detail_image_count(i32::try_from(count).unwrap_or(0));
    view.set_detail_image_index(i32::try_from(index).unwrap_or(0));
    view.set_detail_image_kind(
        types
            .get(index)
            .filter(|_| count > 0)
            .map(String::as_str)
            .unwrap_or_default()
            .into(),
    );
    let key = detail_key(ctx, model, row, CoverSize::detail(app));
    let placeholder =
        placeholder_color(ctx, row, &key).filter(|_| row.media_capable && row.has_cover);
    let folder = !row.media_capable;
    view.set_detail_placeholder(placeholder.unwrap_or_default());
    view.set_detail_has_placeholder(placeholder.is_some());
    view.set_detail_path(SharedString::from(row.path.as_str()));
    if !row.wants_cover() || key.system.is_empty() {
        view.set_detail_has_cover(false);
        view.set_detail_cover_absent(!row.is_dir());
        return;
    }
    if let Some(image) = ctx.media.get(&key) {
        view.set_detail_cover(slint::Image::from_rgba8(image.buffer));
        view.set_detail_has_cover(true);
        view.set_detail_cover_absent(false);
    } else {
        view.set_detail_has_cover(false);
        // A folder without art of its own keeps its plain pane.
        view.set_detail_cover_absent(!folder && ctx.media.is_negative(&key));
        // render owns the cancellable detail demand, including its lookahead.
        // Do not enqueue here: that would keep obsolete selections alive.
    }
}

/// Repaint once for a visible grid cover/color preview or the focused list
/// detail cover. Hidden list-row artwork and neighbor-only arrivals do no work.
pub fn covers_landed(ctx: &Ctx, app: &App, keys: &[MediaKey]) {
    if crate::navigation::retaining(
        app,
        &[
            crate::Screen::Games,
            crate::Screen::Favorites,
            crate::Screen::Recents,
            crate::Screen::SearchResults,
            crate::Screen::Deck,
            crate::Screen::Decks,
            crate::Screen::TopPlayed,
        ],
    ) {
        return;
    }
    let relevant = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let list = list_layout(&shared);
        let (first, window, tier) = if list {
            (model.grid.current_index(), 1, CoverSize::detail(app))
        } else {
            (
                model.grid.current_page() * model.grid.page_size(),
                model.grid.page_size(),
                cover_tier(app, model.mode),
            )
        };
        model.rows.iter().skip(first).take(window).any(|row| {
            let key = media_key(row, &model.system_id, tier);
            keys.contains(&key)
                || (list && keys.contains(&detail_key(ctx, model, row, tier)))
                || background_game_key(model).is_some_and(|bg| keys.contains(&bg))
                || (!list
                    && row.has_cover
                    && row.cover_color.is_none()
                    && keys.contains(&key.color_preview()))
        })
    };
    if relevant {
        render(ctx, app);
    }
}

/// Settings changed the naming or the count language: recompute the
/// display strings and repaint.
pub fn reproject(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        refresh_display(&mut shared);
    }
    render(ctx, app);
}

/// The browse layout setting flipped: the cursor carries over; the list
/// keeps a screenful loaded.
pub fn on_layout_changed(ctx: &Ctx, app: &App) {
    let fill = {
        let shared = lock(&ctx.shared);
        let visible = list_rows_visible(ctx, app, &shared);
        list_layout(&shared)
            && rules::list_fill_page(shared.games.rows.len(), visible, shared.games.has_more())
    };
    render(ctx, app);
    schedule_detail(ctx, app, true);
    if fill {
        let size = {
            let shared = lock(&ctx.shared);
            page_size(ctx, app, &shared)
        };
        fetch_more(ctx, app, size, false);
    }
}

// ---------- Selection persistence ----------

fn schedule_persist(ctx: &Ctx, path: &str) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        if !model.persist.schedule(path) {
            return;
        }
        model.persist_seq += 1;
        model.persist_seq
    };
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        Duration::from_millis(rules::PERSIST_DEBOUNCE_MS),
        move || {
            if lock(&ctx.shared).games.persist_seq == seq {
                flush_persist(&ctx);
            }
        },
    );
}

/// Commit a scheduled selection write now (Accept, Back, hold release).
pub fn flush_persist(ctx: &Ctx) {
    let path = {
        let mut shared = lock(&ctx.shared);
        shared.games.persist_seq += 1;
        shared.games.persist.flush()
    };
    if let Some(path) = path {
        {
            let mut shared = lock(&ctx.shared);
            write_saved_path(&mut shared, path);
        }
        crate::router::save_persist(&ctx.shared);
    }
}

/// Persist the current row synchronously (restore landings, launches).
fn persist_now(ctx: &Ctx) {
    let path = {
        let mut shared = lock(&ctx.shared);
        shared.games.persist_seq += 1;
        shared.games.persist.discard();
        shared.games.current().map(GameRow::selection_key)
    };
    if let Some(path) = path {
        {
            let mut shared = lock(&ctx.shared);
            write_saved_path(&mut shared, path);
        }
        crate::router::save_persist(&ctx.shared);
    }
}

fn persist_current(ctx: &Ctx) {
    let path = lock(&ctx.shared)
        .games
        .current()
        .map(GameRow::selection_key);
    if let Some(path) = path {
        schedule_persist(ctx, &path);
    }
}

// ---------- Focused detail (list layout) ----------

/// The selection, count or layout changed: peek the row's identity now,
/// load its metadata after the debounce.
fn schedule_detail(ctx: &Ctx, app: &App, force: bool) {
    if crate::navigation::retaining(
        app,
        &[
            crate::Screen::Games,
            crate::Screen::Favorites,
            crate::Screen::Recents,
            crate::Screen::SearchResults,
            crate::Screen::Deck,
            crate::Screen::Decks,
            crate::Screen::TopPlayed,
        ],
    ) {
        return;
    }
    let (steps, seq) = {
        let mut shared = lock(&ctx.shared);
        let enabled = list_layout(&shared)
            && !list_only_layout(&shared)
            && !shared.games.loading
            && shared.games.error.is_empty()
            && !app.global::<crate::Shell>().get_transitioning();
        let model = &mut shared.games;
        let index = model.grid.current_index();
        let identity = model
            .current()
            .map(|row| row.identity(&model.system_id))
            .unwrap_or_default();
        let rapid = model.rapid.active;
        let steps = if enabled {
            model.detail.schedule(true, rapid, &identity, index, force)
        } else {
            model.detail.disable(true)
        };
        model.detail_seq += 1;
        (steps, model.detail_seq)
    };
    let view = app.global::<GamesView>();
    for step in steps {
        match step {
            DetailStep::Clear => {
                view.set_detail_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
                view.set_detail_loading(false);
            }
            // The row's own metadata shows at once, so the table never
            // holds the previous row's values through the load window; what
            // Core already sent for it, when it has, so it does not blank.
            DetailStep::Peek(index) => {
                let shared = lock(&ctx.shared);
                let model = &shared.games;
                let rows = model
                    .rows
                    .get(index)
                    .map(|row| {
                        let fetched = detail_cache().get(&row.identity(&model.system_id));
                        detail_rows_for(
                            model,
                            row,
                            fetched
                                .as_ref()
                                .map_or(row.detail_rows.as_slice(), |detail| {
                                    detail.rows.as_slice()
                                }),
                        )
                    })
                    .unwrap_or_default();
                view.set_detail_rows(ModelRc::new(VecModel::from(rows)));
                refresh_detail_cover(ctx, app, model);
            }
            DetailStep::Arm => {
                view.set_detail_loading(true);
                let ctx = ctx.clone();
                let weak = app.as_weak();
                slint::Timer::single_shot(
                    Duration::from_millis(rules::DETAIL_DEBOUNCE_MS),
                    move || {
                        if let Some(app) = weak.upgrade() {
                            fire_detail(&ctx, &app, seq);
                        }
                    },
                );
            }
            DetailStep::Disarm => view.set_detail_loading(false),
        }
    }
}

/// The pane's rows from a game's full record. The same facts the details
/// screen shows (`game_info_data::rows`: the title's tags, or the media
/// record's own when Core sent none, then title and ROM properties, which
/// is where scraped metadata lives), cut down to the pane's fixed rows. A
/// game whose year, developer or publisher is a property and not a tag
/// would otherwise show nothing under its cover.
fn detail_from_meta(meta: &MediaMeta, path: &str) -> rules::Detail {
    rules::Detail {
        rows: rules::detail_rows_from_tags(&crate::game_info_data::rows(meta, path)),
        image_types: crate::game_info_data::image_types(meta),
    }
}

fn fire_detail(ctx: &Ctx, app: &App, seq: u64) {
    let (row, system, identity, neighbors) = {
        let mut shared = lock(&ctx.shared);
        if shared.games.detail_seq != seq {
            return;
        }
        let enabled = list_layout(&shared);
        let model = &mut shared.games;
        let identity_now = model
            .current()
            .map(|row| row.identity(&model.system_id))
            .unwrap_or_default();
        let Some(index) = model.detail.fire(enabled, &identity_now) else {
            app.global::<GamesView>().set_detail_loading(false);
            return;
        };
        let Some(row) = model.rows.get(index).cloned() else {
            app.global::<GamesView>().set_detail_loading(false);
            return;
        };
        let system = row.system_or(&model.system_id).to_string();
        let identity = row.identity(&model.system_id);
        (row, system, identity, detail_neighbors(model, index))
    };
    prefetch_details(ctx, app, neighbors);
    if row.is_dir() {
        app.global::<GamesView>().set_detail_loading(false);
        return;
    }
    // Already shown by the peek.
    if detail_cache().contains(&identity) {
        app.global::<GamesView>().set_detail_loading(false);
        return;
    }
    let Some((client, handle)) = detail_ctx() else {
        return;
    };
    let params = row.meta_params(system);
    let generation = detail_cache().generation();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    handle.spawn(async move {
        let result = client.media_meta(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let rows = {
                let shared = lock(&ctx.shared);
                if shared.games.detail_seq != seq {
                    return;
                }
                // The title-level tags describe the game; the media
                // record's own tags stand in when Core sent none.
                result.ok().map(|result| {
                    let detail = detail_from_meta(&result.media, &row.path);
                    let rows = detail_rows_for(&shared.games, &row, &detail.rows);
                    detail_cache().insert(generation, identity, detail);
                    rows
                })
            };
            let view = app.global::<GamesView>();
            let landed = rows.is_some();
            if let Some(rows) = rows {
                view.set_detail_rows(ModelRc::new(VecModel::from(rows)));
            }
            view.set_detail_loading(false);
            // The image count arrived with the metadata, and with it maybe
            // a typed image in the cover's place (a game with only a map):
            // a render asks for it, as stepping an image does, since the
            // detail demand is the render's to own.
            if landed && lock(&ctx.shared).games.list_images {
                render(&ctx, &app);
            }
        });
    });
}

/// The rows beside `index` whose covers the pane already warms
/// (`rules::detail_prefetch_rows`), with Core's metadata not yet cached:
/// each row with its system and identity.
fn detail_neighbors(model: &GamesModel, index: usize) -> Vec<(GameRow, String, String)> {
    let cache = detail_cache();
    rules::detail_prefetch_rows(model.rows.len(), index)
        .into_iter()
        .filter(|&neighbor| neighbor != index)
        .filter_map(|neighbor| model.rows.get(neighbor))
        .filter(|row| !row.is_dir())
        .filter_map(|row| {
            let identity = row.identity(&model.system_id);
            (!identity.is_empty() && !cache.contains(&identity)).then(|| {
                let system = row.system_or(&model.system_id).to_string();
                (row.clone(), system, identity)
            })
        })
        .collect()
}

/// Fetches the neighbors' metadata one at a time into the cache, so a
/// step onto one shows it at once. One that the selection has reached by
/// the time it lands is shown then.
fn prefetch_details(ctx: &Ctx, app: &App, rows: Vec<(GameRow, String, String)>) {
    if rows.is_empty() {
        return;
    }
    let Some((client, handle)) = detail_ctx() else {
        return;
    };
    let weak = app.as_weak();
    let ctx = ctx.clone();
    handle.spawn(async move {
        for (row, system, identity) in rows {
            let params = row.meta_params(system);
            let generation = detail_cache().generation();
            let Ok(result) = client.media_meta(params).await else {
                continue;
            };
            let detail = detail_from_meta(&result.media, &row.path);
            let pairs = detail.rows.clone();
            detail_cache().insert(generation, identity.clone(), detail);
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let shared = lock(&ctx.shared);
                let model = &shared.games;
                let current = model
                    .current()
                    .filter(|current| current.identity(&model.system_id) == identity);
                if let Some(current) = current {
                    let rows = detail_rows_for(model, current, &pairs);
                    app.global::<GamesView>()
                        .set_detail_rows(ModelRc::new(VecModel::from(rows)));
                    if model.list_images {
                        refresh_detail_cover(&ctx, &app, model);
                    }
                }
            });
        }
    });
}

// ---------- Input ----------

/// The games grids on the shared page turn.
pub(crate) struct GamesScreen;

impl crate::page_motion::PagedScreen for GamesScreen {
    type View<'a> = GamesView<'a>;
    fn view(app: &App) -> GamesView<'_> {
        app.global::<GamesView>()
    }
    fn motion(shared: &mut Shared) -> &mut crate::page_motion::PageMotion {
        &mut shared.games.page
    }
    fn render(ctx: &Ctx, app: &App) {
        render(ctx, app);
    }
    fn on_screen(app: &App) -> bool {
        matches!(
            app.global::<crate::Shell>().get_active_screen(),
            crate::Screen::Games
                | crate::Screen::Favorites
                | crate::Screen::Recents
                | crate::Screen::SearchResults
                | crate::Screen::Deck
                | crate::Screen::Decks
                | crate::Screen::TopPlayed
        )
    }
    fn list_layout(app: &App) -> bool {
        app.global::<crate::Shell>().get_browse_list_layout()
    }
    /// A new fill replaces the rows a turn was showing.
    fn epoch(shared: &Shared) -> u64 {
        shared.games.ticket
    }
}

/// Turn to the grid's current page in its committed direction, including
/// wrapped and deferred turns.
fn slide_to_current_page(ctx: &Ctx, app: &App) {
    let (direction, local) = {
        let shared = lock(&ctx.shared);
        let grid = &shared.games.grid;
        (
            grid.page_direction().offset(),
            grid.current_index() - grid.current_page() * grid.page_size(),
        )
    };
    crate::page_motion::turn::<GamesScreen>(ctx, app, direction, local, true);
}

pub(crate) fn interrupt_page(ctx: &Ctx, app: &App) {
    crate::page_motion::interrupt::<GamesScreen>(ctx, app);
}

/// Rows ahead of the letter buckets' first item: the virtual roots, plus
/// every directory when the buckets index files.
fn letter_leading(shared: &Shared) -> usize {
    if shared.letter_directories {
        shared.games.root_count()
    } else {
        shared.games.non_media_total()
    }
}

/// Go to has something to jump through: a ready browse list. The L1+R1
/// chord holds a shoulder press back only where this is true.
pub(crate) fn go_to_available(shared: &Shared) -> bool {
    shared.games.mode == GamesMode::Browse
        && !shared.games.live
        && shared.games.state() == State::Ready
}

/// The letter bucket the selection is in, where the folder on screen has
/// its letters: where Go to opens.
pub(crate) fn current_letter_bucket(shared: &Shared) -> Option<usize> {
    let groups = scope_letters(shared)?;
    let offsets: Vec<u32> = groups.iter().map(|group| group.offset).collect();
    rules::letter_at(
        &offsets,
        letter_leading(shared),
        shared.games.grid.current_index(),
    )
}

/// The scope's letter buckets, when the list is browsed by title and the
/// fetched index belongs to the folder on screen.
fn scope_letters(shared: &Shared) -> Option<&[zaparoo_core::media_types::BrowseIndexGroup]> {
    let model = &shared.games;
    if model.mode != GamesMode::Browse || shared.letter_buckets.is_empty() {
        return None;
    }
    let (system, path) = shared.letter_scope.as_ref()?;
    (*system == model.system_id && *path == model.browse_path)
        .then_some(shared.letter_buckets.as_slice())
}

/// The fast-scroll rail: the scope's letters with the current one, or,
/// before the letter index has loaded, only the position marker. A list
/// with no fixed total gets no rail (`rules::rail_offered`).
fn publish_rail(app: &App, shared: &Shared) {
    let view = app.global::<GamesView>();
    let model = &shared.games;
    let index = model.grid.current_index();
    let (labels, at) = scope_letters(shared).map_or((Vec::new(), None), |groups| {
        let offsets: Vec<u32> = groups.iter().map(|group| group.offset).collect();
        (
            groups
                .iter()
                .map(|group| SharedString::from(group.label.as_str()))
                .collect::<Vec<_>>(),
            rules::letter_at(&offsets, letter_leading(shared), index),
        )
    });
    // The letters change per folder, not per move: keep the model (and
    // the rail's text items) across moves.
    let current = view.get_rail_letters();
    let same = slint::Model::row_count(&current) == labels.len()
        && labels
            .iter()
            .enumerate()
            .all(|(i, label)| slint::Model::row_data(&current, i).as_ref() == Some(label));
    view.set_rail_index(at.map_or(-1, |i| i32::try_from(i).unwrap_or(-1)));
    view.set_rail_letter(at.and_then(|i| labels.get(i).cloned()).unwrap_or_default());
    if !same {
        view.set_rail_letters(ModelRc::new(VecModel::from(labels)));
    }
    let total = model.grid.total_items().max(model.rows.len());
    view.set_rail_fraction(rules::rail_fraction(index, total));
    view.set_rail_visible(model.rapid.rail_visible && rules::rail_offered(model.known_total()));
}

/// A held page flip stops at the list's ends: a tapped one wraps around,
/// but a held one wrapping sends the user away from the end they were
/// heading for. That holds from the first repeat, before the hold counts
/// as a fast scroll.
fn at_rapid_edge(ctx: &Ctx, dir: i64) -> bool {
    let shared = lock(&ctx.shared);
    let grid = &shared.games.grid;
    if dir < 0 {
        grid.current_page() == 0
    } else {
        grid.pagination_total_known && grid.current_page() + 1 >= grid.total_page_count()
    }
}

/// A long hold's letter step: jump to the next or previous letter. True
/// when the hold is handled here, including while an earlier step's jump
/// is still loading; false where there are no letters or no further
/// letter, so the hold keeps paging.
fn letter_step(ctx: &Ctx, app: &App, forward: bool) -> bool {
    let target = {
        let shared = lock(&ctx.shared);
        let Some(groups) = scope_letters(&shared) else {
            return false;
        };
        let model = &shared.games;
        if model.jump_loading {
            return true;
        }
        let offsets: Vec<u32> = groups.iter().map(|group| group.offset).collect();
        let Some(bucket) = rules::letter_step(
            &offsets,
            letter_leading(&shared),
            model.grid.current_index(),
            forward,
        ) else {
            return false;
        };
        offsets[bucket]
    };
    jump_to_item(ctx, app, target);
    true
}

/// Held rapid navigation: covers pause and the detail pane stops loading
/// while it runs; the rail comes up with it and stays `RAIL_LINGER_MS`
/// after it ends.
pub fn set_rapid(ctx: &Ctx, app: &App, active: bool) {
    let (changed, linger) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let (changed, linger) = model.rapid.set(active);
        if changed && !active {
            // The quiet period that ended the scroll was the rest.
            let index = model.grid.current_index();
            model.cover_settle.settle_now(index);
        }
        (changed, linger)
    };
    if let Some(seq) = linger {
        crate::browse_motion::linger_rail(
            ctx,
            app,
            seq,
            |shared| &mut shared.games.rapid,
            |app| app.global::<GamesView>().set_rail_visible(false),
        );
    }
    if !changed {
        return;
    }
    crate::perf::scroll(active);
    render(ctx, app);
    schedule_detail(ctx, app, false);
}

/// The screen's input gate.
fn gate_hide(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_transitioning() {
        return true;
    }
    let shared = lock(&ctx.shared);
    shared.games.loading || !shared.games.error.is_empty()
}

fn grid_move(ctx: &Ctx, app: &App, d_col: i32, d_row: i32, page_delta: i32) {
    let (moved, from_page, to_page) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        if model.page.sliding {
            return;
        }
        model.focus_armed = true;
        let from_page = model.grid.current_page();
        let moved = if page_delta != 0 {
            model.grid.page_by(page_delta)
        } else {
            model.grid.move_selection(d_col, d_row)
        };
        (moved, from_page, model.grid.current_page())
    };
    if moved {
        persist_current(ctx);
        if to_page != from_page {
            slide_to_current_page(ctx, app);
            drain_load_requests(ctx, app);
            return;
        }
    }
    render(ctx, app);
    drain_load_requests(ctx, app);
}

fn list_move(ctx: &Ctx, app: &App, delta: i64) {
    // A held page move stops at the list's ends, as a held grid page turn
    // does; a fresh press wraps.
    let held = rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let (outcome, size, tail, tail_target) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared);
        let visible = list_rows_visible(ctx, app, &shared);
        let model = &mut shared.games;
        model.focus_armed = true;
        model.detail_image = (String::new(), 0);
        let count = model.rows.len();
        let has_more = rules::list_has_more(count, model.known_total(), model.has_more());
        let mut outcome =
            rules::linear_move(model.grid.current_index(), count, delta, has_more, held);
        let tail_target = model.known_total().map(|total| total.saturating_sub(1));
        if outcome == LinearMove::WrapToTail && tail_target.is_none() {
            // No total to aim at: the last loaded row is the best tail known.
            outcome = LinearMove::To {
                index: count.saturating_sub(1),
                fetch: has_more,
            };
        }
        let mut tail = false;
        if let LinearMove::To { index, .. } = outcome {
            model.grid.set_current_index_immediate(index);
            let lookahead = rules::list_lookahead(delta, visible);
            tail = rules::list_tail_prefetch(index, count, lookahead, has_more, model.loading_more);
        }
        (outcome, size, tail, tail_target)
    };
    let limit = rules::list_fetch_limit(delta, size);
    match outcome {
        LinearMove::To { fetch, .. } => {
            persist_current(ctx);
            render(ctx, app);
            schedule_detail(ctx, app, false);
            if fetch || tail {
                fetch_more(ctx, app, limit, false);
            }
        }
        LinearMove::FetchMore => fetch_more(ctx, app, limit, false),
        LinearMove::Stay { fetch } => {
            if fetch {
                fetch_more(ctx, app, limit, false);
            }
        }
        LinearMove::WrapToTail => {
            if let Some(target) = tail_target {
                jump_to_index(ctx, app, target);
            }
        }
    }
}

/// The action handler shared by Games, Favorites and Recently played.
/// Menu on a list below 240p's chip row: the page menu its mode has.
fn open_page_menu(ctx: &Ctx, app: &App, mode: GamesMode) {
    match mode {
        // A live folder has nothing to jump through, filter or search.
        GamesMode::Browse if lock(&ctx.shared).games.live => {}
        GamesMode::Browse => crate::router::open_view_menu(ctx, app),
        GamesMode::Favorites => crate::router::open_favorites_page_menu(ctx, app),
        GamesMode::Search => crate::router::open_search_results_page_menu(ctx, app),
        GamesMode::Deck => crate::decks::open_page_menu(ctx, app),
        GamesMode::Decks => crate::decks::open_list_menu(ctx, app),
        GamesMode::TopPlayed => crate::router::open_top_played_page_menu(ctx, app),
        GamesMode::Recents => {}
    }
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    interrupt_page(ctx, app);
    if crate::decks::moving(ctx) {
        return crate::decks::move_action(ctx, app, action);
    }
    let is_move = matches!(
        action,
        actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN | actions::CONTEXT_MENU
    );
    if is_move && gate_hide(ctx, app) {
        tracing::debug!(action, "move dropped: the list is loading or failed");
        return;
    }
    let (list, state, mode) = {
        let mut shared = lock(&ctx.shared);
        shared.games.focus_armed = true;
        (
            list_layout(&shared),
            shared.games.state(),
            shared.games.mode,
        )
    };
    let list_page = i64::try_from(lock(&ctx.shared).grid_or_list_page(ctx, app)).unwrap_or(10);
    match action {
        actions::LEFT | actions::RIGHT => {
            let dir: i64 = if action == actions::LEFT { -1 } else { 1 };
            if list {
                list_left_right(ctx, app, dir, state == State::Ready, list_page);
            } else {
                grid_move(ctx, app, dir as i32, 0, 0);
            }
        }
        actions::UP | actions::DOWN => {
            let dir: i64 = if action == actions::UP { -1 } else { 1 };
            let ready = state == State::Ready;
            // A long hold speeds up: a letter per step, else a page.
            if ready && crate::input::letter_step(ctx) && letter_step(ctx, app, dir > 0) {
                return;
            }
            let step = if ready && crate::input::rapid_page(ctx) {
                if list {
                    list_page
                } else {
                    if !at_rapid_edge(ctx, dir) {
                        grid_move(ctx, app, 0, 0, dir as i32);
                    }
                    return;
                }
            } else {
                1
            };
            if list {
                list_move(ctx, app, dir * step);
            } else {
                grid_move(ctx, app, 0, dir as i32, 0);
            }
        }
        actions::PAGE_PREV | actions::PAGE_NEXT => {
            if state != State::Ready {
                return;
            }
            let dir: i64 = if action == actions::PAGE_PREV { -1 } else { 1 };
            if crate::input::letter_step(ctx) && letter_step(ctx, app, dir > 0) {
                return;
            }
            if list {
                list_move(ctx, app, dir * list_page);
            } else if !(crate::input::dispatching_repeat(ctx) && at_rapid_edge(ctx, dir)) {
                grid_move(ctx, app, 0, 0, dir as i32);
            }
        }
        actions::PAGE_MENU => {
            // Favorites scope can produce an empty folder: keep the Menu
            // reachable so the user can clear the filter.
            if !rules::page_menu_allowed(state, true) {
                return;
            }
            // Above 240p the page actions are a chip row on the page.
            if crate::page_bar::enter(ctx, app) {
                return;
            }
            open_page_menu(ctx, app, mode);
        }
        actions::ACCEPT => match state {
            State::Loading => {}
            State::Error | State::Empty => retry(ctx, app),
            State::Ready => accept_current(ctx, app),
        },
        actions::CONTEXT_MENU => {
            if state == State::Ready {
                flush_persist(ctx);
                open_context_menu(ctx, app);
            }
        }
        actions::CANCEL => cancel(ctx, app),
        _ => {}
    }
}

impl Shared {
    /// The list's own page size in list layout, the grid's otherwise.
    fn grid_or_list_page(&self, ctx: &Ctx, app: &App) -> usize {
        page_size(ctx, app, self) as usize
    }
}

/// Accept on Error or Empty: re-run the current scope's fill.
fn retry(ctx: &Ctx, app: &App) {
    let (mode, top) = {
        let shared = lock(&ctx.shared);
        (
            shared.games.mode,
            shared
                .persist
                .games
                .path_stack
                .last()
                .cloned()
                .unwrap_or_default(),
        )
    };
    match mode {
        GamesMode::Browse => browse(ctx, app, &top, false),
        GamesMode::Favorites
        | GamesMode::Recents
        | GamesMode::Search
        | GamesMode::Deck
        | GamesMode::Decks
        | GamesMode::TopPlayed => {
            enter_flat(ctx, app, mode, false);
        }
    }
}

pub(crate) fn press_duration(app: &App) -> u64 {
    if app.global::<crate::Motion>().get_enabled() {
        34
    } else {
        0
    }
}

/// Dispatch the selected row after the router's grid push. List feedback
/// retires locally without a navigation delay.
fn accept_current(ctx: &Ctx, app: &App) {
    let list_feedback = app.global::<crate::Shell>().get_browse_list_layout();
    let (row, seq, live) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let Some(row) = model.current().cloned() else {
            return;
        };
        model.press_seq += 1;
        if list_feedback {
            model.activate_pulse += 1;
        }
        let live = model.live && model.mode == GamesMode::Browse;
        (row, model.press_seq, live)
    };
    persist_now(ctx);
    render(ctx, app);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    if row.is_dir() {
        navigate_into_folder(ctx, app, &row.path);
        return;
    }
    if let Some(deck) = &row.deck_entry {
        crate::decks::open_deck(ctx, app, &deck.deck_id, false);
        return;
    }
    if let Some(item) = &row.deck_item {
        // Inside a deck, an item plays the deck from there.
        crate::decks::play_from(ctx, app, item.position, &row.display);
    } else if let Some(text) = row.launch_text_from(live) {
        crate::router::launch(ctx, app, text, &row.display);
    }
    if !list_feedback {
        return;
    }
    slint::Timer::single_shot(Duration::from_millis(press_duration(app)), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        {
            let mut shared = lock(&ctx2.shared);
            if shared.games.press_seq != seq {
                return;
            }
            shared.games.release_pulse += 1;
        }
        render(&ctx2, &app);
    });
}

/// Stage the folder stack for the request. Disk retains the coherent source
/// until the destination rows and restored selection are ready.
fn navigate_into_folder(ctx: &Ctx, app: &App, path: &str) {
    if path.is_empty() {
        return;
    }
    crate::perf::open_pressed("folder");
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let levels = shared.persist.games.path_stack.len();
        shared.persist.games.list_top_at_level.resize(levels, 0);
        shared.persist.games.list_top_at_level.push(0);
        shared.persist.games.path_stack.push(path.to_string());
        shared.persist.games.selected_at_level.push(String::new());
        // Cut the accept flash before the rows swap.
        shared.games.release_pulse += 1;
    }
    browse_with_motion(ctx, app, path, false, 1);
}

/// Pop one level and re-browse the parent. False at the root.
fn navigate_out_of_folder(ctx: &Ctx, app: &App) -> bool {
    if !rules::at_folder_level(lock(&ctx.shared).persist.games.path_stack.len()) {
        return false;
    }
    crate::navigation::stage(ctx, app);
    let target = {
        let mut shared = lock(&ctx.shared);
        if !rules::at_folder_level(shared.persist.games.path_stack.len()) {
            return false;
        }
        shared.persist.games.path_stack.pop();
        shared.persist.games.selected_at_level.pop();
        let levels = shared.persist.games.path_stack.len();
        shared.persist.games.list_top_at_level.truncate(levels);
        shared
            .persist
            .games
            .path_stack
            .last()
            .cloned()
            .unwrap_or_default()
    };
    browse_with_motion(ctx, app, &target, false, -1);
    true
}

/// Back: disarm a pending press, commit the selection, then climb a
/// folder or leave the screen (Systems, or the Hub for a shortcut entry
/// and the `MiSTer` Arcade bypass).
fn cancel(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.games.press_seq += 1;
        shared.games.release_pulse += 1;
    }
    flush_persist(ctx);
    let mode = lock(&ctx.shared).games.mode;
    if mode == GamesMode::Browse && navigate_out_of_folder(ctx, app) {
        return;
    }
    // A typed search steps back to the Search screen; one a Hub tile opened
    // has no Search screen behind it and leaves for the Hub below.
    // A tag search Game Info opened goes back to it.
    if mode == GamesMode::Search && crate::game_info::return_from_drill(ctx, app) {
        return;
    }
    if mode == GamesMode::Search && !lock(&ctx.shared).persist.search.from_hub {
        crate::search::return_from_results(ctx, app);
        return;
    }
    let target = {
        let mut shared = lock(&ctx.shared);
        let arcade_bypass = ctx.is_mister
            && shared.systems_model.category == ARCADE_SYSTEM_ID
            && shared.systems_model.rows.len() == 1
            && shared.games.system_id == ARCADE_SYSTEM_ID;
        // A scoped list climbs back to the systems that hold its games,
        // the screen it was entered from.
        let grouped_favorites = !shared.games.list_system.is_empty()
            && crate::user_lists::grouped(&shared, crate::user_lists::active(&shared));
        let grouped_top = !shared.games.list_system.is_empty()
            && zaparoo_app::top_played::grouped(&shared.persist.top_played.grouping);
        let target = match mode {
            GamesMode::Browse if arcade_bypass => crate::Screen::Hub,
            GamesMode::Browse if shared.persist.games.entered_from_hub => {
                shared.persist.games.entered_from_hub = false;
                crate::Screen::Hub
            }
            GamesMode::Browse => crate::Screen::Systems,
            GamesMode::Favorites if grouped_favorites => crate::Screen::FavoriteSystems,
            GamesMode::Search => {
                shared.persist.search.from_hub = false;
                crate::Screen::Hub
            }
            GamesMode::Deck if shared.persist.deck.entered_from_hub => {
                shared.persist.deck.entered_from_hub = false;
                crate::Screen::Hub
            }
            GamesMode::Deck => crate::Screen::Decks,
            GamesMode::TopPlayed if grouped_top => crate::Screen::TopPlayedSystems,
            GamesMode::Favorites | GamesMode::Recents | GamesMode::Decks | GamesMode::TopPlayed => {
                crate::Screen::Hub
            }
        };
        shared.persist.active_screen = target.token().to_string();
        target
    };
    crate::router::save_persist(&ctx.shared);
    if target == crate::Screen::FavoriteSystems {
        crate::systems::return_to_list(ctx, app);
        return;
    }
    if target == crate::Screen::Decks {
        crate::decks::return_to_decks(ctx, app);
        return;
    }
    if target == crate::Screen::TopPlayedSystems {
        crate::systems::return_to_top_played(ctx, app);
        return;
    }
    if target == crate::Screen::Hub {
        crate::router::return_to_hub(ctx, app);
    } else {
        // Publish Systems and render it in the same turn, as the Hub is.
        // Only a render of the active Systems screen asks for its logos,
        // and a grid filled unseen under a restored Games screen was built
        // before any were prepared: nothing else would repaint it.
        crate::router::transition_to_screen(app, target, -1);
        crate::systems::render(ctx, app);
    }
}

/// Jump-to-letter landing (`GamesScreen.jumpToItem`): the bucket's first
/// item sits after the rows the buckets do not count; unloaded targets
/// walk in.
pub fn jump_to_item(ctx: &Ctx, app: &App, item_offset: u32) {
    let target = {
        let shared = lock(&ctx.shared);
        rules::jump_target(letter_leading(&shared), item_offset as usize)
    };
    jump_to_index(ctx, app, target);
}

/// Land on an absolute row of the whole list, walking in the rows before
/// it when it is not loaded yet.
fn jump_to_index(ctx: &Ctx, app: &App, target: usize) {
    let landed = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        model.focus_armed = true;
        let landed = model.grid.jump_to_index(target);
        model.jump_loading = !landed;
        landed
    };
    if landed {
        persist_now(ctx);
    }
    render(ctx, app);
    schedule_detail(ctx, app, false);
    drain_load_requests(ctx, app);
}

// ---------- Options menu ----------

fn cell_anchor(ctx: &Ctx, app: &App) -> crate::router::ContextAnchor {
    let shared = lock(&ctx.shared);
    let model = &shared.games;
    let list = list_layout(&shared);
    if list {
        let g = list_geometry(ctx, app, &shared);
        let layout = app.global::<crate::Layout>();
        let top = app.global::<GamesView>().get_list_scroll_top().max(0) as usize;
        let local = model.grid.current_index().saturating_sub(top) as f32;
        let row_h = g.row_height as f32;
        crate::router::ContextAnchor {
            x: (g.card_x + g.list_x) as f32 + layout.get_card_padding_left(),
            y: (g.card_y + g.list_y) as f32
                + layout.get_card_padding_top()
                + local * (row_h + layout.get_row_spacing()),
            w: g.list_width as f32
                - layout.get_card_padding_left()
                - layout.get_card_padding_right(),
            h: row_h,
            // A row's silhouette is its selection fill, not a card.
            radius: layout.get_row_radius(),
            zoomed: false,
        }
    } else {
        let geometry = geometry(app, model.mode);
        let rect = paged_grid::cell_rect(
            &geometry.fit,
            &geometry.insets,
            i32::try_from(model.grid.current_row()).unwrap_or(0),
            i32::try_from(model.grid.current_column()).unwrap_or(0),
        );
        crate::router::ContextAnchor {
            x: rect.x as f32,
            y: (geometry.grid_y + rect.y) as f32,
            w: rect.width as f32,
            h: rect.height as f32,
            radius: app.global::<crate::Layout>().get_card_radius(),
            zoomed: true,
        }
    }
}

/// What `row` can do, for its Options menu. `system` is the row's system
/// with the screen's own as the fallback; `mode` decides only how a folder
/// pins. Shared with the Hub, whose game tiles show the same menu.
pub(crate) fn game_input(
    app: &App,
    shared: &Shared,
    mode: GamesMode,
    row: &GameRow,
    system: &str,
    tile: Option<menu::Tile>,
) -> menu::GameInput {
    let pin = hub_pin(mode, row);
    let addressable = row.media_id.is_some() || (!system.is_empty() && !row.path.is_empty());
    let live = mode == GamesMode::Browse && shared.games.live;
    menu::GameInput {
        flags: (row.tags_known && addressable).then_some(row.flags),
        can_add_to_deck: row.tags_known && !system.is_empty() && !row.path.is_empty(),
        multi_disc: row.multi_disc,
        versions: row.versions() > 0,
        has_payload: !row.zap_script.trim().is_empty() || !row.path.is_empty(),
        has_nfc: shared.has_nfc,
        can_discover: zaparoo_app::alternate_versions::can_discover(system, &row.name, &row.path),
        has_launchers: shared.launchers.iter().any(|l| l.system_id == system),
        pinnable: pin.is_some(),
        on_hub: pin.is_some_and(|pin| {
            crate::hub::has_target(
                shared,
                pin.kind,
                "",
                &pin.path,
                &pin.relative,
                &pin.script,
                system,
            )
        }),
        hideable: row.tags_known && addressable,
        is_hidden: row.is_hidden,
        has_system: !system.is_empty(),
        media_busy: crate::router::media_busy(app),
        tile,
        details_unknown: live && row.media_id.is_none(),
    }
}

/// The focused row's whole Options menu, empty when it gets none.
fn row_menu(app: &App, shared: &Shared, row: &GameRow) -> menu::Menu {
    if let Some(deck) = &row.deck_entry {
        return crate::decks::menu(shared, &deck.deck_id, false);
    }
    let model = &shared.games;
    let system = row.system_or(&model.system_id);
    let own = item_menu(app, shared, row, system);
    // An item of a deck the user can change adds its place in the deck.
    match &row.deck_item {
        Some(item)
            if model
                .deck
                .as_ref()
                .is_some_and(zaparoo_core::collections::Deck::editable) =>
        {
            own.with_deck_item(menu::DeckItem {
                can_be_cover: item.game,
            })
        }
        _ => own,
    }
}

/// The row's own menu, wherever it is.
fn item_menu(app: &App, shared: &Shared, row: &GameRow, system: &str) -> menu::Menu {
    let model = &shared.games;
    match rules::menu_target(row.entry_type, row.media_capable, &row.path) {
        rules::MenuTarget::None => menu::Menu::default(),
        rules::MenuTarget::Folder(_) if model.live && model.mode == GamesMode::Browse => {
            menu::live_folder(crate::hub::has_live_folder(shared, &row.path))
        }
        rules::MenuTarget::Folder(kind) => {
            let on_hub = hub_pin(model.mode, row).is_some_and(|pin| {
                crate::hub::has_target(
                    shared,
                    pin.kind,
                    "",
                    &pin.path,
                    &pin.relative,
                    &pin.script,
                    system,
                )
            });
            menu::folder(kind, on_hub, row.is_hidden)
        }
        rules::MenuTarget::Game => {
            menu::game(&game_input(app, shared, model.mode, row, system, None))
        }
    }
}

/// The focused item's info line: everything about it its tile does not
/// carry. See `zaparoo_app::info_line`.
fn info_line(row: Option<&GameRow>) -> zaparoo_app::info_line::InfoLine {
    let Some(row) = row else {
        return zaparoo_app::info_line::InfoLine::default();
    };
    let deck = row.deck_entry.as_ref();
    zaparoo_app::info_line::InfoLine {
        name: row.display.clone(),
        flags: row.flags,
        // A search result standing for several versions of one title: how
        // many, from Core's title grouping; zero for one.
        versions: u32::try_from(row.versions()).unwrap_or(0),
        play_time: row
            .play_seconds
            .map(zaparoo_app::top_played::PlayTime::from_seconds),
        owner: deck.map(|d| d.owner.clone()).unwrap_or_default(),
        locked: deck.is_some_and(|d| d.locked),
        hidden: row.is_hidden,
        item_count: deck.map(|d| d.item_count),
        tags: if deck.is_some() {
            String::new()
        } else {
            row.suffix.clone()
        },
    }
}

/// The info line as the views take it.
fn info_data(line: &zaparoo_app::info_line::InfoLine) -> crate::InfoLineData {
    let (hours, minutes) = line.play_time.map_or((0, -1), |time| {
        let (hours, minutes) = time.parts();
        (
            i32::try_from(hours).unwrap_or(i32::MAX),
            i32::try_from(minutes).unwrap_or(0),
        )
    });
    crate::InfoLineData {
        name: SharedString::from(line.name.as_str()),
        favorite: line.flags.favorite,
        liked: line.flags.liked,
        disliked: line.flags.disliked,
        play_later: line.flags.play_later,
        versions: i32::try_from(line.versions).unwrap_or(i32::MAX),
        has_play_time: line.play_time.is_some(),
        hours,
        minutes,
        owner: SharedString::from(line.owner.as_str()),
        locked: line.locked,
        hidden: line.hidden,
        item_count: line
            .item_count
            .map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX)),
        tags: SharedString::from(line.tags.as_str()),
    }
}

/// Whether Options opens a menu on the focused row, for the help bar.
fn options_available(app: &App, shared: &Shared) -> bool {
    shared
        .games
        .current()
        .is_some_and(|row| !row_menu(app, shared, row).is_empty())
}

/// Options on the focused row, for every list of games.
fn open_context_menu(ctx: &Ctx, app: &App) {
    let menu = {
        let shared = lock(&ctx.shared);
        let Some(row) = shared.games.current() else {
            return;
        };
        row_menu(app, &shared, row)
    };
    if menu.is_empty() {
        return;
    }
    let anchor = cell_anchor(ctx, app);
    crate::router::set_context_anchor(app, &anchor);
    let index = lock(&ctx.shared).games.grid.current_index();
    crate::router::present_games_context_menu(ctx, app, index, menu);
    crate::router::refresh_readers(ctx);
}

/// Where the menu sits when Back brings it back: the focused cell now.
pub(crate) fn context_anchor(ctx: &Ctx, app: &App) -> crate::router::ContextAnchor {
    cell_anchor(ctx, app)
}

/// A menu entry was accepted (the menu is already closed).
pub fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    let (row, mode, system, live) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let Some(row) = model.current().cloned() else {
            return;
        };
        let system = row.system_or(&model.system_id).to_string();
        let live = model.live && model.mode == GamesMode::Browse;
        (row, model.mode, system, live)
    };
    if let Some(deck) = &row.deck_entry {
        crate::decks::context_accept(ctx, app, id, &deck.deck_id, crate::Screen::Decks);
        return;
    }
    match id {
        "add_to_hub" if live && row.is_dir() => crate::hub::toggle_live_folder(ctx, app, &row.path),
        "toggle_hidden" => toggle_hidden(ctx, app, &row, &system),
        "add_to_hub" => add_to_hub(ctx, app, mode, &row, &system),
        "deck_use_cover" | "deck_move" | "deck_remove" => {
            crate::decks::item_action(ctx, app, id, &row, &system);
        }
        _ => game_action(ctx, app, id, &row, &system),
    }
}

/// The game actions that open their own surface and leave no list to
/// update, whichever screen the game was reached from.
pub(crate) fn game_action(ctx: &Ctx, app: &App, id: &str, row: &GameRow, system: &str) {
    match id {
        "more_info" => crate::router::open_game_info(ctx, app, row),
        "write_card" => crate::router::begin_card_write(ctx, app, row),
        "qr_code" => crate::router::open_qr_code(ctx, app, row),
        "scrape_game" if !system.is_empty() => {
            crate::router::open_scrape_setup(
                ctx,
                app,
                zaparoo_app::media_setup::Scope::Game(zaparoo_app::media_setup::GameTarget {
                    media_id: row.media_id,
                    system: system.to_string(),
                    path: row.path.clone(),
                    name: row.display.clone(),
                }),
            );
        }
        "change_launcher" if !system.is_empty() => {
            crate::launchers::open_game_picker(ctx, app, system, &row.path, row.media_id);
        }
        _ => {}
    }
}

/// "Discover alt. versions", "Choose disc" and "Choose version" turn the
/// menu to a page of its own: it stays open while Core answers. False when
/// `id` is an ordinary entry.
pub fn begin_context_page(ctx: &Ctx, app: &App, id: &str) -> bool {
    if !crate::context_page::asks_core(id) {
        return false;
    }
    let (system, row) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let Some(row) = model.current() else {
            return true;
        };
        (row.system_or(&model.system_id).to_string(), row.clone())
    };
    begin_game_page(ctx, app, id, &row, &system)
}

/// Ask Core for the page `id` opens on `row`. False when `id` opens none.
pub(crate) fn begin_game_page(ctx: &Ctx, app: &App, id: &str, row: &GameRow, system: &str) -> bool {
    use crate::context_page::Page;
    if id == Page::AddToDeck.menu_id() {
        crate::add_to_deck::begin(ctx, app, system, &row.path, &row.display);
    } else if id == Page::Alternates.menu_id() {
        crate::alternates::begin(ctx, app, system, &row.name, &row.path);
    } else if id == Page::Discs.menu_id() {
        let include_hidden = include_hidden(&lock(&ctx.shared));
        crate::discs::begin(ctx, app, system, &row.path, &row.display, include_hidden);
    } else if id == Page::Versions.menu_id() {
        let Some(title_id) = row.title_id else {
            return true;
        };
        // The search that counted the versions filters the list of them.
        let search = crate::search::args(&lock(&ctx.shared));
        crate::title_versions::begin(ctx, app, title_id, &row.display, &search);
    } else {
        return false;
    }
    true
}

/// What Add to Hub stores for a row: every identifier Core reported for
/// it, so the tile outlives the media moving to another drive.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HubPin {
    pub kind: &'static str,
    pub path: String,
    pub relative: String,
    pub script: String,
    pub name: String,
}

/// Folders and filesystem roots pin as `folder` items; games pin as
/// `zapscript` items carrying Core's title command (its path when Core sent
/// none). A media-capable directory already launches by its script, so it
/// pins without a relative path.
pub(crate) fn hub_pin(mode: GamesMode, row: &GameRow) -> Option<HubPin> {
    let folder = mode == GamesMode::Browse
        && !row.media_capable
        && (row.entry_type == EntryType::Directory
            || rules::is_filesystem_root(row.entry_type, &row.path));
    if folder {
        return (!row.path.is_empty()).then(|| HubPin {
            kind: "folder",
            path: row.path.clone(),
            relative: row.relative_path.clone(),
            script: String::new(),
            name: String::new(),
        });
    }
    let run_text = row.launch_text()?;
    let container = row.entry_type == EntryType::Directory && row.media_capable;
    Some(HubPin {
        kind: "zapscript",
        path: row.path.clone(),
        relative: if container {
            String::new()
        } else {
            row.relative_path.clone()
        },
        script: if row.zap_script.trim().is_empty() {
            run_text
        } else {
            row.zap_script.clone()
        },
        name: row.display.clone(),
    })
}

/// "Add to Hub" on a row not pinned yet, "Remove from Hub" on one that is.
pub(crate) fn add_to_hub(ctx: &Ctx, app: &App, mode: GamesMode, row: &GameRow, system: &str) {
    let Some(pin) = hub_pin(mode, row) else {
        tracing::warn!("add to hub skipped: nothing to pin for {}", row.name);
        crate::router::report_action_error(ctx, app, "add_to_hub", &row.display);
        return;
    };
    crate::hub::toggle_target(
        ctx,
        app,
        pin.kind,
        "",
        &pin.path,
        &pin.relative,
        &pin.script,
        &pin.name,
        "",
        system,
    );
}

/// A strip button on the Options menu of the list's own row.
pub(crate) fn strip_toggle(ctx: &Ctx, app: &App, flag: Flag) {
    let (row, system, index) = {
        let shared = lock(&ctx.shared);
        let index = shared.context_target;
        let Some(row) = shared.games.rows.get(index).cloned() else {
            return;
        };
        let system = row.system_or(&shared.games.system_id).to_string();
        (row, system, index)
    };
    let path = row.path.clone();
    change_flag(ctx, app, &row, &system, flag, move |ctx, app, flags| {
        let flags_changed = {
            let mut shared = lock(&ctx.shared);
            let Some(row) = shared.games.rows.get_mut(index).filter(|r| r.path == path) else {
                return;
            };
            let changed = row.flags != flags;
            row.flags = flags;
            changed
        };
        // The heart is on the tile and the row; every flag is on the info line.
        if flags_changed {
            render(ctx, app);
        }
    });
}

/// Toggle one of `row`'s collections. The strip and `apply` show the
/// result at once, as Core will leave it, and take the old flags back if
/// Core refuses.
pub(crate) fn change_flag(
    ctx: &Ctx,
    app: &App,
    row: &GameRow,
    system: &str,
    flag: Flag,
    apply: impl Fn(&Ctx, &App, Flags) + Send + 'static,
) {
    use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;

    let before = row.flags;
    let after = before.toggled(flag);
    let Some(params) = tag_update_params(
        row,
        system,
        crate::user_lists::user_list(flag).tag(),
        after.has(flag),
    ) else {
        tracing::warn!(
            "collection update skipped: missing media identity for {}",
            row.name
        );
        crate::router::report_action_error(ctx, app, "collection", &row.display);
        return;
    };
    let seq = crate::router::context_seq(ctx);
    crate::router::show_strip_flags(ctx, app, seq, after);
    apply(ctx, app, after);
    // The game may be toggled again before Core answers: only the answer
    // to its newest change decides what it shows.
    let key = format!("{}\u{0}{}", row.system_or(system), row.path);
    let ticket = lock(&ctx.shared).flag_requests.begin(&key);
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let name = row.display.clone();
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Saving, "", "");
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<MediaTagsUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            let newest = lock(&ctx2.shared).flag_requests.settle(&key, ticket);
            match result {
                // Core's answer is the game's flags as it left them.
                Ok(result) if newest => {
                    let flags = user_flags(&result.tags);
                    if flags != after {
                        crate::router::show_strip_flags(&ctx2, &app, seq, flags);
                        apply(&ctx2, &app, flags);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("collection update failed for {name}: {}", e.message);
                    if newest {
                        crate::router::show_strip_flags(&ctx2, &app, seq, before);
                        apply(&ctx2, &app, before);
                    }
                    report_tag_error(&ctx2, &app, &e, "collection", &name);
                }
            }
        });
    });
}

/// Build Core's exclusive media reference and change only the requested tag.
fn tag_update_params(
    row: &GameRow,
    fallback_system: &str,
    tag: &str,
    adding: bool,
) -> Option<MediaTagsUpdateParams> {
    let mut params = MediaTagsUpdateParams::default();
    if let Some(media_id) = row.media_id {
        params.media_id = Some(media_id);
    } else {
        let system = row.system_or(fallback_system);
        if system.is_empty() || row.path.is_empty() {
            return None;
        }
        params.system = system.to_string();
        params.path.clone_from(&row.path);
    }
    if adding {
        params.add.push(tag.to_string());
    } else {
        params.remove.push(tag.to_string());
    }
    Some(params)
}

/// Hide or unhide a game from a screen that lists no rows to update
/// afterwards (a Hub tile). `done` hears whether Core took it.
pub(crate) fn toggle_hidden_detached(
    ctx: &Ctx,
    app: &App,
    row: &GameRow,
    system: &str,
    done: impl FnOnce(&Ctx, &App, bool) + Send + 'static,
) {
    use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;

    let (tag, adding, kind) = (HIDDEN_TAG, !row.is_hidden, "media_visibility");
    let Some(params) = tag_update_params(row, system, tag, adding) else {
        tracing::warn!(
            "{kind} update skipped: missing media identity for {}",
            row.name
        );
        crate::router::report_action_error(ctx, app, kind, &row.name);
        return;
    };
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let name = row.name.clone();
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Saving, "", "");
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<MediaTagsUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            if let Err(e) = &result {
                tracing::warn!("{kind} update failed for {name}: {}", e.message);
                report_tag_error(&ctx2, &app, e, kind, &name);
            }
            done(&ctx2, &app, result.is_ok());
        });
    });
}

/// A failed collection or visibility write. Core refuses these while a job
/// owns the media database, which is not a fault and has its own wording;
/// anything else is the action's own failure.
fn report_tag_error(
    ctx: &Ctx,
    app: &App,
    error: &zaparoo_core::client::ClientError,
    kind: &str,
    name: &str,
) {
    if error.is_busy() {
        crate::router::report_action_error(ctx, app, "media_busy", "");
    } else {
        crate::router::report_action_error(ctx, app, kind, name);
    }
}

/// Hide/unhide is installation-wide in Core. Commit only after success, then
/// restart the list: preference edits invalidate Core's existing cursors.
fn toggle_hidden(ctx: &Ctx, app: &App, row: &GameRow, system: &str) {
    use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;

    let Some(params) = tag_update_params(row, system, HIDDEN_TAG, !row.is_hidden) else {
        tracing::warn!(
            "visibility update skipped: missing media identity for {}",
            row.name
        );
        crate::router::report_action_error(ctx, app, "media_visibility", &row.name);
        return;
    };
    let ticket = lock(&ctx.shared).games.ticket;
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let row = row.clone();
    let system = system.to_string();
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Saving, "", "");
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<MediaTagsUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            on_hidden_updated(&ctx2, &app, ticket, &row, &system, result);
        });
    });
}

pub(crate) fn on_hidden_updated(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    target: &GameRow,
    system: &str,
    result: Result<MediaTagsUpdateResult, zaparoo_core::client::ClientError>,
) {
    let mut shared = lock(&ctx.shared);
    let shell = app.global::<crate::Shell>();
    if shared.games.ticket != ticket
        || shell.get_active_screen() != shared.games.mode.screen()
        || shell.get_transitioning()
    {
        return;
    }
    match result {
        Ok(result) => {
            let fallback = shared.games.system_id.clone();
            for row in &mut shared.games.rows {
                if row.path == target.path && row.system_or(&fallback) == system {
                    row.is_hidden = has_user_tag(&result.tags, "hidden");
                    row.flags = user_flags(&result.tags);
                }
            }
            if shared.games.mode == GamesMode::Browse
                && !shared.show_hidden
                && !crate::browse_filter::shows_hidden(&shared)
                && shared.games.current().is_some_and(|row| row.is_hidden)
            {
                let hidden: Vec<bool> = shared.games.rows.iter().map(|row| row.is_hidden).collect();
                if let Some(index) =
                    rules::selection_after_hide(shared.games.grid.current_index(), &hidden)
                {
                    shared.games.grid.set_current_index_immediate(index);
                }
            }
            drop(shared);
            refresh_visibility(ctx, app);
        }
        Err(e) => {
            drop(shared);
            tracing::warn!(
                "visibility update failed for {}: {}",
                target.name,
                e.message
            );
            report_tag_error(ctx, app, &e, "media_visibility", &target.name);
        }
    }
}

// ---------- Pointer ----------

fn pointer_select(ctx: &Ctx, app: &App, local: i32) -> bool {
    if crate::press_feedback::pending(app) {
        return false;
    }
    interrupt_page(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let list = list_layout(&shared);
        let model = &mut shared.games;
        if model.page.sliding || model.loading {
            return false;
        }
        let Ok(local) = usize::try_from(local) else {
            return false;
        };
        let base = if list {
            app.global::<GamesView>().get_list_view_top().max(0) as usize
        } else {
            model.grid.current_page() * model.grid.page_size()
        };
        let index = base + local;
        if index >= model.rows.len() {
            return false;
        }
        model.focus_armed = true;
        model.grid.set_current_index_immediate(index);
    }
    persist_current(ctx);
    render(ctx, app);
    schedule_detail(ctx, app, false);
    true
}

/// Wire the pointer and page-cue callbacks.
pub fn bind_input(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<GamesInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_detail_art_box(move |width, height| {
            let Some(app) = weak.upgrade() else { return };
            let view = app.global::<GamesView>();
            if view.get_detail_art_width() == width && view.get_detail_art_height() == height {
                return;
            }
            view.set_detail_art_width(width);
            view.set_detail_art_height(height);
            // The detail cover is keyed by its box: ask again at this one.
            // A turn later, because the view reports its box while it is
            // being laid out, which can be inside a driver's own call.
            let ctx = ctx.clone();
            let weak = weak.clone();
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let Some(app) = weak.upgrade() {
                    render(&ctx, &app);
                }
            });
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_hovered(move |local| {
            if let Some(app) = weak.upgrade() {
                pointer_select(&ctx, &app, local);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_clicked(move |local| {
            if let Some(app) = weak.upgrade() {
                if pointer_select(&ctx, &app, local) {
                    crate::router::handle_action(&ctx, &app, actions::ACCEPT);
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_right_clicked(move |local| {
            if let Some(app) = weak.upgrade() {
                if pointer_select(&ctx, &app, local) {
                    handle_action(&ctx, &app, actions::CONTEXT_MENU);
                }
            }
        });
    }
    let page = |ctx: Arc<Ctx>, weak: slint::Weak<App>| {
        move |delta: i32| {
            if let Some(app) = weak.upgrade() {
                let action = if delta > 0 {
                    actions::PAGE_NEXT
                } else {
                    actions::PAGE_PREV
                };
                handle_action(&ctx, &app, action);
            }
        }
    };
    input.on_wheel(page(ctx.clone(), app.as_weak()));
    input.on_page_requested(page(ctx.clone(), app.as_weak()));
}

#[cfg(test)]
mod tests {
    /// The software renderer samples bitmaps nearest-neighbor, so a cover
    /// decoded for any box but the one its tile paints is resampled and
    /// pixelates. A system label rides over the art, so a labelled game, a
    /// card with no system and a deck all paint in the same box.
    #[test]
    fn every_cover_is_decoded_for_the_box_its_tile_paints() {
        use zaparoo_app::sizing::{tile_art_box, Inputs, TileArt};
        let inputs = Inputs::default();
        for mode in [
            GamesMode::Deck,
            GamesMode::Decks,
            GamesMode::Browse,
            GamesMode::Favorites,
            GamesMode::Recents,
            GamesMode::TopPlayed,
        ] {
            let geometry = geometry_for(&inputs, mode);
            let (width, height) = tile_art_box(
                &inputs,
                geometry.fit.cell_width,
                geometry.fit.cell_height,
                TileArt {
                    compact_padding: false,
                    caption: true,
                },
            );
            assert_eq!(
                cover_size(&inputs, mode, 256).fit,
                zaparoo_app::covers::Fit::new(width, height),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn the_detail_pane_shows_scraped_properties_when_no_tag_carries_them() {
        use zaparoo_core::media_types::{MediaMeta, MediaMetaProperty, TagInfo};
        let property = |text: &str| MediaMetaProperty {
            text: text.into(),
            ..Default::default()
        };
        // The tags Core sent say nothing the pane lists.
        let mut meta = MediaMeta {
            tags: vec![TagInfo {
                tag_type: "region".into(),
                tag: "ntsc".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(
            rules::detail_rows_from_tags(&tag_pairs(&meta.tags)).is_empty(),
            "tags alone leave the pane empty"
        );
        // The scraped record does, as properties.
        meta.title
            .properties
            .insert("property:developer".into(), property("Atari"));
        meta.properties
            .insert("property:year".into(), property("1987"));
        meta.properties
            .insert("property:description".into(), property("A long text."));
        let rows = detail_from_meta(&meta, "/games/Desert Falcon.a78").rows;
        assert_eq!(
            rows,
            vec![
                ("year", "1987".to_string()),
                ("developer", "Atari".to_string())
            ]
        );
        // A tag still wins over a property of the same kind.
        meta.tags.push(TagInfo {
            tag_type: "year".into(),
            tag: "1988".into(),
            ..Default::default()
        });
        let rows = detail_from_meta(&meta, "/games/Desert Falcon.a78").rows;
        assert_eq!(rows[0], ("year", "1988".to_string()));
    }

    use super::*;

    fn entry(entry_type: &str, name: &str, path: &str) -> BrowseEntry {
        BrowseEntry {
            media_id: None,
            name: name.into(),
            path: path.into(),
            entry_type: entry_type.into(),
            file_count: 0,
            system_id: "NES".into(),
            system_ids: Vec::new(),
            zap_script: String::new(),
            relative_path: String::new(),
            group: String::new(),
            description: String::new(),
            tags: Vec::new(),
            disambiguating_tags: Vec::new(),
            has_cover: true,
            cover_color: None,
            multi_disc: false,
        }
    }

    #[test]
    fn browse_rows_drop_ancestor_roots_and_distinguish_siblings() {
        let entries = vec![
            entry("root", "NES", "/media/fat/games/NES"),
            entry("root", "NES", "/media/usb0/games/NES"),
            entry("root", "games", "/media/fat/games"),
        ];
        let rows = browse_rows(&entries, false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].root_distinguisher, "fat");
        assert_eq!(rows[1].root_distinguisher, "usb0");
    }

    #[test]
    fn hidden_tags_are_read_from_browse_favorites_and_history_rows() {
        let tags = vec![TagInfo {
            tag: "hidden".into(),
            tag_type: "user".into(),
            ..Default::default()
        }];
        let browse = BrowseEntry {
            tags: tags.clone(),
            ..entry("media", "Game", "/g/Game.nes")
        };
        let favorite = MediaItem {
            tags: tags.clone(),
            ..MediaItem::default()
        };
        let history = MediaHistoryEntry {
            tags,
            ..MediaHistoryEntry::default()
        };
        for row in [
            GameRow::from(&browse),
            GameRow::from(&favorite),
            GameRow::from(&history),
        ] {
            assert!(row.is_hidden);
        }
        let mut visible = entry("media", "Game", "/g/Game.nes");
        visible.tags = vec![TagInfo {
            tag: "hidden".into(),
            tag_type: "genre".into(),
            ..Default::default()
        }];
        let row = GameRow::from(&visible);
        assert!(!row.is_hidden);
    }

    #[test]
    fn only_history_core_resolved_knows_its_tags() {
        let resolved = MediaHistoryEntry {
            media_id: Some(7),
            ..MediaHistoryEntry::default()
        };
        assert!(GameRow::from(&resolved).tags_known);
        // No media id is no tags: the row is not a favorite or a visible
        // game, its state is simply not known.
        assert!(!GameRow::from(&MediaHistoryEntry::default()).tags_known);
        assert!(GameRow::from(&MediaItem::default()).tags_known);
        assert!(GameRow::from(&entry("media", "Game", "/g/Game.nes")).tags_known);
    }

    #[test]
    #[allow(clippy::expect_used, reason = "known media identities must serialize")]
    fn hide_requests_change_only_the_hidden_tag_and_use_exclusive_identity() {
        let row = GameRow::from(&BrowseEntry {
            media_id: Some(42),
            ..entry("media", "Game", "/g/Game.nes")
        });
        let hiding = tag_update_params(&row, "SNES", HIDDEN_TAG, true).expect("media id");
        let request = serde_json::to_value(&hiding).expect("serialize");
        assert_eq!(
            request,
            serde_json::json!({"mediaId": 42, "add": ["user:hidden"]})
        );
        let mut path_row = row;
        path_row.media_id = None;
        path_row.system_id.clear();
        let unhiding =
            tag_update_params(&path_row, "NES", HIDDEN_TAG, false).expect("path identity");
        assert_eq!(
            serde_json::to_value(&unhiding).expect("serialize"),
            serde_json::json!({
                "system": "NES", "path": "/g/Game.nes", "remove": ["user:hidden"]
            })
        );
        assert!(tag_update_params(&path_row, "", HIDDEN_TAG, true).is_none());
        path_row.path.clear();
        assert!(tag_update_params(&path_row, "NES", HIDDEN_TAG, true).is_none());
    }

    #[test]
    fn fetches_never_exceed_the_cap_core_validates() {
        // media.history rejects a limit above 100; browse and search allow 1000.
        assert_eq!(fetch_cap(GamesMode::Recents), 100);
        assert_eq!(fetch_cap(GamesMode::Browse), 1000);
        assert_eq!(fetch_cap(GamesMode::Favorites), 1000);
        assert!(rules::RESTORE_FETCH_CHUNK.clamp(1, fetch_cap(GamesMode::Recents)) <= 100);
    }

    #[test]
    fn virtual_roots_count_toward_totals_and_letter_jump_prefix() {
        let mut model = GamesModel::new();
        model.total_files = 2;
        model.total_dirs = 2;
        model.rows = [
            entry("root", "Virtual", "mock://"),
            entry("directory", "Favorites", "/g/Favorites"),
            entry("directory", "Extras", "/g/Extras"),
            entry("media", "Alpha", "/g/Alpha"),
            entry("media", "Bravo", "/g/Bravo"),
        ]
        .iter()
        .map(GameRow::from)
        .collect();

        assert_eq!(model.known_total(), Some(5));
        assert_eq!(rules::jump_target(model.non_media_total(), 1), 4);
        assert_eq!(
            model.rows[rules::jump_target(model.non_media_total(), 1)].name,
            "Bravo"
        );
    }

    #[test]
    fn partial_directory_prefix_still_uses_core_directory_total() {
        let mut model = GamesModel::new();
        model.total_files = 2;
        model.total_dirs = 2;
        model.rows = [
            entry("root", "Virtual", "mock://"),
            entry("directory", "Favorites", "/g/Favorites"),
        ]
        .iter()
        .map(GameRow::from)
        .collect();

        assert_eq!(model.known_total(), Some(5));
        assert_eq!(rules::jump_target(model.non_media_total(), 1), 4);
    }

    #[test]
    fn display_refresh_folds_counts_and_original_filenames() {
        let mut model = GamesModel::new();
        let mut folder = GameRow::from(&entry("directory", "RPGs", "/g/RPGs"));
        folder.file_count = 5;
        let game = GameRow::from(&entry("media", "Sonic CD", "/g/Sonic CD (USA).chd"));
        model.rows = vec![folder, game];
        model.refresh_display(false, "en");
        assert_eq!(model.rows[0].display, "RPGs");
        assert_eq!(model.rows[0].suffix, "5");
        assert_eq!(model.rows[1].display, "Sonic CD");
        assert_eq!(model.rows[1].suffix, "");
        model.refresh_display(true, "en");
        assert_eq!(model.rows[1].display, "Sonic CD (USA)");
    }

    #[test]
    fn appended_projection_matches_full_rebuild_across_every_batch_boundary() {
        let mut rows: Vec<_> = ["Alpha", "Beta", "Beta", "Beta", "Éclair", "Éclair", "Omega"]
            .iter()
            .enumerate()
            .map(|(n, name)| GameRow::from(&entry("media", name, &format!("/g/{name} {n}.rom"))))
            .collect();
        rows[0].entry_type = EntryType::Directory;
        rows[0].file_count = 12_345;
        rows[1].tag_labels = vec!["atari-joystick".into()];
        rows[2].tag_labels = vec!["atari-lightgun".into()];
        rows[3].tag_labels = vec!["sega-pad".into()];
        rows[4].root_distinguisher = "SD".into();
        for original in [false, true] {
            for language in ["en", "de"] {
                for batch in 1..=rows.len() {
                    let mut incremental = GamesModel::new();
                    for page in rows.chunks(batch) {
                        let first = incremental.rows.len();
                        incremental.rows.extend_from_slice(page);
                        incremental.refresh_display_from(first, original, language);
                        let mut rebuilt = incremental.clone();
                        rebuilt.refresh_display(original, language);
                        for (actual, expected) in incremental.rows.iter().zip(&rebuilt.rows) {
                            assert_eq!(
                                (&actual.display, &actual.suffix),
                                (&expected.display, &expected.suffix)
                            );
                        }
                    }
                    assert_eq!(
                        incremental.refresh_display_from(rows.len(), original, language),
                        0
                    );
                }
            }
        }
    }

    fn tag(tag_type: &str, value: &str) -> TagInfo {
        TagInfo {
            tag: value.into(),
            tag_type: tag_type.into(),
            ..Default::default()
        }
    }

    #[test]
    fn show_tags_off_drops_tags_but_keeps_folder_counts() {
        let mut model = GamesModel::new();
        let mut folder = GameRow::from(&entry("directory", "RPGs", "/g/RPGs"));
        folder.file_count = 5;
        let game = GameRow::from(&BrowseEntry {
            disambiguating_tags: vec![tag("region", "us")],
            ..entry("media", "Sonic CD", "/g/Sonic CD (USA).chd")
        });
        model.rows = vec![folder, game];
        model.refresh_display(false, "en");
        assert_eq!(model.rows[1].suffix, "US");
        model.show_tags = false;
        model.refresh_display(false, "en");
        assert_eq!(model.rows[0].suffix, "5");
        assert_eq!(model.rows[1].suffix, "");
    }

    #[test]
    fn a_multi_disc_folder_asks_for_art_by_path_and_launch_disc() {
        let mut model = GamesModel::new();
        model.rows = vec![GameRow::from(&BrowseEntry {
            media_id: Some(4),
            zap_script: "@PSX/Chrono Cross (disc:1) (region:us)".into(),
            // Core keeps the launch disc in `tags` and out of the tags
            // that tell siblings apart.
            tags: vec![tag("disc", "1"), tag("region", "us")],
            disambiguating_tags: vec![tag("region", "us")],
            multi_disc: true,
            ..entry("directory", "Chrono Cross", "/g/PSX/Chrono Cross")
        })];
        model.refresh_display(false, "en");
        let row = &model.rows[0];
        assert!(row.multi_disc && !row.is_dir());
        assert_eq!(row.suffix, "D1 US");

        let tier = CoverSize {
            tier: 256,
            fit: zaparoo_app::covers::Fit::default(),
        };
        let key = media_key(row, "PSX", tier);
        assert_eq!(
            (key.media_id, key.path.as_str(), key.folder),
            (Some(4), "/g/PSX/Chrono Cross", true)
        );
        let meta = row.meta_params("PSX".into());
        assert_eq!(
            (meta.media_id, meta.path.as_str()),
            (Some(4), "/g/PSX/Chrono Cross")
        );

        // After disc 2 is played, Core names it: the art and the details
        // are asked for again rather than served from disc 1's.
        let mut disc_two = row.clone();
        disc_two.media_id = Some(5);
        assert_ne!(media_key(&disc_two, "PSX", tier), key);
        assert_ne!(disc_two.identity("PSX"), row.identity("PSX"));

        // A game keeps its id alone.
        let game = GameRow::from(&BrowseEntry {
            media_id: Some(7),
            ..entry("media", "Game", "/g/Game.nes")
        });
        let key = media_key(&game, "NES", tier);
        assert_eq!((key.media_id, key.folder), (Some(7), false));
        let meta = game.meta_params("NES".into());
        assert_eq!((meta.media_id, meta.path.as_str()), (Some(7), ""));
    }

    #[test]
    #[allow(clippy::expect_used, reason = "a folder row always has a path")]
    fn a_plain_folder_hides_by_its_path_and_shows_art_of_its_own() {
        // Core names no single system on a plain folder.
        let folder = GameRow::from(&BrowseEntry {
            system_id: String::new(),
            tags: vec![tag("user", "hidden")],
            ..entry("directory", "_alternatives", "/g/Arcade/_alternatives")
        });
        assert!(folder.is_dir() && folder.is_hidden);
        let unhiding =
            tag_update_params(&folder, "Arcade", HIDDEN_TAG, false).expect("folder identity");
        assert_eq!(
            serde_json::to_value(&unhiding).expect("serialize"),
            serde_json::json!({
                "system": "Arcade",
                "path": "/g/Arcade/_alternatives",
                "remove": ["user:hidden"],
            })
        );

        assert!(folder.wants_cover());
        let bare = GameRow::from(&BrowseEntry {
            has_cover: false,
            ..entry("directory", "Extras", "/g/NES/Extras")
        });
        assert!(!bare.wants_cover());
        // A system root has no artwork to ask for.
        assert!(!GameRow::from(&entry("root", "NES", "/g/NES")).wants_cover());
    }

    #[test]
    fn unique_title_append_does_not_reproject_the_loaded_prefix() {
        let mut model = GamesModel::new();
        model.rows = (0..1000)
            .map(|n| GameRow::from(&entry("media", &format!("Game {n}"), &format!("/g/{n}"))))
            .collect();
        model.refresh_display(false, "en");
        let first_name_allocation = model.rows[0].display.as_ptr();
        model.rows.extend(
            (1000..2000)
                .map(|n| GameRow::from(&entry("media", &format!("Game {n}"), &format!("/g/{n}")))),
        );
        assert_eq!(model.refresh_display_from(1000, false, "en"), 1000);
        assert_eq!(model.rows[0].display.as_ptr(), first_name_allocation);
        assert_eq!(model.rows[1999].display, "Game 1999");
        model.refresh_display(true, "en");
        assert_eq!(
            model.rows[0].display, "0",
            "naming changes still rebuild every title"
        );
    }

    #[test]
    fn a_live_file_launches_by_cores_script_before_its_path() {
        let mut e = entry("media", "Atari7800", "/media/fat/_RA_Cores/Atari7800.mgl");
        e.zap_script = "**launch:/media/fat/_RA_Cores/Atari7800.mgl".into();
        let row = GameRow::from(&e);
        assert_eq!(
            row.launch_text_from(true).as_deref(),
            Some(e.zap_script.as_str())
        );
        assert_eq!(
            row.launch_text_from(false).as_deref(),
            Some(e.path.as_str())
        );
    }

    #[test]
    fn launch_text_prefers_the_script_for_media_capable_folders() {
        let mut e = entry("directory", "Game", "/g/Game");
        e.media_id = Some(4);
        e.zap_script = "**launch:/g/Game/disc.cue".into();
        let row = GameRow::from(&e);
        assert!(!row.is_dir());
        assert_eq!(
            row.launch_text().as_deref(),
            Some("**launch:/g/Game/disc.cue")
        );
        let plain = GameRow::from(&entry("media", "Game", "/g/Game.nes"));
        assert_eq!(plain.launch_text().as_deref(), Some("/g/Game.nes"));
        assert!(GameRow::from(&entry("directory", "Folder", "/g/Folder")).is_dir());
    }

    #[test]
    fn rows_keep_the_portable_identifiers_core_sends() {
        let mut browsed = entry("media", "Game", "/g/NES/Game.nes");
        browsed.relative_path = "NES/Game.nes".into();
        browsed.zap_script = "@NES/Game".into();
        let row = GameRow::from(&browsed);
        assert_eq!(row.relative_path, "NES/Game.nes");
        assert_eq!(row.zap_script, "@NES/Game");

        let searched = GameRow::from(&MediaItem {
            path: "/g/NES/Game.nes".into(),
            relative_path: Some("NES/Game.nes".into()),
            zap_script: "@NES/Game".into(),
            ..MediaItem::default()
        });
        assert_eq!(searched.relative_path, "NES/Game.nes");

        let played = GameRow::from(&MediaHistoryEntry {
            media_path: "/g/NES/Game.nes".into(),
            relative_path: Some("NES/Game.nes".into()),
            zap_script: "@NES/Game".into(),
            ..MediaHistoryEntry::default()
        });
        assert_eq!(played.relative_path, "NES/Game.nes");
        assert_eq!(played.zap_script, "@NES/Game");

        // An older Core, or media outside every launcher folder.
        let bare = GameRow::from(&MediaHistoryEntry {
            media_path: "/g/NES/Game.nes".into(),
            ..MediaHistoryEntry::default()
        });
        assert!(bare.relative_path.is_empty());
        assert!(bare.zap_script.is_empty());
    }

    #[test]
    fn a_pinned_game_stores_every_identifier_core_reported() {
        let mut e = entry("media", "Game", "/g/NES/Game.nes");
        e.relative_path = "NES/Game.nes".into();
        e.zap_script = "@NES/Game".into();
        let mut row = GameRow::from(&e);
        row.display = "Game".into();
        for mode in [
            GamesMode::Browse,
            GamesMode::Favorites,
            GamesMode::Recents,
            GamesMode::Search,
        ] {
            assert_eq!(
                hub_pin(mode, &row),
                Some(HubPin {
                    kind: "zapscript",
                    path: "/g/NES/Game.nes".into(),
                    relative: "NES/Game.nes".into(),
                    script: "@NES/Game".into(),
                    name: "Game".into(),
                })
            );
        }
    }

    #[test]
    fn a_pinned_game_core_sent_no_script_for_falls_back_to_its_path() {
        let mut row = GameRow::from(&entry("media", "Game", "/g/NES/Game.nes"));
        row.display = "Game".into();
        assert_eq!(
            hub_pin(GamesMode::Recents, &row),
            Some(HubPin {
                kind: "zapscript",
                path: "/g/NES/Game.nes".into(),
                relative: String::new(),
                script: "/g/NES/Game.nes".into(),
                name: "Game".into(),
            })
        );
    }

    #[test]
    fn a_pinned_game_folder_keeps_its_script_as_the_launch_form() {
        let mut e = entry("directory", "Game", "/g/PSX/Game");
        e.media_id = Some(4);
        e.zap_script = "@PSX/Game".into();
        e.relative_path = "PSX/Game/Game (Disc 1).cue".into();
        let mut row = GameRow::from(&e);
        row.display = "Game".into();
        assert_eq!(
            hub_pin(GamesMode::Browse, &row),
            Some(HubPin {
                kind: "zapscript",
                path: "/g/PSX/Game".into(),
                // The relative path names one disc, not the game.
                relative: String::new(),
                script: "@PSX/Game".into(),
                name: "Game".into(),
            })
        );
    }

    #[test]
    fn a_pinned_folder_stores_its_relative_path() {
        let mut e = entry("directory", "USA", "/g/SNES/USA");
        e.relative_path = "SNES/USA".into();
        assert_eq!(
            hub_pin(GamesMode::Browse, &GameRow::from(&e)),
            Some(HubPin {
                kind: "folder",
                path: "/g/SNES/USA".into(),
                relative: "SNES/USA".into(),
                script: String::new(),
                name: String::new(),
            })
        );
    }

    #[test]
    fn detail_rows_prepend_the_system_for_flat_lists() {
        let mut model = GamesModel::new();
        model.mode = GamesMode::Favorites;
        let mut e = entry("media", "Sonic", "/g/Sonic.md");
        e.tags = vec![TagInfo {
            tag_type: "year".into(),
            tag: "1991".into(),
            ..Default::default()
        }];
        let mut row = GameRow::from(&e);
        row.system_name = "Genesis".into();
        let rows = detail_rows_for(&model, &row, &row.detail_rows);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key.as_str(), "system");
        assert_eq!(rows[0].value.as_str(), "Genesis");
        assert_eq!(rows[1].key.as_str(), "year");
        model.mode = GamesMode::Browse;
        assert_eq!(detail_rows_for(&model, &row, &row.detail_rows).len(), 1);
    }
}
