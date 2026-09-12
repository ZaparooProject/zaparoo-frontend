// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The router owns all forward orchestration, mirroring `Main.qml`'s
// contract: screens (the .slint views) never route; every action lands
// here, and this module decides what fills and what flips. The
// games entry uses deferred "select -> loading -> next" routing:
// source remains visible until destination data is ready, then the complete
// route commits in one turn. Only local controls and spatial paging animate.

use crate::games::GameRow;
use crate::media_cache::MediaCache;
use crate::sizing;
use crate::{App, Sizing};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::runtime::Handle;
use zaparoo_app::action_error;
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
    pub game_info: crate::game_info::GameInfoModel,
    /// The media-job setup panel's form state.
    pub setup: crate::media_setup::SetupModel,
    /// The log uploader's own panel state.
    pub log_upload: crate::log_upload::LogUploadModel,
    /// Failed user actions waiting for the alert surface.
    pub errors: action_error::ErrorQueue,
    /// The context menu's alternate-versions page.
    pub alternates: crate::alternates::AlternatesModel,
    /// The key path: duplicate guard, hold-repeat, rapid navigation.
    pub input: crate::input::InputModel,
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
    pub card_write: crate::card_write::Model,
    pub launcher_save_seq: u64,
    /// Jump-to-letter buckets for the open picker (cursor kept
    /// Rust-side; the UI only shows label + count).
    pub letter_buckets: Vec<zaparoo_core::media_types::BrowseIndexGroup>,
    /// Ticket for the picker's facet fetch; bumped on open/close so a
    /// stale index response cannot fill a reopened picker.
    pub letter_seq: u64,
    /// Ticket for the per-game launcher read, so a picker only opens
    /// for the row the user is still on.
    pub game_launcher_seq: u64,
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
pub use crate::FirstRunPhase;
use crate::{DialogButton, DialogKind, DialogProgress, ErrorKind};

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
    /// The per-game "Change launcher" picker; the payload is the game's
    /// system id and path.
    GameLauncher(String, String),
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
    /// Cooperative desktop suspension. Core lifecycle tracking keeps
    /// this true while primary media runs; background tasks retain a
    /// receiver and sleep instead of spending CPU behind the emulator.
    pub dormant: tokio::sync::watch::Sender<bool>,
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
            game_info: crate::game_info::GameInfoModel::default(),
            setup: crate::media_setup::SetupModel::new(),
            log_upload: crate::log_upload::LogUploadModel::new(),
            errors: action_error::ErrorQueue::new(),
            alternates: crate::alternates::AlternatesModel::default(),
            input: crate::input::InputModel::new(),
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
            card_write: crate::card_write::Model::default(),
            launcher_save_seq: 0,
            letter_buckets: Vec::new(),
            letter_seq: 0,
            game_launcher_seq: 0,
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
    let snapshot =
        crate::navigation::source_persist().unwrap_or_else(|| lock(shared).persist.clone());
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
    save_hidden_prefs(ctx, app, &cats, &sys);
    reproject_systems(ctx, app);
}

/// Durable hidden-browse prefs go to `frontend.toml`, not the volatile
/// state file (which lives in `/tmp` on `MiSTer`). Small atomic write.
fn save_hidden_prefs(ctx: &Ctx, app: &App, categories: &[String], system_ids: &[String]) {
    if let Err(e) =
        zaparoo_core::config::save_hidden_browse_prefs(&ctx.config_path, categories, system_ids)
    {
        tracing::warn!("could not save hidden browse prefs: {e}");
        report_action_error(ctx, app, "setting", "");
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
    fn letter_jump_uses_core_offset_not_a_count_sum() {
        use zaparoo_core::media_types::BrowseIndexGroup;
        let groups = [
            BrowseIndexGroup {
                count: 4,
                offset: 0,
                ..Default::default()
            },
            BrowseIndexGroup {
                count: 3,
                offset: 100,
                ..Default::default()
            },
        ];
        assert_eq!(super::letter_offset(&groups, 1), Some(100));
        assert_eq!(super::letter_offset(&groups, 2), None);
    }

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

/// Open a dialog. Rust names the kind, its sub-kind and the one
/// runtime value the copy needs; `DialogLabels` in the UI composes
/// every word, so the buttons are keys too.
fn open_dialog(
    app: &App,
    kind: DialogKind,
    detail: &str,
    arg: &str,
    buttons: &[DialogButton],
    focus: i32,
) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_kind(kind);
    overlays.set_dialog_error(ErrorKind::Generic);
    overlays.set_first_run_phase(FirstRunPhase::Idle);
    overlays.set_dialog_detail(SharedString::from(detail));
    overlays.set_dialog_arg(SharedString::from(arg));
    overlays.set_dialog_status(DialogProgress::None);
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(buttons.to_vec())));
    overlays.set_dialog_focus(focus);
    overlays.set_dialog_open(true);
}

/// The first-run progress line, as the key plus the numbers the copy
/// puts in it.
fn set_dialog_status(app: &App, key: DialogProgress, step: i32, total: i32, name: &str) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_status(key);
    overlays.set_dialog_status_step(step);
    overlays.set_dialog_status_total(total);
    overlays.set_dialog_status_name(SharedString::from(name));
}

fn close_dialog(app: &App) {
    crate::press_feedback::cancel(app);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_open(false);
    overlays.set_dialog_kind(DialogKind::None);
    overlays.set_dialog_detail(SharedString::default());
    overlays.set_dialog_arg(SharedString::default());
}

/// Hub Back lands here instead of quitting outright, so a stray B
/// can't kill the frontend (Main.qml's quit-confirm rule). Default
/// focus is "No".
pub(crate) fn open_quit_confirm(app: &App) {
    open_dialog(
        app,
        DialogKind::QuitConfirm,
        "",
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
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
            DialogKind::Notice,
            "",
            "",
            &[DialogButton::IUnderstand],
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
                DialogKind::CoreVersion,
                MIN_CORE_VERSION,
                &version,
                &[DialogButton::Ok],
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
            DialogKind::FirstRun,
            "",
            "",
            &[DialogButton::StartScan],
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
    let kind = overlays.get_dialog_kind();
    let len = overlays.get_dialog_buttons().row_count();
    let focus = overlays.get_dialog_focus().max(0) as usize;
    let retry = (action == actions::ACCEPT
        && kind == DialogKind::ActionError
        && overlays.get_dialog_error() == ErrorKind::CardWrite)
        .then(|| overlays.get_dialog_arg().to_string());
    let launcher_retry = (action == actions::ACCEPT
        && kind == DialogKind::ActionError
        && overlays.get_dialog_error() == ErrorKind::LauncherSave)
        .then(|| overlays.get_dialog_arg().to_string());
    match action {
        actions::LEFT if len > 1 && focus > 0 => {
            overlays.set_dialog_focus((focus - 1) as i32);
        }
        actions::RIGHT if len > 1 && focus + 1 < len => {
            overlays.set_dialog_focus((focus + 1) as i32);
        }
        actions::ACCEPT => dialog_accept(ctx, app, kind, focus),
        actions::CANCEL => dialog_cancel(ctx, app, kind),
        _ => return,
    }
    // The surface came free (this dialog closed, or the alert on it was
    // dismissed): hand it to whatever failure was waiting.
    if !app.global::<crate::Overlays>().get_dialog_open() {
        let next = if kind == DialogKind::ActionError {
            lock(&ctx.shared).errors.dismiss()
        } else {
            lock(&ctx.shared).errors.take_next()
        };
        if let Some(entry) = next {
            show_action_error(app, &entry);
        }
    }
    // Retire the old alert before retrying: an empty payload can fail
    // synchronously and must enqueue a fresh alert, not deduplicate away.
    if let Some(text) = retry {
        crate::card_write::begin(ctx, app, text);
    }
    if let Some(payload) = launcher_retry {
        crate::launchers::retry(ctx, app, &payload);
    }
}

fn dialog_accept(ctx: &Ctx, app: &App, kind: DialogKind, focus: usize) {
    let confirmed = app
        .global::<crate::Overlays>()
        .get_dialog_buttons()
        .row_data(focus)
        .is_some_and(|id| id == DialogButton::Yes);
    match kind {
        DialogKind::QuitConfirm => {
            if confirmed {
                let _ = slint::quit_event_loop();
            } else {
                close_dialog(app);
            }
        }
        DialogKind::Notice => {
            lock(&ctx.shared).notice_ack = true;
            if let Err(e) = zaparoo_core::config::save_notice_ack(&ctx.config_path, true) {
                tracing::warn!("could not persist notice ack: {e}");
            }
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::CoreVersion => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::FirstRun => first_run_accept(ctx, app),
        DialogKind::RestartSetting => {
            if confirmed {
                confirm_pending_restart(ctx, app);
            } else {
                lock(&ctx.shared).pending_restart = None;
                close_dialog(app);
            }
        }
        _ => close_dialog(app),
    }
}

fn dialog_cancel(ctx: &Ctx, app: &App, kind: DialogKind) {
    match kind {
        // The notice must be acknowledged; the idle first-run gate has
        // no skip (Qt closes it only when indexed systems appear out
        // of band).
        DialogKind::Notice => {}
        DialogKind::FirstRun => {
            let running = lock(&ctx.shared).first_run == FirstRunPhase::Running;
            if running {
                lock(&ctx.shared).first_run_cancelling = true;
                let client = ctx.store.client();
                let ctx2 = ctx.clone();
                let weak = app.as_weak();
                ctx.handle.spawn(async move {
                    if let Err(e) = client.media_generate_cancel().await {
                        tracing::warn!("first-run cancel failed: {}", e.message);
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            report_action_error(&ctx2, &app, "media_cancel", "");
                        });
                    }
                });
            }
        }
        DialogKind::CoreVersion => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::RestartSetting => {
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
            set_dialog_status(app, DialogProgress::Preparing, 0, 0, "");
            let overlays = app.global::<crate::Overlays>();
            overlays.set_first_run_phase(FirstRunPhase::Running);
            overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![DialogButton::Cancel])));
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
        || app.global::<crate::Overlays>().get_dialog_kind() != DialogKind::FirstRun
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
        if ms.optimizing {
            set_dialog_status(app, DialogProgress::Optimizing, 0, 0, "");
        } else if ms.paused {
            set_dialog_status(app, DialogProgress::Paused, 0, 0, "");
        } else if ms.total_steps > 0 {
            set_dialog_status(
                app,
                DialogProgress::Step,
                ms.current_step.max(0),
                ms.total_steps,
                &ms.current_step_display,
            );
        } else {
            set_dialog_status(app, DialogProgress::Preparing, 0, 0, "");
        }
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
        overlays.set_dialog_status(DialogProgress::None);
        overlays.set_first_run_phase(FirstRunPhase::Idle);
        overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![DialogButton::StartScan])));
        overlays.set_dialog_focus(0);
        return;
    }
    lock(&ctx.shared).first_run = FirstRunPhase::Done;
    overlays.set_dialog_status(DialogProgress::None);
    overlays.set_first_run_phase(FirstRunPhase::Done);
    overlays.set_dialog_arg(SharedString::from(
        ms.total_files.max(0).to_string().as_str(),
    ));
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![DialogButton::Ok])));
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
            || app.global::<crate::Shell>().get_dormant()
            || app.global::<crate::Shell>().get_transitioning()
        {
            return;
        }
        app.global::<crate::Shell>().set_saver_armed(true);
    });
}

/// Slow work adds feedback without hiding the source or delaying a ready route.
const LOADING_CUE_DELAY_MS: u64 = 300;

thread_local! {
    static CUE_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn begin_pending(app: &App, target: crate::Screen) {
    begin_pending_with_direction(app, target, 1);
}

pub(crate) fn begin_pending_with_direction(app: &App, target: crate::Screen, _direction: i32) {
    let shell = app.global::<crate::Shell>();
    if shell.get_transitioning() {
        return;
    }
    shell.set_transition_target(target);
    shell.set_transitioning(true);
    let ticket = CUE_SEQ.with(|sequence| {
        sequence.set(sequence.get().wrapping_add(1));
        sequence.get()
    });
    let weak = app.as_weak();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(LOADING_CUE_DELAY_MS),
        move || {
            if CUE_SEQ.with(std::cell::Cell::get) != ticket {
                return;
            }
            let Some(app) = weak.upgrade() else {
                return;
            };
            let shell = app.global::<crate::Shell>();
            if shell.get_transitioning() {
                shell.set_transition_cue(true);
            }
        },
    );
}

pub(crate) fn clear_pending(app: &App) {
    CUE_SEQ.with(|sequence| sequence.set(sequence.get().wrapping_add(1)));
    let shell = app.global::<crate::Shell>();
    shell.set_transitioning(false);
    shell.set_transition_cue(false);
}

/// Publish a ready destination in this turn, never after a decorative timer.
pub(crate) fn transition_to_screen(app: &App, target: crate::Screen, _direction: i32) {
    clear_pending(app);
    let motion = app.global::<crate::Motion>();
    motion.set_epoch(motion.get_epoch().wrapping_add(1));
    app.global::<crate::Shell>().set_active_screen(target);
    refresh_layout(app);
}

pub(crate) fn transition_settings_page(
    app: &App,
    _direction: i32,
    update: impl FnOnce(&App) + 'static,
) {
    update(app);
    let motion = app.global::<crate::Motion>();
    motion.set_epoch(motion.get_epoch().wrapping_add(1));
    refresh_layout(app);
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if crate::press_feedback::pending(app) {
        if action == actions::ACCEPT {
            return;
        }
        crate::press_feedback::cancel(app);
        if action == actions::CANCEL {
            return;
        }
    }
    if action == actions::ACCEPT {
        if let Some(target) = crate::press_feedback::prepare(ctx, app) {
            reset_idle(ctx, app);
            let ctx = ctx.clone();
            crate::press_feedback::dispatch(app, &target, move |app| {
                dispatch_with_focus(&ctx, app, actions::ACCEPT);
            });
            return;
        }
    }
    dispatch_with_focus(ctx, app, action);
}

fn focus_index(app: &App) -> i32 {
    let overlays = app.global::<crate::Overlays>();
    if overlays.get_list_open() {
        return overlays.get_list_index();
    }
    if overlays.get_context_open() {
        return overlays.get_context_index();
    }
    if overlays.get_letter_open() {
        return overlays.get_letter_index();
    }
    if overlays.get_dialog_open() {
        return overlays.get_dialog_focus();
    }
    let setup = app.global::<crate::SetupModalView>();
    if setup.get_open() {
        return if setup.get_picker_page() {
            setup.get_picker_sel()
        } else {
            setup.get_index()
        };
    }
    match app.global::<crate::Shell>().get_active_screen() {
        crate::Screen::Hub => app.global::<crate::HubView>().get_selected_local(),
        crate::Screen::Settings => app.global::<crate::SettingsView>().get_index(),
        crate::Screen::Systems | crate::Screen::FavoriteSystems => {
            app.global::<crate::SystemsView>().get_current_index()
        }
        _ => app.global::<crate::GamesView>().get_current_index(),
    }
}

fn dispatch_with_focus(ctx: &Ctx, app: &App, action: &str) {
    let before = focus_index(app);
    dispatch_action(ctx, app, action);
    let after = focus_index(app);
    // A wrap can look geometrically adjacent on a two-row/two-column grid.
    // Input direction disambiguates it; never glide backward across that wrap.
    let wrapped = match action {
        actions::UP | actions::LEFT => after > before,
        actions::DOWN | actions::RIGHT => after < before,
        actions::PAGE_PREV | actions::PAGE_NEXT => true,
        _ => false,
    };
    if wrapped {
        let motion = app.global::<crate::Motion>();
        motion.set_epoch(motion.get_epoch().wrapping_add(1));
    }
}

fn dispatch_action(ctx: &Ctx, app: &App, action: &str) {
    // Screensaver eats the waking press whole (the Qt dismiss path):
    // disarm, restart the idle clock, swallow.
    if app.global::<crate::Shell>().get_saver_armed() {
        app.global::<crate::Shell>().set_saver_armed(false);
        // A held key dismissed mid-repeat would otherwise keep ticking
        // against a screen the user cannot see.
        crate::input::stop_repeat(ctx);
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
    if app.global::<crate::LogUploadView>().get_open() {
        crate::log_upload::handle_action(ctx, app, action);
        return;
    }
    // The setup panel owns input above every screen, below the alerts
    // and the picker overlays that can sit on top of it.
    if app.global::<crate::SetupModalView>().get_open() {
        crate::media_setup::handle_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Overlays>().get_qr_open() {
        // Static display: any Cancel closes, everything else swallowed.
        if action == actions::CANCEL {
            app.global::<crate::Overlays>().set_qr_open(false);
        }
        return;
    }
    if app.global::<crate::Overlays>().get_card_write_open() {
        // Accept commits the focused Cancel button; Back cancels directly.
        if action == actions::CANCEL || action == actions::ACCEPT {
            crate::card_write::cancel(ctx, app);
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
        crate::game_info::handle_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Shell>().get_transitioning() {
        if action == actions::CANCEL {
            crate::navigation::cancel(ctx, app);
        }
        return;
    }
    // A fresh press always keeps (or restores) the live grid; only the
    // repeat path may set the flag.
    if !crate::input::rapid_page(ctx) {
        crate::input::note_rapid(ctx, app, action, false);
    }
    match app.global::<crate::Shell>().get_active_screen() {
        crate::Screen::Hub => crate::hub::handle_action(ctx, app, action),
        crate::Screen::Systems | crate::Screen::FavoriteSystems => {
            crate::systems::handle_action(ctx, app, action);
        }
        crate::Screen::Games | crate::Screen::Favorites | crate::Screen::Recents => {
            crate::games::handle_action(ctx, app, action);
        }
        crate::Screen::Settings => crate::settings::handle_action(ctx, app, action),
        crate::Screen::About => about_action(ctx, app, action),
        crate::Screen::None => {}
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

/// Current media-database state, read synchronously off the store's
/// watch resource (the Qt _indexBusy/_scrapeBusy gates).
pub(crate) fn media_state(ctx: &Ctx) -> zaparoo_core::store::MediaStatusState {
    let rx = ctx.store.media_status().subscribe();
    let state = rx.borrow().clone();
    state
}

/// About screen: enter from Settings, Back returns there. The screen
/// token persists so a kill on the About page restores to it.
pub fn enter_about(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).persist.active_screen = "about".to_string();
    save_persist(&ctx.shared);
    transition_to_screen(app, crate::Screen::About, 1);
}

fn about_action(ctx: &Ctx, app: &App, action: &str) {
    if action == actions::UP {
        app.global::<crate::AboutView>().invoke_move(-80);
    } else if action == actions::DOWN {
        app.global::<crate::AboutView>().invoke_move(80);
    } else if action == actions::CANCEL {
        // About is reached from the Support page; Back lands there.
        crate::settings::return_from_about(ctx, app);
    }
}

pub(crate) fn stage_restart(ctx: &Ctx, app: &App, pending: PendingRestart) {
    lock(&ctx.shared).pending_restart = Some(pending);
    open_dialog(
        app,
        DialogKind::RestartSetting,
        "",
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
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
            crate::settings::save(ctx, app);
            crate::request_restart();
        }
        PendingRestart::CrtStandard(value) => {
            let standard = zaparoo_core::config::normalize_crt_video_standard(&value).to_string();
            lock(&ctx.shared)
                .persist
                .settings
                .crt_video_standard
                .clone_from(&standard);
            crate::settings::save(ctx, app);
            if let Err(e) = write_crt_state_file(true, &standard) {
                tracing::warn!("{e}");
                report_action_error(ctx, app, "setting", "");
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
                report_action_error(ctx, app, "setting", "");
                return;
            }
            crate::request_main_reload();
        }
    }
}

pub(crate) fn open_crt_calibration(ctx: &Ctx, app: &App) {
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
            crate::settings::save(ctx, app);
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

/// Scoped scraper run: Core's in-tree ES gamelist.xml scraper (the
/// only shipped scraper - `scraperId` has no server-side default, so
/// the Qt model hardcodes the same id). `force` re-scrapes existing
/// metadata; the one-shot toggle resets when the run is kicked off.
pub(crate) fn start_scrape(ctx: &Ctx, app: &App, systems: Vec<String>, force: bool) {
    use zaparoo_core::media_types::MediaScrapeParams;
    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
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
            Err(e) => {
                tracing::warn!("start_scrape failed: {}", e.message);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    report_action_error(&ctx2, &app, "media_scrape", "");
                });
            }
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
    let sizing_global = app.global::<Sizing>();
    let crt = sizing_global.get_crt();
    // The sizing rules work in the logical scene the views lay out in:
    // rotated, and already trimmed to the action-safe canvas on the CRT
    // path. `MiSTer` prefers the live output size because DRS moves it
    // under the window; everywhere else the window's own logical size is
    // the scene, and reading the physical size here would scale the whole
    // layout by the display's device pixel ratio.
    let (w, h) = crate::output_size().map_or_else(
        || {
            (
                f64::from(sizing_global.get_screen_width()),
                f64::from(sizing_global.get_screen_height()),
            )
        },
        |(ow, oh)| {
            let orientation = app.global::<crate::Shell>().get_orientation();
            crate::scene_size(f64::from(ow), f64::from(oh), orientation, crt)
        },
    );
    sizing::Scene::of(app, w, h, crt)
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

/// The scene changed size (a desktop window resize, a live orientation
/// flip): re-solve every screen's geometry. The views read grid shapes,
/// cell sizes and block positions that Rust pushed for the previous
/// size, so without this the whole layout keeps the old scene's
/// proportions inside the new window.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    refresh_layout(app);
    crate::hub::rebuild(ctx, app);
    crate::systems::render(ctx, app);
    crate::games::on_layout_changed(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// Clock format or language changed: re-decide 12/24 hour and repaint.
pub(crate) fn apply_clock_setting(ctx: &Ctx, app: &App) {
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
        .set_list_setting_id(SharedString::default());
    app.global::<crate::Overlays>()
        .set_list_title(SharedString::from("title:view"));
    app.global::<crate::Overlays>()
        .set_list_entries(ModelRc::new(VecModel::from(vec![menu_row("jump_letter")])));
    app.global::<crate::Overlays>().set_list_index(0);
    app.global::<crate::Overlays>().set_list_open(true);
}

fn list_action(ctx: &Ctx, app: &App, action: &str) {
    if app.global::<crate::Overlays>().get_launcher_saving() {
        return;
    }
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
                let context = lock(&ctx.shared).list_context.clone();
                if !matches!(
                    context,
                    ListContext::SystemLauncher(_) | ListContext::GameLauncher(_, _)
                ) {
                    app.global::<crate::Overlays>().set_list_open(false);
                }
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
                        crate::settings::picker_selected(ctx, app, &field, &id);
                    }
                    ListContext::SystemLauncher(system_id) => {
                        crate::launchers::set_system_launcher(ctx, app, &system_id, &id);
                    }
                    ListContext::GameLauncher(system_id, path) => {
                        crate::launchers::set_game_launcher(ctx, app, &system_id, &path, &id);
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
                    lock(&shared).letter_buckets = result.groups;
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
    crate::press_feedback::cancel(app);
    lock(&ctx.shared).letter_seq += 1;
    app.global::<crate::Overlays>().set_letter_open(false);
}

fn letter_offset(
    groups: &[zaparoo_core::media_types::BrowseIndexGroup],
    index: usize,
) -> Option<u32> {
    groups.get(index).map(|group| group.offset)
}

fn letter_action(ctx: &Ctx, app: &App, action: &str) {
    let len = app
        .global::<crate::Overlays>()
        .get_letter_buckets()
        .row_count();
    let index = app.global::<crate::Overlays>().get_letter_index().max(0) as usize;
    let cols = app.global::<crate::Overlays>().get_letter_columns().max(1) as usize;
    match action {
        actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN => {
            app.global::<crate::Overlays>()
                .set_letter_index(
                    zaparoo_app::letter_jump::next_index(action, index, len, cols) as i32,
                );
        }
        actions::ACCEPT if len > 0 => {
            // Core supplies the authoritative browse-order position.
            // Counts need not reconstruct it (LetterJumpModal._offsetForIndex).
            let offset: u32 = {
                let guard = lock(&ctx.shared);
                let Some(offset) = letter_offset(&guard.letter_buckets, index) else {
                    return;
                };
                offset
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
/// The Frontend guide as a scannable link (the Documentation row).
pub(crate) fn open_documentation_qr(app: &App) {
    const DOCS_URL: &str = "https://zaparoo.org/docs/frontend/";
    if let Some((image, modules)) = crate::qr::qr_image(DOCS_URL) {
        app.global::<crate::Overlays>().set_qr_documentation(true);
        app.global::<crate::Overlays>().set_qr_image(image);
        app.global::<crate::Overlays>()
            .set_qr_modules(i32::try_from(modules).unwrap_or(0));
        app.global::<crate::Overlays>().set_qr_open(true);
    }
}

pub(crate) fn open_qr_code(ctx: &Ctx, app: &App, entry: &GameRow) {
    let text = if entry.zap_script.trim().is_empty() {
        entry.path.clone()
    } else {
        entry.zap_script.clone()
    };
    if text.is_empty() {
        tracing::warn!("QR code for {} has no launch payload", entry.name);
        report_action_error(ctx, app, "qr_code", "");
        return;
    }
    let Some((image, modules)) = crate::qr::qr_image(&crate::qr::write_url(&text)) else {
        tracing::warn!("QR code for {} could not be rendered", entry.name);
        report_action_error(ctx, app, "qr_code", "");
        return;
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_qr_documentation(false);
    overlays.set_qr_image(image);
    overlays.set_qr_modules(i32::try_from(modules).unwrap_or(0));
    overlays.set_qr_open(true);
}

/// A menu row whose text is literal (a launcher id, a system name).
pub(crate) fn menu_entry(id: &str, label: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        id: SharedString::from(id),
        label: SharedString::from(label),
        label_key: SharedString::default(),
    }
}

/// A menu row whose text comes from the `Labels.menu` vocabulary. The
/// key is the row id unless the copy depends on state (a favorite that
/// is already set, a hub item that hides rather than removes), and
/// `name` fills the one placeholder a row can carry.
pub(crate) fn menu_row(id: &str) -> crate::MenuEntry {
    menu_row_keyed(id, id, "")
}

pub(crate) fn menu_row_keyed(id: &str, key: &str, name: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        id: SharedString::from(id),
        label: SharedString::from(name),
        label_key: SharedString::from(key),
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

pub(crate) fn scrape_category(ctx: &Ctx, app: &App, category: &str) {
    let ids = indexable_system_ids(ctx, category);
    if !ids.is_empty() {
        start_scrape(ctx, app, ids, false);
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
    overlays.set_list_setting_id(SharedString::default());
    overlays.set_list_title(SharedString::from(title));
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(0);
    overlays.set_list_open(true);
}

pub(crate) fn present_hub_page_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubPageMenu, "title:view", entries);
}

pub(crate) fn present_hub_add_picker(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubAdd, "title:add_item", entries);
}

/// The vocabulary key for the current grouping, so the View menu row
/// can name it in the reader's language.
fn favorites_grouping_label(ctx: &Ctx) -> &'static str {
    if lock(&ctx.shared).persist.settings.favorites_grouping == "system" {
        "group:system"
    } else {
        "group:none"
    }
}

fn favorites_sort_label(ctx: &Ctx) -> &'static str {
    if lock(&ctx.shared).favorites_sort == "name" {
        "sort:name"
    } else {
        "sort:default"
    }
}

/// The favorites list's West "View" menu: order, grouping, a random
/// favorite, and the way back to the Hub.
pub(crate) fn open_favorites_page_menu(ctx: &Ctx, app: &App) {
    let entries = vec![
        menu_row_keyed(
            "favorites_sort",
            "favorites_sort",
            favorites_sort_label(ctx),
        ),
        menu_row_keyed(
            "favorites_grouping",
            "favorites_grouping",
            favorites_grouping_label(ctx),
        ),
        menu_row("launch_random_favorite"),
        menu_row("back_to_hub"),
    ];
    present_list(
        ctx,
        app,
        ListContext::FavoritesPageMenu,
        "title:view",
        entries,
    );
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
                "",
            );
        }
        "back_to_hub" => {
            lock(&ctx.shared).persist.active_screen = "hub".to_string();
            save_persist(&ctx.shared);
            transition_to_screen(app, crate::Screen::Hub, -1);
        }
        _ => {}
    }
}

/// The grouping page, reachable from either favorites screen.
pub(crate) fn open_favorites_grouping_menu(ctx: &Ctx, app: &App) {
    let entries = vec![
        menu_row_keyed("none", "group:none", ""),
        menu_row_keyed("system", "group:system", ""),
    ];
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
        "title:group_by",
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
    crate::settings::save(ctx, app);
    if id == "system" {
        crate::systems::enter_favorites(ctx, app);
    } else {
        crate::games::enter_favorites(ctx, app);
    }
}

fn open_favorites_sort_menu(ctx: &Ctx, app: &App) {
    let entries = vec![
        menu_row_keyed("default", "sort:default", ""),
        menu_row_keyed("name", "sort:name", ""),
    ];
    let index = usize::from(lock(&ctx.shared).favorites_sort == "name");
    present_list(ctx, app, ListContext::FavoritesSort, "title:sort", entries);
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
        report_action_error(ctx, app, "setting", "");
    }
    crate::games::refresh_favorites(ctx, app);
}

/// A one-button informational alert above the current screen (the
/// `action_error` surface, with its own copy kind).
pub(crate) fn open_alert(app: &App, kind: DialogKind) {
    open_dialog(app, kind, "", "", &[DialogButton::Ok], 0);
}

/// Report a failed user action. The technical detail is already in the
/// log at the call site; this is the user-facing half, deduplicated
/// and queued by `zaparoo_app::action_error` so a burst of failures is
/// read one alert at a time.
pub(crate) fn report_action_error(ctx: &Ctx, app: &App, kind: &str, context: &str) {
    // An alert is the one thing allowed above a modal, but a failed
    // discovery arrives while the context menu still holds its
    // "Searching…" row: close the menu first so Back returns to the
    // screen rather than to a row that can never resolve.
    if action_error::closes_context_menu(kind) && app.global::<crate::Overlays>().get_context_open()
    {
        close_context_menu(ctx, app);
    }
    let slot_free = !app.global::<crate::Overlays>().get_dialog_open();
    let entry = lock(&ctx.shared).errors.present(kind, context, slot_free);
    if let Some(entry) = entry {
        show_action_error(app, &entry);
    }
}

fn show_action_error(app: &App, entry: &action_error::Entry) {
    let error = ErrorKind::try_from(entry.kind.as_str()).unwrap_or(ErrorKind::Generic);
    let button = if matches!(error, ErrorKind::CardWrite | ErrorKind::LauncherSave) {
        DialogButton::Retry
    } else {
        DialogButton::Ok
    };
    open_dialog(
        app,
        DialogKind::ActionError,
        "",
        &entry.context,
        &[button],
        0,
    );
    app.global::<crate::Overlays>().set_dialog_error(error);
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
    crate::press_feedback::cancel(app);
    // Bumping the seq abandons any in-flight card write (its result
    // is ignored on arrival - the Qt cancel rule), and any discovery
    // still looking for a menu to fill.
    {
        let mut shared = lock(&ctx.shared);
        shared.card_write.cancel();
        shared.alternates.seq += 1;
        shared.alternates.showing = false;
        shared.alternates.rows.clear();
    }
    app.global::<crate::Overlays>().set_context_open(false);
}

/// Pointer input on the context menu's rows: hover moves focus, a click
/// accepts (ContextMenu.qml's own per-row mouse areas).
pub fn bind_context_input(ctx: &Arc<Ctx>, app: &App) {
    crate::about::bind(ctx, app);
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        app.global::<crate::Overlays>()
            .on_pointer_choice(move |kind, index, accept| {
                let Some(app) = weak.upgrade() else { return };
                if index < 0
                    || crate::press_feedback::pending(&app)
                    || !lock(&ctx.shared).persist.settings.mouse_enabled
                {
                    return;
                }
                if kind == crate::PressOwner::Wake {
                    if app.global::<crate::Shell>().get_saver_armed() {
                        app.global::<crate::Shell>().set_saver_armed(false);
                        reset_idle(&ctx, &app);
                    }
                    return;
                }
                let ov = app.global::<crate::Overlays>();
                let row = index as usize;
                match kind {
                    crate::PressOwner::CardWrite
                        if !ov.get_dialog_open() && ov.get_card_write_open() && row == 0 => {}
                    crate::PressOwner::Dialog
                        if ov.get_dialog_open() && row < ov.get_dialog_buttons().row_count() =>
                    {
                        ov.set_dialog_focus(index);
                    }
                    crate::PressOwner::List
                        if !ov.get_dialog_open()
                            && ov.get_list_open()
                            && !ov.get_launcher_saving()
                            && row < ov.get_list_entries().row_count() =>
                    {
                        ov.set_list_index(index);
                    }
                    crate::PressOwner::Letter
                        if !ov.get_dialog_open()
                            && ov.get_letter_open()
                            && row < ov.get_letter_buckets().row_count() =>
                    {
                        ov.set_letter_index(index);
                    }
                    _ => return,
                }
                reset_idle(&ctx, &app);
                if accept {
                    handle_action(&ctx, &app, actions::ACCEPT);
                }
            });
    }
    let input = app.global::<crate::ContextInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_row_hovered(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if crate::press_feedback::pending(&app)
                || !lock(&ctx.shared).persist.settings.mouse_enabled
            {
                return;
            }
            app.global::<crate::Overlays>().set_context_index(index);
        });
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    input.on_row_clicked(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if crate::press_feedback::pending(&app) || !lock(&ctx.shared).persist.settings.mouse_enabled
        {
            return;
        }
        app.global::<crate::Overlays>().set_context_index(index);
        handle_action(&ctx, &app, actions::ACCEPT);
    });
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
        actions::CANCEL | actions::CONTEXT_MENU => {
            // The alternates page is a page of this menu, not a menu of
            // its own: Back returns to the rows it replaced.
            if crate::alternates::showing(ctx) {
                crate::alternates::leave(ctx, app);
            } else {
                close_context_menu(ctx, app);
            }
        }
        _ => {}
    }
}

fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    if crate::alternates::showing(ctx) {
        crate::alternates::accept(ctx, app, id);
        return;
    }
    let owner = lock(&ctx.shared).context_owner;
    match owner {
        ContextOwner::Games => {
            // Discovery keeps the menu open: its rows become the
            // alternates once Core answers.
            if id == "discover" {
                crate::games::begin_discovery(ctx, app);
                return;
            }
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

/// Kick a scoped media-database rebuild; progress lands in the header
/// via the store's media-status resource, same as the full rebuild.
pub(crate) fn start_index(ctx: &Ctx, app: &App, systems: Option<Vec<String>>) {
    use zaparoo_core::media_types::MediaIndexParams;
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        if let Err(e) = client.media_generate(MediaIndexParams { systems }).await {
            tracing::warn!("start_index failed: {}", e.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                report_action_error(&ctx2, &app, "media_index", "");
            });
        }
    });
}

/// Snapshot the portable command before handing ownership to the transient.
pub(crate) fn begin_card_write(ctx: &Ctx, app: &App, entry: &GameRow) {
    let text = if entry.zap_script.trim().is_empty() {
        entry.path.clone()
    } else {
        entry.zap_script.clone()
    };
    crate::card_write::begin(ctx, app, text);
}

/// Details remains a router-owned modal; its driver owns transient data.
pub(crate) fn open_game_info(ctx: &Ctx, app: &App, entry: &GameRow) {
    crate::game_info::open(ctx, app, entry);
}

/// Update action: fire `media.generate` for all systems. Progress
/// surfaces through the header's media-status line (the store's
/// `MediaStatusResource` watches Core's indexing notifications), and
/// the catalog refetches automatically on the busy -> idle edge via
/// the store's `Tag::MEDIA_DB` invalidation watcher.
pub(crate) fn launch(ctx: &Ctx, app: &App, text: String, name: &str) {
    app.global::<crate::Shell>()
        .set_status_text(crate::AppCue::Launching);
    let store = ctx.store.clone();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let name = name.to_string();
    ctx.handle.spawn(async move {
        let failed = match store.run_mutation::<RunMutation>(RunParams { text }).await {
            Ok(()) => false,
            Err(e) => {
                tracing::warn!("launch failed for {name}: {e}");
                true
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.global::<crate::Shell>()
                .set_status_text(crate::AppCue::None);
            if failed {
                report_action_error(&ctx2, &app, "launch", &name);
            }
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
