// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The router owns all forward orchestration, mirroring `Main.qml`'s
// contract: screens (the .slint views) never route; every action lands
// here, and this module decides what fills and what flips. The
// games entry uses deferred "select -> loading -> next" routing:
// source remains visible under the Loading cue until destination data
// is Ready, then cached/native route motion commits the new screen.

use crate::hub_nav;
use crate::media_cache::{MediaCache, MediaKey};
use crate::sizing;
use crate::{App, GameTile, Sizing, SystemTile};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::runtime::Handle;
use zaparoo_core::endpoints::media_browse::{BrowseArgs, MediaBrowseEndpoint};
use zaparoo_core::endpoints::media_favorites::{FavoritesArgs, MediaFavoritesEndpoint};
use zaparoo_core::endpoints::media_history::{HistoryArgs, MediaHistoryEndpoint};
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    BrowseEntry, MediaHistoryEntry, MediaHistoryLatestEntry, MediaItem, RunParams, SystemInfo,
};
use zaparoo_core::persist::{self, PersistedState};
use zaparoo_core::remote_resource::ResourceStatus;
use zaparoo_core::store::Store;

/// What the games-style grid is currently showing. Selects the back
/// target, whether folders/pagination apply, and where the selection
/// persists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamesMode {
    Browse,
    Favorites,
    Recents,
}

/// One row of the games-style grid, unified across `media.browse`
/// entries, favorites (`media.search`), and history entries.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent row property mirrored from the Qt model roles"
)]
pub struct GameRow {
    pub media_id: Option<i64>,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub system_id: String,
    pub zap_script: String,
    /// Compact disambiguation token labels (pre-sibling-diff).
    pub tag_labels: Vec<String>,
    /// `false` only when Core confirmed no cover exists; skips the
    /// image request entirely.
    pub has_cover: bool,
    /// `user:favorite` tag present (drives the context-menu toggle).
    pub is_favorite: bool,
    /// Media-capable rule from the Qt model: `media` entries, and
    /// directories that carry a media identity or zapscript.
    pub media_capable: bool,
}

impl From<&BrowseEntry> for GameRow {
    fn from(e: &BrowseEntry) -> Self {
        // A directory with a media id is a singleton media container
        // (folder- or zip-as-game, e.g. a CD game's folder): it
        // launches and carries cover art like a game, it does not
        // browse (the Qt model's
        // `singleton_directory_needs_launch_resolution` rule).
        let singleton_container = e.entry_type == "directory" && e.media_id.is_some();
        Self {
            media_id: e.media_id,
            name: e.name.clone(),
            path: e.path.clone(),
            // A system-filtered browse of the empty path answers with
            // the system's index roots as `root` entries; they browse
            // like any directory.
            is_dir: (e.entry_type == "directory" || e.entry_type == "root") && !singleton_container,
            system_id: e.system_id.clone(),
            zap_script: e.zap_script.clone(),
            tag_labels: crate::tag_utils::disambiguating_tag_labels(&e.disambiguating_tags),
            has_cover: e.has_cover,
            is_favorite: has_favorite_tag(&e.tags),
            media_capable: e.entry_type == "media"
                || (e.entry_type == "directory"
                    && (e.media_id.is_some() || !e.zap_script.is_empty())),
        }
    }
}

impl From<&MediaItem> for GameRow {
    fn from(item: &MediaItem) -> Self {
        Self {
            media_id: item.media_id,
            name: item.name.clone(),
            path: item.path.clone(),
            is_dir: false,
            system_id: item.system.id.clone(),
            zap_script: item.zap_script.clone(),
            tag_labels: Vec::new(),
            has_cover: true,
            is_favorite: has_favorite_tag(&item.tags),
            media_capable: true,
        }
    }
}

impl From<&MediaHistoryEntry> for GameRow {
    fn from(e: &MediaHistoryEntry) -> Self {
        Self {
            media_id: e.media_id,
            name: e.media_name.clone(),
            path: e.media_path.clone(),
            is_dir: false,
            system_id: e.system_id.clone(),
            zap_script: String::new(),
            tag_labels: Vec::new(),
            has_cover: true,
            // History rows carry no tag data; the Recents menu offers
            // no favorite toggle (the Qt rule), so this stays false.
            is_favorite: false,
            media_capable: true,
        }
    }
}

fn has_favorite_tag(tags: &[zaparoo_core::media_types::TagInfo]) -> bool {
    tags.iter()
        .any(|tag| tag.tag_type == "user" && tag.tag == "favorite")
}

/// Drop any `root`-type browse entry whose path is a strict ancestor
/// of another root entry's path (the Qt model's
/// `dedup_roots_drop_ancestors` rule): Core surfaces the shared parent
/// (e.g. `/media/fat`) as a root alongside the per-system root beneath
/// it, and the parent would browse into every other system.
fn dedup_roots_drop_ancestors(entries: &[BrowseEntry]) -> Vec<BrowseEntry> {
    let root_paths: Vec<&str> = entries
        .iter()
        .filter(|e| e.entry_type == "root" && !e.path.is_empty())
        .map(|e| e.path.trim_end_matches('/'))
        .collect();
    entries
        .iter()
        .filter(|e| {
            if e.entry_type != "root" || e.path.is_empty() {
                return true;
            }
            let candidate = e.path.trim_end_matches('/');
            !root_paths.iter().any(|other| {
                !candidate.is_empty()
                    && candidate != *other
                    && other
                        .strip_prefix(candidate)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        })
        .cloned()
        .collect()
}

/// Bottom-row Hub actions. Lower-case ids persist in `HubState::
/// selected_action`; display labels live in `ui/app.slint`. `resume`
/// leads, matching the Qt hub's action row.
pub const HUB_ACTIONS: [&str; 5] = ["resume", "favorites", "recents", "update", "settings"];

/// State shared between the router (Slint event loop thread) and the
/// tokio watcher tasks. Everything UI-visible is projected into App
/// properties; this holds the authoritative Rust-side copies.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent router state flags, not a state machine"
)]
pub struct Shared {
    /// Visible category names in display order (built-in hidden
    /// categories already filtered, user-hidden ones projected out
    /// unless `show_hidden`).
    pub categories: Vec<String>,
    /// All category names post built-in filter, pre user-hidden
    /// projection; the reproject source after a hide/unhide toggle.
    pub all_categories: Vec<String>,
    /// User-hidden browse prefs. Durable preferences, persisted to
    /// `frontend.toml` `[settings]` (not the volatile state file, which
    /// lives in `/tmp` on `MiSTer`) via `save_hidden_browse_prefs`.
    pub hidden_categories: Vec<String>,
    pub hidden_system_ids: Vec<String>,
    /// "Show hidden items" setting: hidden entries reappear dimmed
    /// with the "Hidden" badge instead of being filtered out.
    pub show_hidden: bool,
    /// Full sorted systems list from the catalog.
    pub systems: Vec<SystemInfo>,
    /// Systems currently shown on the Systems screen.
    pub screen_systems: Vec<SystemInfo>,
    /// What the games-style grid shows (browse / favorites / recents).
    pub games_mode: GamesMode,
    /// Entries currently shown on the Games screen (the active page).
    pub games_entries: Vec<GameRow>,
    /// All fetched pages for the current system, in order.
    pub games_pages: Vec<Vec<GameRow>>,
    /// Index into `games_pages` of the page on screen.
    pub games_page: usize,
    /// Cursor for the next unfetched page, if Core reported one.
    pub games_next_cursor: Option<String>,
    /// Total rows (dirs + files) Core reported for the current browse
    /// scope; None for the non-paginated favorites/recents fills.
    /// Drives the Qt-style "Page N / M" counter and position jumps.
    pub games_total_rows: Option<u32>,
    /// Leading directory count from the same totals: a letter bucket's
    /// item offset is relative to the media sequence, which starts
    /// after the dirs (the Qt jumpToItem rule).
    pub games_total_dirs: u32,
    /// Path of the browse backing the current fill: the cursor is a
    /// continuation of THIS query, so next-page fetches must repeat
    /// the same path or Core pages a different listing.
    pub games_browse_path: String,
    /// System id backing the games screen (fetches and cover keys).
    pub games_system_id: String,
    /// Display name of that system, for the screen title on folder
    /// navigation (which re-browses without passing through Systems).
    pub games_system_name: String,
    /// True while a next-page fetch is in flight; gates re-requests.
    pub games_fetching: bool,
    /// Set when the fetch was triggered by a page-swoop edge press:
    /// the arriving page slides in instead of cutting.
    pub games_slide_on_arrival: bool,
    /// Core reports at least one connected reader. Refreshed lazily.
    pub has_readers: bool,
    /// An NFC-class reader is present; gates the context-menu "Write
    /// to NFC token" entry (the Qt `hasNfc` rule - `has_readers` alone
    /// would count non-NFC readers too).
    pub has_nfc: bool,
    /// Which surface the open context menu targets, with the target's
    /// projected index on that surface.
    pub context_owner: ContextOwner,
    pub context_target: usize,
    /// What the open `ListPickerModal` is for (View menu vs a settings
    /// picker field).
    pub list_context: ListContext,
    /// Restart-applied Display setting waiting behind the confirmation
    /// dialog. CRT toggles/standards hand control back to Main; HDMI
    /// resolution changes self-exec after clean presenter shutdown.
    pub pending_restart: Option<PendingRestart>,
    /// Session-scoped "Re-scrape existing" one-shot; deliberately not
    /// persisted (Qt's `_visibleRescrapeExisting`), resets after the
    /// forced run starts.
    pub rescrape_existing: bool,
    /// Screensaver idle ticket: every input bumps it, so an armed
    /// timer from an earlier idle stretch fires as a no-op.
    pub saver_seq: u64,
    /// Detailed-list layout: absolute selection across every fetched
    /// row (`games_entries` holds the flattened list in this mode).
    pub list_index: usize,
    /// Core's launcher inventory + per-system defaults, for the
    /// "Change launcher" picker (`SystemLaunchers` model port).
    pub launchers: Vec<zaparoo_core::media_types::LauncherInfo>,
    pub system_defaults: Vec<zaparoo_core::media_types::SystemDefault>,
    /// Commercial-notice acknowledgement (durable, frontend.toml).
    pub notice_ack: bool,
    /// Core's reported version, once fetched; drives the min-version
    /// warning between the notice and the first-run gate.
    pub core_version: String,
    pub core_version_checked: bool,
    /// The version gate has resolved this session (shown or skipped).
    pub version_warning_shown: bool,
    /// The first-run index modal has opened once this session.
    pub first_run_shown: bool,
    pub first_run: FirstRunPhase,
    /// Core has confirmed the scan actually started; without this the
    /// optimistic Running phase would read the pre-start indexing=false
    /// snapshot as instant completion.
    pub first_run_saw_indexing: bool,
    pub first_run_cancelling: bool,
    /// Debounce/staleness ticket for the detail pane's media.meta
    /// fetch (`FocusedMediaDetailController`'s 220 ms rule).
    pub detail_seq: u64,
    /// Rapid-paging badge: when the previous page flip happened plus
    /// the ticket for the trailing hide timer.
    pub rapid_last_flip: Option<std::time::Instant>,
    pub rapid_seq: u64,
    /// Monotonic ticket for card writes: closing the menu bumps it so
    /// an in-flight write's result is ignored (the Qt cancel rule).
    pub card_write_seq: u64,
    /// Jump-to-letter buckets for the open picker (cursor kept
    /// Rust-side; the UI only shows label + count).
    pub letter_buckets: Vec<zaparoo_core::media_types::BrowseIndexGroup>,
    /// Ticket for the picker's facet fetch; bumped on open/close so a
    /// stale index response cannot fill a reopened picker.
    pub letter_seq: u64,
    pub persist: PersistedState,
    /// Monotonic ticket for games fills: a browse response only
    /// applies if its ticket is still current, so a stale response
    /// from a previous system cannot fill the grid.
    pub games_ticket: u64,
    /// True until the first catalog Ready has restored the persisted
    /// screen after a cold start.
    pub restore_pending: bool,
    /// Last-played entry backing the Hub's Resume action; None until
    /// `media.history.latest` answers (or when history is empty).
    pub resume_entry: Option<MediaHistoryLatestEntry>,
}

/// Which surface an open context menu was invoked on. Favorites and
/// Recents share the games grid, so `Games` covers all three
/// games-style modes (`GamesMode` disambiguates the entry set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextOwner {
    Games,
    Categories,
    Systems,
}

/// First-run index modal phase (FirstRunIndexModal.qml's idle /
/// running / completed vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRunPhase {
    Idle,
    Running,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingRestart {
    Resolution(String),
    CrtStandard(String),
    CrtEnabled(bool),
}

/// What the shared `ListPickerModal` is currently picking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListContext {
    /// The games-screen West "View" menu.
    ViewMenu,
    /// A settings picker row; the payload is the field id.
    SettingsPicker(String),
    /// The "Change launcher" picker; the payload is the system id.
    SystemLauncher(String),
}

#[derive(Debug, Clone)]
pub struct Ctx {
    pub store: Arc<Store>,
    pub handle: Handle,
    pub media: Arc<MediaCache>,
    pub shared: Arc<Mutex<Shared>>,
    /// Live 12-hour clock flag shared with the clock task; the
    /// Settings toggle flips it without a restart.
    pub clock_twelve_hour: Arc<std::sync::atomic::AtomicBool>,
    /// `frontend.toml` location, for durable settings mirrors.
    pub config_path: std::path::PathBuf,
    /// Immutable process mode; changing it requires Main to respawn us.
    pub crt_enabled: bool,
    pub is_mister: bool,
    /// Physical framebuffer geometry, used to resize desktop previews
    /// and by the `MiSTer` renderer when orientation changes live.
    pub framebuffer_size: (u32, u32),
}

/// Current games page size: the solved grid shape, so browse batches
/// are always a multiple of what one page shows.
fn games_page_size(app: &App) -> u32 {
    let cols = app
        .global::<crate::GamesView>()
        .get_games_grid_cols()
        .max(1) as u32;
    let rows = app
        .global::<crate::GamesView>()
        .get_games_grid_rows()
        .max(1) as u32;
    cols * rows
}

impl Shared {
    /// Fresh router state around the persisted snapshot; everything
    /// else starts empty and fills from Core.
    pub fn new(
        persist: PersistedState,
        restore_pending: bool,
        hidden_categories: Vec<String>,
        hidden_system_ids: Vec<String>,
    ) -> Self {
        let show_hidden = persist.settings.show_hidden;
        Self {
            categories: Vec::new(),
            all_categories: Vec::new(),
            hidden_categories,
            hidden_system_ids,
            show_hidden,
            systems: Vec::new(),
            screen_systems: Vec::new(),
            games_mode: GamesMode::Browse,
            games_entries: Vec::new(),
            games_pages: Vec::new(),
            games_page: 0,
            games_next_cursor: None,
            games_total_rows: None,
            games_total_dirs: 0,
            games_system_id: String::new(),
            games_system_name: String::new(),
            games_fetching: false,
            games_slide_on_arrival: false,
            has_readers: false,
            has_nfc: false,
            context_owner: ContextOwner::Games,
            context_target: 0,
            list_context: ListContext::ViewMenu,
            pending_restart: None,
            rescrape_existing: false,
            saver_seq: 0,
            list_index: 0,
            launchers: Vec::new(),
            system_defaults: Vec::new(),
            notice_ack: false,
            core_version: String::new(),
            core_version_checked: false,
            version_warning_shown: false,
            first_run_shown: false,
            first_run: FirstRunPhase::Idle,
            first_run_saw_indexing: false,
            first_run_cancelling: false,
            detail_seq: 0,
            rapid_last_flip: None,
            rapid_seq: 0,
            card_write_seq: 0,
            letter_buckets: Vec::new(),
            letter_seq: 0,
            games_browse_path: String::new(),
            persist,
            games_ticket: 0,
            restore_pending,
            resume_entry: None,
        }
    }
}

pub fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
    // A poisoned mutex means a panicking thread died mid-update; the
    // data is still structurally valid, so keep going.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn save_persist(shared: &Arc<Mutex<Shared>>) {
    let snapshot = lock(shared).persist.clone();
    persist::save(&snapshot);
}

/// Systems belonging to `category`, using the same "empty category
/// buckets as Other" rule as `CatalogData::systems_by_category`.
fn systems_for_category(systems: &[SystemInfo], category: &str) -> Vec<SystemInfo> {
    systems
        .iter()
        .filter(|s| {
            if s.category.is_empty() {
                category == "Other"
            } else {
                s.category == category
            }
        })
        .cloned()
        .collect()
}

/// Category systems with the user-hidden projection applied: hidden
/// systems drop out, or stay (rendered dimmed with the badge) when
/// Show hidden items is on.
fn projected_systems_for_category(shared: &Shared, category: &str) -> Vec<SystemInfo> {
    systems_for_category(&shared.systems, category)
        .into_iter()
        .filter(|s| shared.show_hidden || !shared.hidden_system_ids.iter().any(|h| h == &s.id))
        .collect()
}

/// Rebuild the Hub's category projection from the master list and the
/// hidden prefs, restoring focus to the persisted category when it
/// survives the projection.
pub fn reproject_hub(ctx: &Ctx, app: &App) {
    let (all, hidden, show, persisted_category) = {
        let shared = lock(&ctx.shared);
        (
            shared.all_categories.clone(),
            shared.hidden_categories.clone(),
            shared.show_hidden,
            shared.persist.hub.category.clone(),
        )
    };
    let visible: Vec<(String, bool)> = all
        .into_iter()
        .filter_map(|name| {
            let is_hidden = hidden.iter().any(|h| h == &name);
            (show || !is_hidden).then_some((name, is_hidden))
        })
        .collect();
    lock(&ctx.shared).categories = visible.iter().map(|(n, _)| n.clone()).collect();
    let tiles: Vec<crate::CategoryTile> = visible
        .iter()
        .map(|(name, is_hidden)| crate::CategoryTile {
            name: SharedString::from(name.as_str()),
            hidden: *is_hidden,
        })
        .collect();
    let index = visible
        .iter()
        .position(|(n, _)| *n == persisted_category)
        .unwrap_or(0);
    app.global::<crate::HubView>()
        .set_categories(ModelRc::new(VecModel::from(tiles)));
    app.global::<crate::HubView>()
        .set_hub_category_index(i32::try_from(index).unwrap_or(0));
}

/// Re-run the Systems screen's projection after a hide/unhide or a
/// Show-hidden flip, keeping the current page in range.
pub fn reproject_systems(ctx: &Ctx, app: &App) {
    let category = app
        .global::<crate::SystemsView>()
        .get_systems_category()
        .to_string();
    if category.is_empty() {
        return;
    }
    let projected = {
        let shared = lock(&ctx.shared);
        projected_systems_for_category(&shared, &category)
    };
    lock(&ctx.shared).screen_systems = projected;
    let page = app.global::<crate::SystemsView>().get_systems_page().max(0) as usize;
    show_systems_page(ctx, app, page);
}

/// Flip a category's hidden flag, persist the durable pref, and
/// reproject the Hub.
fn toggle_hidden_category(ctx: &Ctx, app: &App, name: &str) {
    let (cats, sys) = {
        let mut shared = lock(&ctx.shared);
        if shared.hidden_categories.iter().any(|h| h == name) {
            shared.hidden_categories.retain(|h| h != name);
        } else {
            shared.hidden_categories.push(name.to_string());
        }
        (
            shared.hidden_categories.clone(),
            shared.hidden_system_ids.clone(),
        )
    };
    save_hidden_prefs(ctx, &cats, &sys);
    reproject_hub(ctx, app);
}

/// Flip a system's hidden flag, persist, and reproject the grid.
fn toggle_hidden_system(ctx: &Ctx, app: &App, id: &str) {
    let (cats, sys) = {
        let mut shared = lock(&ctx.shared);
        if shared.hidden_system_ids.iter().any(|h| h == id) {
            shared.hidden_system_ids.retain(|h| h != id);
        } else {
            shared.hidden_system_ids.push(id.to_string());
        }
        (
            shared.hidden_categories.clone(),
            shared.hidden_system_ids.clone(),
        )
    };
    save_hidden_prefs(ctx, &cats, &sys);
    reproject_systems(ctx, app);
}

/// Durable hidden-browse prefs go to `frontend.toml`, not the volatile
/// state file (which lives in `/tmp` on `MiSTer`). Small atomic write.
fn save_hidden_prefs(ctx: &Ctx, categories: &[String], system_ids: &[String]) {
    if let Err(e) =
        zaparoo_core::config::save_hidden_browse_prefs(&ctx.config_path, categories, system_ids)
    {
        tracing::warn!("could not save hidden browse prefs: {e}");
    }
}

// ---------- Startup decision dialogs (Modal.qml chain port) ----------

/// Dev builds and unparseable versions fail open (the Qt
/// `version_supported` rule): only a parsed semver below the floor
/// triggers the warning.
const MIN_CORE_VERSION: &str = "2.15.0";

fn version_supported(raw: &str) -> bool {
    let v = raw.trim();
    if v == "DEVELOPMENT" || v.ends_with("-dev") {
        return true;
    }
    match (
        semver::Version::parse(v),
        semver::Version::parse(MIN_CORE_VERSION),
    ) {
        (Ok(current), Ok(min)) => current >= min,
        _ => true,
    }
}

#[cfg(test)]
mod version_gate_tests {
    use super::version_supported;

    #[test]
    fn dev_and_unparseable_fail_open() {
        assert!(version_supported("DEVELOPMENT"));
        assert!(version_supported("2.14.0-dev"));
        assert!(version_supported("weird"));
        assert!(version_supported(""));
    }

    #[test]
    fn semver_floor_enforced() {
        assert!(version_supported("2.15.0"));
        assert!(version_supported("3.0.1"));
        assert!(!version_supported("2.14.9"));
        assert!(!version_supported("1.0.0"));
    }
}

fn open_dialog(app: &App, kind: &str, title: &str, body: &str, buttons: &[&str], focus: i32) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_kind(SharedString::from(kind));
    overlays.set_dialog_title(SharedString::from(title));
    overlays.set_dialog_body(SharedString::from(body));
    overlays.set_dialog_status(SharedString::default());
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(
        buttons
            .iter()
            .map(|b| SharedString::from(*b))
            .collect::<Vec<_>>(),
    )));
    overlays.set_dialog_focus(focus);
    overlays.set_dialog_open(true);
}

fn close_dialog(app: &App) {
    app.global::<crate::Overlays>().set_dialog_open(false);
    app.global::<crate::Overlays>()
        .set_dialog_kind(SharedString::default());
}

/// Hub Back lands here instead of quitting outright, so a stray B
/// can't kill the frontend (Main.qml's quit-confirm rule). Default
/// focus is "No".
fn open_quit_confirm(app: &App) {
    open_dialog(
        app,
        "quit_confirm",
        "Quit Zaparoo Frontend?",
        "Are you sure you want to exit?",
        &["Yes", "No"],
        1,
    );
}

/// Advance the sequential startup chain: commercial notice ->
/// core-version warning -> first-run index gate. Re-entered from each
/// dialog's close and from the catalog/version fetch completions, so
/// the modals never stack.
pub fn maybe_open_startup_notices(ctx: &Ctx, app: &App) {
    if app.global::<crate::Overlays>().get_dialog_open()
        || !app.global::<crate::Shell>().get_boot_complete()
    {
        return;
    }
    let (notice_ack, version_checked, version_shown, version, first_run_shown, indexed) = {
        let guard = lock(&ctx.shared);
        (
            guard.notice_ack,
            guard.core_version_checked,
            guard.version_warning_shown,
            guard.core_version.clone(),
            guard.first_run_shown,
            guard
                .systems
                .iter()
                .filter(|s| s.zap_script.is_empty())
                .count(),
        )
    };
    if !notice_ack {
        open_dialog(
            app,
            "notice",
            "Welcome to Zaparoo Frontend",
            "Copyright 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.\n\n\
             This free source-available build is for personal and non-commercial \
             use only. Commercial use requires a separate license.\n\n\
             Contact: legal@zaparoo.org\n\n\
             Full details available any time under Settings > About / License.",
            &["I understand"],
            0,
        );
        return;
    }
    if !version_shown {
        if !version_checked {
            check_core_version(ctx, app);
            return;
        }
        lock(&ctx.shared).version_warning_shown = true;
        if !version_supported(&version) {
            open_dialog(
                app,
                "core_version",
                "Update Zaparoo Core",
                &format!(
                    "This frontend needs Zaparoo Core {MIN_CORE_VERSION} or newer. \
                     You're running {version}. Some features may not work until you update."
                ),
                &["OK"],
                0,
            );
            return;
        }
    }
    // First-run gate: a catalog with no indexed (non-launchable)
    // systems means the media database has never been built.
    if !first_run_shown && indexed == 0 {
        lock(&ctx.shared).first_run_shown = true;
        open_dialog(
            app,
            "first_run",
            "First-time setup",
            "Zaparoo needs to scan your games before you can use the frontend. \
             This usually takes a few minutes.",
            &["Start scan"],
            0,
        );
    }
}

/// One-shot Core version fetch; failure fails open (checked with an
/// empty version string, which `version_supported` accepts).
fn check_core_version(ctx: &Ctx, app: &App) {
    {
        let mut guard = lock(&ctx.shared);
        if guard.core_version_checked {
            return;
        }
        guard.core_version_checked = true;
    }
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let version = match client.version().await {
            Ok(result) => result.version,
            Err(_) => String::new(),
        };
        lock(&ctx2.shared).core_version = version;
        let _ = weak.upgrade_in_event_loop(move |app| {
            maybe_open_startup_notices(&ctx2, &app);
        });
    });
}

fn dialog_action(ctx: &Ctx, app: &App, action: &str) {
    use slint::Model as _;
    let overlays = app.global::<crate::Overlays>();
    let kind = overlays.get_dialog_kind().to_string();
    let len = overlays.get_dialog_buttons().row_count();
    let focus = overlays.get_dialog_focus().max(0) as usize;
    match action {
        actions::LEFT if len > 1 && focus > 0 => {
            overlays.set_dialog_focus((focus - 1) as i32);
        }
        actions::RIGHT if len > 1 && focus + 1 < len => {
            overlays.set_dialog_focus((focus + 1) as i32);
        }
        actions::ACCEPT => dialog_accept(ctx, app, &kind, focus),
        actions::CANCEL => dialog_cancel(ctx, app, &kind),
        _ => {}
    }
}

fn dialog_accept(ctx: &Ctx, app: &App, kind: &str, focus: usize) {
    match kind {
        "quit_confirm" => {
            if focus == 0 {
                let _ = slint::quit_event_loop();
            } else {
                close_dialog(app);
            }
        }
        "notice" => {
            lock(&ctx.shared).notice_ack = true;
            if let Err(e) = zaparoo_core::config::save_notice_ack(&ctx.config_path, true) {
                tracing::warn!("could not persist notice ack: {e}");
            }
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        "core_version" => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        "first_run" => first_run_accept(ctx, app),
        "restart_setting" => {
            if focus == 0 {
                confirm_pending_restart(ctx, app);
            } else {
                lock(&ctx.shared).pending_restart = None;
                close_dialog(app);
            }
        }
        _ => close_dialog(app),
    }
}

fn dialog_cancel(ctx: &Ctx, app: &App, kind: &str) {
    match kind {
        // The notice must be acknowledged; the idle first-run gate has
        // no skip (Qt closes it only when indexed systems appear out
        // of band).
        "notice" => {}
        "first_run" => {
            let running = lock(&ctx.shared).first_run == FirstRunPhase::Running;
            if running {
                lock(&ctx.shared).first_run_cancelling = true;
                let client = ctx.store.client();
                ctx.handle.spawn(async move {
                    if let Err(e) = client.media_generate_cancel().await {
                        tracing::warn!("first-run cancel failed: {}", e.message);
                    }
                });
            }
        }
        "core_version" => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        "restart_setting" => {
            lock(&ctx.shared).pending_restart = None;
            close_dialog(app);
        }
        _ => close_dialog(app),
    }
}

fn first_run_accept(ctx: &Ctx, app: &App) {
    let phase = lock(&ctx.shared).first_run;
    match phase {
        FirstRunPhase::Idle => {
            {
                let mut guard = lock(&ctx.shared);
                guard.first_run = FirstRunPhase::Running;
                guard.first_run_saw_indexing = false;
                guard.first_run_cancelling = false;
            }
            start_index(ctx, app, None);
            let overlays = app.global::<crate::Overlays>();
            overlays.set_dialog_status(SharedString::from("Preparing…"));
            overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![SharedString::from(
                "Cancel",
            )])));
            overlays.set_dialog_focus(0);
        }
        FirstRunPhase::Running => {}
        FirstRunPhase::Done => close_dialog(app),
    }
}

/// Track a running first-run scan against Core's media-status flags:
/// progress line while indexing/optimizing, then Done (or back to
/// Idle after a cancel). Called from the media-status watcher.
pub fn refresh_first_run(ctx: &Ctx, app: &App) {
    if !app.global::<crate::Overlays>().get_dialog_open()
        || app.global::<crate::Overlays>().get_dialog_kind().as_str() != "first_run"
    {
        return;
    }
    if lock(&ctx.shared).first_run != FirstRunPhase::Running {
        return;
    }
    let ms = media_state(ctx);
    let overlays = app.global::<crate::Overlays>();
    if ms.indexing || ms.optimizing {
        lock(&ctx.shared).first_run_saw_indexing = true;
        let line = if ms.optimizing {
            "Optimizing database - almost done".to_string()
        } else if ms.paused {
            "Indexing paused".to_string()
        } else if ms.total_steps > 0 {
            if ms.current_step_display.is_empty() {
                format!("Step {} of {}", ms.current_step.max(0), ms.total_steps)
            } else {
                format!(
                    "Step {} of {} - {}",
                    ms.current_step.max(0),
                    ms.total_steps,
                    ms.current_step_display
                )
            }
        } else {
            "Preparing…".to_string()
        };
        overlays.set_dialog_status(SharedString::from(line.as_str()));
        return;
    }
    // indexing and optimizing both clear: completion or cancel, but
    // only after Core confirmed the run actually started.
    let (saw, cancelling) = {
        let guard = lock(&ctx.shared);
        (guard.first_run_saw_indexing, guard.first_run_cancelling)
    };
    if !saw {
        return;
    }
    if cancelling {
        let mut guard = lock(&ctx.shared);
        guard.first_run = FirstRunPhase::Idle;
        guard.first_run_cancelling = false;
        guard.first_run_saw_indexing = false;
        drop(guard);
        overlays.set_dialog_status(SharedString::default());
        overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![SharedString::from(
            "Start scan",
        )])));
        overlays.set_dialog_focus(0);
        return;
    }
    lock(&ctx.shared).first_run = FirstRunPhase::Done;
    overlays.set_dialog_status(SharedString::default());
    overlays.set_dialog_body(SharedString::from(
        format!("Done. {} files indexed.", ms.total_files.max(0)).as_str(),
    ));
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![SharedString::from("OK")])));
    overlays.set_dialog_focus(0);
}

/// Restart the screensaver idle countdown. Every input calls this;
/// the seq ticket makes earlier armed timers no-ops. "off" disables
/// arming entirely (a stale timer still fires but fails the ticket).
pub fn reset_idle(ctx: &Ctx, app: &App) {
    let (ticket, timeout) = {
        let mut shared = lock(&ctx.shared);
        shared.saver_seq += 1;
        (
            shared.saver_seq,
            shared.persist.settings.screensaver_timeout.clone(),
        )
    };
    let Ok(secs) = timeout.parse::<u64>() else {
        return; // "off" or malformed - never arms
    };
    if secs == 0 {
        return;
    }
    let weak = app.as_weak();
    let shared = ctx.shared.clone();
    slint::Timer::single_shot(std::time::Duration::from_secs(secs), move || {
        if lock(&shared).saver_seq != ticket {
            return;
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        // The boot curtain and the transition "Loading…" cue are not
        // burn targets; skip and let the next input re-arm the clock.
        if !app.global::<crate::Shell>().get_boot_complete()
            || app.global::<crate::Shell>().get_transitioning()
            || app.global::<crate::Shell>().get_route_transitioning()
        {
            return;
        }
        app.global::<crate::Shell>().set_saver_armed(true);
    });
}

const ROUTE_PREPARE_MS: u64 = 34;
const ROUTE_SETTLE_MS: u64 = 190;

#[cfg(feature = "mister")]
fn request_cached_route_transition(app: &App, direction: i32) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_orientation().as_str() != "horizontal" {
        return false;
    }
    let sizing = app.global::<Sizing>();
    let width = sizing.get_screen_width().round().max(0.0) as u32;
    let height = sizing.get_screen_height().round().max(0.0) as u32;
    let Some(geometry) = sizing::mister_route_transition_geometry(width, height) else {
        return false;
    };
    crate::mister::request_route_transition(geometry, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached_route_transition(_app: &App, _direction: i32) -> bool {
    false
}

fn finish_route_transition(app: &App) {
    let shell = app.global::<crate::Shell>();
    let target = shell.get_route_to_screen();
    shell.set_route_slide_anim(false);
    shell.set_active_screen(target);
    shell.set_route_cached_transition(false);
    shell.set_route_page_slide(0.0);
    shell.set_route_from_screen(SharedString::default());
    shell.set_route_to_screen(SharedString::default());
    shell.set_route_transitioning(false);

    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(app) = weak.upgrade() {
            app.global::<crate::Shell>().set_route_slide_anim(true);
        }
    });
}

fn begin_route_transition(app: &App) {
    let shell = app.global::<crate::Shell>();
    if !shell.get_route_transitioning() {
        return;
    }
    let direction = shell.get_route_slide_dir();
    if request_cached_route_transition(app, direction) {
        shell.set_route_cached_transition(true);
        shell.set_active_screen(shell.get_route_to_screen());
    } else {
        shell.set_route_slide_anim(true);
        shell.set_route_page_slide(direction as f32);
    }

    let expected_target = shell.get_route_to_screen();
    let weak = app.as_weak();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(ROUTE_SETTLE_MS),
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let shell = app.global::<crate::Shell>();
            if shell.get_route_transitioning() && shell.get_route_to_screen() == expected_target {
                finish_route_transition(&app);
            }
        },
    );
}

/// Navigate one level through the screen hierarchy. Async fills currently
/// show a Loading cue; clear it and allow two clean source frames before
/// caching so that cue never rides out with the old page.
fn transition_to_screen(app: &App, target: &str, direction: i32) {
    let shell = app.global::<crate::Shell>();
    let current = shell.get_active_screen();
    if current.as_str() == target {
        shell.set_transitioning(false);
        return;
    }
    if shell.get_reduce_motion() {
        shell.set_transitioning(false);
        shell.set_active_screen(SharedString::from(target));
        return;
    }
    if shell.get_route_transitioning() {
        return;
    }

    shell.set_route_transitioning(true);
    shell.set_route_from_screen(current);
    shell.set_route_to_screen(SharedString::from(target));
    shell.set_route_slide_dir(if direction > 0 { 1 } else { -1 });
    shell.set_route_page_slide(0.0);

    if shell.get_transitioning() {
        shell.set_transitioning(false);
        let weak = app.as_weak();
        slint::Timer::single_shot(
            std::time::Duration::from_millis(ROUTE_PREPARE_MS),
            move || {
                if let Some(app) = weak.upgrade() {
                    begin_route_transition(&app);
                }
            },
        );
    } else {
        begin_route_transition(app);
    }
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    // Screensaver eats the waking press whole (the Qt dismiss path):
    // disarm, restart the idle clock, swallow.
    if app.global::<crate::Shell>().get_saver_armed() {
        app.global::<crate::Shell>().set_saver_armed(false);
        reset_idle(ctx, app);
        return;
    }
    reset_idle(ctx, app);
    // CRT calibration owns the full physical framebuffer and routes
    // arrows directly to live DDR trims above every regular modal.
    if app.global::<crate::Overlays>().get_crt_calibration_open() {
        crt_calibration_action(ctx, app, action);
        return;
    }
    // Decision dialogs (quit confirm / startup chain) own all input
    // while open, above every other surface.
    if app.global::<crate::Overlays>().get_dialog_open() {
        dialog_action(ctx, app, action);
        return;
    }
    // Cold-launch curtain swallows input except Cancel, which still
    // quits from the (hidden) hub root so a frontend that can't reach
    // Core is never a trap on desktop.
    if app.global::<crate::Shell>().get_boot_curtain() && action != actions::CANCEL {
        return;
    }
    // Single input gate during forward transitions, as in Main.qml.
    // Modals run on top of the gate; pickers and menus are topmost.
    if app.global::<crate::Overlays>().get_qr_open() {
        // Static display: any Cancel closes, everything else swallowed.
        if action == actions::CANCEL {
            app.global::<crate::Overlays>().set_qr_open(false);
        }
        return;
    }
    if app.global::<crate::Overlays>().get_card_write_open() {
        // Transient modal: Cancel only, everything else swallowed.
        if action == actions::CANCEL {
            lock(&ctx.shared).card_write_seq += 1;
            app.global::<crate::Overlays>().set_card_write_open(false);
        }
        return;
    }
    if app.global::<crate::Overlays>().get_letter_open() {
        letter_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Overlays>().get_list_open() {
        list_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Overlays>().get_context_open() {
        context_action(ctx, app, action);
        return;
    }
    if app.global::<crate::GameInfoView>().get_modal_open() {
        modal_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Shell>().get_transitioning()
        || app.global::<crate::Shell>().get_route_transitioning()
    {
        return;
    }
    match app.global::<crate::Shell>().get_active_screen().as_str() {
        "hub" => hub_action(ctx, app, action),
        "systems" => systems_action(ctx, app, action),
        // Favorites and Recents reuse the games-style grid; the mode
        // stored in Shared adjusts back/paging/persist behavior.
        "games" | "favorites" | "recents" => games_action(ctx, app, action),
        "settings" => settings_action(ctx, app, action),
        "about" => about_action(ctx, app, action),
        _ => {}
    }
}

// ---------- Settings editor (SettingsScreen.qml port) ----------
//
// The Qt screen is a root category grid over per-domain field pages;
// the registries below mirror its field ids and option lists (the
// canonical value sets from rust/frontend/src/models/settings.rs).
// Display includes live orientation plus restart-applied MiSTer HDMI
// resolution and native-CRT controls. Log upload remains outside this
// migration slice.

const SETTINGS_PAGES: &[(&str, &str, &str)] = &[
    ("pageDisplayInterface", "Display", "Display"),
    ("pageBrowsing", "Browsing", "Browsing"),
    ("pageLanguage", "Language", "Language"),
    ("pageControlsInput", "Controls", "Controls"),
    ("pageLibraryData", "Library", "Library"),
    ("pageSupportAbout", "Support", "Support"),
];

const LANGUAGES: &[&str] = &[
    "auto", "en", "it_IT", "es", "eu", "de", "el", "ja", "ko", "nl", "ro", "sk", "uk", "zh_CN",
    "zh_TW", "he", "ar", "hi",
];
const CLOCK_FORMATS: &[&str] = &["auto", "12h", "24h"];
const REGIONS: &[&str] = &["auto", "us", "eu", "jp"];
const ORIENTATIONS: &[&str] = &["horizontal", "cw", "ccw"];
const MISTER_RESOLUTIONS: &[&str] = &[
    "",
    "1280x720",
    "1920x1080",
    "2560x1440",
    "1920x1200",
    "1920x1440",
    "640x480",
    "2048x1536",
];
const CRT_VIDEO_STANDARDS: &[&str] = &["ntsc", "pal"];
const BROWSE_LAYOUTS: &[&str] = &["grid", "list"];
const SYSTEM_LOGO_STYLES: &[&str] = &["tinted", "color"];
const BUTTON_LAYOUTS: &[&str] = &["a", "b", "c", "d"];
const SCREENSAVER_TIMEOUTS: &[&str] = &["off", "60", "120", "300", "600", "900", "1800"];
const MEDIA_IMAGE_TYPES: &[&str] = &[
    "auto",
    "image",
    "thumbnail",
    "boxart",
    "boxart3d",
    "screenshot",
    "wheel",
    "titleshot",
    "map",
    "marquee",
    "fanart",
    "boxartside",
    "boxartback",
];

fn language_display(value: &str) -> &'static str {
    match value {
        "en" | "en_US" | "en_GB" => "English",
        "it" | "it_IT" => "Italian",
        "es" | "es_ES" => "Spanish",
        "eu" | "eu_ES" => "Basque",
        "de" | "de_DE" => "German",
        "el" | "el_GR" => "Greek",
        "ja" | "ja_JP" => "Japanese",
        "ko" | "ko_KR" => "Korean",
        "nl" | "nl_NL" => "Dutch",
        "ro" | "ro_RO" => "Romanian",
        "sk" | "sk_SK" => "Slovak",
        "uk" | "uk_UA" => "Ukrainian",
        "zh_CN" => "Chinese (Simplified)",
        "zh_TW" | "zh_HK" => "Chinese (Traditional)",
        "he" | "he_IL" => "Hebrew",
        "ar" | "ar_SA" => "Arabic",
        "hi" | "hi_IN" => "Hindi",
        _ => "Auto",
    }
}

fn clock_format_display(value: &str) -> &'static str {
    match value {
        "12h" => "12-hour",
        "24h" => "24-hour",
        _ => "Auto",
    }
}

fn region_display(value: &str) -> &'static str {
    match value {
        "us" => "Americas",
        "eu" => "Europe",
        "jp" => "Japan",
        _ => "Automatic",
    }
}

fn orientation_display(value: &str) -> &'static str {
    match value {
        "cw" => "Rotated CW",
        "ccw" => "Rotated CCW",
        _ => "Horizontal",
    }
}

fn resolution_display(value: &str) -> &str {
    if value.is_empty() {
        "Default"
    } else {
        value
    }
}

fn crt_standard_display(value: &str) -> &'static str {
    match value {
        "pal" => "PAL 288p50",
        _ => "NTSC 240p60",
    }
}

fn browse_layout_display(value: &str) -> &'static str {
    if value == "list" {
        "Detailed list view"
    } else {
        "Grid view"
    }
}

fn system_logo_style_display(value: &str) -> &'static str {
    if value == "color" {
        "Full color"
    } else {
        "Tinted"
    }
}

fn button_layout_display(value: &str) -> &'static str {
    match value {
        "b" => "Style B",
        "c" => "Style C",
        "d" => "Style D",
        _ => "Style A",
    }
}

fn screensaver_timeout_display(value: &str) -> &'static str {
    match value {
        "off" => "Off",
        "60" => "1 minute",
        "120" => "2 minutes",
        "600" => "10 minutes",
        "900" => "15 minutes",
        "1800" => "30 minutes",
        _ => "5 minutes",
    }
}

fn media_image_type_display(value: &str) -> &'static str {
    match value {
        "image" => "Image",
        "thumbnail" => "Thumbnail",
        "boxart" => "Box art",
        "boxart3d" => "3D box art",
        "screenshot" => "Screenshot",
        "wheel" => "Wheel",
        "titleshot" => "Title screen",
        "map" => "Map",
        "marquee" => "Marquee",
        "fanart" => "Fan art",
        "boxartside" => "Box art (side)",
        "boxartback" => "Box art (back)",
        _ => "Auto",
    }
}

fn make_field(
    kind: &str,
    id: &str,
    label: &str,
    value: &str,
    control: &str,
    checked: bool,
    enabled: bool,
) -> crate::SettingsField {
    crate::SettingsField {
        kind: SharedString::from(kind),
        id: SharedString::from(id),
        label: SharedString::from(label),
        value: SharedString::from(value),
        control: SharedString::from(control),
        checked,
        enabled,
    }
}

fn picker_field(id: &str, label: &str, value: &str) -> crate::SettingsField {
    make_field("field", id, label, value, "picker", false, true)
}

fn toggle_field(id: &str, label: &str, checked: bool, enabled: bool) -> crate::SettingsField {
    make_field("field", id, label, "", "toggle", checked, enabled)
}

fn action_field(id: &str, label: &str, verb: &str, enabled: bool) -> crate::SettingsField {
    make_field("field", id, label, verb, "action", false, enabled)
}

/// Current media-database state, read synchronously off the store's
/// watch resource (the Qt _indexBusy/_scrapeBusy gates).
fn media_state(ctx: &Ctx) -> zaparoo_core::store::MediaStatusState {
    let rx = ctx.store.media_status().subscribe();
    let state = rx.borrow().clone();
    state
}

fn settings_page_title(page: &str) -> &'static str {
    SETTINGS_PAGES
        .iter()
        .find(|(id, _, _)| *id == page)
        .map_or("Settings", |(_, label, _)| label)
}

fn display_settings_fields(
    ctx: &Ctx,
    settings: &persist::SettingsState,
) -> Vec<crate::SettingsField> {
    let mut fields = Vec::new();
    if ctx.is_mister {
        fields.push(picker_field(
            "resolution",
            "Resolution",
            resolution_display(&settings.resolution),
        ));
    }
    fields.push(picker_field(
        "orientation",
        "Orientation",
        orientation_display(&settings.orientation),
    ));
    fields.push(picker_field(
        "screensaverTimeout",
        "Screensaver",
        screensaver_timeout_display(&settings.screensaver_timeout),
    ));
    if ctx.is_mister {
        fields.push(make_field(
            "header",
            "",
            "Analog video",
            "",
            "",
            false,
            true,
        ));
        fields.push(toggle_field(
            "crtEnabled",
            "CRT mode",
            ctx.crt_enabled,
            true,
        ));
        if ctx.crt_enabled {
            fields.push(picker_field(
                "crtVideoStandard",
                "Video standard",
                crt_standard_display(&settings.crt_video_standard),
            ));
            fields.push(action_field(
                "crtCalibration",
                "Screen position",
                "Open",
                true,
            ));
        }
    }
    fields
}

/// Field registry for a settings page ("" = the root category grid),
/// mirroring SettingsScreen.qml's per-domain lists.
fn settings_fields(ctx: &Ctx, page: &str) -> Vec<crate::SettingsField> {
    let (s, show_hidden, rescrape) = {
        let shared = lock(&ctx.shared);
        (
            shared.persist.settings.clone(),
            shared.show_hidden,
            shared.rescrape_existing,
        )
    };
    match page {
        "" => SETTINGS_PAGES
            .iter()
            .map(|(id, label, glyph)| make_field("field", id, label, glyph, "action", false, true))
            .collect(),
        "pageDisplayInterface" => display_settings_fields(ctx, &s),
        "pageBrowsing" => vec![
            picker_field(
                "browseLayout",
                "Browsing layout",
                browse_layout_display(&s.games_browse_layout),
            ),
            picker_field(
                "systemLogoStyle",
                "System logos",
                system_logo_style_display(&s.system_logo_style),
            ),
            picker_field(
                "mediaImageType",
                "Preferred artwork",
                media_image_type_display(&s.media_image_type),
            ),
            toggle_field("showHidden", "Show hidden items", show_hidden, true),
            toggle_field(
                "showOriginalFilenames",
                "Show original filenames",
                s.show_original_filenames,
                true,
            ),
        ],
        "pageLanguage" => vec![
            picker_field("language", "Language", language_display(&s.language)),
            picker_field("region", "System names", region_display(&s.region)),
            picker_field(
                "clockFormat",
                "Clock format",
                clock_format_display(&s.clock_format),
            ),
        ],
        "pageControlsInput" => vec![
            picker_field(
                "buttonLayout",
                "Button style",
                button_layout_display(&s.button_layout),
            ),
            toggle_field("mouseEnabled", "Mouse support", s.mouse_enabled, true),
            toggle_field("reduceMotion", "Reduce motion", s.reduce_motion, true),
        ],
        "pageLibraryData" => {
            let ms = media_state(ctx);
            let index_busy = ms.indexing || ms.optimizing;
            let scrape_busy = ms.scraping;
            vec![
                action_field(
                    "updateMediaDb",
                    "Update media database",
                    if index_busy { "Cancel" } else { "Start" },
                    !scrape_busy,
                ),
                action_field(
                    "runScraper",
                    "Scrape metadata",
                    if scrape_busy { "Cancel" } else { "Start" },
                    !index_busy,
                ),
                toggle_field(
                    "rescrapeExisting",
                    "Re-scrape existing",
                    rescrape,
                    !index_busy && !scrape_busy,
                ),
            ]
        }
        "pageSupportAbout" => vec![
            action_field("aboutLicense", "About / License", "Open", true),
            toggle_field("debugLogging", "Debug logging", s.debug_logging, true),
        ],
        _ => Vec::new(),
    }
}

/// First focusable row (headers are transparent to focus).
fn first_navigable(fields: &[crate::SettingsField]) -> usize {
    fields.iter().position(|f| f.kind == "field").unwrap_or(0)
}

/// Walk from `from` in `dir` until a focusable row, wrapping at the
/// edges (the Qt _seekNavigable rule).
fn seek_navigable(fields: &[crate::SettingsField], from: usize, dir: i64) -> usize {
    let len = fields.len();
    if len == 0 {
        return from;
    }
    let mut i = from as i64;
    for _ in 0..len {
        i += dir;
        if i < 0 {
            i = len as i64 - 1;
        } else if i >= len as i64 {
            i = 0;
        }
        if fields[i as usize].kind == "field" {
            return i as usize;
        }
    }
    from
}

/// Persist the settings snapshot and mirror it into `frontend.toml`
/// (the Qt model writes both on every change so a config-driven
/// launch and the state file never disagree).
fn save_settings(ctx: &Ctx) {
    let snapshot = {
        let shared = lock(&ctx.shared);
        shared.persist.clone()
    };
    persist::save(&snapshot);
    let s = &snapshot.settings;
    let mirror = zaparoo_core::config::SettingsMirror {
        resolution: &s.resolution,
        language: &s.language,
        orientation: &s.orientation,
        clock_format: &s.clock_format,
        interface_profile: &s.interface_profile,
        systems_browse_layout: &s.systems_browse_layout,
        games_browse_layout: &s.games_browse_layout,
        system_logo_style: &s.system_logo_style,
        color_scheme: &s.color_scheme,
        color_intensity: &s.color_intensity,
        metadata_scraper: &s.metadata_scraper,
        button_layout: &s.button_layout,
        mouse_enabled: s.mouse_enabled,
        reduce_motion: s.reduce_motion,
        debug_logging: s.debug_logging,
        screensaver_timeout: &s.screensaver_timeout,
        media_image_type: &s.media_image_type,
        favorites_grouping: &s.favorites_grouping,
        show_hidden: s.show_hidden,
        show_original_filenames: s.show_original_filenames,
        swap_confirm_cancel: s.swap_confirm_cancel,
        swap_options_view: s.swap_options_view,
        region: &s.region,
        crt_video_standard: &s.crt_video_standard,
        crt_h_offset: s.crt_h_offset,
        crt_v_offset: s.crt_v_offset,
    };
    if let Err(e) = zaparoo_core::config::save_settings_mirror(&ctx.config_path, mirror) {
        tracing::warn!("could not mirror settings to config: {e}");
    }
}

/// Rebuild the visible field list in place (busy verbs, toggle
/// states). Called after every change and from the media-status
/// watcher so Start/Cancel labels track the running job.
pub fn refresh_settings_fields(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_active_screen().as_str() != "settings" {
        return;
    }
    let page = app
        .global::<crate::SettingsView>()
        .get_settings_page()
        .to_string();
    let fields = settings_fields(ctx, &page);
    let index = app
        .global::<crate::SettingsView>()
        .get_settings_index()
        .max(0) as usize;
    let clamped = index.min(fields.len().saturating_sub(1));
    app.global::<crate::SettingsView>()
        .set_settings_fields(ModelRc::new(VecModel::from(fields)));
    app.global::<crate::SettingsView>()
        .set_settings_index(clamped as i32);
}

fn open_settings_page(ctx: &Ctx, app: &App, page: &str) {
    let fields = settings_fields(ctx, page);
    let view = app.global::<crate::SettingsView>();
    view.set_settings_page(SharedString::from(page));
    view.set_settings_title(SharedString::from(settings_page_title(page)));
    view.set_settings_index(first_navigable(&fields) as i32);
    view.set_settings_fields(ModelRc::new(VecModel::from(fields)));
}

/// About screen: enter from Settings, Back returns there. The screen
/// token persists so a kill on the About page restores to it.
pub fn enter_about(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).persist.active_screen = "about".to_string();
    save_persist(&ctx.shared);
    app.global::<crate::Shell>()
        .set_about_version_line(SharedString::from(
            concat!("Version ", env!("CARGO_PKG_VERSION"), " \u{b7} Slint demo")
                .to_string()
                .as_str(),
        ));
    transition_to_screen(app, "about", 1);
}

fn about_action(ctx: &Ctx, app: &App, action: &str) {
    if action == actions::CANCEL {
        // About is reached from the Support page; Back lands there.
        enter_settings_with_direction(ctx, app, -1);
        open_settings_page(ctx, app, "pageSupportAbout");
    }
}

fn enter_settings_with_direction(ctx: &Ctx, app: &App, direction: i32) {
    lock(&ctx.shared).persist.active_screen = "settings".to_string();
    save_persist(&ctx.shared);
    open_settings_page(ctx, app, "");
    transition_to_screen(app, "settings", direction);
}

pub fn enter_settings(ctx: &Ctx, app: &App) {
    enter_settings_with_direction(ctx, app, 1);
}

fn settings_action(ctx: &Ctx, app: &App, action: &str) {
    use slint::Model as _;
    let view = app.global::<crate::SettingsView>();
    let page = view.get_settings_page().to_string();
    let fields: Vec<crate::SettingsField> = view.get_settings_fields().iter().collect();
    let index = view.get_settings_index().max(0) as usize;

    if page.is_empty() {
        // Root category grid: 3x2, Accept opens the focused page.
        match action {
            actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN => {
                let (dx, dy) = match action {
                    actions::LEFT => (-1, 0),
                    actions::RIGHT => (1, 0),
                    actions::UP => (0, -1),
                    _ => (0, 1),
                };
                let next = hub_nav::grid_move(index, fields.len(), 3, dx, dy);
                view.set_settings_index(i32::try_from(next).unwrap_or(0));
            }
            actions::ACCEPT => {
                if let Some(field) = fields.get(index) {
                    open_settings_page(ctx, app, field.id.as_str());
                }
            }
            actions::CANCEL => {
                lock(&ctx.shared).persist.active_screen = "hub".to_string();
                save_persist(&ctx.shared);
                transition_to_screen(app, "hub", -1);
            }
            _ => {}
        }
        return;
    }

    match action {
        actions::UP => {
            view.set_settings_index(seek_navigable(&fields, index, -1) as i32);
        }
        actions::DOWN => {
            view.set_settings_index(seek_navigable(&fields, index, 1) as i32);
        }
        // Left/right flips toggles in place (the Qt SettingsField
        // contract); pickers and actions only react to Accept.
        actions::LEFT | actions::RIGHT => {
            if let Some(field) = fields.get(index) {
                if field.control.as_str() == "toggle" && field.enabled {
                    settings_toggle(ctx, app, field.id.as_str());
                }
            }
        }
        actions::ACCEPT => {
            if let Some(field) = fields.get(index) {
                if field.enabled {
                    settings_accept(ctx, app, field.id.as_str(), field.control.as_str());
                }
            }
        }
        actions::CANCEL => {
            open_settings_page(ctx, app, "");
        }
        _ => {}
    }
}

fn settings_accept(ctx: &Ctx, app: &App, id: &str, control: &str) {
    match control {
        "toggle" => settings_toggle(ctx, app, id),
        "picker" => open_settings_picker(ctx, app, id),
        _ => match id {
            "aboutLicense" => enter_about(ctx, app),
            "crtCalibration" => open_crt_calibration(ctx, app),
            "updateMediaDb" => {
                let ms = media_state(ctx);
                if ms.indexing || ms.optimizing {
                    let client = ctx.store.client();
                    ctx.handle.spawn(async move {
                        if let Err(e) = client.media_generate_cancel().await {
                            tracing::warn!("cancel index failed: {}", e.message);
                        }
                    });
                } else {
                    start_index(ctx, app, None);
                }
            }
            "runScraper" => {
                let ms = media_state(ctx);
                if ms.scraping {
                    let client = ctx.store.client();
                    ctx.handle.spawn(async move {
                        if let Err(e) = client.media_scrape_cancel().await {
                            tracing::warn!("cancel scrape failed: {}", e.message);
                        }
                    });
                } else {
                    let force = lock(&ctx.shared).rescrape_existing;
                    start_scrape(ctx, Vec::new(), force);
                }
            }
            _ => {}
        },
    }
}

fn stage_restart(ctx: &Ctx, app: &App, pending: PendingRestart) {
    lock(&ctx.shared).pending_restart = Some(pending);
    open_dialog(
        app,
        "restart_setting",
        "Quit and restart Zaparoo Frontend?",
        "This setting needs a frontend restart to take effect.",
        &["Yes", "No"],
        1,
    );
}

#[cfg(feature = "mister")]
fn write_crt_state_file(enabled: bool, standard: &str) -> Result<(), String> {
    const PATH: &str = "/media/fat/config/zaparoo_launcher_crt.bin";
    let mode = zaparoo_core::config::crt_mode_id(standard);
    std::fs::write(PATH, [u8::from(enabled), mode])
        .map_err(|e| format!("could not write {PATH}: {e}"))
}

#[cfg(not(feature = "mister"))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "desktop stub preserves the fallible MiSTer call-site contract"
)]
fn write_crt_state_file(enabled: bool, standard: &str) -> Result<(), String> {
    let _ = (enabled, standard);
    Ok(())
}

fn confirm_pending_restart(ctx: &Ctx, app: &App) {
    let Some(pending) = lock(&ctx.shared).pending_restart.take() else {
        close_dialog(app);
        return;
    };
    close_dialog(app);
    match pending {
        PendingRestart::Resolution(value) => {
            lock(&ctx.shared).persist.settings.resolution = value;
            save_settings(ctx);
            crate::request_restart();
        }
        PendingRestart::CrtStandard(value) => {
            let standard = zaparoo_core::config::normalize_crt_video_standard(&value).to_string();
            lock(&ctx.shared)
                .persist
                .settings
                .crt_video_standard
                .clone_from(&standard);
            save_settings(ctx);
            if let Err(e) = write_crt_state_file(true, &standard) {
                tracing::warn!("{e}");
                app.global::<crate::Shell>()
                    .set_status_text(SharedString::from("Could not update CRT mode"));
                return;
            }
            crate::request_main_reload();
        }
        PendingRestart::CrtEnabled(enabled) => {
            let standard = lock(&ctx.shared)
                .persist
                .settings
                .crt_video_standard
                .clone();
            if let Err(e) = write_crt_state_file(enabled, &standard) {
                tracing::warn!("{e}");
                app.global::<crate::Shell>()
                    .set_status_text(SharedString::from("Could not update CRT mode"));
                return;
            }
            crate::request_main_reload();
        }
    }
}

fn open_crt_calibration(ctx: &Ctx, app: &App) {
    if !ctx.crt_enabled {
        return;
    }
    let (h, v) = {
        let shared = lock(&ctx.shared);
        (
            shared.persist.settings.crt_h_offset,
            shared.persist.settings.crt_v_offset,
        )
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_crt_h_offset(h);
    overlays.set_crt_v_offset(v);
    overlays.set_crt_calibration_open(true);
}

fn crt_calibration_action(ctx: &Ctx, app: &App, action: &str) {
    let overlays = app.global::<crate::Overlays>();
    let (mut h, mut v) = (overlays.get_crt_h_offset(), overlays.get_crt_v_offset());
    match action {
        actions::LEFT => h -= 1,
        actions::RIGHT => h += 1,
        actions::UP => v -= 1,
        actions::DOWN => v += 1,
        actions::ACCEPT | actions::CANCEL => {
            save_settings(ctx);
            overlays.set_crt_calibration_open(false);
            return;
        }
        _ => return,
    }
    let (h, v) = zaparoo_core::config::clamp_crt_offsets(h, v);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.settings.crt_h_offset = h;
        shared.persist.settings.crt_v_offset = v;
    }
    overlays.set_crt_h_offset(h);
    overlays.set_crt_v_offset(v);
    crate::set_live_crt_offsets(h, v);
}

/// Flip a boolean setting and run its live consumer, mirroring the
/// Qt setters' side effects.
fn settings_toggle(ctx: &Ctx, app: &App, id: &str) {
    if id == "crtEnabled" {
        stage_restart(ctx, app, PendingRestart::CrtEnabled(!ctx.crt_enabled));
        return;
    }
    let value = {
        let mut shared = lock(&ctx.shared);
        match id {
            "showHidden" => {
                shared.show_hidden = !shared.show_hidden;
                shared.persist.settings.show_hidden = shared.show_hidden;
                shared.show_hidden
            }
            "showOriginalFilenames" => {
                let v = !shared.persist.settings.show_original_filenames;
                shared.persist.settings.show_original_filenames = v;
                v
            }
            "reduceMotion" => {
                let v = !shared.persist.settings.reduce_motion;
                shared.persist.settings.reduce_motion = v;
                v
            }
            "mouseEnabled" => {
                let v = !shared.persist.settings.mouse_enabled;
                shared.persist.settings.mouse_enabled = v;
                v
            }
            "debugLogging" => {
                let v = !shared.persist.settings.debug_logging;
                shared.persist.settings.debug_logging = v;
                v
            }
            // Session-scoped one-shot; intentionally NOT persisted
            // (Qt's _visibleRescrapeExisting rule).
            "rescrapeExisting" => {
                shared.rescrape_existing = !shared.rescrape_existing;
                shared.rescrape_existing
            }
            _ => return,
        }
    };
    if id != "rescrapeExisting" {
        save_settings(ctx);
    }
    match id {
        "showHidden" => {
            reproject_hub(ctx, app);
            reproject_systems(ctx, app);
        }
        "showOriginalFilenames" => {
            let page = lock(&ctx.shared).games_page;
            show_games_page(&ctx.shared, &ctx.media, app, page);
        }
        "reduceMotion" => {
            app.global::<crate::Shell>().set_reduce_motion(value);
        }
        _ => {}
    }
    refresh_settings_fields(ctx, app);
}

/// Options for a picker field: (value, display label) pairs from the
/// canonical lists.
fn settings_picker_options(id: &str) -> Vec<(String, String)> {
    let map = |values: &[&str], display: fn(&str) -> &'static str| {
        values
            .iter()
            .map(|v| ((*v).to_string(), display(v).to_string()))
            .collect::<Vec<_>>()
    };
    match id {
        "orientation" => map(ORIENTATIONS, orientation_display),
        "resolution" => MISTER_RESOLUTIONS
            .iter()
            .map(|v| ((*v).to_string(), resolution_display(v).to_string()))
            .collect(),
        "crtVideoStandard" => map(CRT_VIDEO_STANDARDS, crt_standard_display),
        "screensaverTimeout" => map(SCREENSAVER_TIMEOUTS, screensaver_timeout_display),
        "browseLayout" => map(BROWSE_LAYOUTS, browse_layout_display),
        "systemLogoStyle" => map(SYSTEM_LOGO_STYLES, system_logo_style_display),
        "mediaImageType" => map(MEDIA_IMAGE_TYPES, media_image_type_display),
        "language" => map(LANGUAGES, language_display),
        "region" => map(REGIONS, region_display),
        "clockFormat" => map(CLOCK_FORMATS, clock_format_display),
        "buttonLayout" => map(BUTTON_LAYOUTS, button_layout_display),
        _ => Vec::new(),
    }
}

fn settings_picker_title(id: &str) -> &'static str {
    match id {
        "orientation" => "Orientation",
        "resolution" => "Resolution",
        "crtVideoStandard" => "Video standard",
        "screensaverTimeout" => "Screensaver",
        "browseLayout" => "Browsing layout",
        "systemLogoStyle" => "System logos",
        "mediaImageType" => "Preferred artwork",
        "language" => "Language",
        "region" => "System names",
        "clockFormat" => "Clock format",
        "buttonLayout" => "Button style",
        _ => "Select",
    }
}

fn settings_current_value(ctx: &Ctx, id: &str) -> String {
    let s = lock(&ctx.shared).persist.settings.clone();
    match id {
        "orientation" => s.orientation,
        "resolution" => s.resolution,
        "crtVideoStandard" => s.crt_video_standard,
        "screensaverTimeout" => s.screensaver_timeout,
        "browseLayout" => s.games_browse_layout,
        "systemLogoStyle" => s.system_logo_style,
        "mediaImageType" => s.media_image_type,
        "language" => s.language,
        "region" => s.region,
        "clockFormat" => s.clock_format,
        "buttonLayout" => s.button_layout,
        _ => String::new(),
    }
}

/// Open the shared `ListPickerModal` on a settings field, focused on
/// the current value (the Qt requestListPicker flow).
fn open_settings_picker(ctx: &Ctx, app: &App, id: &str) {
    let options = settings_picker_options(id);
    if options.is_empty() {
        return;
    }
    let current = settings_current_value(ctx, id);
    let initial = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
    let entries: Vec<crate::MenuEntry> = options
        .iter()
        .map(|(value, label)| menu_entry(value, label))
        .collect();
    lock(&ctx.shared).list_context = ListContext::SettingsPicker(id.to_string());
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_title(SharedString::from(settings_picker_title(id)));
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(i32::try_from(initial).unwrap_or(0));
    overlays.set_list_open(true);
}

/// Apply a picker selection and run its live consumer.
fn settings_picker_selected(ctx: &Ctx, app: &App, id: &str, value: &str) {
    if id == "resolution" {
        if value != lock(&ctx.shared).persist.settings.resolution {
            stage_restart(ctx, app, PendingRestart::Resolution(value.to_string()));
        }
        return;
    }
    if id == "crtVideoStandard" {
        if value != lock(&ctx.shared).persist.settings.crt_video_standard {
            stage_restart(ctx, app, PendingRestart::CrtStandard(value.to_string()));
        }
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let s = &mut shared.persist.settings;
        match id {
            "orientation" => s.orientation = value.to_string(),
            "screensaverTimeout" => s.screensaver_timeout = value.to_string(),
            "browseLayout" => s.games_browse_layout = value.to_string(),
            "systemLogoStyle" => s.system_logo_style = value.to_string(),
            "mediaImageType" => s.media_image_type = value.to_string(),
            "language" => s.language = value.to_string(),
            "region" => s.region = value.to_string(),
            "clockFormat" => s.clock_format = value.to_string(),
            "buttonLayout" => s.button_layout = value.to_string(),
            _ => return,
        }
    }
    save_settings(ctx);
    match id {
        "orientation" => {
            let rotated = matches!(value, "cw" | "ccw");
            app.global::<crate::Shell>()
                .set_orientation(SharedString::from(value));
            app.global::<Sizing>().set_swap_axes(rotated);
            crate::set_live_orientation(app, value, ctx.framebuffer_size);
        }
        "clockFormat" => {
            ctx.clock_twelve_hour
                .store(value == "12h", std::sync::atomic::Ordering::Relaxed);
            app.global::<crate::Shell>()
                .set_clock_text(SharedString::from(
                    crate::clock_string(value == "12h").as_str(),
                ));
        }
        "systemLogoStyle" => reproject_systems(ctx, app),
        "mediaImageType" => ctx.media.set_preferred_image_type(value),
        // Restart the idle clock so the new timeout takes effect now,
        // not after the old countdown fires into the seq guard.
        "screensaverTimeout" => reset_idle(ctx, app),
        "browseLayout" => {
            let is_list = value == "list";
            app.global::<crate::Shell>().set_browse_list_layout(is_list);
            app.global::<crate::GamesView>()
                .set_games_list_layout(is_list);
            // Carry the selection into the other presentation and
            // re-render when a games-style screen is up (settings is
            // on screen right now, but the state must be coherent the
            // moment the user backs out).
            let page_size = games_page_size(app).max(1) as usize;
            let target_page = {
                let mut guard = lock(&ctx.shared);
                if is_list {
                    let grid_index =
                        app.global::<crate::GamesView>().get_games_index().max(0) as usize;
                    guard.list_index = guard.games_page * page_size + grid_index;
                    0
                } else {
                    (guard.list_index / page_size).min(guard.games_pages.len().saturating_sub(1))
                }
            };
            if is_list {
                show_games_list(&ctx.shared, &ctx.media, app);
            } else {
                show_games_page(&ctx.shared, &ctx.media, app, target_page);
            }
        }
        _ => {}
    }
    refresh_settings_fields(ctx, app);
}

/// Scoped scraper run: Core's in-tree ES gamelist.xml scraper (the
/// only shipped scraper - `scraperId` has no server-side default, so
/// the Qt model hardcodes the same id). `force` re-scrapes existing
/// metadata; the one-shot toggle resets when the run is kicked off.
fn start_scrape(ctx: &Ctx, systems: Vec<String>, force: bool) {
    use zaparoo_core::media_types::MediaScrapeParams;
    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    ctx.handle.spawn(async move {
        let params = MediaScrapeParams {
            scraper_id: "gamelist.xml".to_string(),
            systems,
            force,
        };
        match client.media_scrape(params).await {
            Ok(()) => {
                if force {
                    lock(&shared).rescrape_existing = false;
                }
            }
            Err(e) => tracing::warn!("start_scrape failed: {}", e.message),
        }
    });
}

#[allow(
    clippy::too_many_lines,
    reason = "action match keeps hub navigation and persistence transitions auditable together"
)]
fn hub_action(ctx: &Ctx, app: &App, action: &str) {
    let n_cats = lock(&ctx.shared).categories.len();
    let row = app.global::<crate::HubView>().get_hub_row();
    match action {
        actions::LEFT | actions::RIGHT => {
            let delta = if action == actions::LEFT { -1 } else { 1 };
            if row == 0 {
                let next = hub_nav::row_move(
                    app.global::<crate::HubView>().get_hub_category_index() as usize,
                    n_cats,
                    delta,
                );
                app.global::<crate::HubView>()
                    .set_hub_category_index(next as i32);
                let mut shared = lock(&ctx.shared);
                if let Some(name) = shared.categories.get(next).cloned() {
                    shared.persist.hub.category = name;
                }
                drop(shared);
                save_persist(&ctx.shared);
            } else {
                let next = hub_nav::row_move(
                    app.global::<crate::HubView>().get_hub_action_index() as usize,
                    HUB_ACTIONS.len(),
                    delta,
                );
                app.global::<crate::HubView>()
                    .set_hub_action_index(next as i32);
                lock(&ctx.shared).persist.hub.selected_action = HUB_ACTIONS[next].to_string();
                save_persist(&ctx.shared);
            }
        }
        actions::UP | actions::DOWN => {
            let target_row = i32::from(action == actions::DOWN);
            if target_row == row {
                return;
            }
            if target_row == 1 {
                let next = hub_nav::nearest_by_center(
                    app.global::<crate::HubView>().get_hub_category_index() as usize,
                    n_cats,
                    hub_nav::HUB_TILE_W,
                    HUB_ACTIONS.len(),
                    hub_nav::HUB_TILE_W,
                );
                app.global::<crate::HubView>()
                    .set_hub_action_index(next as i32);
                lock(&ctx.shared).persist.hub.selected_action = HUB_ACTIONS[next].to_string();
            } else {
                let next = hub_nav::nearest_by_center(
                    app.global::<crate::HubView>().get_hub_action_index() as usize,
                    HUB_ACTIONS.len(),
                    hub_nav::HUB_TILE_W,
                    n_cats,
                    hub_nav::HUB_TILE_W,
                );
                app.global::<crate::HubView>()
                    .set_hub_category_index(next as i32);
                let mut shared = lock(&ctx.shared);
                if let Some(name) = shared.categories.get(next).cloned() {
                    shared.persist.hub.category = name;
                }
            }
            app.global::<crate::HubView>().set_hub_row(target_row);
            lock(&ctx.shared).persist.hub.selected_row = target_row as u32;
            save_persist(&ctx.shared);
        }
        actions::ACCEPT => {
            if row == 0 {
                let category = lock(&ctx.shared)
                    .categories
                    .get(app.global::<crate::HubView>().get_hub_category_index() as usize)
                    .cloned();
                if let Some(category) = category {
                    enter_systems(ctx, app, &category);
                }
                // Empty payload convention: press on an empty row is a no-op.
            } else {
                match HUB_ACTIONS[app
                    .global::<crate::HubView>()
                    .get_hub_action_index()
                    .clamp(0, 4) as usize]
                {
                    "resume" => {
                        let entry = lock(&ctx.shared).resume_entry.clone();
                        if let Some(entry) = entry {
                            launch(ctx, app, entry.media_path);
                        } else {
                            app.global::<crate::Shell>()
                                .set_status_text(SharedString::from("Nothing to resume yet"));
                        }
                    }
                    "favorites" => enter_favorites(ctx, app),
                    "recents" => enter_recents(ctx, app),
                    "update" => start_media_update(ctx, app),
                    "settings" => enter_settings(ctx, app),
                    other => {
                        app.global::<crate::Shell>()
                            .set_status_text(SharedString::from(
                                format!("{other} is outside the demo scope").as_str(),
                            ));
                    }
                }
            }
        }
        actions::CONTEXT_MENU => {
            // Category tiles carry the hide/index menu; the action row
            // has none (the Qt owner split).
            if row == 0 {
                let index = app
                    .global::<crate::HubView>()
                    .get_hub_category_index()
                    .max(0) as usize;
                open_category_context_menu(ctx, app, index);
            }
        }
        actions::CANCEL => {
            // Back on the Hub asks first - a stray B can't kill the
            // frontend (the Qt quit-confirm rule).
            open_quit_confirm(app);
        }
        _ => {}
    }
}

fn systems_page_size(app: &App) -> usize {
    let cols = app
        .global::<crate::SystemsView>()
        .get_systems_grid_cols()
        .max(1) as usize;
    let rows = app
        .global::<crate::SystemsView>()
        .get_systems_grid_rows()
        .max(1) as usize;
    cols * rows
}

fn system_tile(s: &SystemInfo, hidden: bool, logo_style: &str) -> SystemTile {
    // Both tint variants of the Qt color-grade (rest + focus); the
    // tile switches sources on focus, no colorize. The "color" logo
    // style skips the grade and shows the original art in both
    // states (Settings.qml's "Full color" option).
    let to_image = |px: crate::system_logos::LogoPixels| {
        slint::Image::from_rgba8(
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &px.rgba, px.width, px.height,
            ),
        )
    };
    let (rest, focus) = if logo_style == "color" {
        let original = crate::system_logos::logo_for(&s.id);
        (original.clone(), original)
    } else {
        (
            crate::system_logos::tinted_logo_for(&s.id, false),
            crate::system_logos::tinted_logo_for(&s.id, true),
        )
    };
    let has_logo = rest.is_some();
    SystemTile {
        id: SharedString::from(s.id.as_str()),
        name: SharedString::from(s.name.as_str()),
        logo: rest.map_or_else(slint::Image::default, to_image),
        logo_focus: focus.map_or_else(slint::Image::default, to_image),
        has_logo,
        hidden,
    }
}

/// Project one page of the category's systems into the grid (the
/// `PagedGrid` model): page slice, counter, and focus restored to the
/// persisted system when it sits on this page.
fn show_systems_page(ctx: &Ctx, app: &App, page: usize) {
    let page_size = systems_page_size(app);
    let (tiles, total_pages, restored) = {
        let guard = lock(&ctx.shared);
        let total = guard.screen_systems.len();
        let total_pages = total.div_ceil(page_size).max(1);
        let page = page.min(total_pages - 1);
        let slice =
            &guard.screen_systems[page * page_size..(page * page_size + page_size).min(total)];
        let restored = slice
            .iter()
            .position(|s| s.id == guard.persist.systems.system_id)
            .unwrap_or(0);
        (
            slice
                .iter()
                .map(|s| {
                    system_tile(
                        s,
                        guard.hidden_system_ids.iter().any(|h| h == &s.id),
                        &guard.persist.settings.system_logo_style,
                    )
                })
                .collect::<Vec<SystemTile>>(),
            total_pages,
            restored,
        )
    };
    let view = app.global::<crate::SystemsView>();
    view.set_systems(ModelRc::new(VecModel::from(tiles)));
    view.set_systems_page(i32::try_from(page).unwrap_or(0));
    view.set_systems_total_pages(i32::try_from(total_pages).unwrap_or(0));
    view.set_systems_index(restored as i32);
}

fn enter_systems_with_motion(ctx: &Ctx, app: &App, category: &str, animate: bool) {
    let page = {
        let mut shared = lock(&ctx.shared);
        let systems = projected_systems_for_category(&shared, category);
        let restore_id = shared.persist.systems.system_id.clone();
        let absolute = systems.iter().position(|s| s.id == restore_id).unwrap_or(0);
        shared.screen_systems = systems;
        shared.persist.hub.category = category.to_string();
        shared.persist.active_screen = "systems".to_string();
        absolute / systems_page_size(app)
    };
    save_persist(&ctx.shared);
    show_systems_page(ctx, app, page);
    app.global::<crate::SystemsView>()
        .set_systems_category(SharedString::from(category));
    if animate {
        transition_to_screen(app, "systems", 1);
    } else {
        app.global::<crate::Shell>()
            .set_active_screen(SharedString::from("systems"));
    }
}

pub fn enter_systems(ctx: &Ctx, app: &App, category: &str) {
    enter_systems_with_motion(ctx, app, category, true);
}

pub fn enter_systems_immediate(ctx: &Ctx, app: &App, category: &str) {
    enter_systems_with_motion(ctx, app, category, false);
}

#[cfg(feature = "mister")]
fn request_cached_systems_page_transition(app: &App, direction: i32) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_orientation().as_str() != "horizontal" || shell.get_browse_list_layout() {
        return false;
    }
    let sizing = app.global::<Sizing>();
    let width = sizing.get_screen_width().round().max(0.0) as u32;
    let height = sizing.get_screen_height().round().max(0.0) as u32;
    let view = app.global::<crate::SystemsView>();
    let Some(geometry) = sizing::mister_browse_grid_transition_geometry(
        width,
        height,
        view.get_systems_grid_cols().max(0) as u32,
        view.get_systems_grid_rows().max(0) as u32,
    ) else {
        return false;
    };
    crate::mister::request_page_transition(geometry, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached_systems_page_transition(_app: &App, _direction: i32) -> bool {
    false
}

#[cfg(feature = "mister")]
fn request_cached_games_page_transition(app: &App, direction: i32) -> bool {
    let shell = app.global::<crate::Shell>();
    let view = app.global::<crate::GamesView>();
    if shell.get_orientation().as_str() != "horizontal" || view.get_games_list_layout() {
        return false;
    }
    let sizing = app.global::<Sizing>();
    let width = sizing.get_screen_width().round().max(0.0) as u32;
    let height = sizing.get_screen_height().round().max(0.0) as u32;
    let Some(geometry) = sizing::mister_browse_grid_transition_geometry(
        width,
        height,
        view.get_games_grid_cols().max(0) as u32,
        view.get_games_grid_rows().max(0) as u32,
    ) else {
        return false;
    };
    crate::mister::request_page_transition(geometry, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached_games_page_transition(_app: &App, _direction: i32) -> bool {
    false
}

/// Animated page flip for the systems grid. The latch path
/// renders the destination once and moves cached RGB565 endpoint pixels;
/// every other backend keeps the original two-page Slint strip.
fn slide_systems_page(ctx: &Ctx, app: &App, page: usize, dir: i32) {
    let view = app.global::<crate::SystemsView>();
    if view.get_systems_page_slide() != 0.0 || view.get_systems_cached_transition() {
        return;
    }
    let page_size = systems_page_size(app);
    let cols = view.get_systems_grid_cols().max(1) as usize;
    let col = (view.get_systems_index().max(0) as usize) % cols;
    let (tiles, target, total_pages) = {
        let mut guard = lock(&ctx.shared);
        let total = guard.screen_systems.len();
        if page * page_size >= total {
            return;
        }
        let total_pages = total.div_ceil(page_size).max(1);
        let slice = guard.screen_systems
            [page * page_size..(page * page_size + page_size).min(total)]
            .to_vec();
        // Focus lands same-column: top row sliding forward, bottom
        // row sliding back; persist it for the commit's restore.
        let target = if dir > 0 {
            col.min(slice.len() - 1)
        } else {
            let rows = slice.len().div_ceil(cols);
            ((rows - 1) * cols + col).min(slice.len() - 1)
        };
        guard
            .persist
            .systems
            .system_id
            .clone_from(&slice[target].id);
        (
            slice
                .iter()
                .map(|s| {
                    system_tile(
                        s,
                        guard.hidden_system_ids.iter().any(|h| h == &s.id),
                        &guard.persist.settings.system_logo_style,
                    )
                })
                .collect::<Vec<SystemTile>>(),
            target,
            total_pages,
        )
    };
    save_persist(&ctx.shared);

    let reduce_motion = lock(&ctx.shared).persist.settings.reduce_motion;
    view.set_systems_slide_dir(dir);
    view.set_systems_transition_target_index(i32::try_from(target).unwrap_or(0));
    if !reduce_motion && request_cached_systems_page_transition(app, dir) {
        view.set_systems_slide_anim(false);
        view.set_systems_cached_transition(true);
        view.set_systems_next_page(ModelRc::new(VecModel::from(Vec::<SystemTile>::new())));
        view.set_systems(ModelRc::new(VecModel::from(tiles)));
        view.set_systems_page(i32::try_from(page).unwrap_or(0));
        view.set_systems_total_pages(i32::try_from(total_pages).unwrap_or(0));
        view.set_systems_index(i32::try_from(target).unwrap_or(0));

        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(260), move || {
            if let Some(app) = weak.upgrade() {
                let view = app.global::<crate::SystemsView>();
                view.set_systems_cached_transition(false);
                view.set_systems_slide_anim(true);
            }
        });
        return;
    }

    // Fallback Slint strip retains the shared DRS phase declaration;
    // Reduce Motion turns it into a cut.
    if !reduce_motion {
        crate::drs::heavy_begin();
    }
    view.set_systems_slide_anim(true);
    view.set_systems_next_page(ModelRc::new(VecModel::from(tiles)));
    view.set_systems_page_slide(dir as f32);

    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(260), move || {
        if !reduce_motion {
            crate::drs::heavy_end();
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        let view = app.global::<crate::SystemsView>();
        view.set_systems_slide_anim(false);
        show_systems_page(&ctx2, &app, page);
        view.set_systems_page_slide(0.0);
        view.set_systems_next_page(ModelRc::new(VecModel::from(Vec::<SystemTile>::new())));
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(app) = weak.upgrade() {
                app.global::<crate::SystemsView>()
                    .set_systems_slide_anim(true);
            }
        });
    });
}

fn systems_action(ctx: &Ctx, app: &App, action: &str) {
    let view = app.global::<crate::SystemsView>();
    // A page transition in flight owns the grid (the games-grid gate).
    if view.get_systems_page_slide() != 0.0 || view.get_systems_cached_transition() {
        return;
    }
    let len = view.get_systems().row_count();
    let cols = view.get_systems_grid_cols().max(1) as usize;
    let page_size = systems_page_size(app);
    let page = view.get_systems_page().max(0) as usize;
    let total_pages = view.get_systems_total_pages().max(1) as usize;
    let index = view.get_systems_index().max(0) as usize;
    let list_layout = app.global::<crate::Shell>().get_browse_list_layout();
    match action {
        actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN => {
            let next = if list_layout {
                if matches!(action, actions::LEFT | actions::RIGHT) || len == 0 {
                    return;
                }
                if action == actions::UP {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(len - 1)
                }
            } else {
                let (dx, dy) = match action {
                    actions::LEFT => (-1, 0),
                    actions::RIGHT => (1, 0),
                    actions::UP => (0, -1),
                    _ => (0, 1),
                };
                hub_nav::grid_move(index, len, cols, dx, dy)
            };
            // Vertical edge press = page change. Grid uses a swoop;
            // list uses the same transition machinery for consistent
            // persistence and page ownership.
            if next == index && len > 0 {
                if action == actions::DOWN && page + 1 < total_pages {
                    slide_systems_page(ctx, app, page + 1, 1);
                    return;
                }
                if action == actions::UP && (list_layout || index < cols) && page > 0 {
                    slide_systems_page(ctx, app, page - 1, -1);
                    return;
                }
            }
            view.set_systems_index(next as i32);
            let mut shared = lock(&ctx.shared);
            if let Some(sys) = shared.screen_systems.get(page * page_size + next).cloned() {
                shared.persist.systems.system_id = sys.id;
            }
            drop(shared);
            save_persist(&ctx.shared);
        }
        actions::PAGE_NEXT if page + 1 < total_pages => {
            slide_systems_page(ctx, app, page + 1, 1);
        }
        actions::PAGE_PREV if page > 0 => {
            slide_systems_page(ctx, app, page - 1, -1);
        }
        actions::ACCEPT => {
            let sys = lock(&ctx.shared)
                .screen_systems
                .get(page * page_size + index)
                .cloned();
            if let Some(sys) = sys {
                enter_games(ctx, app, &sys);
            }
        }
        actions::CONTEXT_MENU => {
            open_system_context_menu(ctx, app, page * page_size + index);
        }
        actions::CANCEL => {
            lock(&ctx.shared).persist.active_screen = "hub".to_string();
            save_persist(&ctx.shared);
            transition_to_screen(app, "hub", -1);
        }
        _ => {}
    }
}

pub fn enter_games(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    if !sys.zap_script.is_empty() {
        // Launch-only virtual system: run its script directly, no
        // browse. Stays on the Systems screen.
        launch(ctx, app, sys.zap_script.clone());
        return;
    }

    // Entering a system resets the folder stack to root level — the
    // new system's browse always starts at the initial view (same rule
    // as GamesState.system_id assignment in Main.qml).
    {
        let mut shared = lock(&ctx.shared);
        shared.games_mode = GamesMode::Browse;
        shared.persist.games.system_id.clone_from(&sys.id);
        shared.persist.games.path_stack = vec![String::new()];
        shared.persist.games.selected_at_level = vec![String::new()];
        shared.games_system_name.clone_from(&sys.name);
    }
    browse_games(ctx, app, &sys.id, &sys.name, "", true);
}

/// Re-enter the games screen from a cold start, preserving the
/// persisted folder stack: browse the stack's top level directly.
pub fn enter_games_restored(ctx: &Ctx, app: &App, sys: &SystemInfo) {
    if !sys.zap_script.is_empty() {
        return;
    }
    let top = {
        let mut shared = lock(&ctx.shared);
        shared.games_mode = GamesMode::Browse;
        shared.games_system_name.clone_from(&sys.name);
        shared
            .persist
            .games
            .path_stack
            .last()
            .cloned()
            .unwrap_or_default()
    };
    browse_games(ctx, app, &sys.id, &sys.name, &top, true);
}

/// Shared Ready-side fill for the games-style grid: store the rows,
/// persist the screen token, flip (or clear the light cue), show
/// page 0. Runs on the event loop.
#[allow(
    clippy::too_many_arguments,
    reason = "internal fill plumbing shared by three list sources; a params struct would just re-name these"
)]
fn apply_list_fill(
    shared: &Arc<Mutex<Shared>>,
    media: &Arc<MediaCache>,
    app: &App,
    ticket: u64,
    rows: Vec<GameRow>,
    next_cursor: Option<String>,
    total_rows: Option<u32>,
    total_dirs: u32,
    screen_token: &str,
    title: &str,
    flip: bool,
) {
    {
        let mut guard = lock(shared);
        if guard.games_ticket != ticket {
            return;
        }
        // List layout: restore the absolute selection from the
        // persisted path for this mode/level (fresh fill = one page).
        if list_layout_on(&guard) {
            let selected = match guard.games_mode {
                GamesMode::Browse => guard
                    .persist
                    .games
                    .selected_at_level
                    .last()
                    .cloned()
                    .unwrap_or_default(),
                GamesMode::Favorites => guard.persist.favorites.selected_path.clone(),
                GamesMode::Recents => guard.persist.recents.selected_path.clone(),
            };
            guard.list_index = (!selected.is_empty())
                .then(|| rows.iter().position(|e| e.path == selected))
                .flatten()
                .unwrap_or(0);
        }
        guard.games_pages = vec![rows];
        guard.games_page = 0;
        guard.games_next_cursor = next_cursor;
        guard.games_total_rows = total_rows;
        guard.games_total_dirs = total_dirs;
        guard.games_fetching = false;
        guard.persist.active_screen = screen_token.to_string();
        let snapshot = guard.persist.clone();
        drop(guard);
        persist::save(&snapshot);
    }
    app.global::<crate::GamesView>()
        .set_games_system(SharedString::from(title));
    // ScreenStateOverlay vocabulary: a successful fill clears any
    // terminal error and refreshes the per-mode empty copy.
    app.global::<crate::GamesView>()
        .set_games_error(SharedString::default());
    app.global::<crate::GamesView>()
        .set_games_empty_text(SharedString::from(match screen_token {
            "favorites" => "No favorites yet",
            "recents" => "Nothing played yet",
            _ => "No games in this system",
        }));
    if flip {
        transition_to_screen(app, screen_token, 1);
    } else {
        app.global::<crate::Shell>()
            .set_status_text(SharedString::default());
    }
    show_games_page(shared, media, app, 0);
}

/// Terminal in-screen error (`ScreenStateOverlay`'s Error state): flip
/// to the destination and paint "Failed to load" + the message where
/// the grid would be, like the Qt overlay bound to the model's
/// `error_message`. Replaces the old header-line-only surface.
fn show_games_error(app: &App, screen_token: &str, title: &str, message: &str) {
    let view = app.global::<crate::GamesView>();
    view.set_games(ModelRc::new(VecModel::from(Vec::<GameTile>::new())));
    view.set_games_error(SharedString::from(message));
    view.set_games_total_pages(0);
    view.set_games_has_more(false);
    view.set_games_system(SharedString::from(title));
    transition_to_screen(app, screen_token, 1);
    app.global::<crate::Shell>()
        .set_status_text(SharedString::default());
}

/// Favorites entry (Hub action): first page of media tagged
/// `user:favorite`, deferred-flip like the games entry.
pub fn enter_favorites(ctx: &Ctx, app: &App) {
    app.global::<crate::Shell>().set_transitioning(true);
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.games_mode = GamesMode::Favorites;
        shared.games_system_id = String::new();
        shared.games_system_name = "Favorites".to_string();
        shared.games_ticket += 1;
        shared.games_ticket
    };

    let resource = ctx
        .store
        .subscribe::<MediaFavoritesEndpoint>(FavoritesArgs::new(
            games_page_size(app),
            None,
            Vec::new(),
        ));
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let shared = ctx.shared.clone();
    let media = ctx.media.clone();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        let rows: Vec<GameRow> = result.results.iter().map(GameRow::from).collect();
                        apply_list_fill(
                            &shared,
                            &media,
                            &app,
                            ticket,
                            rows,
                            None,
                            None,
                            0,
                            "favorites",
                            "Favorites",
                            true,
                        );
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_games_error(&app, "favorites", "Favorites", &message);
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

/// Recents entry (Hub action): recently-played history, deferred-flip.
pub fn enter_recents(ctx: &Ctx, app: &App) {
    app.global::<crate::Shell>().set_transitioning(true);
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.games_mode = GamesMode::Recents;
        shared.games_system_id = String::new();
        shared.games_system_name = "Recently Played".to_string();
        shared.games_ticket += 1;
        shared.games_ticket
    };

    let resource = ctx
        .store
        .subscribe::<MediaHistoryEndpoint>(HistoryArgs::new(Vec::new(), games_page_size(app)));
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let shared = ctx.shared.clone();
    let media = ctx.media.clone();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        let rows: Vec<GameRow> = result.entries.iter().map(GameRow::from).collect();
                        apply_list_fill(
                            &shared,
                            &media,
                            &app,
                            ticket,
                            rows,
                            None,
                            None,
                            0,
                            "recents",
                            "Recently Played",
                            true,
                        );
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_games_error(&app, "recents", "Recently Played", &message);
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

/// Fill the games screen from a browse of (`system_id`, `path`).
/// `flip` selects deferred route entry (input gate + screen push on
/// Ready); folder navigation inside the screen passes false
/// and shows the lightweight status cue instead, mirroring Main.qml's
/// "no pendingTransition on drill-down" rule.
fn browse_games(ctx: &Ctx, app: &App, system_id: &str, system_name: &str, path: &str, flip: bool) {
    if flip {
        app.global::<crate::Shell>().set_transitioning(true);
    } else {
        app.global::<crate::Shell>()
            .set_status_text(SharedString::from("Loading…"));
    }
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.games_ticket += 1;
        shared.games_ticket
    };
    save_persist(&ctx.shared);

    let args = BrowseArgs::new(
        path.to_string(),
        vec![system_id.to_string()],
        games_page_size(app),
        Vec::new(),
    );
    let resource = ctx.store.subscribe::<MediaBrowseEndpoint>(args);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let shared = ctx.shared.clone();
    let media = ctx.media.clone();
    let system_name = system_name.to_string();
    let system_id = system_id.to_string();
    let browse_path = path.to_string();
    let at_root = path.is_empty();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        {
                            let mut guard = lock(&shared);
                            guard.games_system_id.clone_from(&system_id);
                            guard.games_browse_path.clone_from(&browse_path);
                        }
                        let entries = dedup_roots_drop_ancestors(&result.entries);

                        // Single-root auto-nav (the Qt model's rule):
                        // a system whose scoped roots collapse to one
                        // folder skips the pointless one-entry level -
                        // the root REPLACES the stack's base so Back
                        // still exits the screen.
                        if at_root && entries.len() == 1 && entries[0].entry_type == "root" {
                            let root_path = entries[0].path.clone();
                            {
                                let mut guard = lock(&shared);
                                if guard.games_ticket != ticket {
                                    return;
                                }
                                guard.persist.games.path_stack = vec![root_path.clone()];
                                guard.persist.games.selected_at_level = vec![String::new()];
                            }
                            browse_games(&ctx2, &app, &system_id, &system_name, &root_path, flip);
                            return;
                        }

                        let rows: Vec<GameRow> = entries.iter().map(GameRow::from).collect();
                        let cursor = result
                            .pagination
                            .as_ref()
                            .filter(|p| p.has_next_page)
                            .and_then(|p| p.next_cursor.clone());
                        let total_dirs = result.total_dirs.unwrap_or(0);
                        let total_rows = Some(result.total_files + total_dirs);
                        apply_list_fill(
                            &shared,
                            &media,
                            &app,
                            ticket,
                            rows,
                            cursor,
                            total_rows,
                            total_dirs,
                            "games",
                            system_name.as_str(),
                            flip,
                        );
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let title = system_name.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        show_games_error(&app, "games", &title, &message);
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

/// The cover decode tier for the games grid at the current output
/// geometry (request size == decode size).
fn games_cover_tier(app: &App) -> u32 {
    sizing::games_grid_cover_source_size(output_scene(app))
}

/// The scene at the current output geometry, as the sizing rules see it:
/// the physical output size during DRS rather than the transient Slint
/// window size, plus the rendering flags the `Sizing` global carries.
fn output_scene(app: &App) -> sizing::Scene {
    let size = app.window().size();
    let (out_w, out_h) = crate::output_size()
        .map_or((f64::from(size.width), f64::from(size.height)), |(w, h)| {
            (f64::from(w), f64::from(h))
        });
    let sizing_global = app.global::<Sizing>();
    sizing::Scene {
        width: out_w,
        height: out_h,
        crt: sizing_global.get_crt(),
        bitmap_fonts: sizing_global.get_bitmap_fonts(),
        swap_axes: sizing_global.get_swap_axes(),
    }
}

/// Tiles for a page, with any already-cached art filled in
/// synchronously (cache reads only, no fetches). Used for the
/// page-swoop strip: the incoming page shows its cached covers while
/// it slides, and the commit's `show_games_page` fetches the rest.
/// Displayed row name: Core's cleaned title, or the on-disk filename
/// without its extension when Show original filenames is on (the Qt
/// `display_name_for_entry` / `file_stem_or_name` pair).
fn display_name(e: &GameRow, show_original_filenames: bool) -> String {
    if show_original_filenames && !e.is_dir && !e.path.is_empty() {
        if let Some(stem) = std::path::Path::new(&e.path)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }
    e.name.clone()
}

fn build_game_tiles(
    entries: &[GameRow],
    media: &Arc<MediaCache>,
    system_id: &str,
    tier: u32,
    show_original_filenames: bool,
) -> Vec<GameTile> {
    let tag_rows: Vec<(String, Vec<String>)> = entries
        .iter()
        .map(|e| {
            (
                display_name(e, show_original_filenames),
                e.tag_labels.clone(),
            )
        })
        .collect();
    let tag_displays = crate::tag_utils::sibling_disambiguation_displays(&tag_rows);
    entries
        .iter()
        .zip(tag_displays.iter())
        .map(|(e, tags)| {
            let mut tile = GameTile {
                name: SharedString::from(display_name(e, show_original_filenames).as_str()),
                path: SharedString::from(e.path.as_str()),
                cover: slint::Image::default(),
                has_cover: false,
                is_favorite: e.is_favorite,
                thumb: slint::Image::default(),
                has_thumb: false,
                tags: SharedString::from(tags.as_str()),
            };
            if !e.is_dir && e.has_cover {
                let system = if e.system_id.is_empty() {
                    system_id.to_string()
                } else {
                    e.system_id.clone()
                };
                let mut key = MediaKey {
                    media_id: e.media_id,
                    system,
                    path: e.path.clone(),
                    max_size: tier,
                };
                if let Some(image) = media.get(&key) {
                    tile.cover = slint::Image::from_rgba8(image.buffer.clone());
                    tile.has_cover = true;
                } else {
                    key.max_size = crate::media_cache::THUMB_TIER;
                    if let Some(image) = media.get(&key) {
                        tile.thumb = slint::Image::from_rgba8(image.buffer.clone());
                        tile.has_thumb = true;
                    }
                }
            }
            tile
        })
        .collect()
}

/// Animate a page flip: the incoming page's tiles slide in as one
/// full-viewport sweep (the animation Qt's software path could never
/// afford), then the flip commits invisibly - `show_games_page` swaps
/// the real grid at slide end while the strip snaps back to rest with
/// animations gated off, so the final frame's pixels are identical.
#[allow(
    clippy::too_many_lines,
    reason = "page projection keeps selection, detail, and animation state atomic"
)]
fn slide_to_page(
    shared: &Arc<Mutex<Shared>>,
    media: &Arc<MediaCache>,
    app: &App,
    page: usize,
    dir: i32,
) {
    let view = app.global::<crate::GamesView>();
    if view.get_games_page_slide() != 0.0 || view.get_games_cached_transition() {
        return;
    }
    let cols = view.get_games_grid_cols().max(1) as usize;
    let col = (view.get_games_index().max(0) as usize) % cols;
    let (entries, system_id, show_files, reduce_motion, target) = {
        let mut guard = lock(shared);
        let Some(entries) = guard.games_pages.get(page).cloned() else {
            return;
        };
        if entries.is_empty() {
            return;
        }
        // Focus lands on the same column, entry row (top row sliding
        // down, bottom row sliding up); persist it now so the commit's
        // show_games_page restores exactly there.
        let target = if dir > 0 {
            col.min(entries.len() - 1)
        } else {
            let rows = entries.len().div_ceil(cols);
            ((rows - 1) * cols + col).min(entries.len() - 1)
        };
        if guard.games_mode == GamesMode::Browse {
            if let Some(top) = guard.persist.games.selected_at_level.last_mut() {
                top.clone_from(&entries[target].path);
            }
        }
        (
            entries,
            guard.games_system_id.clone(),
            guard.persist.settings.show_original_filenames,
            guard.persist.settings.reduce_motion,
            target,
        )
    };
    save_persist(shared);

    // Rapid-paging badge (RapidScrollIndicator.qml): a second flip
    // within 600 ms shows the landing page's first letter, cleared
    // 700 ms after the last flip by the seq-guarded timer.
    {
        let now = std::time::Instant::now();
        let (rapid, ticket) = {
            let mut guard = lock(shared);
            let rapid = guard
                .rapid_last_flip
                .is_some_and(|t| now.duration_since(t).as_millis() < 600);
            guard.rapid_last_flip = Some(now);
            guard.rapid_seq += 1;
            (rapid, guard.rapid_seq)
        };
        if rapid {
            let letter = entries.first().map_or_else(String::new, |e| {
                let trimmed = e.name.trim();
                trimmed
                    .chars()
                    .next()
                    .map_or("#".to_string(), |c| c.to_uppercase().to_string())
            });
            app.global::<crate::GamesView>()
                .set_rapid_letter(SharedString::from(letter.as_str()));
            let weak = app.as_weak();
            let shared2 = shared.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(700), move || {
                if lock(&shared2).rapid_seq != ticket {
                    return;
                }
                if let Some(app) = weak.upgrade() {
                    app.global::<crate::GamesView>()
                        .set_rapid_letter(SharedString::default());
                }
            });
        }
    }

    view.set_games_slide_dir(dir);
    view.set_games_transition_target_index(i32::try_from(target).unwrap_or(0));
    if !reduce_motion && request_cached_games_page_transition(app, dir) {
        view.set_games_slide_anim(false);
        view.set_games_cached_transition(true);
        view.set_games_next_page(ModelRc::default());
        show_games_page(shared, media, app, page);

        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(260), move || {
            if let Some(app) = weak.upgrade() {
                let view = app.global::<crate::GamesView>();
                view.set_games_cached_transition(false);
                view.set_games_slide_anim(true);
            }
        });
        return;
    }

    let tiles = build_game_tiles(
        &entries,
        media,
        &system_id,
        games_cover_tier(app),
        show_files,
    );
    // Declare the heavy phase: the strip repaints the whole viewport
    // every frame of the slide, so DRS drops to motion res for
    // exactly this stretch (crate::drs). Ends at the commit. With
    // Reduce motion on the strip cuts in one frame - nothing to mask,
    // so no DRS switch either.
    if !reduce_motion {
        crate::drs::heavy_begin();
    }
    app.global::<crate::GamesView>().set_games_slide_anim(true);
    app.global::<crate::GamesView>()
        .set_games_next_page(ModelRc::new(VecModel::from(tiles)));
    // Kick the strip: page-slide drives the animated y.
    app.global::<crate::GamesView>()
        .set_games_page_slide(dir as f32);

    let weak = app.as_weak();
    let shared = shared.clone();
    let media = media.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(260), move || {
        // Phase over regardless of what the upgrade says - the count
        // must never leak (and must not cancel someone else's phase
        // when reduce-motion skipped the begin).
        if !reduce_motion {
            crate::drs::heavy_end();
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        // Commit with animations off: the grid swap + snap to rest
        // must not glide (the strip already moved these pixels).
        app.global::<crate::GamesView>().set_games_slide_anim(false);
        show_games_page(&shared, &media, &app, page);
        app.global::<crate::GamesView>().set_games_page_slide(0.0);
        app.global::<crate::GamesView>()
            .set_games_next_page(ModelRc::new(VecModel::from(Vec::<GameTile>::new())));
        // Re-arm animations a frame later, after the snap rendered.
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(app) = weak.upgrade() {
                app.global::<crate::GamesView>().set_games_slide_anim(true);
            }
        });
    });
}

// ---------- Detailed-list layout (BrowseListDetailView port) ----------

/// Visible rows follow the original profile: portrait non-CRT list
/// views show more rows along their long logical axis.
const DEFAULT_LIST_VISIBLE_ROWS: usize = 10;
const TATE_LIST_VISIBLE_ROWS: usize = 16;

fn list_visible_rows(app: &App) -> usize {
    let shell = app.global::<crate::Shell>();
    if !shell.get_crt_enabled() && matches!(shell.get_orientation().as_str(), "cw" | "ccw") {
        TATE_LIST_VISIBLE_ROWS
    } else {
        DEFAULT_LIST_VISIBLE_ROWS
    }
}

/// The list layout is a browse-setting, not per-screen state.
fn list_layout_on(guard: &Shared) -> bool {
    guard.persist.settings.games_browse_layout == "list"
}

/// Client + runtime handle for the detail pane's debounced media.meta
/// fetch. The fills that trigger a re-render run inside spawned
/// closures that only carry `shared`/`media`, so the fetch pair lives
/// as a seeded global (the Qt models' `global_store`/`global_handle`
/// pattern).
static DETAIL_CTX: std::sync::OnceLock<(Arc<zaparoo_core::client::Client>, Handle)> =
    std::sync::OnceLock::new();

pub fn seed_detail_ctx(client: Arc<zaparoo_core::client::Client>, handle: Handle) {
    let _ = DETAIL_CTX.set((client, handle));
}

/// Detail-table rows from media.meta tags: fixed label set, values
/// resolved through the Qt alias lists, empty rows dropped.
fn detail_rows_from_tags(source: &[zaparoo_core::media_types::TagInfo]) -> Vec<crate::DetailRow> {
    fn tag_display_value(tag: &zaparoo_core::media_types::TagInfo) -> String {
        let label = tag.label.trim();
        if label.is_empty() {
            tag.tag.trim().to_string()
        } else {
            label.to_string()
        }
    }
    let value_for = |aliases: &[&str]| -> String {
        source
            .iter()
            .filter(|tag| {
                aliases
                    .iter()
                    .any(|alias| tag.tag_type.eq_ignore_ascii_case(alias))
                    && !tag_display_value(tag).is_empty()
            })
            .map(tag_display_value)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let rows = [
        ("Year", value_for(&["year", "release date", "release_date"])),
        ("Genre", value_for(&["genre", "gamegenre"])),
        ("Players", value_for(&["players"])),
        ("Developer", value_for(&["developer"])),
        ("Publisher", value_for(&["publisher"])),
        ("Rating", value_for(&["rating"])),
    ];
    rows.into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(label, value)| crate::DetailRow {
            label: SharedString::from(label),
            value: SharedString::from(value.as_str()),
        })
        .collect()
}

/// Refresh the detail pane for the current list selection: identity
/// fields immediately (title, cached cover), then the media.meta rows
/// and description after the 220 ms debounce so rapid scrolling never
/// queues a fetch per row (`FocusedMediaDetailController`'s contract).
fn update_detail(shared: &Arc<Mutex<Shared>>, media: &Arc<MediaCache>, app: &App) {
    let (entry, ticket, show_files, fallback_system) = {
        let mut guard = lock(shared);
        guard.detail_seq += 1;
        (
            guard.games_entries.get(guard.list_index).cloned(),
            guard.detail_seq,
            guard.persist.settings.show_original_filenames,
            guard.games_system_id.clone(),
        )
    };
    let view = app.global::<crate::GamesView>();
    view.set_detail_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
    view.set_detail_description(SharedString::default());
    let Some(entry) = entry else {
        view.set_detail_title(SharedString::default());
        view.set_detail_path(SharedString::default());
        view.set_detail_has_cover(false);
        return;
    };
    view.set_detail_title(SharedString::from(
        display_name(&entry, show_files).as_str(),
    ));
    view.set_detail_path(SharedString::from(entry.path.as_str()));

    // Cover: cached bytes paint immediately, misses stream in through
    // apply_cover's detail patch.
    let system = if entry.system_id.is_empty() {
        fallback_system
    } else {
        entry.system_id.clone()
    };
    let tier = sizing::detail_cover_source_size(output_scene(app));
    let key = MediaKey {
        media_id: entry.media_id,
        system: system.clone(),
        path: entry.path.clone(),
        max_size: tier,
    };
    if let Some(image) = media.get(&key) {
        view.set_detail_cover(slint::Image::from_rgba8(image.buffer.clone()));
        view.set_detail_has_cover(true);
    } else {
        view.set_detail_has_cover(false);
        if !entry.is_dir && entry.has_cover {
            media.enqueue(key);
        }
    }
    if entry.is_dir {
        return;
    }

    let weak = app.as_weak();
    let shared = shared.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(220), move || {
        if lock(&shared).detail_seq != ticket {
            return;
        }
        let Some((client, handle)) = DETAIL_CTX.get().cloned() else {
            return;
        };
        let params = zaparoo_core::media_types::MediaMetaParams {
            media_id: entry.media_id,
            system,
            path: entry.path.clone(),
        };
        handle.spawn(async move {
            let Ok(result) = client.media_meta(params).await else {
                return;
            };
            let rows = detail_rows_from_tags(&result.media.tags);
            let description = result
                .media
                .properties
                .get("property:description")
                .map(|p| p.text.clone())
                .unwrap_or_default();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if lock(&shared).detail_seq != ticket {
                    return;
                }
                let view = app.global::<crate::GamesView>();
                view.set_detail_rows(ModelRc::new(VecModel::from(rows)));
                view.set_detail_description(SharedString::from(description.as_str()));
            });
        });
    });
}

/// Project the list layout: flatten every fetched page (the absolute-
/// index invariant `games_entries` carries in this mode), window the
/// visible slice around the centered selection slot, and refresh the
/// detail pane.
fn show_games_list(shared: &Arc<Mutex<Shared>>, media: &Arc<MediaCache>, app: &App) {
    let target_visible = list_visible_rows(app);
    let (slice, sel, view_top, total, system_id, show_files) = {
        let mut guard = lock(shared);
        let all: Vec<GameRow> = guard.games_pages.iter().flatten().cloned().collect();
        let sel = guard.list_index.min(all.len().saturating_sub(1));
        guard.list_index = sel;
        guard.games_entries.clone_from(&all);
        let visible = target_visible.min(all.len().max(1));
        let center = (target_visible - 1) / 2;
        let view_top = sel
            .saturating_sub(center)
            .min(all.len().saturating_sub(visible));
        let slice: Vec<GameRow> = all.iter().skip(view_top).take(visible).cloned().collect();
        (
            slice,
            sel,
            view_top,
            all.len(),
            guard.games_system_id.clone(),
            guard.persist.settings.show_original_filenames,
        )
    };
    let tiles = build_game_tiles(
        &slice,
        media,
        &system_id,
        crate::media_cache::THUMB_TIER,
        show_files,
    );
    let view = app.global::<crate::GamesView>();
    view.set_list_rows(ModelRc::new(VecModel::from(tiles)));
    view.set_list_sel(i32::try_from(sel - view_top).unwrap_or(0));
    view.set_list_view_top(i32::try_from(view_top).unwrap_or(0));
    view.set_list_total(i32::try_from(total).unwrap_or(0));
    view.set_list_visible(i32::try_from(target_visible).unwrap_or(10));
    update_detail(shared, media, app);
}

/// Project a fetched page into the grid: tiles, focus reset, page
/// indicator, and cover fetches at the tile decode tier. In the
/// detailed-list layout every caller delegates to the list projection
/// instead (page arguments only mean something to the grid).
fn show_games_page(shared: &Arc<Mutex<Shared>>, media: &Arc<MediaCache>, app: &App, page: usize) {
    // Bound first: an inline `lock()` in the condition would hold the
    // guard across the body while the list projection re-locks.
    let list_mode = {
        let guard = lock(shared);
        list_layout_on(&guard)
    };
    if list_mode {
        show_games_list(shared, media, app);
        return;
    }
    let (entries, system_id, has_more, selected_path) = {
        let mut guard = lock(shared);
        let Some(entries) = guard.games_pages.get(page).cloned() else {
            return;
        };
        guard.games_page = page;
        guard.games_entries.clone_from(&entries);
        let has_more = page + 1 < guard.games_pages.len() || guard.games_next_cursor.is_some();
        let selected = match guard.games_mode {
            GamesMode::Browse => guard
                .persist
                .games
                .selected_at_level
                .last()
                .cloned()
                .unwrap_or_default(),
            GamesMode::Favorites => guard.persist.favorites.selected_path.clone(),
            GamesMode::Recents => guard.persist.recents.selected_path.clone(),
        };
        (entries, guard.games_system_id.clone(), has_more, selected)
    };

    // Tiles carry any already-cached art from the start (the same
    // builder the swoop strip uses): revisiting a page paints its
    // covers in the very frame the grid swaps, with no thumb-to-cover
    // replay. Only misses go through the fetch driver below.
    let tier = games_cover_tier(app);
    let show_files = lock(shared).persist.settings.show_original_filenames;
    let tiles = build_game_tiles(&entries, media, &system_id, tier, show_files);
    app.global::<crate::GamesView>()
        .set_games(ModelRc::new(VecModel::from(tiles)));
    // Restore focus to the persisted selection for this level when it
    // is on the page (kill-relaunch and folder pop both land here);
    // otherwise start at the top.
    let restored = (!selected_path.is_empty())
        .then(|| entries.iter().position(|e| e.path == selected_path))
        .flatten()
        .unwrap_or(0);
    app.global::<crate::GamesView>()
        .set_games_index(restored as i32);
    app.global::<crate::GamesView>().set_games_page(page as i32);
    app.global::<crate::GamesView>()
        .set_games_has_more(has_more);
    // Qt's TopStatusStrip counter: "Page N / M" from Core's totals
    // (0 = unknown totals, hides the counter - favorites/recents).
    let total_pages = lock(shared)
        .games_total_rows
        .map_or(0, |total| total.div_ceil(games_page_size(app)).max(1));
    app.global::<crate::GamesView>()
        .set_games_total_pages(i32::try_from(total_pages).unwrap_or(0));

    request_page_covers(media, &entries, &system_id, tier);
}

/// Queue fetches for the page's MISSING cover art: cache hits are
/// already baked into the tiles by `build_game_tiles`, so only misses
/// reach the driver - the whole page of 32px thumbs first (every tile
/// paints a preview within the first disk round-trips), then the full
/// tier, each landing through the driver's `on_ready` apply.
fn request_page_covers(media: &Arc<MediaCache>, entries: &[GameRow], system_id: &str, tier: u32) {
    let missing: Vec<MediaKey> = entries
        .iter()
        .filter(|e| !e.is_dir && e.has_cover)
        .map(|entry| {
            let system = if entry.system_id.is_empty() {
                system_id.to_string()
            } else {
                entry.system_id.clone()
            };
            MediaKey {
                media_id: entry.media_id,
                system,
                path: entry.path.clone(),
                max_size: tier,
            }
        })
        .filter(|key| media.get(key).is_none())
        .collect();
    for key in &missing {
        media.enqueue(MediaKey {
            max_size: crate::media_cache::THUMB_TIER,
            ..key.clone()
        });
    }
    for key in missing {
        media.enqueue(key);
    }
}

/// Fetch the next page with the stored cursor, bypassing the endpoint
/// cache exactly like the Qt `GamesModel::fetch_more` (each follow-up
/// has a different cursor, so caching would pollute the key space).
fn fetch_games_next(ctx: &Ctx, app: &App) {
    let (cursor, system_id, browse_path, ticket) = {
        let mut guard = lock(&ctx.shared);
        if guard.games_fetching {
            return;
        }
        let Some(cursor) = guard.games_next_cursor.clone() else {
            return;
        };
        guard.games_fetching = true;
        (
            cursor,
            guard.games_system_id.clone(),
            guard.games_browse_path.clone(),
            guard.games_ticket,
        )
    };
    app.global::<crate::Shell>()
        .set_status_text(SharedString::from("Loading more…"));

    let client = ctx.store.client();
    let page_size = games_page_size(app);
    let weak = app.as_weak();
    let shared = ctx.shared.clone();
    let media = ctx.media.clone();
    ctx.handle.spawn(async move {
        let outcome = client
            .media_browse(zaparoo_core::media_types::MediaBrowseParams {
                root_view: zaparoo_core::media_types::merged_root_view(
                    &browse_path,
                    std::slice::from_ref(&system_id),
                ),
                path: browse_path,
                systems: vec![system_id],
                max_results: Some(page_size),
                cursor: Some(cursor),
                tags: Vec::new(),
                letter: None,
                sort: None,
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.global::<crate::Shell>()
                .set_status_text(SharedString::default());
            let (next_page, slide) = {
                let mut guard = lock(&shared);
                guard.games_fetching = false;
                let slide = std::mem::take(&mut guard.games_slide_on_arrival);
                if guard.games_ticket != ticket {
                    return;
                }
                match outcome {
                    Ok(result) => {
                        guard.games_next_cursor = result
                            .pagination
                            .as_ref()
                            .filter(|p| p.has_next_page)
                            .and_then(|p| p.next_cursor.clone());
                        guard
                            .games_pages
                            .push(result.entries.iter().map(GameRow::from).collect());
                        let next_page = guard.games_pages.len() - 1;
                        // Only swoop into the page directly after the
                        // one on screen; anything else cuts.
                        (next_page, slide && next_page == guard.games_page + 1)
                    }
                    Err(e) => {
                        app.global::<crate::Shell>()
                            .set_status_text(SharedString::from(
                                format!("Page fetch failed: {}", e.message).as_str(),
                            ));
                        return;
                    }
                }
            };
            if slide {
                slide_to_page(&shared, &media, &app, next_page, 1);
            } else {
                show_games_page(&shared, &media, &app, next_page);
            }
        });
    });
}

/// Advance one page with the swoop: slide immediately when the next
/// page is already fetched, otherwise fetch it and slide on arrival.
fn page_forward(ctx: &Ctx, app: &App) {
    let (page, fetched, has_cursor) = {
        let guard = lock(&ctx.shared);
        (
            guard.games_page,
            guard.games_pages.len(),
            guard.games_next_cursor.is_some(),
        )
    };
    if page + 1 < fetched {
        slide_to_page(&ctx.shared, &ctx.media, app, page + 1, 1);
    } else if has_cursor {
        lock(&ctx.shared).games_slide_on_arrival = true;
        fetch_games_next(ctx, app);
    }
}

/// Detailed-list input: linear selection over the flattened fetched
/// rows with a centered scroll slot; Left/Right (and L/R) jump by a
/// screenful, Down at the end streams the next Core page in.
fn games_list_action(ctx: &Ctx, app: &App, action: &str) {
    let (len, sel, mode, has_cursor) = {
        let guard = lock(&ctx.shared);
        (
            guard.games_entries.len(),
            guard.list_index,
            guard.games_mode,
            guard.games_next_cursor.is_some(),
        )
    };
    let visible_rows = list_visible_rows(app);
    let mut target: Option<usize> = None;
    match action {
        actions::UP => {
            if sel > 0 {
                target = Some(sel - 1);
            }
        }
        actions::DOWN => {
            if sel + 1 < len {
                target = Some(sel + 1);
            } else if has_cursor {
                fetch_games_next(ctx, app);
                return;
            }
        }
        actions::LEFT | actions::PAGE_PREV => {
            if sel > 0 {
                target = Some(sel.saturating_sub(visible_rows));
            }
        }
        actions::RIGHT | actions::PAGE_NEXT => {
            if sel + visible_rows < len {
                target = Some(sel + visible_rows);
            } else if len > 0 && sel + 1 < len {
                target = Some(len - 1);
            } else if has_cursor {
                fetch_games_next(ctx, app);
                return;
            }
        }
        actions::ACCEPT => {
            let entry = lock(&ctx.shared).games_entries.get(sel).cloned();
            if let Some(entry) = entry {
                if entry.is_dir {
                    descend_into_folder(ctx, app, &entry.path);
                } else if !entry.zap_script.is_empty() {
                    launch(ctx, app, entry.zap_script);
                } else {
                    launch(ctx, app, entry.path);
                }
            }
        }
        actions::CONTEXT_MENU => open_context_menu(ctx, app, sel),
        actions::PAGE_MENU if mode == GamesMode::Browse => open_view_menu(ctx, app),
        actions::CANCEL => match mode {
            GamesMode::Browse => {
                let popped = pop_folder_level(ctx, app);
                if !popped {
                    lock(&ctx.shared).persist.active_screen = "systems".to_string();
                    save_persist(&ctx.shared);
                    transition_to_screen(app, "systems", -1);
                }
            }
            GamesMode::Favorites | GamesMode::Recents => {
                lock(&ctx.shared).persist.active_screen = "hub".to_string();
                save_persist(&ctx.shared);
                transition_to_screen(app, "hub", -1);
            }
        },
        _ => {}
    }
    if let Some(next) = target {
        list_move_to(ctx, app, next.min(len.saturating_sub(1)));
    }
}

/// Land the list selection on `index`: persist the per-mode path,
/// re-window, and prefetch the next Core page when the selection is
/// within a screenful of the fetched end.
fn list_move_to(ctx: &Ctx, app: &App, index: usize) {
    {
        let mut shared = lock(&ctx.shared);
        shared.list_index = index;
        if let Some(entry) = shared.games_entries.get(index) {
            let path = entry.path.clone();
            match shared.games_mode {
                GamesMode::Browse => {
                    if let Some(top) = shared.persist.games.selected_at_level.last_mut() {
                        *top = path;
                    }
                }
                GamesMode::Favorites => shared.persist.favorites.selected_path = path,
                GamesMode::Recents => shared.persist.recents.selected_path = path,
            }
        }
    }
    save_persist(&ctx.shared);
    show_games_list(&ctx.shared, &ctx.media, app);
    let (near_end, cursor) = {
        let guard = lock(&ctx.shared);
        (
            index + list_visible_rows(app) >= guard.games_entries.len(),
            guard.games_next_cursor.is_some(),
        )
    };
    if near_end && cursor {
        fetch_games_next(ctx, app);
    }
}

fn games_action(ctx: &Ctx, app: &App, action: &str) {
    let list_mode = {
        let guard = lock(&ctx.shared);
        list_layout_on(&guard)
    };
    if list_mode {
        games_list_action(ctx, app, action);
        return;
    }
    // A page transition in flight owns the grid; swallow input until
    // either transition gate clears.
    let view = app.global::<crate::GamesView>();
    if view.get_games_page_slide() != 0.0 || view.get_games_cached_transition() {
        return;
    }
    let len = app.global::<crate::GamesView>().get_games().row_count();
    let index = app.global::<crate::GamesView>().get_games_index() as usize;
    let cols = app
        .global::<crate::GamesView>()
        .get_games_grid_cols()
        .max(1) as usize;
    let mode = lock(&ctx.shared).games_mode;
    let mut moved = None;
    match action {
        actions::LEFT => moved = Some(hub_nav::grid_move(index, len, cols, -1, 0)),
        actions::RIGHT => moved = Some(hub_nav::grid_move(index, len, cols, 1, 0)),
        actions::UP => moved = Some(hub_nav::grid_move(index, len, cols, 0, -1)),
        actions::DOWN => moved = Some(hub_nav::grid_move(index, len, cols, 0, 1)),
        actions::ACCEPT => {
            let entry = lock(&ctx.shared).games_entries.get(index).cloned();
            if let Some(entry) = entry {
                if entry.is_dir {
                    descend_into_folder(ctx, app, &entry.path);
                } else if !entry.zap_script.is_empty() {
                    launch(ctx, app, entry.zap_script);
                } else {
                    launch(ctx, app, entry.path);
                }
            }
        }
        actions::CONTEXT_MENU => open_context_menu(ctx, app, index),
        actions::PAGE_MENU if mode == GamesMode::Browse => open_view_menu(ctx, app),
        actions::PAGE_NEXT if mode == GamesMode::Browse => {
            page_forward(ctx, app);
        }
        actions::PAGE_PREV if mode == GamesMode::Browse => {
            let page = lock(&ctx.shared).games_page;
            if page > 0 {
                slide_to_page(&ctx.shared, &ctx.media, app, page - 1, -1);
            }
        }
        actions::CANCEL => match mode {
            GamesMode::Browse => {
                // Inside a folder, Back pops one level; at root it
                // leaves the screen (_navigateOutOfFolder's rule).
                let popped = pop_folder_level(ctx, app);
                if !popped {
                    lock(&ctx.shared).persist.active_screen = "systems".to_string();
                    save_persist(&ctx.shared);
                    transition_to_screen(app, "systems", -1);
                }
            }
            GamesMode::Favorites | GamesMode::Recents => {
                lock(&ctx.shared).persist.active_screen = "hub".to_string();
                save_persist(&ctx.shared);
                transition_to_screen(app, "hub", -1);
            }
        },
        _ => {}
    }
    // Vertical edge press = page swoop: Down on the bottom row slides
    // the next page in from below, Up on the top row slides the
    // previous page back down (Browse only, like L/R paging).
    if mode == GamesMode::Browse && len > 0 && moved == Some(index) {
        if action == actions::DOWN {
            page_forward(ctx, app);
            return;
        }
        if action == actions::UP && index < cols {
            let page = lock(&ctx.shared).games_page;
            if page > 0 {
                slide_to_page(&ctx.shared, &ctx.media, app, page - 1, -1);
            }
            return;
        }
    }
    if let Some(next) = moved {
        app.global::<crate::GamesView>()
            .set_games_index(next as i32);
        let mut shared = lock(&ctx.shared);
        if let Some(entry) = shared.games_entries.get(next) {
            let path = entry.path.clone();
            match shared.games_mode {
                // Selection is per folder level: update the top of the
                // stack, never flatten it (selected_at_level and
                // path_stack stay the same length).
                GamesMode::Browse => {
                    if let Some(top) = shared.persist.games.selected_at_level.last_mut() {
                        *top = path;
                    }
                }
                GamesMode::Favorites => shared.persist.favorites.selected_path = path,
                GamesMode::Recents => shared.persist.recents.selected_path = path,
            }
        }
        drop(shared);
        save_persist(&ctx.shared);
    }
}

/// Open the West "View" menu (the page/list-scoped operations menu,
/// counterpart to North's item-scoped Options). One entry today -
/// Go to..., pre-focused so the common path is a fixed West-then-
/// Accept chord. The letter facet fetch is kicked off here so the
/// buckets are likely ready by the time the user advances into the
/// grid (the Qt openPageMenu flow).
fn open_view_menu(ctx: &Ctx, app: &App) {
    fetch_letter_index(ctx, app);
    lock(&ctx.shared).list_context = ListContext::ViewMenu;
    app.global::<crate::Overlays>()
        .set_list_title(SharedString::from("View"));
    app.global::<crate::Overlays>()
        .set_list_entries(ModelRc::new(VecModel::from(vec![menu_entry(
            "jump_letter",
            "Go to...",
        )])));
    app.global::<crate::Overlays>().set_list_index(0);
    app.global::<crate::Overlays>().set_list_open(true);
}

fn list_action(ctx: &Ctx, app: &App, action: &str) {
    let len = app
        .global::<crate::Overlays>()
        .get_list_entries()
        .row_count();
    let index = app.global::<crate::Overlays>().get_list_index().max(0) as usize;
    match action {
        actions::UP if len > 0 => {
            app.global::<crate::Overlays>()
                .set_list_index(((index + len - 1) % len) as i32);
        }
        actions::DOWN if len > 0 => {
            app.global::<crate::Overlays>()
                .set_list_index(((index + 1) % len) as i32);
        }
        actions::ACCEPT => {
            let id = app
                .global::<crate::Overlays>()
                .get_list_entries()
                .row_data(index)
                .map(|e| e.id.to_string());
            if let Some(id) = id {
                app.global::<crate::Overlays>().set_list_open(false);
                let context = lock(&ctx.shared).list_context.clone();
                match context {
                    ListContext::ViewMenu => {
                        if id == "jump_letter" {
                            open_letter_jump(ctx, app);
                        }
                    }
                    ListContext::SettingsPicker(field) => {
                        settings_picker_selected(ctx, app, &field, &id);
                    }
                    ListContext::SystemLauncher(system_id) => {
                        set_system_launcher(ctx, &system_id, &id);
                    }
                }
            }
        }
        actions::CANCEL | actions::PAGE_MENU => {
            app.global::<crate::Overlays>().set_list_open(false);
        }
        _ => {}
    }
}

/// Fetch the browse facet for the current scope (`media.browse.index`,
/// non-empty buckets in sort order) and publish it to the picker
/// properties as it lands; the seq ticket drops stale responses.
fn fetch_letter_index(ctx: &Ctx, app: &App) {
    let (browse_path, system_id, ticket) = {
        let mut guard = lock(&ctx.shared);
        guard.letter_seq += 1;
        guard.letter_buckets.clear();
        (
            guard.games_browse_path.clone(),
            guard.games_system_id.clone(),
            guard.letter_seq,
        )
    };
    app.global::<crate::Overlays>()
        .set_letter_buckets(ModelRc::new(VecModel::from(
            Vec::<crate::LetterBucket>::new(),
        )));
    app.global::<crate::Overlays>().set_letter_index(0);
    app.global::<crate::Overlays>().set_letter_loading(true);

    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let outcome = client
            .media_browse_index(zaparoo_core::media_types::MediaBrowseIndexParams {
                root_view: zaparoo_core::media_types::merged_root_view(
                    &browse_path,
                    std::slice::from_ref(&system_id),
                ),
                path: browse_path,
                systems: vec![system_id],
                tags: Vec::new(),
                sort: None,
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if lock(&shared).letter_seq != ticket {
                return;
            }
            match outcome {
                Ok(result) => {
                    let rows: Vec<crate::LetterBucket> = result
                        .groups
                        .iter()
                        .map(|g| crate::LetterBucket {
                            label: SharedString::from(g.label.as_str()),
                            count: i32::try_from(g.count).unwrap_or(i32::MAX),
                        })
                        .collect();
                    let columns = rows.len().clamp(1, 9);
                    lock(&shared).letter_buckets = result.groups;
                    app.global::<crate::Overlays>()
                        .set_letter_columns(columns as i32);
                    app.global::<crate::Overlays>()
                        .set_letter_buckets(ModelRc::new(VecModel::from(rows)));
                    app.global::<crate::Overlays>().set_letter_loading(false);
                }
                Err(e) => {
                    tracing::warn!("letter index fetch failed: {}", e.message);
                    app.global::<crate::Overlays>().set_letter_open(false);
                    app.global::<crate::Overlays>().set_letter_loading(false);
                }
            }
        });
    });
}

/// Open the jump-to-letter picker over whatever facet state the View
/// menu's prefetch has reached (loading or filled).
fn open_letter_jump(_ctx: &Ctx, app: &App) {
    app.global::<crate::Overlays>().set_letter_index(0);
    app.global::<crate::Overlays>().set_letter_open(true);
}

fn close_letter_jump(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).letter_seq += 1;
    app.global::<crate::Overlays>().set_letter_open(false);
}

fn letter_action(ctx: &Ctx, app: &App, action: &str) {
    let len = app
        .global::<crate::Overlays>()
        .get_letter_buckets()
        .row_count();
    let index = app.global::<crate::Overlays>().get_letter_index().max(0) as usize;
    let cols = app.global::<crate::Overlays>().get_letter_columns().max(1) as usize;
    match action {
        actions::LEFT if len > 0 => {
            app.global::<crate::Overlays>()
                .set_letter_index(((index + len - 1) % len) as i32);
        }
        actions::RIGHT if len > 0 => {
            app.global::<crate::Overlays>()
                .set_letter_index(((index + 1) % len) as i32);
        }
        actions::UP if len > 0 => {
            if index >= cols {
                app.global::<crate::Overlays>()
                    .set_letter_index((index - cols) as i32);
            }
        }
        actions::DOWN if len > 0 => {
            if index + cols < len {
                app.global::<crate::Overlays>()
                    .set_letter_index((index + cols) as i32);
            }
        }
        actions::ACCEPT if len > 0 => {
            // Bucket offset = cumulative count of all earlier buckets
            // (the Qt LetterJumpModal's accepted(itemOffset) rule; the
            // per-bucket cursor stays unused - this is a position
            // jump, not a forward-only seek).
            let offset: u32 = {
                let guard = lock(&ctx.shared);
                if index >= guard.letter_buckets.len() {
                    return;
                }
                guard
                    .letter_buckets
                    .iter()
                    .take(index)
                    .map(|g| g.count)
                    .sum()
            };
            close_letter_jump(ctx, app);
            jump_to_offset(ctx, app, offset);
        }
        actions::CANCEL | actions::PAGE_MENU => close_letter_jump(ctx, app),
        _ => {}
    }
}

/// Position jump, the Qt `jumpToItem` semantics on the demo's page
/// architecture: the absolute target is leading dirs + bucket offset;
/// pages already fetched are kept (`PAGE_PREV` keeps working), and the
/// gap up to the target is fetched in bulk chunks sized to whole
/// pages (batches always derive from `page_size`) with the Qt ceiling.
#[allow(
    clippy::too_many_lines,
    reason = "jump transaction keeps async fetch and selection publication together"
)]
fn jump_to_offset(ctx: &Ctx, app: &App, item_offset: u32) {
    const JUMP_FETCH_CEILING: u32 = 1000;

    let page_size = games_page_size(app).max(1);
    let (absolute, target_page, ticket, already_fetched) = {
        let guard = lock(&ctx.shared);
        let absolute = guard.games_total_dirs + item_offset;
        (
            absolute,
            (absolute / page_size) as usize,
            guard.games_ticket,
            guard.games_pages.len(),
        )
    };
    let target_idx = (absolute % page_size) as usize;

    if target_page < already_fetched {
        land_on(ctx, app, target_page, target_idx);
        return;
    }

    app.global::<crate::Shell>()
        .set_status_text(SharedString::from("Loading…"));
    let client = ctx.store.client();
    let (system_id, browse_path) = {
        let guard = lock(&ctx.shared);
        (
            guard.games_system_id.clone(),
            guard.games_browse_path.clone(),
        )
    };
    let weak = app.as_weak();
    let shared = ctx.shared.clone();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        // Fetch forward from the last loaded page until the target is
        // covered (or the list ends). Each request is whole pages:
        // gap rounded up, clamped to the ceiling's whole-page floor.
        let ceiling = (JUMP_FETCH_CEILING / page_size).max(1) * page_size;
        let landed: Result<(), String> = loop {
            let (cursor, loaded_rows, pages_len) = {
                let guard = lock(&shared);
                if guard.games_ticket != ticket {
                    return;
                }
                let loaded: u32 = guard.games_pages.iter().map(|p| p.len() as u32).sum();
                (
                    guard.games_next_cursor.clone(),
                    loaded,
                    guard.games_pages.len(),
                )
            };
            if pages_len > (absolute / page_size) as usize {
                break Ok(());
            }
            let Some(cursor) = cursor else {
                // List ended before the target: land on what exists.
                break Ok(());
            };
            let gap = (absolute + 1).saturating_sub(loaded_rows).max(1);
            let limit = gap.div_ceil(page_size).max(1) * page_size;
            let limit = limit.min(ceiling);
            let outcome = client
                .media_browse(zaparoo_core::media_types::MediaBrowseParams {
                    root_view: zaparoo_core::media_types::merged_root_view(
                        &browse_path,
                        std::slice::from_ref(&system_id),
                    ),
                    path: browse_path.clone(),
                    systems: vec![system_id.clone()],
                    max_results: Some(limit),
                    cursor: Some(cursor),
                    tags: Vec::new(),
                    letter: None,
                    sort: None,
                })
                .await;
            match outcome {
                Ok(result) => {
                    let mut guard = lock(&shared);
                    if guard.games_ticket != ticket {
                        return;
                    }
                    guard.games_next_cursor = result
                        .pagination
                        .as_ref()
                        .filter(|p| p.has_next_page)
                        .and_then(|p| p.next_cursor.clone());
                    let rows: Vec<GameRow> = result.entries.iter().map(GameRow::from).collect();
                    if rows.is_empty() {
                        guard.games_next_cursor = None;
                    }
                    append_jump_rows(&mut guard, &rows, page_size as usize);
                }
                Err(e) => break Err(e.message),
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.global::<crate::Shell>()
                .set_status_text(SharedString::default());
            match landed {
                Ok(()) => {
                    let pages_len = lock(&shared).games_pages.len();
                    if pages_len == 0 {
                        return;
                    }
                    let page = (absolute / page_size).min(pages_len as u32 - 1) as usize;
                    land_on(&ctx2, &app, page, target_idx);
                }
                Err(message) => {
                    app.global::<crate::Shell>()
                        .set_status_text(SharedString::from(
                            format!("Jump failed: {message}").as_str(),
                        ));
                }
            }
        });
    });
}

/// Append jump-fetched rows in exact `page_size` chunks so page
/// shapes stay uniform for the pager and the swoop; a short trailing
/// page (list ended earlier) is topped up first.
fn append_jump_rows(guard: &mut Shared, rows: &[GameRow], page_size: usize) {
    for chunk in rows.chunks(page_size) {
        if let Some(last) = guard.games_pages.last_mut() {
            if last.len() < page_size {
                let room = page_size - last.len();
                let (fill, rest) = chunk.split_at(room.min(chunk.len()));
                last.extend_from_slice(fill);
                if !rest.is_empty() {
                    guard.games_pages.push(rest.to_vec());
                }
                continue;
            }
        }
        guard.games_pages.push(chunk.to_vec());
    }
}

/// Focus a specific row on a specific (already fetched) page: persist
/// the selection so `show_games_page` restores exactly there.
fn land_on(ctx: &Ctx, app: &App, page: usize, index: usize) {
    {
        let mut guard = lock(&ctx.shared);
        let Some(rows) = guard.games_pages.get(page) else {
            return;
        };
        let clamped = index.min(rows.len().saturating_sub(1));
        let path = rows
            .get(clamped)
            .map(|r| r.path.clone())
            .unwrap_or_default();
        if guard.games_mode == GamesMode::Browse {
            if let Some(top) = guard.persist.games.selected_at_level.last_mut() {
                top.clone_from(&path);
            }
        }
        // List layout: the landing is an absolute index across the
        // fetched pages (whole-page chunks, so summing is exact).
        if list_layout_on(&guard) {
            let before: usize = guard.games_pages.iter().take(page).map(Vec::len).sum();
            guard.list_index = before + clamped;
        }
    }
    save_persist(&ctx.shared);
    show_games_page(&ctx.shared, &ctx.media, app, page);
}

/// Refresh the connected-readers flag from Core (gates the "Write to
/// NFC token" entry). Lazy: the flag read at menu-open time may be one
/// refresh old.
fn refresh_readers(ctx: &Ctx) {
    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    ctx.handle.spawn(async move {
        if let Ok(result) = client.readers().await {
            let mut guard = lock(&shared);
            guard.has_readers = !result.readers.is_empty();
            guard.has_nfc = result
                .readers
                .iter()
                .any(zaparoo_core::media_types::ReaderInfo::is_nfc_reader);
        }
    });
}

/// QR write deep-link for the focused row (QrCodeModal.qml's flow):
/// the scanning device opens zaparoo.app, which hands the zapscript
/// back to a Core/frontend pairing.
fn open_qr_code(app: &App, entry: &GameRow) {
    let text = if entry.zap_script.trim().is_empty() {
        entry.path.clone()
    } else {
        entry.zap_script.clone()
    };
    if text.is_empty() {
        tracing::warn!("QR code for {} has no launch payload", entry.name);
        return;
    }
    if let Some((image, modules)) = crate::qr::qr_image(&crate::qr::write_url(&text)) {
        app.global::<crate::Overlays>().set_qr_image(image);
        app.global::<crate::Overlays>()
            .set_qr_modules(i32::try_from(modules).unwrap_or(0));
        app.global::<crate::Overlays>().set_qr_open(true);
    }
}

fn menu_entry(id: &str, label: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        id: SharedString::from(id),
        label: SharedString::from(label),
    }
}

/// Open the item-scoped context menu on the focused games-style row,
/// with the Qt entry rules: no menu on plain folders; favorite toggle
/// on media-capable rows outside Recents; card write only with a
/// connected reader; Game info and Launch always.
fn open_context_menu(ctx: &Ctx, app: &App, index: usize) {
    let (entry, mode, has_nfc) = {
        let guard = lock(&ctx.shared);
        (
            guard.games_entries.get(index).cloned(),
            guard.games_mode,
            guard.has_nfc,
        )
    };
    let Some(entry) = entry else {
        return;
    };
    // Qt rule: plain folders get no menu, but media-capable directory
    // rows (a folder Core can launch as one item) keep theirs.
    if entry.is_dir && !entry.media_capable {
        return;
    }
    let mut entries: Vec<crate::MenuEntry> = Vec::new();
    if mode == GamesMode::Recents {
        // Recents is launch-only in the Qt menu.
        entries.push(menu_entry("launch_game", "Launch game"));
    } else {
        if entry.media_capable {
            entries.push(menu_entry(
                "toggle_favorite",
                if entry.is_favorite {
                    "Remove from favorites"
                } else {
                    "Add to favorites"
                },
            ));
        }
        // hasNfc, not has_readers: a connected non-NFC reader can't
        // take a token write (the Qt gate).
        if has_nfc {
            entries.push(menu_entry("write_card", "Write to NFC token"));
        }
        entries.push(menu_entry("qr_code", "QR code"));
        entries.push(menu_entry("more_info", "Game info"));
        entries.push(menu_entry("launch_game", "Launch game"));
    }

    present_context_menu(ctx, app, ContextOwner::Games, index, entries);
    refresh_readers(ctx);
}

/// Category tile menu (Hub top row): hide/unhide plus a scoped media
/// database rebuild when the category has indexable systems and no
/// media job is running.
fn open_category_context_menu(ctx: &Ctx, app: &App, index: usize) {
    let (is_hidden, has_indexable) = {
        let guard = lock(&ctx.shared);
        let Some(name) = guard.categories.get(index) else {
            return;
        };
        let is_hidden = guard.hidden_categories.iter().any(|h| h == name);
        let has_indexable = systems_for_category(&guard.systems, name)
            .iter()
            .any(|s| s.zap_script.trim().is_empty());
        (is_hidden, has_indexable)
    };
    let mut entries = vec![menu_entry(
        "toggle_hide_category",
        if is_hidden { "Unhide" } else { "Hide" },
    )];
    if has_indexable && !media_busy(app) {
        entries.push(menu_entry("index_category", "Update media database"));
        entries.push(menu_entry("scrape_category", "Scrape metadata"));
    }
    present_context_menu(ctx, app, ContextOwner::Categories, index, entries);
}

/// System tile menu: launch, a scoped index for real (non-launchable)
/// systems, and hide/unhide. `index` is absolute into the projected
/// `screen_systems` list.
fn open_system_context_menu(ctx: &Ctx, app: &App, index: usize) {
    let (launchable, is_hidden, has_launchers) = {
        let guard = lock(&ctx.shared);
        let Some(system) = guard.screen_systems.get(index) else {
            return;
        };
        (
            !system.zap_script.trim().is_empty(),
            guard.hidden_system_ids.iter().any(|h| h == &system.id),
            guard.launchers.iter().any(|l| l.system_id == system.id),
        )
    };
    let mut entries = vec![menu_entry("launch_system", "Launch core")];
    if !launchable && has_launchers {
        entries.push(menu_entry("change_launcher", "Change launcher"));
    }
    if !launchable && !media_busy(app) {
        entries.push(menu_entry("index_system", "Update media database"));
        entries.push(menu_entry("scrape_system", "Scrape metadata"));
    }
    entries.push(menu_entry(
        "toggle_hide_system",
        if is_hidden { "Unhide" } else { "Hide" },
    ));
    present_context_menu(ctx, app, ContextOwner::Systems, index, entries);
}

/// A media-database job is running; index/scrape entries drop out
/// while it does (the Qt mediaBusy gate, read off the same status
/// line the header shows).
fn media_busy(app: &App) -> bool {
    !app.global::<crate::Shell>()
        .get_media_status_text()
        .is_empty()
}

fn present_context_menu(
    ctx: &Ctx,
    app: &App,
    owner: ContextOwner,
    target: usize,
    entries: Vec<crate::MenuEntry>,
) {
    if entries.is_empty() {
        return;
    }
    {
        let mut guard = lock(&ctx.shared);
        guard.context_owner = owner;
        guard.context_target = target;
    }
    app.global::<crate::Overlays>()
        .set_context_entries(ModelRc::new(VecModel::from(entries)));
    app.global::<crate::Overlays>().set_context_index(0);
    app.global::<crate::Overlays>().set_context_open(true);
}

fn close_context_menu(ctx: &Ctx, app: &App) {
    // Bumping the seq abandons any in-flight card write (its result
    // is ignored on arrival - the Qt cancel rule).
    lock(&ctx.shared).card_write_seq += 1;
    app.global::<crate::Overlays>().set_context_open(false);
}

fn context_action(ctx: &Ctx, app: &App, action: &str) {
    let len = app
        .global::<crate::Overlays>()
        .get_context_entries()
        .row_count();
    let index = app.global::<crate::Overlays>().get_context_index().max(0) as usize;
    match action {
        actions::UP if len > 0 => {
            app.global::<crate::Overlays>()
                .set_context_index(((index + len - 1) % len) as i32);
        }
        actions::DOWN if len > 0 => {
            app.global::<crate::Overlays>()
                .set_context_index(((index + 1) % len) as i32);
        }
        actions::ACCEPT => {
            if let Some(entry) = app
                .global::<crate::Overlays>()
                .get_context_entries()
                .row_data(index)
            {
                context_accept(ctx, app, entry.id.as_str());
            }
        }
        actions::CANCEL | actions::CONTEXT_MENU => close_context_menu(ctx, app),
        _ => {}
    }
}

fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    let owner = lock(&ctx.shared).context_owner;
    match owner {
        ContextOwner::Games => context_accept_games(ctx, app, id),
        ContextOwner::Categories => context_accept_category(ctx, app, id),
        ContextOwner::Systems => context_accept_system(ctx, app, id),
    }
}

fn context_accept_games(ctx: &Ctx, app: &App, id: &str) {
    let (entry, target) = {
        let guard = lock(&ctx.shared);
        (
            guard.games_entries.get(guard.context_target).cloned(),
            guard.context_target,
        )
    };
    let Some(entry) = entry else {
        close_context_menu(ctx, app);
        return;
    };
    match id {
        "launch_game" => {
            close_context_menu(ctx, app);
            if entry.zap_script.is_empty() {
                launch(ctx, app, entry.path);
            } else {
                launch(ctx, app, entry.zap_script);
            }
        }
        "more_info" => {
            close_context_menu(ctx, app);
            open_game_info(ctx, app, &entry);
        }
        "toggle_favorite" => {
            close_context_menu(ctx, app);
            toggle_favorite(ctx, app, target, &entry);
        }
        "write_card" => {
            close_context_menu(ctx, app);
            begin_card_write(ctx, app, &entry);
        }
        "qr_code" => {
            close_context_menu(ctx, app);
            open_qr_code(app, &entry);
        }
        _ => {}
    }
}

fn context_accept_category(ctx: &Ctx, app: &App, id: &str) {
    let name = {
        let guard = lock(&ctx.shared);
        guard.categories.get(guard.context_target).cloned()
    };
    close_context_menu(ctx, app);
    let Some(name) = name else {
        return;
    };
    match id {
        "toggle_hide_category" => toggle_hidden_category(ctx, app, &name),
        "index_category" => {
            // Indexable = real systems only; launch-only ones carry no
            // media and Core would reject their ids.
            let ids: Vec<String> = {
                let guard = lock(&ctx.shared);
                systems_for_category(&guard.systems, &name)
                    .iter()
                    .filter(|s| s.zap_script.trim().is_empty())
                    .map(|s| s.id.clone())
                    .collect()
            };
            if !ids.is_empty() {
                start_index(ctx, app, Some(ids));
            }
        }
        "scrape_category" => {
            let ids: Vec<String> = {
                let guard = lock(&ctx.shared);
                systems_for_category(&guard.systems, &name)
                    .iter()
                    .filter(|s| s.zap_script.trim().is_empty())
                    .map(|s| s.id.clone())
                    .collect()
            };
            if !ids.is_empty() {
                start_scrape(ctx, ids, false);
            }
        }
        _ => {}
    }
}

fn context_accept_system(ctx: &Ctx, app: &App, id: &str) {
    let system = {
        let guard = lock(&ctx.shared);
        guard.screen_systems.get(guard.context_target).cloned()
    };
    close_context_menu(ctx, app);
    let Some(system) = system else {
        return;
    };
    match id {
        "launch_system" => {
            // Launchables run their own zaparoo:// script; normal
            // systems use the **launch.system directive (the Qt
            // launch_text_for rule).
            let text = if system.zap_script.trim().is_empty() {
                format!("**launch.system:{}", system.id)
            } else {
                system.zap_script.clone()
            };
            launch(ctx, app, text);
        }
        "index_system" => start_index(ctx, app, Some(vec![system.id.clone()])),
        "scrape_system" => start_scrape(ctx, vec![system.id.clone()], false),
        "toggle_hide_system" => toggle_hidden_system(ctx, app, &system.id),
        "change_launcher" => open_launcher_picker(ctx, app, &system.id),
        _ => {}
    }
}

/// "Change launcher" picker (the `SystemLaunchers` model's
/// `picker_entries_for_system`): Default first, the system's launchers
/// by id, plus a "Current: x" row when the stored default no longer
/// matches a known launcher. Focused on the current selection.
fn open_launcher_picker(ctx: &Ctx, app: &App, system_id: &str) {
    const DEFAULT_LAUNCHER_ID: &str = "__default__";
    let (launchers, current) = {
        let guard = lock(&ctx.shared);
        let launchers: Vec<String> = guard
            .launchers
            .iter()
            .filter(|l| l.system_id == system_id)
            .map(|l| l.id.clone())
            .collect();
        let current = guard
            .system_defaults
            .iter()
            .find(|d| d.system == system_id)
            .map(|d| d.launcher.clone())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| DEFAULT_LAUNCHER_ID.to_string());
        (launchers, current)
    };
    let mut entries = vec![menu_entry(DEFAULT_LAUNCHER_ID, "Default")];
    for id in &launchers {
        entries.push(menu_entry(id, id));
    }
    if current != DEFAULT_LAUNCHER_ID && !launchers.contains(&current) {
        entries.push(menu_entry(&current, &format!("Current: {current}")));
    }
    let initial = entries
        .iter()
        .position(|e| e.id.as_str() == current)
        .unwrap_or(0);
    lock(&ctx.shared).list_context = ListContext::SystemLauncher(system_id.to_string());
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_title(SharedString::from("Change launcher"));
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(i32::try_from(initial).unwrap_or(0));
    overlays.set_list_open(true);
}

/// Persist a launcher choice through the store mutation (which owns
/// the settings invalidation), then mirror it into the local defaults
/// so the next picker open reflects it immediately.
fn set_system_launcher(ctx: &Ctx, system_id: &str, launcher_id: &str) {
    use zaparoo_core::endpoints::system_launcher_default::{
        SetSystemLauncherDefaultArgs, SetSystemLauncherDefaultMutation,
    };
    let launcher = if launcher_id == "__default__" {
        String::new()
    } else {
        launcher_id.to_string()
    };
    let store = ctx.store.clone();
    let shared = ctx.shared.clone();
    let system_id = system_id.to_string();
    ctx.handle.spawn(async move {
        let args = SetSystemLauncherDefaultArgs {
            system_id: system_id.clone(),
            launcher: launcher.clone(),
        };
        match store
            .run_mutation::<SetSystemLauncherDefaultMutation>(args)
            .await
        {
            Ok(()) => {
                let mut guard = lock(&shared);
                if let Some(existing) = guard
                    .system_defaults
                    .iter_mut()
                    .find(|d| d.system == system_id)
                {
                    existing.launcher = launcher;
                } else {
                    guard
                        .system_defaults
                        .push(zaparoo_core::media_types::SystemDefault {
                            system: system_id,
                            launcher,
                            before_exit: String::new(),
                        });
                }
            }
            Err(e) => tracing::warn!("set launcher failed: {}", e.message),
        }
    });
}

/// Kick a scoped media-database rebuild; progress lands in the header
/// via the store's media-status resource, same as the full rebuild.
fn start_index(ctx: &Ctx, app: &App, systems: Option<Vec<String>>) {
    use zaparoo_core::media_types::MediaIndexParams;
    let client = ctx.store.client();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        if let Err(e) = client.media_generate(MediaIndexParams { systems }).await {
            let message = format!("Update failed: {}", e.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<crate::Shell>()
                    .set_status_text(SharedString::from(message.as_str()));
            });
        }
    });
}

/// Add/remove `user:favorite` through the store mutation (which owns
/// the favorites-endpoint invalidation), then flip the row's flag in
/// place on success - the same optimistic-after-ack shape as the Qt
/// `toggle_favorite_at`.
fn toggle_favorite(ctx: &Ctx, app: &App, index: usize, entry: &GameRow) {
    use slint::Model as _;
    use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;
    use zaparoo_core::media_types::MediaTagsUpdateParams;

    let adding = !entry.is_favorite;
    let mut params = MediaTagsUpdateParams::default();
    if adding {
        params.add.push("user:favorite".to_string());
    } else {
        params.remove.push("user:favorite".to_string());
    }
    // Exclusive media ref: id when Core provided one, else the
    // canonical (system, path) pair.
    if let Some(media_id) = entry.media_id {
        params.media_id = Some(media_id);
    } else if !entry.system_id.is_empty() && !entry.path.is_empty() {
        params.system.clone_from(&entry.system_id);
        params.path.clone_from(&entry.path);
    } else {
        tracing::warn!(
            "favorite update skipped: missing media identity for {}",
            entry.name
        );
        return;
    }

    let store = ctx.store.clone();
    let shared = ctx.shared.clone();
    let weak = app.as_weak();
    let path = entry.path.clone();
    let name = entry.name.clone();
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<MediaTagsUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => {
                {
                    let mut guard = lock(&shared);
                    let page = guard.games_page;
                    if let Some(row) = guard
                        .games_entries
                        .get_mut(index)
                        .filter(|row| row.path == path)
                    {
                        row.is_favorite = adding;
                    }
                    if let Some(row) = guard
                        .games_pages
                        .get_mut(page)
                        .and_then(|rows| rows.get_mut(index))
                        .filter(|row| row.path == path)
                    {
                        row.is_favorite = adding;
                    }
                }
                // The heart badge on the tile IS the feedback (the Qt
                // rule) - patch the visible row in place.
                let games = app.global::<crate::GamesView>().get_games();
                for i in 0..games.row_count() {
                    if let Some(mut tile) = games.row_data(i) {
                        if tile.path.as_str() == path {
                            tile.is_favorite = adding;
                            games.set_row_data(i, tile);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("favorite update failed for {name}: {}", e.message);
            }
        });
    });
}

/// Card write through the Qt "transient" modal flow: the menu closes,
/// the modal shows the tap prompt while Core waits for a token, and B
/// cancels (result ignored via the seq ticket). Success closes the
/// modal silently; failure swaps the title to the failure text and
/// waits for Cancel - no toasts, exactly the Qt surfaces.
fn begin_card_write(ctx: &Ctx, app: &App, entry: &GameRow) {
    use zaparoo_core::endpoints::readers_write::ReadersWriteMutation;
    use zaparoo_core::media_types::ReadersWriteParams;

    // Portable payload: zapscript when Core provided one, else path.
    let text = if entry.zap_script.trim().is_empty() {
        entry.path.clone()
    } else {
        entry.zap_script.clone()
    };
    app.global::<crate::Overlays>().set_card_write_failed(false);
    app.global::<crate::Overlays>().set_card_write_open(true);
    if text.is_empty() {
        tracing::warn!("card write for {} has no launch payload", entry.name);
        app.global::<crate::Overlays>().set_card_write_failed(true);
        return;
    }

    let ticket = {
        let mut guard = lock(&ctx.shared);
        guard.card_write_seq += 1;
        guard.card_write_seq
    };
    let store = ctx.store.clone();
    let shared = ctx.shared.clone();
    let weak = app.as_weak();
    let name = entry.name.clone();
    ctx.handle.spawn(async move {
        let result = store
            .run_mutation::<ReadersWriteMutation>(ReadersWriteParams { text })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if lock(&shared).card_write_seq != ticket {
                return;
            }
            match result {
                Ok(()) => app.global::<crate::Overlays>().set_card_write_open(false),
                Err(e) => {
                    tracing::warn!("card write failed for {name}: {}", e.message);
                    app.global::<crate::Overlays>().set_card_write_failed(true);
                }
            }
        });
    });
}

/// Game-info modal: opened with the context-menu action on a
/// games-style row. Shows the detail-tier cover immediately from the
/// row data, then fills tags/description from a one-shot `media.meta`
/// (no cache - the modal is transient, like the Qt `GameInfoModal`).
fn open_game_info(ctx: &Ctx, app: &App, entry: &GameRow) {
    app.global::<crate::GameInfoView>()
        .set_modal_name(SharedString::from(entry.name.as_str()));
    app.global::<crate::GameInfoView>()
        .set_modal_system(SharedString::from(entry.system_id.as_str()));
    app.global::<crate::GameInfoView>()
        .set_modal_path(SharedString::from(entry.path.as_str()));
    app.global::<crate::GameInfoView>()
        .set_modal_tags(SharedString::from(entry.tag_labels.join(" ").as_str()));
    app.global::<crate::GameInfoView>()
        .set_modal_description(SharedString::default());
    app.global::<crate::GameInfoView>()
        .set_modal_has_cover(false);
    app.global::<crate::GameInfoView>().set_modal_open(true);

    // Detail-tier cover through the shared cache; apply_cover in
    // main.rs patches the modal when the decode lands.
    let tier = sizing::detail_cover_source_size(output_scene(app));
    let system = if entry.system_id.is_empty() {
        lock(&ctx.shared).games_system_id.clone()
    } else {
        entry.system_id.clone()
    };
    ctx.media.enqueue(MediaKey {
        media_id: entry.media_id,
        system: system.clone(),
        path: entry.path.clone(),
        max_size: tier,
    });

    let client = ctx.store.client();
    let weak = app.as_weak();
    let params = zaparoo_core::media_types::MediaMetaParams {
        media_id: entry.media_id,
        system,
        path: entry.path.clone(),
    };
    let opened_path = entry.path.clone();
    ctx.handle.spawn(async move {
        let Ok(result) = client.media_meta(params).await else {
            return;
        };
        let description = result
            .media
            .properties
            .get("property:description")
            .map(|p| p.text.clone())
            .unwrap_or_default();
        let tags = crate::tag_utils::disambiguating_tag_labels(&result.media.tags).join(" ");
        let _ = weak.upgrade_in_event_loop(move |app| {
            // The modal may have moved on to another row (or closed)
            // while the RPC was in flight.
            if !app.global::<crate::GameInfoView>().get_modal_open()
                || app
                    .global::<crate::GameInfoView>()
                    .get_modal_path()
                    .as_str()
                    != opened_path
            {
                return;
            }
            if !description.is_empty() {
                app.global::<crate::GameInfoView>()
                    .set_modal_description(SharedString::from(description.as_str()));
            }
            if !tags.is_empty() {
                app.global::<crate::GameInfoView>()
                    .set_modal_tags(SharedString::from(tags.as_str()));
            }
        });
    });
}

/// Input while the game-info modal is open: Accept launches the shown
/// item, Cancel/context-menu closes. Directional input is swallowed.
fn modal_action(_ctx: &Ctx, app: &App, action: &str) {
    // Info-only, like the Qt GameInfoModal: B (or North again)
    // closes; launching lives in the context menu's Launch entry.
    if matches!(action, actions::CANCEL | actions::CONTEXT_MENU) {
        app.global::<crate::GameInfoView>().set_modal_open(false);
    }
}

/// Folder drill-down: push the level onto the persisted stack BEFORE
/// the browse fires, so a kill mid-load still resumes inside the
/// folder (same ordering as Main.qml's _navigateIntoFolder).
fn descend_into_folder(ctx: &Ctx, app: &App, path: &str) {
    if path.is_empty() {
        return;
    }
    let (system_id, system_name) = {
        let mut shared = lock(&ctx.shared);
        shared.persist.games.path_stack.push(path.to_string());
        shared.persist.games.selected_at_level.push(String::new());
        (
            shared.games_system_id.clone(),
            shared.games_system_name.clone(),
        )
    };
    browse_games(ctx, app, &system_id, &system_name, path, false);
}

/// Pop one folder level and re-browse the parent. Returns false when
/// already at root level (stack holds only the "" sentinel).
fn pop_folder_level(ctx: &Ctx, app: &App) -> bool {
    let (target, system_id, system_name) = {
        let mut shared = lock(&ctx.shared);
        if shared.persist.games.path_stack.len() <= 1 {
            return false;
        }
        shared.persist.games.path_stack.pop();
        shared.persist.games.selected_at_level.pop();
        (
            shared
                .persist
                .games
                .path_stack
                .last()
                .cloned()
                .unwrap_or_default(),
            shared.games_system_id.clone(),
            shared.games_system_name.clone(),
        )
    };
    browse_games(ctx, app, &system_id, &system_name, &target, false);
    true
}

/// Update action: fire `media.generate` for all systems. Progress
/// surfaces through the header's media-status line (the store's
/// `MediaStatusResource` watches Core's indexing notifications), and
/// the catalog refetches automatically on the busy -> idle edge via
/// the store's `Tag::MEDIA_DB` invalidation watcher.
fn start_media_update(ctx: &Ctx, app: &App) {
    app.global::<crate::Shell>()
        .set_status_text(SharedString::from("Updating media database…"));
    let client = ctx.store.client();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let outcome = client
            .media_generate(zaparoo_core::media_types::MediaIndexParams { systems: None })
            .await;
        let message = match outcome {
            Ok(()) => String::new(),
            Err(e) => format!("Update failed: {}", e.message),
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.global::<crate::Shell>()
                .set_status_text(SharedString::from(message.as_str()));
        });
    });
}

fn launch(ctx: &Ctx, app: &App, text: String) {
    app.global::<crate::Shell>()
        .set_status_text(SharedString::from("Launching…"));
    let store = ctx.store.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let outcome = store.run_mutation::<RunMutation>(RunParams { text }).await;
        let message = match outcome {
            Ok(()) => String::new(),
            Err(e) => format!("Launch failed: {e}"),
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.global::<crate::Shell>()
                .set_status_text(SharedString::from(message.as_str()));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::systems_for_category;
    use zaparoo_core::media_types::SystemInfo;

    fn sys(id: &str, category: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: id.into(),
            category: category.into(),
            release_date: None,
            manufacturer: None,
            media_count: None,
            zap_script: String::new(),
        }
    }

    #[test]
    fn filters_by_exact_category() {
        let all = vec![sys("nes", "Console"), sys("mame", "Arcade")];
        let picked = systems_for_category(&all, "Console");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, "nes");
    }

    #[test]
    fn empty_category_lands_in_other() {
        let all = vec![sys("weird", ""), sys("nes", "Console")];
        let picked = systems_for_category(&all, "Other");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, "weird");
    }

    fn root_entry(path: &str) -> zaparoo_core::media_types::BrowseEntry {
        zaparoo_core::media_types::BrowseEntry {
            path: path.into(),
            entry_type: "root".into(),
            name: path.rsplit('/').next().unwrap_or_default().into(),
            ..Default::default()
        }
    }

    // The real Core answers a system-scoped empty-path browse with the
    // per-system root PLUS the shared parent root (e.g. /media/fat);
    // the parent must be dropped or it browses every other system.
    #[test]
    fn ancestor_roots_are_dropped() {
        let entries = vec![
            root_entry("/media/fat/games/SNES"),
            root_entry("/media/fat"),
        ];
        let deduped = super::dedup_roots_drop_ancestors(&entries);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].path, "/media/fat/games/SNES");
    }

    #[test]
    fn sibling_roots_survive_dedup() {
        let entries = vec![
            root_entry("/media/fat/games/SNES"),
            root_entry("/media/usb0/games/SNES"),
        ];
        assert_eq!(super::dedup_roots_drop_ancestors(&entries).len(), 2);
    }

    #[test]
    fn root_entries_browse_as_directories() {
        let row = super::GameRow::from(&root_entry("/media/fat/games/SNES"));
        assert!(row.is_dir);
    }

    // A CD game's folder: directory type + media id = launches like a
    // game and fetches its cover, never browses.
    #[test]
    fn singleton_media_container_is_a_game() {
        let entry = zaparoo_core::media_types::BrowseEntry {
            path: "/media/fat/games/3DO/Gex".into(),
            entry_type: "directory".into(),
            media_id: Some(2),
            name: "Gex".into(),
            ..Default::default()
        };
        let row = super::GameRow::from(&entry);
        assert!(!row.is_dir);
        assert!(row.has_cover);
    }
}
