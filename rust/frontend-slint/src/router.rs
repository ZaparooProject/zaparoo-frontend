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

use crate::games::GameRow;
use crate::hub_nav;
use crate::media_cache::{MediaCache, MediaKey};
use crate::sizing;
use crate::{App, Sizing};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::runtime::Handle;
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{RunParams, SystemInfo};
use zaparoo_core::persist::{self, PersistedState};
use zaparoo_core::store::Store;

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
    /// Favorites list order: "name" for A-Z, empty for Core's default.
    /// Durable in `frontend.toml`, like the hidden-browse prefs.
    pub favorites_sort: String,
    /// Full sorted systems list from the catalog.
    pub systems: Vec<SystemInfo>,
    /// The Systems screen: rows, cursor and swoop state.
    pub systems_model: crate::systems::SystemsModel,
    /// The games-style screens: rows, cursor, fetch and cue state.
    pub games: crate::games::GamesModel,
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
    /// True until the first catalog Ready has restored the persisted
    /// screen after a cold start.
    pub restore_pending: bool,
    /// Last-played entry backing the Hub's Resume action; None until
    /// `media.history.latest` answers (or when history is empty).
    /// The Hub: persisted layout, entries, cursor and Move session.
    pub hub: crate::hub::HubModel,
}

/// Which surface an open context menu was invoked on. Favorites and
/// Recents share the games grid, so `Games` covers all three
/// games-style modes (`crate::games::GamesMode` disambiguates the entry set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextOwner {
    Games,
    Systems,
    /// A Hub tile's Options menu (`crate::hub`).
    Hub,
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
    /// The Hub's West "View" menu (Add item, Reset layout, Settings, Quit).
    HubPageMenu,
    /// The Hub's "Add item" picker.
    HubAdd,
    /// The favorites list's West "View" menu.
    FavoritesPageMenu,
    /// Its "Group by" and "Sort" pages.
    FavoritesGrouping,
    FavoritesSort,
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
    /// Header status line ladder state.
    pub status: crate::status::Shared,
    /// `frontend.toml` location, for durable settings mirrors.
    pub config_path: std::path::PathBuf,
    /// Immutable process mode; changing it requires Main to respawn us.
    pub crt_enabled: bool,
    pub is_mister: bool,
    /// Physical framebuffer geometry, used to resize desktop previews
    /// and by the `MiSTer` renderer when orientation changes live.
    pub framebuffer_size: (u32, u32),
}

impl Shared {
    /// Fresh router state around the persisted snapshot; everything
    /// else starts empty and fills from Core.
    pub fn new(
        persist: PersistedState,
        restore_pending: bool,
        hidden_categories: Vec<String>,
        hidden_system_ids: Vec<String>,
        favorites_sort: String,
        hub_layout_path: std::path::PathBuf,
    ) -> Self {
        let show_hidden = persist.settings.show_hidden;
        Self {
            favorites_sort,
            categories: Vec::new(),
            all_categories: Vec::new(),
            hidden_categories,
            hidden_system_ids,
            show_hidden,
            systems: Vec::new(),
            systems_model: crate::systems::SystemsModel::new(),
            games: crate::games::GamesModel::new(),
            has_readers: false,
            has_nfc: false,
            context_owner: ContextOwner::Games,
            context_target: 0,
            list_context: ListContext::ViewMenu,
            pending_restart: None,
            rescrape_existing: false,
            saver_seq: 0,
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
            card_write_seq: 0,
            letter_buckets: Vec::new(),
            letter_seq: 0,
            persist,
            restore_pending,
            hub: crate::hub::HubModel::new(hub_layout_path),
        }
    }
}

pub fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
    // A poisoned mutex means a panicking thread died mid-update; the
    // data is still structurally valid, so keep going.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn save_persist(shared: &Arc<Mutex<Shared>>) {
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

/// Rebuild the visible category list from the master list and the
/// hidden prefs, then re-resolve the Hub's tiles.
pub fn reproject_hub(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let visible: Vec<String> = shared
            .all_categories
            .iter()
            .filter(|name| shared.show_hidden || !shared.hidden_categories.contains(name))
            .cloned()
            .collect();
        shared.categories = visible;
    }
    crate::hub::rebuild(ctx, app);
}

/// Re-run the Systems screen's projection after a hide/unhide, a
/// Show-hidden flip or a region change.
pub fn reproject_systems(ctx: &Ctx, app: &App) {
    crate::systems::reproject(ctx, app);
}

/// Flip a system's hidden flag, persist, and reproject the grid.
pub(crate) fn toggle_hidden_system(ctx: &Ctx, app: &App, id: &str) {
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
pub(crate) fn open_quit_confirm(app: &App) {
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
    refresh_layout(app);
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
        refresh_layout(app);
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
pub(crate) fn transition_to_screen(app: &App, target: &str, direction: i32) {
    let shell = app.global::<crate::Shell>();
    let current = shell.get_active_screen();
    if current.as_str() == target {
        shell.set_transitioning(false);
        return;
    }
    if shell.get_reduce_motion() {
        shell.set_transitioning(false);
        shell.set_active_screen(SharedString::from(target));
        refresh_layout(app);
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
        "hub" => crate::hub::handle_action(ctx, app, action),
        "systems" | "favorite-systems" => crate::systems::handle_action(ctx, app, action),
        // Favorites and Recents reuse the games-style grid; the mode
        // stored in Shared adjusts back/paging/persist behavior.
        "games" | "favorites" | "recents" => crate::games::handle_action(ctx, app, action),
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
        "showOriginalFilenames" => crate::games::reproject(ctx, app),
        "reduceMotion" => {
            app.global::<crate::Shell>().set_reduce_motion(value);
            app.global::<crate::Motion>().set_enabled(!value);
        }
        "region" => {
            crate::systems::reproject(ctx, app);
            reproject_hub(ctx, app);
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
            "language" => {
                s.language = value.to_string();
                crate::apply_language(value);
                crate::status::set_language(&ctx.status, &crate::effective_language(value));
                apply_clock(ctx, app);
            }
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
        "clockFormat" => apply_clock(ctx, app),
        "buttonLayout" => crate::apply_buttons(ctx, app),
        "systemLogoStyle" => reproject_systems(ctx, app),
        "mediaImageType" => ctx.media.set_preferred_image_type(value),
        // Restart the idle clock so the new timeout takes effect now,
        // not after the old countdown fires into the seq guard.
        "screensaverTimeout" => reset_idle(ctx, app),
        "browseLayout" => {
            app.global::<crate::Shell>()
                .set_browse_list_layout(value == "list");
            refresh_layout(app);
            // The cursor carries into the other presentation; the state
            // must be coherent the moment the user backs out of Settings.
            crate::games::on_layout_changed(ctx, app);
        }
        _ => {}
    }
    refresh_settings_fields(ctx, app);
}

/// Scoped scraper run: Core's in-tree ES gamelist.xml scraper (the
/// only shipped scraper - `scraperId` has no server-side default, so
/// the Qt model hardcodes the same id). `force` re-scrapes existing
/// metadata; the one-shot toggle resets when the run is kicked off.
pub(crate) fn start_scrape(ctx: &Ctx, systems: Vec<String>, force: bool) {
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

/// Shared Ready-side fill for the games-style grid: store the rows,
/// persist the screen token, flip (or clear the light cue), show
/// page 0. Runs on the event loop.
#[allow(
    clippy::too_many_arguments,
    reason = "internal fill plumbing shared by three list sources; a params struct would just re-name these"
)]
/// The scene at the current output geometry, as the sizing rules see it:
/// the physical output size during DRS rather than the transient Slint
/// window size, plus the rendering flags the `Sizing` global carries.
pub(crate) fn output_scene(app: &App) -> sizing::Scene {
    let size = app.window().size();
    let (out_w, out_h) = crate::output_size()
        .map_or((f64::from(size.width), f64::from(size.height)), |(w, h)| {
            (f64::from(w), f64::from(h))
        });
    let crt = app.global::<Sizing>().get_crt();
    sizing::Scene::of(app, out_w, out_h, crt)
}

/// Re-resolve the browse layout profile for the screen now active, at
/// the logical scene geometry (the same one `App` feeds Sizing).
pub(crate) fn refresh_layout(app: &App) {
    let sizing_global = app.global::<Sizing>();
    let (w, h) = (
        f64::from(sizing_global.get_screen_width()),
        f64::from(sizing_global.get_screen_height()),
    );
    let crt = sizing_global.get_crt();
    sizing::refresh_layout(app, sizing::Scene::of(app, w, h, crt));
}

/// Clock format or language changed: re-decide 12/24 hour and repaint.
fn apply_clock(ctx: &Ctx, app: &App) {
    let twelve = {
        let guard = lock(&ctx.shared);
        crate::clock_twelve_hour(&guard.persist.settings)
    };
    ctx.clock_twelve_hour
        .store(twelve, std::sync::atomic::Ordering::Relaxed);
    crate::push_clock(app, twelve);
}

/// Open the West "View" menu (the page/list-scoped operations menu,
/// counterpart to North's item-scoped Options). One entry today -
/// Go to..., pre-focused so the common path is a fixed West-then-
/// Accept chord. The letter facet fetch is kicked off here so the
/// buckets are likely ready by the time the user advances into the
/// grid (the Qt openPageMenu flow).
pub(crate) fn open_view_menu(ctx: &Ctx, app: &App) {
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
                    ListContext::HubPageMenu => crate::hub::page_menu_accept(ctx, app, &id),
                    ListContext::HubAdd => crate::hub::add_picked(ctx, app, &id),
                    ListContext::FavoritesPageMenu => favorites_page_menu_accept(ctx, app, &id),
                    ListContext::FavoritesGrouping => favorites_grouping_picked(ctx, app, &id),
                    ListContext::FavoritesSort => favorites_sort_picked(ctx, app, &id),
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
            guard.games.browse_path.clone(),
            guard.games.system_id.clone(),
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
            crate::games::jump_to_item(ctx, app, offset);
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
/// Refresh the connected-readers flag from Core (gates the "Write to
/// NFC token" entry). Lazy: the flag read at menu-open time may be one
/// refresh old.
pub(crate) fn refresh_readers(ctx: &Ctx) {
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
pub(crate) fn open_qr_code(app: &App, entry: &GameRow) {
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

pub(crate) fn menu_entry(id: &str, label: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        id: SharedString::from(id),
        label: SharedString::from(label),
        label_key: SharedString::default(),
    }
}

/// Hub helpers the driver in `crate::hub` calls back into.
pub(crate) fn category_has_indexable(ctx: &Ctx, category: &str) -> bool {
    let guard = lock(&ctx.shared);
    systems_for_category(&guard.systems, category)
        .iter()
        .any(|s| s.zap_script.trim().is_empty())
}

fn indexable_system_ids(ctx: &Ctx, category: &str) -> Vec<String> {
    let guard = lock(&ctx.shared);
    systems_for_category(&guard.systems, category)
        .iter()
        .filter(|s| s.zap_script.trim().is_empty())
        .map(|s| s.id.clone())
        .collect()
}

pub(crate) fn index_category(ctx: &Ctx, app: &App, category: &str) {
    let ids = indexable_system_ids(ctx, category);
    if !ids.is_empty() {
        start_index(ctx, app, Some(ids));
    }
}

pub(crate) fn scrape_category(ctx: &Ctx, category: &str) {
    let ids = indexable_system_ids(ctx, category);
    if !ids.is_empty() {
        start_scrape(ctx, ids, false);
    }
}

pub(crate) fn present_hub_context_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_context_menu(ctx, app, ContextOwner::Hub, 0, entries);
}

pub(crate) fn present_systems_context_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_context_menu(ctx, app, ContextOwner::Systems, 0, entries);
}

/// The scene rect of the row or tile a context menu is about; the menu
/// panel and its scrim hole follow it.
pub(crate) fn set_context_anchor(app: &App, x: f32, y: f32, w: f32, h: f32) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_anchor_x(x);
    overlays.set_context_anchor_y(y);
    overlays.set_context_anchor_w(w);
    overlays.set_context_anchor_h(h);
}

pub(crate) fn present_games_context_menu(
    ctx: &Ctx,
    app: &App,
    target: usize,
    entries: Vec<crate::MenuEntry>,
) {
    present_context_menu(ctx, app, ContextOwner::Games, target, entries);
}

fn present_list(
    ctx: &Ctx,
    app: &App,
    context: ListContext,
    title: &str,
    entries: Vec<crate::MenuEntry>,
) {
    lock(&ctx.shared).list_context = context;
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_title(SharedString::from(title));
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(0);
    overlays.set_list_open(true);
}

pub(crate) fn present_hub_page_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubPageMenu, "View", entries);
}

pub(crate) fn present_hub_add_picker(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubAdd, "Add item", entries);
}

fn favorites_grouping_label(ctx: &Ctx) -> &'static str {
    if lock(&ctx.shared).persist.settings.favorites_grouping == "system" {
        "System"
    } else {
        "None"
    }
}

fn favorites_sort_label(ctx: &Ctx) -> &'static str {
    if lock(&ctx.shared).favorites_sort == "name" {
        "A-Z"
    } else {
        "Default"
    }
}

/// The favorites list's West "View" menu: order, grouping, a random
/// favorite, and the way back to the Hub.
pub(crate) fn open_favorites_page_menu(ctx: &Ctx, app: &App) {
    let entries = vec![
        menu_entry(
            "favorites_sort",
            &format!("Sort: {}", favorites_sort_label(ctx)),
        ),
        menu_entry(
            "favorites_grouping",
            &format!("Group by: {}", favorites_grouping_label(ctx)),
        ),
        menu_entry("launch_random_favorite", "Random favorite"),
        menu_entry("back_to_hub", "Back to Hub"),
    ];
    present_list(ctx, app, ListContext::FavoritesPageMenu, "View", entries);
}

fn favorites_page_menu_accept(ctx: &Ctx, app: &App, id: &str) {
    match id {
        "favorites_sort" => open_favorites_sort_menu(ctx, app),
        "favorites_grouping" => open_favorites_grouping_menu(ctx, app),
        "launch_random_favorite" => {
            let scope = lock(&ctx.shared).games.favorites_system.clone();
            launch(
                ctx,
                app,
                zaparoo_app::systems::random_favorite_launch_text(&scope),
            );
        }
        "back_to_hub" => {
            lock(&ctx.shared).persist.active_screen = "hub".to_string();
            save_persist(&ctx.shared);
            transition_to_screen(app, "hub", -1);
        }
        _ => {}
    }
}

/// The grouping page, reachable from either favorites screen.
pub(crate) fn open_favorites_grouping_menu(ctx: &Ctx, app: &App) {
    let entries = vec![menu_entry("none", "None"), menu_entry("system", "System")];
    let current = lock(&ctx.shared)
        .persist
        .settings
        .favorites_grouping
        .clone();
    let index = usize::from(current == "system");
    present_list(
        ctx,
        app,
        ListContext::FavoritesGrouping,
        "Group by",
        entries,
    );
    app.global::<crate::Overlays>()
        .set_list_index(i32::try_from(index).unwrap_or(0));
}

/// Switching the grouping re-enters the favorites at the new level.
fn favorites_grouping_picked(ctx: &Ctx, app: &App, id: &str) {
    if !matches!(id, "none" | "system") {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        if shared.persist.settings.favorites_grouping == id {
            return;
        }
        shared.persist.settings.favorites_grouping = id.to_string();
    }
    save_settings(ctx);
    if id == "system" {
        crate::systems::enter_favorites(ctx, app);
    } else {
        crate::games::enter_favorites(ctx, app);
    }
}

fn open_favorites_sort_menu(ctx: &Ctx, app: &App) {
    let entries = vec![menu_entry("default", "Default"), menu_entry("name", "A-Z")];
    let index = usize::from(lock(&ctx.shared).favorites_sort == "name");
    present_list(ctx, app, ListContext::FavoritesSort, "Sort", entries);
    app.global::<crate::Overlays>()
        .set_list_index(i32::try_from(index).unwrap_or(0));
}

/// The order is a durable preference (`frontend.toml`), not screen state.
fn favorites_sort_picked(ctx: &Ctx, app: &App, id: &str) {
    let sort = if id == "name" { "name" } else { "" };
    if lock(&ctx.shared).favorites_sort == sort {
        return;
    }
    lock(&ctx.shared).favorites_sort = sort.to_string();
    if let Err(e) = zaparoo_core::config::save_favorites_sort(&ctx.config_path, sort) {
        tracing::warn!("saving the favorites sort failed: {e}");
    }
    crate::games::refresh_favorites(ctx, app);
}

/// A one-button alert above the current screen (Modal.qml's
/// `action_error` kind).
pub(crate) fn open_action_error(app: &App, title: &str, body: &str) {
    open_dialog(app, "action_error", title, body, &["OK"], 0);
}

/// Accept on a category tile while the catalog errored: refetch it.
pub(crate) fn retry_catalog(ctx: &Ctx) {
    ctx.store
        .subscribe::<zaparoo_core::endpoints::catalog::CatalogEndpoint>(())
        .refetch();
}

/// Category tile menu (Hub top row): hide/unhide plus a scoped media
/// database rebuild when the category has indexable systems and no
/// media job is running.
/// A media-database job is running; index/scrape entries drop out
/// while it does (the Qt mediaBusy gate, read off the same status
/// line the header shows).
pub(crate) fn media_busy(app: &App) -> bool {
    app.global::<crate::Status>().get_show_track()
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
        ContextOwner::Games => {
            close_context_menu(ctx, app);
            crate::games::context_accept(ctx, app, id);
        }
        ContextOwner::Systems => {
            close_context_menu(ctx, app);
            crate::systems::context_accept(ctx, app, id);
        }
        ContextOwner::Hub => {
            close_context_menu(ctx, app);
            crate::hub::context_accept(ctx, app, id);
        }
    }
}

/// "Change launcher" picker (the `SystemLaunchers` model's
/// `picker_entries_for_system`): Default first, the system's launchers
/// by id, plus a "Current: x" row when the stored default no longer
/// matches a known launcher. Focused on the current selection.
pub(crate) fn open_launcher_picker(ctx: &Ctx, app: &App, system_id: &str) {
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
pub(crate) fn start_index(ctx: &Ctx, app: &App, systems: Option<Vec<String>>) {
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

/// Card write through the Qt "transient" modal flow: the menu closes,
/// the modal shows the tap prompt while Core waits for a token, and B
/// cancels (result ignored via the seq ticket). Success closes the
/// modal silently; failure swaps the title to the failure text and
/// waits for Cancel - no toasts, exactly the Qt surfaces.
pub(crate) fn begin_card_write(ctx: &Ctx, app: &App, entry: &GameRow) {
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
pub(crate) fn open_game_info(ctx: &Ctx, app: &App, entry: &GameRow) {
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
        lock(&ctx.shared).games.system_id.clone()
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

/// Update action: fire `media.generate` for all systems. Progress
/// surfaces through the header's media-status line (the store's
/// `MediaStatusResource` watches Core's indexing notifications), and
/// the catalog refetches automatically on the busy -> idle edge via
/// the store's `Tag::MEDIA_DB` invalidation watcher.
pub(crate) fn launch(ctx: &Ctx, app: &App, text: String) {
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
}
