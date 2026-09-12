// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The games-style screen driver (Games, Favorites, Recently played):
// fills the rows from Core, drives the cursor through `paged_grid`, runs
// the page swoop, the folder stack, the selection persist debounce, the
// focused-detail debounce and the Options menu, and paints the current
// page into the `GamesView` global. Mirrors MediaListScreen.qml plus
// GamesScreen.qml, the games half of Main.qml's routing and
// `models/games.rs` / `favorites.rs` / `recents.rs`, with the rules in
// `zaparoo_app::media_list`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::runtime::Handle;
use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::media_list::{
    self as rules, CoverState, DetailStep, EntryType, FocusedDetail, LinearMove, Owner,
    SelectionPersist, State,
};
use zaparoo_app::paged_grid::{self, Grid, Insets};
use zaparoo_core::endpoints::media_browse::{BrowseArgs, MediaBrowseEndpoint};
use zaparoo_core::endpoints::media_favorites::{FavoritesArgs, MediaFavoritesEndpoint};
use zaparoo_core::endpoints::media_history::{HistoryArgs, MediaHistoryEndpoint};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    merged_root_view, BrowseEntry, MediaBrowseParams, MediaHistoryEntry, MediaHistoryParams,
    MediaItem, MediaSearchParams, SystemInfo, TagInfo,
};
use zaparoo_core::remote_resource::ResourceStatus;

use crate::media_cache::MediaKey;
use crate::router::{lock, Ctx, Shared};
use crate::{App, GamesInput, GamesView, GridCell};

/// Swoop duration and the settle before the strip re-arms.
pub(crate) const SWOOP_MS: u64 = 260;
pub(crate) const REARM_MS: u64 = 50;
const FAVORITE_TAG: &str = "user:favorite";
const FILE_GLYPH: &str = "icons/File";
const FOLDER_GLYPH: &str = "icons/Folder";
const ARCADE_SYSTEM_ID: &str = "Arcade";

/// Which list the screen shows; the mode picks the fill source, the
/// copy, the persisted selection slot and the back route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamesMode {
    Browse,
    Favorites,
    Recents,
}

impl GamesMode {
    pub fn token(self) -> &'static str {
        match self {
            Self::Browse => "games",
            Self::Favorites => "favorites",
            Self::Recents => "recents",
        }
    }

    fn owner(self) -> Owner {
        match self {
            Self::Browse => Owner::Games,
            Self::Favorites => Owner::Favorites,
            Self::Recents => Owner::Recents,
        }
    }
}

/// One row as the screen and its menus see it.
#[derive(Debug, Clone)]
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
    /// Compact disambiguation token labels (pre sibling diff).
    pub tag_labels: Vec<String>,
    /// `false` only when Core confirmed no cover exists.
    pub has_cover: bool,
    pub is_favorite: bool,
    pub media_capable: bool,
    /// The roots page distinguisher, when siblings share a name.
    pub root_distinguisher: String,
    /// The metadata rows Core sent with the row (the detail pane's
    /// immediate peek before media.meta answers).
    pub detail_rows: Vec<(&'static str, String)>,
    /// Display fields, recomputed when the naming settings change.
    pub display: String,
    pub suffix: String,
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

    /// The run text (`run_text_for_entry`): a media-capable directory
    /// launches by its zapscript, everything else by path then script.
    pub fn launch_text(&self) -> Option<String> {
        let non_empty = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
        if self.entry_type == EntryType::Directory && self.media_capable {
            return non_empty(&self.zap_script).or_else(|| non_empty(&self.path));
        }
        non_empty(&self.path).or_else(|| non_empty(&self.zap_script))
    }

    fn identity(&self, fallback_system: &str) -> String {
        if !self.media_capable {
            return String::new();
        }
        let system = self.system_or(fallback_system);
        if system.is_empty() || self.path.is_empty() {
            return String::new();
        }
        format!("{system}\n{}", self.path)
    }

    fn system_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.system_id.is_empty() {
            fallback
        } else {
            &self.system_id
        }
    }
}

fn has_favorite_tag(tags: &[TagInfo]) -> bool {
    tags.iter()
        .any(|tag| tag.tag_type == "user" && tag.tag == "favorite")
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
            tag_labels: crate::tag_utils::disambiguating_tag_labels(&e.disambiguating_tags),
            has_cover: e.has_cover,
            is_favorite: has_favorite_tag(&e.tags),
            media_capable: rules::is_media_capable(entry_type, e.media_id.is_some(), &e.zap_script),
            root_distinguisher: String::new(),
            detail_rows: rules::detail_rows_from_tags(&tag_pairs(&e.tags)),
            display: String::new(),
            suffix: String::new(),
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
            tag_labels: crate::tag_utils::disambiguating_tag_labels(&item.disambiguating_tags),
            has_cover: item.has_cover,
            is_favorite: has_favorite_tag(&item.tags),
            media_capable: true,
            root_distinguisher: String::new(),
            detail_rows: rules::detail_rows_from_tags(&tag_pairs(&item.tags)),
            display: String::new(),
            suffix: String::new(),
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
            zap_script: String::new(),
            tag_labels: Vec::new(),
            has_cover: e.has_cover,
            // History rows carry no tag data; the Recents menu offers no
            // favorite toggle (the Qt rule), so this stays false.
            is_favorite: false,
            media_capable: true,
            root_distinguisher: String::new(),
            detail_rows: Vec::new(),
            display: String::new(),
            suffix: String::new(),
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
    pub grid: Grid,
    pub next_cursor: Option<String>,
    /// The initial fill is in flight (the in-screen loading cue).
    pub loading: bool,
    pub loading_more: bool,
    pub error: String,
    /// Core's browse totals (Games only; the flat lists have none).
    pub total_files: u32,
    pub total_dirs: u32,
    pub total_known: bool,
    pub system_id: String,
    pub system_name: String,
    /// Favorites scope: the system the Favorite Systems screen drilled
    /// into, empty for the whole favorites list.
    pub favorites_system: String,
    /// The browse path behind the rows: every follow-up fetch repeats it.
    pub browse_path: String,
    /// A fill applies only while its ticket is current.
    pub ticket: u64,
    pub focus_armed: bool,
    pub restore_done: bool,
    /// A saved selection deeper than the loaded rows: keep fetching
    /// until it lands, unless the user moves first.
    pub pending_restore_path: String,
    pub persist: SelectionPersist,
    pub persist_seq: u64,
    pub detail: FocusedDetail,
    pub detail_seq: u64,
    pub rapid_active: bool,
    pub rapid_seq: u64,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// A page swoop is in flight; input waits for the commit.
    pub sliding: bool,
    pub page_seq: u64,
    pub cut_next_page: bool,
    pub folder_direction: i32,
    pub folder_sliding: bool,
    /// A jump walk is loading its target pages.
    pub jump_loading: bool,
    pub press_seq: u64,
    /// Bulk appends pause cover fetches until they land.
    pub covers_paused: bool,
}

impl GamesModel {
    pub fn new() -> Self {
        Self {
            mode: GamesMode::Browse,
            rows: Vec::new(),
            grid: Grid::new(4, 3),
            next_cursor: None,
            loading: false,
            loading_more: false,
            error: String::new(),
            total_files: 0,
            total_dirs: 0,
            total_known: true,
            system_id: String::new(),
            system_name: String::new(),
            favorites_system: String::new(),
            browse_path: String::new(),
            ticket: 0,
            focus_armed: false,
            restore_done: false,
            pending_restore_path: String::new(),
            persist: SelectionPersist::new(),
            persist_seq: 0,
            detail: FocusedDetail::new(),
            detail_seq: 0,
            rapid_active: false,
            rapid_seq: 0,
            activate_pulse: 0,
            release_pulse: 0,
            sliding: false,
            page_seq: 0,
            cut_next_page: false,
            folder_direction: 0,
            folder_sliding: false,
            jump_loading: false,
            press_seq: 0,
            covers_paused: false,
        }
    }

    pub fn current(&self) -> Option<&GameRow> {
        self.rows.get(self.grid.current_index())
    }

    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    /// Rows Core places before media offsets. `totalDirs` excludes virtual
    /// roots, so the loaded prefix is authoritative when it is larger.
    fn leading_non_media_count(&self) -> usize {
        self.rows
            .iter()
            .take_while(|row| row.entry_type != EntryType::Media)
            .count()
    }

    fn known_total(&self) -> Option<usize> {
        self.total_known.then(|| {
            self.total_files as usize
                + (self.total_dirs as usize).max(self.leading_non_media_count())
        })
    }

    fn jump_target(&self, item_offset: usize) -> usize {
        rules::jump_target(self.leading_non_media_count(), item_offset)
    }

    fn state(&self) -> State {
        rules::screen_state(self.loading, false, !self.error.is_empty(), self.rows.len())
    }

    /// Recompute the display name and suffix of every row (the sibling
    /// disambiguation spans the whole list, so this runs over all rows).
    fn refresh_display(&mut self, show_original_filenames: bool, language: &str) {
        let names: Vec<(String, Vec<String>)> = self
            .rows
            .iter()
            .map(|row| {
                (
                    rules::display_name(&row.name, &row.path, show_original_filenames),
                    row.tag_labels.clone(),
                )
            })
            .collect();
        let displays = crate::tag_utils::sibling_disambiguation_displays(&names);
        let count = |n: u32| zaparoo_app::format::count(i64::from(n), language);
        for ((row, (display, _)), tags) in self.rows.iter_mut().zip(names).zip(displays) {
            let tags = if row.root_distinguisher.is_empty() {
                tags
            } else {
                row.root_distinguisher.clone()
            };
            row.suffix = rules::row_suffix(
                row.entry_type,
                &tags,
                rules::effective_file_count(row.media_capable, row.file_count),
                &count,
            );
            row.display = display;
        }
    }
}

impl Default for GamesModel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- Shared state helpers ----------

fn list_layout(shared: &Shared) -> bool {
    shared.persist.settings.games_browse_layout == "list"
}

fn rotated(shared: &Shared) -> bool {
    matches!(shared.persist.settings.orientation.as_str(), "cw" | "ccw")
}

fn list_rows_visible(ctx: &Ctx, shared: &Shared) -> usize {
    rules::list_visible_rows(ctx.crt_enabled, rotated(shared))
}

/// The fetch page size: the grid page, or a screenful of list rows.
fn page_size(ctx: &Ctx, app: &App, shared: &Shared) -> u32 {
    if list_layout(shared) {
        return u32::try_from(list_rows_visible(ctx, shared)).unwrap_or(10);
    }
    let view = app.global::<GamesView>();
    (view.get_columns().max(1) * view.get_rows().max(1)) as u32
}

/// The list card's geometry from the games list profile (the TATE table
/// on a rotated scene), with the screen's target row count.
fn list_geometry(ctx: &Ctx, app: &App, shared: &Shared) -> rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let view = if rotated(shared) {
        View::GamesListTate
    } else {
        View::GamesList
    };
    let profile = layouts::profile(ThemeId::current(&inputs), view, &inputs);
    let Body::List { list, .. } = profile.body else {
        unreachable!("the games list views resolve to a list body");
    };
    rules::list_geometry(
        &list,
        &rules::ListFrame {
            screen_width: inputs.screen_width as i32,
            screen_height: inputs.screen_height as i32,
            header_bottom: derived.header_bottom,
            status_top_margin: profile.status.top_margin,
            strip_height: profile.status.strip_height,
            help_bar_height: derived.help_bar_height,
            tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
            safe_bottom_gap: inputs.pct_h(6.0),
            target_rows: list_rows_visible(ctx, shared),
            min_row_height: inputs.pct_h(3.0),
            default_row_height: inputs.pct_h(6.0),
        },
    )
}

/// The Core sort for the favorites list: A-Z, or Core's own default.
fn favorites_sort(shared: &Shared) -> Option<String> {
    (shared.favorites_sort == "name").then(|| "name-asc".to_string())
}

/// The favorites list's system scope, as Core's argument list.
fn favorites_scope(shared: &Shared) -> Vec<String> {
    let system = shared.games.favorites_system.trim();
    if system.is_empty() {
        Vec::new()
    } else {
        vec![system.to_string()]
    }
}

fn favorites_tags(shared: &Shared) -> Vec<String> {
    if shared.persist.games.favorites_filter {
        vec![FAVORITE_TAG.to_string()]
    } else {
        Vec::new()
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
        GamesMode::Favorites => shared.persist.favorites.selected_path.clone(),
        GamesMode::Recents => shared.persist.recents.selected_path.clone(),
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
        GamesMode::Favorites => shared.persist.favorites.list_top,
        GamesMode::Recents => shared.persist.recents.list_top,
    }
}

fn list_restore_needs_rows(ctx: &Ctx, shared: &Shared) -> bool {
    if !list_layout(shared) || !shared.games.has_more() {
        return false;
    }
    let visible = list_rows_visible(ctx, shared).max(1);
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
        GamesMode::Favorites => shared.persist.favorites.list_top = Some(top),
        GamesMode::Recents => shared.persist.recents.list_top = Some(top),
    }
}

fn write_saved_path(shared: &mut Shared, path: String) {
    match shared.games.mode {
        GamesMode::Browse => {
            if let Some(top) = shared.persist.games.selected_at_level.last_mut() {
                *top = path;
            }
        }
        GamesMode::Favorites => shared.persist.favorites.selected_path = path,
        GamesMode::Recents => shared.persist.recents.selected_path = path,
    }
}

fn refresh_display(shared: &mut Shared) {
    let show_original = shared.persist.settings.show_original_filenames;
    let language = crate::effective_language(&shared.persist.settings.language);
    shared.games.refresh_display(show_original, &language);
}

fn detail_ctx() -> Option<(Arc<zaparoo_core::client::Client>, Handle)> {
    DETAIL_CTX.get().cloned()
}

/// Client and runtime handle for the detail pane's media.meta fetch.
static DETAIL_CTX: OnceLock<(Arc<zaparoo_core::client::Client>, Handle)> = OnceLock::new();

pub fn seed_detail_ctx(client: Arc<zaparoo_core::client::Client>, handle: Handle) {
    let _ = DETAIL_CTX.set((client, handle));
}

// ---------- Entry points ----------

/// Systems Accept (and the Hub's system shortcut): a launch-only system
/// runs its script; everything else browses the system root with the
/// folder stack reset.
pub fn enter(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    if !sys.zap_script.is_empty() {
        crate::router::launch(ctx, app, sys.zap_script.clone(), &sys.name);
        return;
    }
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.games.system_id.clone_from(&sys.id);
        shared.persist.games.path_stack = vec![String::new()];
        shared.persist.games.selected_at_level = vec![String::new()];
        shared.persist.games.list_top_at_level.clear();
        begin_browse_mode(&mut shared, sys);
    }
    browse(ctx, app, "", true);
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

fn begin_browse_mode(shared: &mut Shared, sys: &SystemInfo) {
    let name = crate::systems::display_name(shared, &sys.id);
    let model = &mut shared.games;
    model.mode = GamesMode::Browse;
    model.system_id.clone_from(&sys.id);
    model.system_name = name;
    model.focus_armed = false;
    model.restore_done = false;
}

/// Favorites (Hub action): media tagged `user:favorite`.
pub fn enter_favorites(ctx: &Ctx, app: &App) {
    crate::navigation::stage(ctx, app);
    lock(&ctx.shared).games.favorites_system.clear();
    enter_flat(ctx, app, GamesMode::Favorites, true);
}

/// One system's favorites, from the Favorite Systems screen.
pub fn enter_favorites_for_system(ctx: &Ctx, app: &App, system_id: &str) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.games.favorites_system = system_id.to_string();
        shared.persist.favorite_systems.selected_path = system_id.to_string();
    }
    enter_flat(ctx, app, GamesMode::Favorites, true);
}

/// The sort or the grouping changed: refill the list in place.
pub fn refresh_favorites(ctx: &Ctx, app: &App) {
    enter_flat(ctx, app, GamesMode::Favorites, false);
}

/// Recently played (Hub action): Core's play history.
pub fn enter_recents(ctx: &Ctx, app: &App) {
    crate::navigation::stage(ctx, app);
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

fn enter_flat(ctx: &Ctx, app: &App, mode: GamesMode, flip: bool) {
    let (ticket, page_size, sort, scope) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared);
        let sort = favorites_sort(&shared);
        let scope = favorites_scope(&shared);
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
        (begin_fill(model), size, sort, scope)
    };
    if flip {
        crate::router::begin_pending(
            app,
            if mode == GamesMode::Favorites {
                "favorites"
            } else {
                "recents"
            },
        );
    } else {
        render(ctx, app);
    }
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    match mode {
        GamesMode::Favorites => {
            let resource = ctx
                .store
                .subscribe::<MediaFavoritesEndpoint>(FavoritesArgs::new(page_size, sort, scope));
            let mut rx = resource.subscribe();
            ctx.handle.spawn(async move {
                loop {
                    let snapshot = rx.borrow_and_update().clone();
                    match snapshot {
                        ResourceStatus::Ready(result) => {
                            let _ = weak.upgrade_in_event_loop(move |app| {
                                let rows: Vec<GameRow> =
                                    result.results.iter().map(GameRow::from).collect();
                                let cursor = next_cursor(result.pagination.as_ref());
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
        }
        GamesMode::Recents => {
            let resource = ctx
                .store
                .subscribe::<MediaHistoryEndpoint>(HistoryArgs::new(Vec::new(), page_size));
            let mut rx = resource.subscribe();
            ctx.handle.spawn(async move {
                loop {
                    let snapshot = rx.borrow_and_update().clone();
                    match snapshot {
                        ResourceStatus::Ready(result) => {
                            let _ = weak.upgrade_in_event_loop(move |app| {
                                let rows: Vec<GameRow> =
                                    result.entries.iter().map(GameRow::from).collect();
                                let cursor = next_cursor(result.pagination.as_ref());
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
        }
        GamesMode::Browse => {}
    }
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
    model.ticket += 1;
    model.sliding = false;
    model.folder_direction = 0;
    model.folder_sliding = false;
    model.loading = true;
    model.error.clear();
    model.persist.begin_replacement();
    model.grid.prepare_for_model_replacement();
    model.next_cursor = None;
    model.loading_more = false;
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
    let (ticket, system_id, tags, page_size) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared);
        let tags = favorites_tags(&shared);
        let model = &mut shared.games;
        model.mode = GamesMode::Browse;
        model.browse_path = path.to_string();
        model.total_known = true;
        let ticket = begin_fill(model);
        model.folder_direction = direction;
        (ticket, model.system_id.clone(), tags, size)
    };
    crate::router::save_persist(&ctx.shared);
    if flip {
        crate::router::begin_pending(app, "games");
    } else {
        render(ctx, app);
    }

    let args = BrowseArgs::new(path.to_string(), vec![system_id], page_size, tags);
    let resource = ctx.store.subscribe::<MediaBrowseEndpoint>(args);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let at_root = path.is_empty();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        on_browse_ready(&ctx2, &app, ticket, &result, at_root, flip);
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
}

fn browse_rows(entries: &[BrowseEntry]) -> Vec<GameRow> {
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
            row
        })
        .collect()
}

fn on_browse_ready(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    result: &zaparoo_core::media_types::MediaBrowseResult,
    at_root: bool,
    flip: bool,
) {
    if lock(&ctx.shared).games.ticket != ticket {
        return;
    }
    let rows = browse_rows(&result.entries);
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
        let visible = list_rows_visible(ctx, &shared);
        let saved = saved_path(&shared);
        let model = &mut shared.games;
        model.rows = rows;
        model.next_cursor = cursor;
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
            .then(|| model.rows.iter().position(|r| r.path == saved))
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
        model.restore_done = true;
        let fill_list = list && rules::list_fill_page(model.rows.len(), visible, model.has_more());
        let token = model.mode.token();
        drop(shared);
        let mut shared = lock(&ctx.shared);
        refresh_display(&mut shared);
        shared.persist.active_screen = token.to_string();
        (token, restore_fetch, fill_list)
    };
    if app.global::<crate::Shell>().get_transitioning()
        && (restore_fetch || list_restore_needs_rows(ctx, &lock(&ctx.shared)))
    {
        fetch_more(ctx, app, rules::RAPID_FETCH_CHUNK, true);
        return;
    }
    crate::navigation::finish(app);
    crate::router::save_persist(&ctx.shared);
    if flip {
        crate::router::transition_to_screen(app, token, 1);
    } else {
        app.global::<crate::Shell>()
            .set_status_text(SharedString::default());
    }
    render(ctx, app);
    crate::folder_motion::start(ctx, app);
    schedule_detail(ctx, app, false);
    if restore_fetch {
        fetch_more(ctx, app, rules::RAPID_FETCH_CHUNK, true);
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
fn show_error(ctx: &Ctx, app: &App, ticket: u64, message: &str, flip: bool) {
    if lock(&ctx.shared).games.ticket != ticket {
        return;
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
        model.mode.token()
    };
    if flip {
        crate::router::transition_to_screen(app, token, 1);
    } else {
        app.global::<crate::Shell>()
            .set_status_text(SharedString::default());
    }
    render(ctx, app);
    if !flip {
        crate::router::clear_pending(app);
    }
}

/// Fetch the next chunk with the stored cursor, bypassing the endpoint
/// cache (each follow-up has a different cursor). `bulk` pauses cover
/// fetches until the chunk lands (jumps and restores).
fn fetch_more(ctx: &Ctx, app: &App, limit: u32, bulk: bool) {
    let (mode, cursor, system_id, browse_path, tags, ticket, sort, scope) = {
        let mut shared = lock(&ctx.shared);
        let tags = favorites_tags(&shared);
        let sort = favorites_sort(&shared);
        let scope = favorites_scope(&shared);
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
        (
            model.mode,
            cursor,
            model.system_id.clone(),
            model.browse_path.clone(),
            tags,
            model.ticket,
            sort,
            scope,
        )
    };
    render(ctx, app);
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let limit = limit.max(1);
    ctx.handle.spawn(async move {
        let outcome: Result<(Vec<GameRow>, Option<String>), String> = match mode {
            GamesMode::Browse => client
                .media_browse(MediaBrowseParams {
                    root_view: merged_root_view(&browse_path, std::slice::from_ref(&system_id)),
                    path: browse_path,
                    systems: vec![system_id],
                    max_results: Some(limit),
                    cursor: Some(cursor),
                    tags,
                    letter: None,
                    sort: None,
                })
                .await
                .map(|r| {
                    let cursor = next_cursor(r.pagination.as_ref());
                    (browse_rows(&r.entries), cursor)
                })
                .map_err(|e| e.message),
            GamesMode::Favorites => client
                .media_search(MediaSearchParams {
                    systems: scope,
                    max_results: Some(limit),
                    cursor: Some(cursor),
                    tags: vec![FAVORITE_TAG.to_string()],
                    sort,
                    ..MediaSearchParams::default()
                })
                .await
                .map(|r| {
                    let cursor = next_cursor(r.pagination.as_ref());
                    (r.results.iter().map(GameRow::from).collect(), cursor)
                })
                .map_err(|e| e.message),
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
                .map_err(|e| e.message),
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            on_append(&ctx2, &app, ticket, outcome);
        });
    });
}

pub(crate) fn on_append(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    outcome: Result<(Vec<GameRow>, Option<String>), String>,
) {
    let (restore_again, landed_restore, from_page, changed_page) = {
        let mut shared = lock(&ctx.shared);
        if shared.games.ticket != ticket {
            return;
        }
        let saved = saved_path(&shared);
        let list = list_layout(&shared);
        let model = &mut shared.games;
        let from_page = model.grid.current_page();
        model.loading_more = false;
        model.covers_paused = false;
        let (rows, cursor) = match outcome {
            Ok(page) => page,
            Err(message) => {
                // The rows already on screen stay; Qt says nothing here
                // either, and the next move retries the fetch.
                tracing::warn!("page fetch failed: {message}");
                model.grid.set_loading_more(false);
                drop(shared);
                if app.global::<crate::Shell>().get_transitioning() {
                    show_error(ctx, app, ticket, &message, true);
                } else {
                    render(ctx, app);
                }
                return;
            }
        };
        let empty = rows.is_empty();
        model.rows.extend(rows);
        model.next_cursor = if empty { None } else { cursor };
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
            } else if let Some(index) = model
                .rows
                .iter()
                .position(|r| r.path == model.pending_restore_path)
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
        drop(shared);
        let mut shared = lock(&ctx.shared);
        refresh_display(&mut shared);
        (restore_again, landed, from_page, changed_page)
    };
    if app.global::<crate::Shell>().get_transitioning() {
        if restore_again || list_restore_needs_rows(ctx, &lock(&ctx.shared)) {
            fetch_more(ctx, app, rules::RAPID_FETCH_CHUNK, true);
            return;
        }
        let token = lock(&ctx.shared).games.mode.token();
        crate::navigation::finish(app);
        crate::router::save_persist(&ctx.shared);
        crate::router::transition_to_screen(app, token, 1);
        crate::folder_motion::start(ctx, app);
    }
    app.global::<crate::Shell>()
        .set_status_text(SharedString::default());
    if landed_restore {
        persist_now(ctx);
    }
    if changed_page && !landed_restore {
        persist_current(ctx);
        slide_to_current_page(ctx, app, from_page);
    } else {
        render(ctx, app);
    }
    schedule_detail(ctx, app, false);
    if restore_again {
        fetch_more(ctx, app, rules::RAPID_FETCH_CHUNK, true);
    } else {
        drain_load_requests(ctx, app);
    }
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
        } else if model.rapid_active {
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

/// The grid band, cell fit and footer slot for the scene
/// (MediaListScreen.qml's anchors through the games browse profile).
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
    let profile = layouts::profile(ThemeId::current(inputs), View::GamesGrid, inputs);
    let (grid, footer) = match profile.body {
        Body::Grid { grid, footer } => (grid, footer),
        Body::List { .. } => unreachable!("the games grid view resolves to a grid body"),
    };
    let screen_h = inputs.screen_height as i32;
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let (bottom, label_y, label_height) = match mode {
        GamesMode::Browse => {
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
        GamesMode::Favorites | GamesMode::Recents => {
            let label_height = inputs.pct_h(7.0);
            let bottom = if t240 {
                derived.help_bar_height + label_height
            } else {
                inputs.pct_h(15.0)
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

fn logo_image(px: &crate::system_logos::LogoPixels) -> slint::Image {
    slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &px.rgba, px.width, px.height,
        ),
    )
}

/// The cover decode tier for the grid at the current output geometry.
fn cover_tier(app: &App) -> u32 {
    crate::sizing::games_grid_cover_source_size(crate::router::output_scene(app))
}

fn media_key(row: &GameRow, fallback_system: &str, tier: u32) -> MediaKey {
    MediaKey {
        media_id: row.media_id,
        system: row.system_or(fallback_system).to_string(),
        path: row.path.clone(),
        max_size: tier,
        image_type: None,
    }
}

/// One tile: the caption with its dim suffix, the favorite heart, the
/// flat lists' system label, and the cover slot per the Qt key policy.
fn cell_for(ctx: &Ctx, model: &GamesModel, row: &GameRow, tier: u32) -> GridCell {
    let mut cell = GridCell {
        name: SharedString::from(row.display.as_str()),
        tags: SharedString::from(row.suffix.as_str()),
        favorite: row.is_favorite,
        ..Default::default()
    };
    if model.mode != GamesMode::Browse {
        cell.top_label = SharedString::from(row.system_name.trim());
        if cell.top_label.is_empty() {
            cell.top_label = SharedString::from(row.system_id.as_str());
        }
    }
    let key = media_key(row, &model.system_id, tier);
    let cached = ctx.media.get(&key);
    let state = rules::cover_state(
        row.entry_type,
        row.media_capable,
        row.has_cover && !key.system.is_empty(),
        cached.is_some(),
        ctx.media.is_negative(&key),
    );
    match state {
        CoverState::Folder => cell.glyph_key = SharedString::from(FOLDER_GLYPH),
        CoverState::Art => {
            if let Some(image) = cached {
                cell.cover = slint::Image::from_rgba8(image.buffer);
                cell.has_cover = true;
            }
        }
        CoverState::Absent => {
            // The flat lists fall back to the system logo, a friendlier
            // "no cover" cue than the file chip; Games keeps the chip.
            let logo = (model.mode != GamesMode::Browse && !key.system.is_empty())
                .then(|| crate::system_logos::tinted_logo_for(&key.system, false))
                .flatten();
            if let Some(px) = logo {
                cell.cover = logo_image(&px);
                cell.has_cover = true;
                if let Some(focus) = crate::system_logos::tinted_logo_for(&key.system, true) {
                    cell.cover_focus = logo_image(&focus);
                    cell.has_cover_focus = true;
                }
            } else {
                cell.glyph_key = SharedString::from(FILE_GLYPH);
            }
        }
        CoverState::Pending => {}
    }
    cell
}

fn page_cells(ctx: &Ctx, model: &GamesModel, page: usize, tier: u32) -> Vec<GridCell> {
    let page_size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|row| cell_for(ctx, model, row, tier))
        .collect()
}

/// Queue fetches for the covers around the visible page that are not
/// cached yet: the page first, then the next page, then the previous.
fn request_covers(
    ctx: &Ctx,
    model: &GamesModel,
    first_visible: usize,
    page_size: usize,
    tier: u32,
) {
    if model.covers_paused || model.rapid_active {
        return;
    }
    for index in rules::prefetch_rows(model.rows.len(), page_size, first_visible) {
        let Some(row) = model.rows.get(index) else {
            continue;
        };
        if !row.media_capable || !row.has_cover {
            continue;
        }
        let key = media_key(row, &model.system_id, tier);
        if key.system.is_empty() || key.path.is_empty() {
            continue;
        }
        ctx.media.enqueue(key);
    }
}

/// Paint the current page (or the list window), the cursor, the caption,
/// the cues and the geometry.
#[allow(
    clippy::too_many_lines,
    reason = "one setter per GamesView property keeps the inventory reviewable"
)]
pub fn render(ctx: &Ctx, app: &App) {
    // Even an offscreen render updates saved viewport positions. Wait for the
    // complete destination so a partial restore cannot overwrite its target.
    if app.global::<crate::Shell>().get_transitioning() {
        return;
    }
    {
        let shared = lock(&ctx.shared);
        // Keep the complete source, including list details, while a folder fills.
        if shared.games.loading && shared.games.folder_direction != 0 {
            return;
        }
    }
    let mode = lock(&ctx.shared).games.mode;
    let geometry = geometry(app, mode);
    let tier = cover_tier(app);
    let view = app.global::<GamesView>();
    let shared = lock(&ctx.shared);
    let list = list_layout(&shared);
    let visible = list_rows_visible(ctx, &shared);
    let path_stack_len = shared.persist.games.path_stack.len();
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
    let shared = lock(&ctx.shared);
    let model = &shared.games;
    let page = model.grid.current_page();
    let start = page * model.grid.page_size();
    let count = model.rows.len();

    view.set_mode(SharedString::from(mode.token()));
    view.set_title(SharedString::from(
        rules::screen_title(path_stack_len, &model.browse_path, &model.system_name).as_str(),
    ));
    view.set_loading(model.loading);
    view.set_loading_more(model.loading_more);
    view.set_error(SharedString::from(model.error.as_str()));
    view.set_count(i32::try_from(count).unwrap_or(0));
    view.set_total_items(i32::try_from(model.grid.total_items()).unwrap_or(0));
    view.set_total_known(model.total_known);
    view.set_total_files(i32::try_from(model.total_files).unwrap_or(0));
    view.set_has_more(model.has_more());
    view.set_page_loading(
        model.loading_more
            && if list {
                count > 0
            } else {
                model.grid.has_pending_target()
            },
    );
    view.set_focus_ready(model.focus_armed || model.restore_done);
    view.set_rapid_active(model.rapid_active);

    if !model.folder_sliding {
        if view.get_folder_slide() {
            crate::folder_motion::clear(app);
        }
        if model.folder_direction == 0 {
            view.set_folder_from_cells(ModelRc::default());
        }
    }
    let strip_sliding = model.sliding && !view.get_cached_transition();
    if strip_sliding {
        crate::view_model::publish_cells(
            &view.get_next_cells(),
            page_cells(ctx, model, page, tier),
            |rows| view.set_next_cells(rows),
        );
    } else if list {
        view.set_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    } else {
        crate::view_model::publish_cells(
            &view.get_cells(),
            page_cells(ctx, model, page, tier),
            |rows| view.set_cells(rows),
        );
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
    if let Some(row) = model.current() {
        view.set_label_name(SharedString::from(row.display.as_str()));
        view.set_label_tags(SharedString::from(row.suffix.as_str()));
    } else {
        view.set_label_name(SharedString::default());
        view.set_label_tags(SharedString::default());
    }

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
        let scroll_top =
            crate::browse_motion::window_top(model.grid.current_index(), count, visible, previous);
        let top = scroll_top.saturating_sub(1);
        let rows: Vec<GridCell> = model
            .rows
            .iter()
            .skip(top)
            .take(visible + 2)
            .map(|row| cell_for(ctx, model, row, tier))
            .collect();
        crate::view_model::publish_cells(&view.get_list_rows(), rows, |rows| {
            view.set_list_rows(rows);
        });
        view.set_list_sel(
            i32::try_from(model.grid.current_index().saturating_sub(top)).unwrap_or(0),
        );
        view.set_list_view_top(i32::try_from(top).unwrap_or(0));
        view.set_list_scroll_top(i32::try_from(scroll_top).unwrap_or(0));
        refresh_detail_cover(ctx, app, model);
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
    request_covers(ctx, model, first_visible, window, tier);
    drop(shared);
    if list {
        remember_list_top(
            &mut lock(&ctx.shared),
            view.get_list_scroll_top().max(0) as usize,
        );
    }
}

/// The detail pane's identity fields for the focused row: title, path and
/// the cached cover (misses stream in through `cover_landed`).
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
        view.set_detail_has_cover(false);
        view.set_detail_cover_absent(false);
        return;
    };
    view.set_detail_title(SharedString::from(row.display.as_str()));
    view.set_detail_path(SharedString::from(row.path.as_str()));
    let tier = crate::sizing::detail_cover_source_size(crate::router::output_scene(app));
    let key = media_key(row, &model.system_id, tier);
    if !row.media_capable || key.system.is_empty() {
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
        view.set_detail_cover_absent(!row.has_cover || ctx.media.is_negative(&key));
        if row.has_cover {
            ctx.media.enqueue(key);
        }
    }
}

/// A media cover landed: repaint when the page (or the detail pane) shows
/// that item.
pub fn cover_landed(ctx: &Ctx, app: &App, key: &MediaKey) {
    if crate::navigation::retaining(app, &["games", "favorites", "recents"]) {
        return;
    }
    let relevant = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let list = list_layout(&shared);
        let (first, window) = if list {
            let visible = list_rows_visible(ctx, &shared);
            (
                app.global::<GamesView>().get_list_view_top().max(0) as usize,
                visible + 2,
            )
        } else {
            (
                model.grid.current_page() * model.grid.page_size(),
                model.grid.page_size(),
            )
        };
        model
            .rows
            .iter()
            .skip(first)
            .take(window)
            .any(|row| row.path == key.path)
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
        let visible = list_rows_visible(ctx, &shared);
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
        shared.games.current().map(|r| r.path.clone())
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
    let path = lock(&ctx.shared).games.current().map(|r| r.path.clone());
    if let Some(path) = path {
        schedule_persist(ctx, &path);
    }
}

// ---------- Focused detail (list layout) ----------

/// The selection, count or layout changed: peek the row's identity now,
/// load its metadata after the debounce.
fn schedule_detail(ctx: &Ctx, app: &App, force: bool) {
    if crate::navigation::retaining(app, &["games", "favorites", "recents"]) {
        return;
    }
    let (steps, seq) = {
        let mut shared = lock(&ctx.shared);
        let enabled = list_layout(&shared)
            && !shared.games.loading
            && shared.games.error.is_empty()
            && !app.global::<crate::Shell>().get_transitioning();
        let model = &mut shared.games;
        let index = model.grid.current_index();
        let identity = model
            .current()
            .map(|row| row.identity(&model.system_id))
            .unwrap_or_default();
        let rapid = model.rapid_active;
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
            // holds the previous row's values through the load window.
            DetailStep::Peek(index) => {
                let shared = lock(&ctx.shared);
                let model = &shared.games;
                let rows = model
                    .rows
                    .get(index)
                    .map(|row| detail_rows_for(model, row, &row.detail_rows))
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

fn fire_detail(ctx: &Ctx, app: &App, seq: u64) {
    let (row, system) = {
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
        (row, system)
    };
    if row.is_dir() {
        app.global::<GamesView>().set_detail_loading(false);
        return;
    }
    let Some((client, handle)) = detail_ctx() else {
        return;
    };
    let params = zaparoo_core::media_types::MediaMetaParams {
        media_id: row.media_id,
        system,
        path: row.path.clone(),
    };
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
                    let source = if result.media.title.tags.is_empty() {
                        result.media.tags.as_slice()
                    } else {
                        result.media.title.tags.as_slice()
                    };
                    let pairs = rules::detail_rows_from_tags(&tag_pairs(source));
                    detail_rows_for(&shared.games, &row, &pairs)
                })
            };
            let view = app.global::<GamesView>();
            if let Some(rows) = rows {
                view.set_detail_rows(ModelRc::new(VecModel::from(rows)));
            }
            view.set_detail_loading(false);
        });
    });
}

// ---------- Input ----------

#[cfg(feature = "mister")]
fn request_cached_page_transition(app: &App, direction: i32, _columns: i32, _rows: i32) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_orientation().as_str() != "horizontal" || shell.get_browse_list_layout() {
        return false;
    }
    let sizing = app.global::<crate::Sizing>();
    let width = sizing.get_screen_width().round().max(0.0) as u32;
    let height = sizing.get_screen_height().round().max(0.0) as u32;
    let Some(geometry) = crate::sizing::mister_browse_grid_transition_geometry(
        width,
        height,
        app.global::<GamesView>().get_grid_y().round().max(0.0) as u32,
        app.global::<GamesView>().get_grid_height().round().max(0.0) as u32,
    ) else {
        return false;
    };
    crate::mister::request_page_transition(geometry, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached_page_transition(_app: &App, _direction: i32, _columns: i32, _rows: i32) -> bool {
    false
}

/// A cursor move landed on another page: swoop the strip one period in
/// `dir`, then commit. Reduce motion cuts instead.
fn slide_to_current_page(ctx: &Ctx, app: &App, from_page: usize) {
    let rapid = crate::input::rapid_page(ctx) || crate::input::rapid_navigation(ctx);
    let (to_page, columns, rows, reduce_motion, tier) = {
        let mut shared = lock(&ctx.shared);
        let reduce_motion =
            shared.persist.settings.reduce_motion || !app.global::<crate::Motion>().get_enabled();
        let model = &mut shared.games;
        model.sliding = true;
        model.page_seq = model.page_seq.wrapping_add(1);
        let reduce_motion = reduce_motion || std::mem::take(&mut model.cut_next_page);
        (
            model.grid.current_page(),
            model.grid.columns() as i32,
            model.grid.rows() as i32,
            reduce_motion,
            cover_tier(app),
        )
    };
    let dir: i32 = if to_page > from_page { 1 } else { -1 };
    note_rapid_flip(ctx, app, rapid);
    let view = app.global::<GamesView>();
    let target_local = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        model.grid.current_index() - to_page * model.grid.page_size()
    };
    view.set_slide_dir(dir);
    view.set_transition_target_index(i32::try_from(target_local).unwrap_or(0));
    if reduce_motion || rapid {
        lock(&ctx.shared).games.sliding = false;
        render(ctx, app);
        return;
    }
    let ticket = lock(&ctx.shared).games.ticket;
    let page_seq = lock(&ctx.shared).games.page_seq;
    if request_cached_page_transition(app, dir, columns, rows) {
        view.set_slide_anim(false);
        view.set_cached_transition(true);
        render(ctx, app);
        let weak = app.as_weak();
        let ctx = ctx.clone();
        slint::Timer::single_shot(Duration::from_millis(SWOOP_MS), move || {
            if let Some(app) = weak.upgrade() {
                if !page_motion_current(&ctx, ticket, page_seq) {
                    return;
                }
                lock(&ctx.shared).games.sliding = false;
                let view = app.global::<GamesView>();
                view.set_cached_transition(false);
                view.set_slide_anim(true);
            }
        });
        return;
    }
    crate::drs::heavy_begin();
    let next: Vec<GridCell> = {
        let shared = lock(&ctx.shared);
        page_cells(ctx, &shared.games, to_page, tier)
    };
    view.set_slide_anim(true);
    view.set_selected_local(-1);
    view.set_next_cells(ModelRc::new(VecModel::from(next)));
    view.set_page_slide(dir as f32);
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(Duration::from_millis(SWOOP_MS), move || {
        crate::drs::heavy_end();
        let Some(app) = weak.upgrade() else {
            return;
        };
        if !page_motion_current(&ctx, ticket, page_seq) {
            return;
        }
        lock(&ctx.shared).games.sliding = false;
        let view = app.global::<GamesView>();
        view.set_slide_anim(false);
        render(&ctx, &app);
        view.set_page_slide(0.0);
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(REARM_MS), move || {
            if !page_motion_current(&ctx, ticket, page_seq) {
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.global::<GamesView>().set_slide_anim(true);
            }
        });
    });
}

fn page_motion_current(ctx: &Ctx, ticket: u64, seq: u64) -> bool {
    let shared = lock(&ctx.shared);
    shared.games.ticket == ticket && shared.games.page_seq == seq
}

pub(crate) fn interrupt_page(ctx: &Ctx, app: &App) {
    let interrupted = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let interrupted = model.sliding;
        model.cut_next_page = interrupted;
        if interrupted {
            model.page_seq = model.page_seq.wrapping_add(1);
            model.sliding = false;
        }
        interrupted
    };
    if !interrupted {
        return;
    }
    let view = app.global::<GamesView>();
    #[cfg(feature = "mister")]
    if view.get_cached_transition() {
        crate::mister::cancel_page_transition();
    }
    view.set_cached_transition(false);
    view.set_slide_anim(false);
    view.set_page_slide(0.0);
    render(ctx, app);
    app.window().request_redraw();
}

/// Only a qualified held navigation may show the landing letter. Page-flip
/// frequency is not hold intent: quick taps must keep normal presentation.
fn note_rapid_flip(ctx: &Ctx, app: &App, rapid: bool) {
    if !rapid {
        clear_rapid_letter(ctx, app);
        return;
    }
    let (seq, letter) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        model.rapid_seq += 1;
        let page_start = model.grid.current_page() * model.grid.page_size();
        let letter = model
            .rows
            .get(page_start)
            .map(|row| rules::rapid_letter(&row.display))
            .unwrap_or_default();
        (model.rapid_seq, letter)
    };
    app.global::<GamesView>()
        .set_rapid_letter(SharedString::from(letter.as_str()));
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        Duration::from_millis(rules::RAPID_LETTER_HOLD_MS),
        move || {
            if lock(&ctx.shared).games.rapid_seq != seq {
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.global::<GamesView>()
                    .set_rapid_letter(SharedString::default());
            }
        },
    );
}

fn clear_rapid_letter(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).games.rapid_seq += 1;
    app.global::<GamesView>()
        .set_rapid_letter(SharedString::default());
}

/// Held rapid navigation (Main.qml's `rapidNavigationActive`): covers
/// pause and the detail pane clears while it runs.
pub fn set_rapid(ctx: &Ctx, app: &App, active: bool) {
    if !active {
        // Retire the badge even when render state was already inactive; otherwise
        // invalidating its timer on a later ordinary flip can strand the letter.
        clear_rapid_letter(ctx, app);
    }
    {
        let mut shared = lock(&ctx.shared);
        if shared.games.rapid_active == active {
            return;
        }
        shared.games.rapid_active = active;
    }
    render(ctx, app);
    schedule_detail(ctx, app, false);
}

/// The screen's input gate (MediaListScreen.qml's `_gateHide`).
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
        if model.sliding {
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
            slide_to_current_page(ctx, app, from_page);
            drain_load_requests(ctx, app);
            return;
        }
    }
    render(ctx, app);
    drain_load_requests(ctx, app);
}

fn list_move(ctx: &Ctx, app: &App, delta: i64) {
    let (outcome, size, tail) = {
        let mut shared = lock(&ctx.shared);
        let size = page_size(ctx, app, &shared);
        let visible = list_rows_visible(ctx, &shared);
        let model = &mut shared.games;
        model.focus_armed = true;
        let count = model.rows.len();
        let has_more = rules::list_has_more(count, model.known_total(), model.has_more());
        let outcome = rules::linear_move(model.grid.current_index(), count, delta, has_more);
        let mut tail = false;
        if let LinearMove::To { index, .. } = outcome {
            model.grid.set_current_index_immediate(index);
            tail = rules::list_tail_prefetch(index, count, visible, has_more, model.loading_more);
        }
        (outcome, size, tail)
    };
    match outcome {
        LinearMove::To { fetch, .. } => {
            persist_current(ctx);
            render(ctx, app);
            schedule_detail(ctx, app, false);
            if fetch || tail {
                fetch_more(ctx, app, size, false);
            }
        }
        LinearMove::FetchMore => fetch_more(ctx, app, size, false),
        LinearMove::Stay { fetch } => {
            if fetch {
                fetch_more(ctx, app, size, false);
            }
        }
    }
}

/// MediaListScreen.qml's `handleAction` with GamesScreen.qml's overrides.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    interrupt_page(ctx, app);
    let is_move = matches!(
        action,
        actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN | actions::CONTEXT_MENU
    );
    if is_move && gate_hide(ctx, app) {
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
                if state == State::Ready {
                    list_move(ctx, app, dir * list_page);
                }
            } else {
                grid_move(ctx, app, dir as i32, 0, 0);
            }
        }
        actions::UP | actions::DOWN => {
            let dir: i64 = if action == actions::UP { -1 } else { 1 };
            if list {
                list_move(ctx, app, dir);
            } else {
                grid_move(ctx, app, 0, dir as i32, 0);
            }
        }
        actions::PAGE_PREV | actions::PAGE_NEXT => {
            if state != State::Ready {
                return;
            }
            let dir: i64 = if action == actions::PAGE_PREV { -1 } else { 1 };
            if list {
                list_move(ctx, app, dir * list_page);
            } else {
                grid_move(ctx, app, 0, 0, dir as i32);
            }
        }
        actions::PAGE_MENU => {
            // Favorites scope can produce an empty folder: keep View
            // reachable so the user can clear the filter.
            if !rules::page_menu_allowed(state, true) {
                return;
            }
            match mode {
                GamesMode::Browse => crate::router::open_view_menu(ctx, app),
                GamesMode::Favorites => crate::router::open_favorites_page_menu(ctx, app),
                GamesMode::Recents => {}
            }
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
        GamesMode::Favorites | GamesMode::Recents => enter_flat(ctx, app, mode, false),
    }
}

fn press_duration(app: &App) -> u64 {
    if app.global::<crate::Motion>().get_enabled() {
        34
    } else {
        0
    }
}

/// Dispatch the selected row after the router's grid push. List feedback
/// retires locally without a navigation delay.
fn accept_current(ctx: &Ctx, app: &App) {
    let (row, seq) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let Some(row) = model.current().cloned() else {
            return;
        };
        model.press_seq += 1;
        model.activate_pulse += 1;
        (row, model.press_seq)
    };
    persist_now(ctx);
    render(ctx, app);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    if row.is_dir() {
        navigate_into_folder(ctx, app, &row.path);
        return;
    }
    if let Some(text) = row.launch_text() {
        crate::router::launch(ctx, app, text, &row.display);
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
    let target = {
        let mut shared = lock(&ctx.shared);
        let arcade_bypass = ctx.is_mister
            && shared.systems_model.category == ARCADE_SYSTEM_ID
            && shared.systems_model.rows.len() == 1
            && shared.games.system_id == ARCADE_SYSTEM_ID;
        // A scoped favorites list climbs back to the systems that hold
        // favorites, the screen it was entered from.
        let grouped_favorites = !shared.games.favorites_system.is_empty()
            && shared.persist.settings.favorites_grouping == "system";
        let target = match mode {
            GamesMode::Browse if arcade_bypass => "hub",
            GamesMode::Browse if shared.persist.games.entered_from_hub => {
                shared.persist.games.entered_from_hub = false;
                "hub"
            }
            GamesMode::Browse => "systems",
            GamesMode::Favorites if grouped_favorites => "favorite-systems",
            GamesMode::Favorites | GamesMode::Recents => "hub",
        };
        shared.persist.active_screen = target.to_string();
        target
    };
    crate::router::save_persist(&ctx.shared);
    if target == "favorite-systems" {
        crate::systems::return_to_favorites(ctx, app);
        return;
    }
    crate::router::transition_to_screen(app, target, -1);
}

/// Jump-to-letter landing (`GamesScreen.jumpToItem`): the bucket's first
/// item sits after the leading directories; unloaded targets walk in.
pub fn jump_to_item(ctx: &Ctx, app: &App, item_offset: u32) {
    let landed = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let target = model.jump_target(item_offset as usize);
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

fn cell_anchor(ctx: &Ctx, app: &App) -> (f32, f32, f32, f32) {
    let shared = lock(&ctx.shared);
    let model = &shared.games;
    let list = list_layout(&shared);
    if list {
        let g = list_geometry(ctx, app, &shared);
        let layout = app.global::<crate::Layout>();
        let top = app.global::<GamesView>().get_list_scroll_top().max(0) as usize;
        let local = model.grid.current_index().saturating_sub(top) as f32;
        let row_h = g.row_height as f32;
        (
            (g.card_x + g.list_x) as f32 + layout.get_card_padding_left(),
            (g.card_y + g.list_y) as f32
                + layout.get_card_padding_top()
                + local * (row_h + layout.get_row_spacing()),
            g.list_width as f32 - layout.get_card_padding_left() - layout.get_card_padding_right(),
            row_h,
        )
    } else {
        let geometry = geometry(app, model.mode);
        let rect = paged_grid::cell_rect(
            &geometry.fit,
            &geometry.insets,
            i32::try_from(model.grid.current_row()).unwrap_or(0),
            i32::try_from(model.grid.current_column()).unwrap_or(0),
        );
        (
            rect.x as f32,
            (geometry.grid_y + rect.y) as f32,
            rect.width as f32,
            rect.height as f32,
        )
    }
}

/// The `Labels.menu` key for a row id; only the favorite toggle's copy
/// depends on the row's own state.
fn menu_key(id: &str, is_favorite: bool) -> &'static str {
    if id == "toggle_favorite" {
        return if is_favorite {
            "favorite:remove"
        } else {
            "favorite:add"
        };
    }
    match id {
        "more_info" | "change_launcher" | "write_card" | "qr_code" | "discover" | "add_to_hub"
        | "scrape_game" => id_static(id),
        _ => "",
    }
}

/// The menu ids are compile-time strings; hand the vocabulary the same
/// `'static` copy rather than allocating one per open.
fn id_static(id: &str) -> &'static str {
    match id {
        "more_info" => "more_info",
        "change_launcher" => "change_launcher",
        "write_card" => "write_card",
        "qr_code" => "qr_code",
        "discover" => "discover",
        "add_to_hub" => "add_to_hub",
        "scrape_game" => "scrape_game",
        _ => "",
    }
}

/// Options on the focused row (Main.qml's games/favorites/recents owners).
fn open_context_menu(ctx: &Ctx, app: &App) {
    let (row, input) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let Some(row) = model.current().cloned() else {
            return;
        };
        if !rules::context_menu_enabled(row.entry_type, row.media_capable, &row.path) {
            return;
        }
        let system = row.system_or(&model.system_id).to_string();
        let input = rules::MenuInput {
            owner: model.mode.owner(),
            entry_type: row.entry_type,
            media_capable: row.media_capable,
            pinnable_root: rules::is_filesystem_root(row.entry_type, &row.path),
            has_nfc: shared.has_nfc,
            is_favorite: row.is_favorite,
            is_arcade_system: system == ARCADE_SYSTEM_ID,
            has_launchers: shared.launchers.iter().any(|l| l.system_id == system),
            media_busy: crate::router::media_busy(app),
        };
        (row, input)
    };
    let entries: Vec<crate::MenuEntry> = rules::context_entries(&input)
        .into_iter()
        .map(|id| crate::router::menu_row_keyed(id, menu_key(id, row.is_favorite), ""))
        .collect();
    if entries.is_empty() {
        return;
    }
    let (x, y, w, h) = cell_anchor(ctx, app);
    crate::router::set_context_anchor(app, x, y, w, h);
    let index = lock(&ctx.shared).games.grid.current_index();
    crate::router::present_games_context_menu(ctx, app, index, entries);
    crate::router::refresh_readers(ctx);
}

/// A menu entry was accepted (the menu is already closed).
pub fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    let (row, index, mode, system) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let Some(row) = model.current().cloned() else {
            return;
        };
        let system = row.system_or(&model.system_id).to_string();
        (row, model.grid.current_index(), model.mode, system)
    };
    match id {
        "more_info" => crate::router::open_game_info(ctx, app, &row),
        "toggle_favorite" => toggle_favorite(ctx, app, index, &row),
        "write_card" => crate::router::begin_card_write(ctx, app, &row),
        "qr_code" => crate::router::open_qr_code(ctx, app, &row),
        "add_to_hub" => add_to_hub(ctx, app, mode, &row, &system),
        "scrape_game" if !system.is_empty() => {
            crate::router::start_scrape(ctx, app, vec![system], false);
        }
        "change_launcher" if !system.is_empty() => {
            crate::launchers::open_game_picker(ctx, app, &system, &row.path, row.media_id);
        }
        _ => {}
    }
}

/// "Discover alt. versions": the menu stays open while Core answers.
pub fn begin_discovery(ctx: &Ctx, app: &App) {
    let (system, name, path) = {
        let shared = lock(&ctx.shared);
        let model = &shared.games;
        let Some(row) = model.current() else {
            return;
        };
        (
            row.system_or(&model.system_id).to_string(),
            row.name.clone(),
            row.path.clone(),
        )
    };
    crate::alternates::begin(ctx, app, &system, &name, &path);
}

/// Rebuild the row's own menu after the alternates page is left.
pub fn reopen_context_menu(ctx: &Ctx, app: &App) {
    open_context_menu(ctx, app);
}

/// `add_to_hub` (Main.qml): folders and filesystem roots pin as `folder`
/// items; games pin as `zapscript` items carrying their run text.
fn add_to_hub(ctx: &Ctx, app: &App, mode: GamesMode, row: &GameRow, system: &str) {
    let folder = mode == GamesMode::Browse
        && !row.media_capable
        && (row.entry_type == EntryType::Directory
            || rules::is_filesystem_root(row.entry_type, &row.path));
    if folder {
        if !row.path.is_empty() {
            crate::hub::add_target(ctx, app, "folder", "", &row.path, "", "", "", system);
        }
        return;
    }
    let Some(script) = row.launch_text() else {
        return;
    };
    crate::hub::add_target(
        ctx,
        app,
        "zapscript",
        "",
        &row.path,
        &script,
        &row.display,
        "",
        system,
    );
}

/// Flip the `user:favorite` tag through the store mutation; the heart on
/// the tile is the feedback (the Qt rule).
fn toggle_favorite(ctx: &Ctx, app: &App, index: usize, row: &GameRow) {
    use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;
    use zaparoo_core::media_types::MediaTagsUpdateParams;

    let adding = !row.is_favorite;
    let mut params = MediaTagsUpdateParams::default();
    if adding {
        params.add.push(FAVORITE_TAG.to_string());
    } else {
        params.remove.push(FAVORITE_TAG.to_string());
    }
    if let Some(media_id) = row.media_id {
        params.media_id = Some(media_id);
    } else if !row.system_id.is_empty() && !row.path.is_empty() {
        params.system.clone_from(&row.system_id);
        params.path.clone_from(&row.path);
    } else {
        tracing::warn!(
            "favorite update skipped: missing media identity for {}",
            row.name
        );
        return;
    }
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let path = row.path.clone();
    let name = row.name.clone();
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<MediaTagsUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => {
                {
                    let mut shared = lock(&ctx2.shared);
                    if let Some(row) = shared.games.rows.get_mut(index).filter(|r| r.path == path) {
                        row.is_favorite = adding;
                    }
                }
                render(&ctx2, &app);
            }
            Err(e) => {
                tracing::warn!("favorite update failed for {name}: {}", e.message);
                crate::router::report_action_error(&ctx2, &app, "favorite", &name);
            }
        });
    });
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
        if model.sliding || model.loading {
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
        }
    }

    #[test]
    fn browse_rows_drop_ancestor_roots_and_distinguish_siblings() {
        let entries = vec![
            entry("root", "NES", "/media/fat/games/NES"),
            entry("root", "NES", "/media/usb0/games/NES"),
            entry("root", "games", "/media/fat/games"),
        ];
        let rows = browse_rows(&entries);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].root_distinguisher, "fat");
        assert_eq!(rows[1].root_distinguisher, "usb0");
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
        assert_eq!(model.jump_target(1), 4);
        assert_eq!(model.rows[model.jump_target(1)].name, "Bravo");
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
    fn detail_rows_prepend_the_system_for_flat_lists() {
        let mut model = GamesModel::new();
        model.mode = GamesMode::Favorites;
        let mut e = entry("media", "Sonic", "/g/Sonic.md");
        e.tags = vec![TagInfo {
            tag_type: "year".into(),
            tag: "1991".into(),
            label: String::new(),
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
