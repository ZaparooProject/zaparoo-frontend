// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The router owns all forward orchestration under one contract: screens
// (the .slint views) never route; every action lands here, and this module
// decides what fills and what flips. The games entry uses deferred
// "select -> loading -> next" routing: source remains visible until
// destination data is ready, then the complete route commits in one turn.
// Only local controls and spatial paging animate.

use crate::games::GameRow;
use crate::media_cache::MediaCache;
use crate::navigation::EntryMode;
use crate::sizing;
use crate::{App, Sizing};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::runtime::Handle;
use zaparoo_app::action_error;
use zaparoo_app::options_menu::Menu as OptionsMenu;
use zaparoo_app::user_flags::{Flag, Flags};
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
    /// Favorites list order: "name" for A-Z, "added" for newest first,
    /// empty for Core's default.
    /// Durable in `frontend.toml`, like the hidden-browse prefs.
    pub favorites_sort: String,
    /// Full sorted systems list from the catalog.
    pub systems: Vec<SystemInfo>,
    /// The Systems screen: rows, cursor and swoop state.
    pub systems_model: crate::systems::SystemsModel,
    /// The games-style screens: rows, cursor, fetch and cue state.
    pub games: crate::games::GamesModel,
    pub game_info: crate::game_info::GameInfoModel,
    /// Core's current UI event and what of it is on screen.
    pub core_ui: crate::core_ui::Model,
    /// The media-job setup panel's form state.
    pub setup: crate::media_setup::SetupModel,
    /// The log uploader's own panel state.
    pub log_upload: crate::log_upload::LogUploadModel,
    /// The "Pair a device" panel: its phase, the PIN on screen and what
    /// leaving still owes Core.
    pub pairing: zaparoo_app::pairing::Session,
    /// Online account and play history consent, as Core last reported them.
    pub online: crate::online::Model,
    /// The Online account link panel.
    pub online_link: zaparoo_app::online_link::Session,
    pub online_list: crate::online_list::State,
    /// Failed user actions waiting for the alert surface.
    pub errors: action_error::ErrorQueue,
    /// The page the context menu is showing in place of its own rows.
    pub context_page: crate::context_page::PageModel,
    /// The key path: duplicate guard, hold-repeat, rapid navigation.
    pub input: crate::input::InputModel,
    /// Core reports at least one connected reader. Refreshed lazily.
    pub has_readers: bool,
    /// An NFC-class reader is present; gates the context-menu "Write
    /// to NFC token" entry (`has_readers` alone would count non-NFC
    /// readers too).
    pub has_nfc: bool,
    /// Which surface the open context menu targets, with the target's
    /// projected index on that surface.
    pub context_owner: ContextOwner,
    pub context_target: usize,
    /// Collection toggles still waiting on Core, per game.
    pub flag_requests: zaparoo_app::user_flags::Requests,
    /// The open menu's rows and the pages they open, kept so Back from a
    /// page returns to the same rows.
    pub context_menu: zaparoo_app::options_menu::Menu,
    /// Where the menu was when one of its rows opened something in its
    /// place, so Back there reopens it (`return_to_menu`).
    pub menu_return: Option<MenuReturn>,
    /// Bumped each time a context menu opens or closes, so a collection
    /// change Core answers late lands only on the menu it was made in.
    pub context_seq: u64,
    /// The strip button focus returns to when Up leaves the rows.
    pub context_strip_at: usize,
    /// What the open list panel is for (the Menu list vs a settings
    /// picker field).
    pub list_context: ListContext,
    /// Restart-applied Display setting waiting behind the confirmation
    /// dialog. CRT toggles/standards hand control back to Main; HDMI
    /// resolution changes self-exec after clean presenter shutdown.
    pub pending_restart: Option<PendingRestart>,
    /// Core's launcher inventory + per-system defaults, for the
    /// "Change launcher" picker (`SystemLaunchers` model port).
    pub launchers: Vec<zaparoo_core::media_types::LauncherInfo>,
    pub system_defaults: Vec<zaparoo_core::media_types::SystemDefault>,
    /// Core's global launcher preference with any change of ours still on
    /// its way, and the list Core last accepted. `None` before Core has
    /// answered, or when it predates the setting.
    pub launcher_preference: Option<Vec<String>>,
    pub launcher_pref_confirmed: Option<Vec<String>>,
    /// The launcher preference modal.
    pub launcher_prefs: crate::launcher_prefs::State,
    /// `MiSTer_RA`'s cfg and install state, as last read from the card.
    pub ra: crate::retro_achievements::Model,
    /// The `RetroAchievements` login screen.
    pub ra_login: crate::ra_login::Model,
    /// Commercial-notice acknowledgement (durable, frontend.toml).
    pub notice_ack: bool,
    /// `frontend.toml` exists but did not load: the path, for the startup
    /// alert. Nothing is written to the file while this is set, because a
    /// save would replace the user's file with the defaults in use.
    pub config_fault: Option<String>,
    /// The startup alert for `config_fault` has been raised this session.
    pub config_fault_shown: bool,
    /// Core's reported version, once fetched; drives the min-version
    /// warning between the notice and the first-run gate.
    pub core_version: String,
    pub core_version_checked: bool,
    /// The version gate has resolved this session (shown or skipped).
    pub version_warning_shown: bool,
    /// Setup was offered or Core was already updating this session.
    pub first_run_shown: bool,
    pub card_write: crate::card_write::Model,
    /// The decks the Decks screen and pinned deck tiles show.
    pub decks: crate::decks::Model,
    pub launcher_save_seq: u64,
    /// The "Saving…" caption's wait while a launcher save is unanswered.
    pub launcher_save_wait: Option<crate::cue::LocalWait>,
    /// The header line's app cue (`crate::cue`).
    pub cue: crate::cue::Model,
    /// The browse scope's letter buckets, for the jump-to-letter picker
    /// and the fast-scroll rail (cursor kept Rust-side; the UI only shows
    /// label + count).
    pub letter_buckets: Vec<zaparoo_core::media_types::BrowseIndexGroup>,
    /// The buckets index the scope's directories, not its files: their
    /// offsets already count from the first directory. Read only while
    /// `letter_buckets` is filled.
    pub letter_directories: bool,
    /// The `(system, browse path)` the buckets belong to; None until the
    /// current fetch lands.
    pub letter_scope: Option<(String, String)>,
    /// Ticket for the facet fetch; bumped per fetch so a stale index
    /// response cannot fill a newer scope.
    pub letter_seq: u64,
    /// The tag filter picker's list, for the system on screen or the search.
    pub filter: crate::browse_filter::Model,
    /// The Search screen: keyboard, focus and live matches.
    pub search: crate::search::Model,
    pub rename: crate::rename::Model,
    pub file_browser: crate::file_browser::Model,
    /// The music player.
    pub music: crate::music::Model,
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
    /// Settings page and row for Back and in-process restoration, not
    /// fresh entry. Memory only: a cold start opens Settings at its root.
    pub settings_focus: Option<(crate::SettingsPage, usize)>,
    /// The Settings page on screen was opened from a pinned Hub tile, so
    /// Back returns to the Hub. Memory only, like `settings_focus`.
    pub settings_from_hub: bool,
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

use crate::{DialogButton, DialogKind, ErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingRestart {
    Resolution(String),
    CrtStandard(String),
    CrtEnabled(bool),
}

/// What the shared list panel is currently picking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListContext {
    /// The games screen's Menu list.
    ViewMenu,
    /// The Hub's Menu list (Add item, Reset layout, Settings, Quit), and
    /// its More list behind the chip row.
    HubPageMenu,
    /// The Hub's "Add item" picker.
    HubAdd,
    /// The file browser's Menu list: the actions its caller offers.
    FileBrowserMenu,
    /// The favorites list's Menu list, and its More list.
    FavoritesPageMenu,
    /// Its "Group by" and "Sort" pages.
    FavoritesGrouping,
    FavoritesSort,
    /// The Search results' Menu list.
    SearchResultsPageMenu,
    /// The Decks screen's Menu list.
    DecksPageMenu,
    /// Top played's Menu list, on either of its screens, and its "Group
    /// by" page.
    TopPlayedPageMenu,
    TopPlayedGrouping,
    /// The Deck screen's Menu list, and its More list.
    DeckPageMenu,
    /// The Games "Filter" page: one row per category, and the values page
    /// of the category being chosen. Both stay inside one panel.
    FilterCategories,
    FilterValues(zaparoo_app::browse_filter::Category),
    /// The Search screen's system picker.
    SearchSystem,
    /// A settings picker row; the payload is the field id.
    SettingsPicker(String),
    /// The music player's Menu, its More list (the Menu chip row's rare
    /// rows), its Volume page, the library's Menu, and a library folder's
    /// Options (the payload is the folder's path).
    MusicMenu,
    MusicMore,
    MusicVolume,
    MusicLibraryMenu,
    MusicFolderOptions(String),
    /// A Settings root tile's Options; the payload is its page id.
    SettingsTileOptions(String),
    /// The "Change launcher" picker; the payload is the system id.
    SystemLauncher(String),
    /// The per-game "Change launcher" picker; the payload is the game's
    /// system id and path.
    GameLauncher(String, String),
}

#[derive(Debug, Clone)]
pub struct Ctx {
    pub folders: crate::folder_picker::Model,
    pub images: crate::image_picker::Model,
    pub launcher_scan: crate::launcher_scan::Model,
    pub playtime_access: crate::playtime_access::Model,
    pub open_url: crate::open_url::Model,
    pub store: Arc<Store>,
    pub handle: Handle,
    pub media: Arc<MediaCache>,
    pub logos: Arc<crate::system_logos::Logos>,
    pub shared: Arc<Mutex<Shared>>,
    /// Live 12-hour clock flag shared with the clock task; the
    /// Settings toggle flips it without a restart.
    pub clock_twelve_hour: Arc<AtomicBool>,
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
    /// Posts the support bundle; `None` hides Upload log.
    pub log_uploader: Option<crate::log_upload::LogUploader>,
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
            core_ui: crate::core_ui::Model::default(),
            setup: crate::media_setup::SetupModel::new(),
            log_upload: crate::log_upload::LogUploadModel::new(),
            pairing: zaparoo_app::pairing::Session::default(),
            online: crate::online::Model::default(),
            online_link: zaparoo_app::online_link::Session::default(),
            online_list: crate::online_list::State::default(),
            errors: action_error::ErrorQueue::new(),
            context_page: crate::context_page::PageModel::default(),
            input: crate::input::InputModel::new(),
            has_readers: false,
            has_nfc: false,
            context_owner: ContextOwner::Games,
            context_target: 0,
            flag_requests: zaparoo_app::user_flags::Requests::default(),
            context_menu: zaparoo_app::options_menu::Menu::default(),
            menu_return: None,
            context_seq: 0,
            context_strip_at: 0,
            list_context: ListContext::ViewMenu,
            pending_restart: None,
            launchers: Vec::new(),
            system_defaults: Vec::new(),
            launcher_preference: None,
            launcher_pref_confirmed: None,
            launcher_prefs: crate::launcher_prefs::State::default(),
            ra: crate::retro_achievements::Model::default(),
            ra_login: crate::ra_login::Model::default(),
            notice_ack: false,
            config_fault: None,
            config_fault_shown: false,
            core_version: String::new(),
            core_version_checked: false,
            version_warning_shown: false,
            first_run_shown: false,
            card_write: crate::card_write::Model::default(),
            decks: crate::decks::Model::default(),
            launcher_save_seq: 0,
            launcher_save_wait: None,
            cue: crate::cue::Model::default(),
            letter_buckets: Vec::new(),
            letter_directories: false,
            letter_scope: None,
            letter_seq: 0,
            filter: crate::browse_filter::Model::default(),
            search: crate::search::Model::default(),
            rename: crate::rename::Model::default(),
            file_browser: crate::file_browser::Model::default(),
            music: crate::music::Model::default(),
            game_launcher_seq: 0,
            persist,
            restore_pending,
            hub: crate::hub::HubModel::new(hub_layout_path),
            settings_focus: None,
            settings_from_hub: false,
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

/// Rebuild the visible category list from the master list and the
/// hidden prefs, then re-resolve the Hub's tiles.
pub fn reproject_hub(ctx: &Ctx, app: &App) {
    project_categories(&mut lock(&ctx.shared));
    crate::hub::rebuild(ctx, app);
}

pub(crate) fn project_categories(shared: &mut Shared) {
    shared.categories = shared
        .all_categories
        .iter()
        .filter(|name| shared.show_hidden || !shared.hidden_categories.contains(name))
        .cloned()
        .collect();
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
    if !config_writable(ctx) {
        return;
    }
    if let Err(e) =
        zaparoo_core::config::save_hidden_browse_prefs(&ctx.config_path, categories, system_ids)
    {
        tracing::warn!("could not save hidden browse prefs: {e}");
        report_action_error(ctx, app, "setting", "");
    }
}

// ---------- Startup decision dialogs ----------

/// Dev builds and unparseable versions fail open: only a parsed semver
/// below the floor triggers the warning. 2.22.0 is the first Core that
/// takes a multi-disc folder's launch disc with its path.
const MIN_CORE_VERSION: &str = "2.22.0";

pub(crate) fn version_supported(raw: &str) -> bool {
    let v = raw.trim();
    if v == "DEVELOPMENT" || v.ends_with("-dev") {
        return true;
    }
    match (
        semver::Version::parse(v),
        semver::Version::parse(MIN_CORE_VERSION),
    ) {
        // A beta of the floor release already has its API.
        (Ok(mut current), Ok(min)) => {
            current.pre = semver::Prerelease::EMPTY;
            current >= min
        }
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
        assert!(version_supported("2.22.0"));
        assert!(version_supported("2.22.0-beta.1"));
        assert!(version_supported("3.0.1"));
        assert!(!version_supported("2.21.0"));
        assert!(!version_supported("2.21.1-beta.1"));
        assert!(!version_supported("1.0.0"));
    }
}

/// Open a dialog. Rust names the kind, its sub-kind and the one
/// runtime value the copy needs; `DialogLabels` in the UI composes
/// every word, so the buttons are keys too.
pub(crate) fn open_dialog(
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
    overlays.set_dialog_detail(SharedString::from(detail));
    overlays.set_dialog_arg(SharedString::from(arg));
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(buttons.to_vec())));
    overlays.set_dialog_focus(focus);
    overlays.set_dialog_open(true);
}

pub(crate) fn close_dialog(app: &App) {
    crate::press_feedback::cancel(app);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_open(false);
    overlays.set_dialog_kind(DialogKind::None);
    overlays.set_dialog_detail(SharedString::default());
    overlays.set_dialog_arg(SharedString::default());
}

/// Hub Back lands here instead of quitting outright, so a stray B
/// can't kill the frontend. Default focus is "No".
pub(crate) fn open_quit_confirm(app: &App) {
    // A game we are hosting dies with us, and Core is watching our end of
    // the launch connection to know when it finished. Quitting now would
    // report the game over while it is still on screen.
    if crate::steam_host::hosting() {
        tracing::debug!("quit declined: a hosted game is still running");
        open_alert(app, DialogKind::QuitBlocked);
        return;
    }
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
    // A notice waits for the restored screen: it never opens over the
    // cold-start curtain.
    if app.global::<crate::Overlays>().get_dialog_open()
        || !app.global::<crate::Shell>().get_boot_complete()
        || app.global::<crate::Shell>().get_boot_curtain()
    {
        return;
    }
    let config_fault = {
        let mut guard = lock(&ctx.shared);
        if guard.config_fault_shown {
            None
        } else {
            guard.config_fault_shown = true;
            guard.config_fault.clone()
        }
    };
    if let Some(path) = config_fault {
        report_action_error(ctx, app, "config_file", &path);
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
            zaparoo_core::systems_catalog::indexed_count(&guard.systems),
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
    // Media status may seed after the catalog. A running update needs no
    // setup gate, including after MiSTer's wrapper relaunches the frontend.
    let media = media_state(ctx);
    let busy = media.indexing || media.optimizing || media.scraping;
    if busy {
        lock(&ctx.shared).first_run_shown = true;
    }
    if zaparoo_app::media_setup::needs_first_run(indexed, first_run_shown, media.seeded, busy) {
        lock(&ctx.shared).first_run_shown = true;
        open_dialog(
            app,
            DialogKind::FirstRun,
            "",
            "",
            &[DialogButton::StartMediaUpdate],
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
    let config_alert =
        kind == DialogKind::ActionError && overlays.get_dialog_error() == ErrorKind::ConfigFile;
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
        } else if config_alert {
            // The config alert opens the startup ladder; the rest follows it.
            maybe_open_startup_notices(ctx, app);
        }
        // A Core confirmation that waited for this dialog takes it now.
        if !app.global::<crate::Overlays>().get_dialog_open() {
            crate::core_ui::surface_free(ctx, app);
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
            if !config_writable(ctx) {
                tracing::warn!("notice ack not persisted: the config file did not load");
            } else if let Err(e) = zaparoo_core::config::save_notice_ack(&ctx.config_path, true) {
                tracing::warn!("could not persist notice ack: {e}");
            }
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::CoreVersion => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::FirstRun => {
            close_dialog(app);
            let media = media_state(ctx);
            if !media.indexing && !media.optimizing && !media.scraping {
                start_index(ctx, app, None);
            }
        }
        DialogKind::RestartSetting => {
            if confirmed {
                confirm_pending_restart(ctx, app);
            } else {
                lock(&ctx.shared).pending_restart = None;
                close_dialog(app);
            }
        }
        DialogKind::UnlinkOnline => {
            close_dialog(app);
            if confirmed {
                crate::online::unlink(ctx, app);
            }
        }
        DialogKind::RaLogOut => {
            close_dialog(app);
            if confirmed {
                crate::retro_achievements::log_out(ctx, app);
            }
        }
        DialogKind::RestoreOnlineBackup => {
            close_dialog(app);
            if confirmed {
                crate::online_list::restore_confirmed(ctx, app);
            }
        }
        DialogKind::UpdateStop => {
            close_dialog(app);
            crate::update::stop_answered(confirmed);
        }
        DialogKind::DeleteDeck => {
            close_dialog(app);
            crate::decks::delete_answered(ctx, app, confirmed);
        }
        DialogKind::CoreConfirm => {
            let ok = app
                .global::<crate::Overlays>()
                .get_dialog_buttons()
                .row_data(focus)
                .is_some_and(|id| id == DialogButton::Ok);
            crate::core_ui::confirm_answered(ctx, app, ok);
        }
        _ => close_dialog(app),
    }
}

fn dialog_cancel(ctx: &Ctx, app: &App, kind: DialogKind) {
    match kind {
        // Setup ends on Start media update; it never owns a running job.
        DialogKind::Notice | DialogKind::FirstRun => {}
        DialogKind::CoreVersion => {
            close_dialog(app);
            maybe_open_startup_notices(ctx, app);
        }
        DialogKind::RestartSetting => {
            lock(&ctx.shared).pending_restart = None;
            close_dialog(app);
        }
        DialogKind::UpdateStop => {
            close_dialog(app);
            crate::update::stop_answered(false);
        }
        DialogKind::DeleteDeck => {
            close_dialog(app);
            crate::decks::delete_answered(ctx, app, false);
        }
        // Back dismisses, unless Core will not let this one go.
        DialogKind::CoreConfirm => crate::core_ui::confirm_answered(ctx, app, false),
        _ => close_dialog(app),
    }
}

/// Retire an obsolete setup prompt when indexing starts out of band or
/// indexed systems arrive. Progress belongs to the header, never a modal.
pub fn refresh_startup_notices(ctx: &Ctx, app: &App) {
    let media = media_state(ctx);
    let busy = media.indexing || media.optimizing || media.scraping;
    let indexed = {
        let mut shared = lock(&ctx.shared);
        if busy {
            shared.first_run_shown = true;
        }
        zaparoo_core::systems_catalog::indexed_count(&shared.systems)
    };
    let overlays = app.global::<crate::Overlays>();
    if overlays.get_dialog_kind() == DialogKind::FirstRun && (busy || indexed > 0) {
        close_dialog(app);
    }
    maybe_open_startup_notices(ctx, app);
}

/// A host showing this window again counts as activity even without a key press.
/// Disarm a saver that may have started while the app was in the background.
#[cfg(any(feature = "hosted", all(test, feature = "mister")))]
pub(crate) fn on_app_activated(ctx: &Ctx, app: &App) {
    app.global::<crate::Shell>().set_saver_armed(false);
    reset_idle(ctx, app);
    #[cfg(feature = "hosted")]
    if crate::host::take_trimmed() {
        // The host dropped decoded art while away: re-resolve the screens
        // that show it, which requests it again.
        crate::hub::rebuild(ctx, app);
        crate::games::render(ctx, app);
    }
}

thread_local! {
    static IDLE_TIMER: slint::Timer = slint::Timer::default();
    #[cfg(all(test, feature = "mister"))]
    static IDLE_FIRES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Drop pending countdown work on dormancy and before a hosted event loop ends.
pub(crate) fn stop_idle() {
    IDLE_TIMER.with(slint::Timer::stop);
}

#[cfg(all(test, feature = "mister"))]
pub(crate) fn idle_firings() -> usize {
    IDLE_FIRES.with(std::cell::Cell::get)
}

/// Restart the same countdown on every input. Invalid/off settings cancel it
/// rather than leaving a ticketed callback queued for minutes.
pub fn reset_idle(ctx: &Ctx, app: &App) {
    stop_idle();
    let timeout = lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout
        .clone();
    let Ok(secs) = timeout.parse::<u64>() else {
        return;
    };
    if secs == 0 {
        return;
    }
    let weak = app.as_weak();
    IDLE_TIMER.with(|timer| {
        timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_secs(secs),
            move || {
                #[cfg(all(test, feature = "mister"))]
                IDLE_FIRES.with(|fires| fires.set(fires.get() + 1));
                let Some(app) = weak.upgrade() else { return };
                let shell = app.global::<crate::Shell>();
                // Busy surfaces are not burn targets. Their completion/input paths
                // re-arm the clock; this timeout never polls or chains another timer.
                if !shell.get_boot_complete()
                    || shell.get_boot_curtain()
                    || shell.get_dormant()
                    || shell.get_transitioning()
                {
                    return;
                }
                if shell.get_active_screen() == crate::Screen::Update
                    && !app.global::<crate::UpdateView>().get_allows_screensaver()
                {
                    return;
                }
                shell.set_saver_armed(true);
            },
        );
    });
}

// Slow work adds feedback without hiding the source or delaying a ready route.
thread_local! {
    // The route's loading word follows the shared wait timing; the token is
    // the wait of the route now pending.
    static ROUTE_CUE: std::cell::RefCell<(zaparoo_app::wait_cue::WaitCue, u64)> =
        std::cell::RefCell::new((zaparoo_app::wait_cue::WaitCue::default(), 0));
}

pub(crate) fn begin_pending(app: &App, target: crate::Screen) {
    begin_pending_with_direction(app, target, 1);
}

pub(crate) fn begin_pending_with_direction(app: &App, target: crate::Screen, _direction: i32) {
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
    let shell = app.global::<crate::Shell>();
    if shell.get_transitioning() {
        return;
    }
    shell.set_transition_target(target);
    shell.set_transitioning(true);
    // A word still held from the route before belongs to that route.
    shell.set_transition_cue(false);
    let token = ROUTE_CUE.with(|cue| {
        let mut cue = cue.borrow_mut();
        cue.1 = cue.0.begin();
        cue.1
    });
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(CUE_DELAY_MS), move || {
        if !ROUTE_CUE.with(|cue| cue.borrow_mut().0.delay_elapsed(token)) {
            return;
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        app.global::<crate::Shell>().set_transition_cue(true);
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(CUE_HOLD_MS), move || {
            // The route landed while the word had only just appeared.
            if ROUTE_CUE.with(|cue| cue.borrow_mut().0.hold_elapsed(token)) {
                if let Some(app) = weak.upgrade() {
                    app.global::<crate::Shell>().set_transition_cue(false);
                }
            }
        });
    });
}

/// A menu whose row started a route to another screen. It stays up, its
/// row held down, until the route ends, and goes in the same turn: closing
/// it first would show the screen underneath on its own for a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeldMenu {
    None,
    Context,
    List,
}

/// What Back does to the wait a held menu is in, other than a route.
type HeldWait = Box<dyn FnOnce(&Ctx)>;

thread_local! {
    static HELD_MENU: std::cell::Cell<HeldMenu> = const { std::cell::Cell::new(HeldMenu::None) };
    static HELD_WAIT: std::cell::RefCell<Option<HeldWait>> = const { std::cell::RefCell::new(None) };
}

fn held_menu() -> HeldMenu {
    HELD_MENU.with(std::cell::Cell::get)
}

/// Run a menu row's action that may route to another screen or wait on
/// Core first, holding the menu open while it does. Whatever is ready in
/// this turn, or nothing at all, takes the menu with it now.
fn hold_menu_through(app: &App, menu: HeldMenu, action: impl FnOnce()) {
    HELD_MENU.with(|held| held.set(menu));
    action();
    let waiting = HELD_WAIT.with(|wait| wait.borrow().is_some());
    if !app.global::<crate::Shell>().get_transitioning() && !waiting {
        release_held_menu(app);
    }
}

/// The action of a held menu row has to wait on Core before it can show
/// what it opens. True when a menu is held for it: it stays up until
/// `end_held_wait`, and Back runs `cancel` and closes it.
pub(crate) fn hold_menu_for(cancel: impl FnOnce(&Ctx) + 'static) -> bool {
    if held_menu() == HeldMenu::None {
        return false;
    }
    HELD_WAIT.with(|wait| *wait.borrow_mut() = Some(Box::new(cancel)));
    true
}

/// The wait is over: the held menu goes now, in the turn that opens what
/// it was waiting for.
pub(crate) fn end_held_wait(app: &App) {
    HELD_WAIT.with(|wait| wait.borrow_mut().take());
    release_held_menu(app);
}

/// Back on a held menu: call off what it is waiting for. A held Options
/// menu stays open on its row; a route the Menu list was waiting on closes it.
fn cancel_held(ctx: &Ctx, app: &App) {
    let wait = HELD_WAIT.with(|wait| wait.borrow_mut().take());
    // Back from an Options row still on its way calls that off and leaves
    // the menu as it was, on the row.
    if held_menu() == HeldMenu::Context {
        HELD_MENU.with(|held| held.set(HeldMenu::None));
        forget_menu_return(ctx);
    }
    match wait {
        Some(cancel) => {
            cancel(ctx);
            crate::press_feedback::cancel(app);
            release_held_menu(app);
        }
        None => {
            crate::navigation::cancel(ctx, app);
        }
    }
}

/// The route a held menu was waiting on ended (it landed, was called off,
/// failed or timed out): the menu goes in this turn.
fn release_held_menu(app: &App) {
    let overlays = app.global::<crate::Overlays>();
    match HELD_MENU.with(|held| held.replace(HeldMenu::None)) {
        HeldMenu::Context => {
            overlays.set_context_open(false);
            overlays.set_context_page(crate::ContextPage::Root);
        }
        HeldMenu::List => overlays.set_list_open(false),
        HeldMenu::None => {}
    }
}

pub(crate) fn clear_pending(app: &App) {
    crate::press_feedback::cancel(app);
    release_held_menu(app);
    let shell = app.global::<crate::Shell>();
    shell.set_transitioning(false);
    let (clear, shown) = ROUTE_CUE.with(|cue| {
        let mut cue = cue.borrow_mut();
        let token = cue.1;
        let clear = cue.0.end(token);
        (clear, cue.0.is_shown(token))
    });
    // A word that only just appeared stays out its hold; its timer clears it.
    if clear || !shown {
        shell.set_transition_cue(false);
    }
}

/// Publish a ready destination in this turn, never after a decorative timer.
/// On a cold start this is also where the curtain lifts: the restored
/// screen is the first one shown.
pub(crate) fn transition_to_screen(app: &App, target: crate::Screen, _direction: i32) {
    clear_pending(app);
    crate::page_bar::screen_changed(app);
    let motion = app.global::<crate::Motion>();
    motion.set_epoch(motion.get_epoch().wrapping_add(1));
    app.global::<crate::Shell>().set_active_screen(target);
    crate::background::screen_changed(target);
    refresh_layout(app);
    finish_restore(app);
}

/// Publish the Hub and render it in the same turn. Only a render of the
/// active Hub asks for its logos, and the screen it comes back from has
/// taken the one logo window in the meantime.
pub(crate) fn return_to_hub(ctx: &Ctx, app: &App) {
    transition_to_screen(app, crate::Screen::Hub, -1);
    crate::hub::render(ctx, app);
}

/// Straight back to the Hub from wherever the user is: Back to Hub in the
/// favorites Menu, and a held Back. A route still pending is called off
/// first, so its fill cannot land after the Hub is up, and the Hub is the
/// screen a relaunch restores.
pub(crate) fn go_home(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_transitioning() {
        crate::navigation::cancel(ctx, app);
    }
    crate::games::flush_persist(ctx);
    lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
    save_persist(&ctx.shared);
    return_to_hub(ctx, app);
}

type RestoreHook = std::rc::Rc<dyn Fn(&App)>;

thread_local! {
    static RESTORE_FINISHED: std::cell::RefCell<Option<RestoreHook>> =
        const { std::cell::RefCell::new(None) };
    static RESTORE_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How long a cold-start restore may wait on Core before it is abandoned,
/// the same bound a staged route has.
pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_secs(15);

/// What runs once the cold-start restore ends: the work that waits for
/// the restored screen (startup notices).
pub(crate) fn on_restore_finished(hook: impl Fn(&App) + 'static) {
    RESTORE_FINISHED.with(|slot| *slot.borrow_mut() = Some(std::rc::Rc::new(hook)));
}

/// The cold-start restore is still deciding what to show: the catalog is
/// in and the curtain is still up.
pub(crate) fn restoring(app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    shell.get_boot_curtain() && shell.get_boot_complete()
}

/// End the cold-start restore: lift the curtain over whatever screen is
/// now final. Every way a restore can end comes through here (commit,
/// failure, a missing target, Cancel, the timeout), so the curtain can
/// never stick and nothing is shown before it lifts.
pub(crate) fn finish_restore(app: &App) {
    let shell = app.global::<crate::Shell>();
    if !shell.get_boot_curtain() {
        return;
    }
    // The user's background image is still being prepared: it lifts the
    // curtain itself when it lands (or gives up), so the restored screen
    // and its background appear together.
    if crate::background::holds_curtain() {
        return;
    }
    shell.set_boot_curtain(false);
    crate::background::curtain_lifted();
    RESTORE_SEQ.with(|seq| seq.set(seq.get().wrapping_add(1)));
    // Deferred a turn: a commit can come from deep inside a driver, and
    // the work that waited for it takes the shared state itself.
    let hook = RESTORE_FINISHED.with(|slot| slot.borrow().clone());
    if let Some(hook) = hook {
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::ZERO, move || {
            if let Some(app) = weak.upgrade() {
                hook(&app);
            }
        });
    }
}

/// Bound a restore that is waiting on Core.
pub(crate) fn arm_restore_timeout(ctx: &Ctx, app: &App) {
    let ticket = RESTORE_SEQ.with(|seq| {
        seq.set(seq.get().wrapping_add(1));
        seq.get()
    });
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(RESTORE_TIMEOUT, move || {
        if RESTORE_SEQ.with(std::cell::Cell::get) != ticket {
            return;
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        if !restoring(&app) {
            return;
        }
        if app.global::<crate::Shell>().get_transitioning() {
            tracing::warn!("cold-start restore timed out");
            abandon_restore(&ctx, &app);
            report_action_error(&ctx, &app, "browse", "");
        } else {
            // Only the Hub's Resume tile was outstanding; show the Hub.
            finish_restore(&app);
        }
    });
}

/// Give up a restore that is waiting on Core (Cancel, or the timeout):
/// retire its fetches so a late answer cannot route anywhere, and land on
/// the nearest screen that is already complete. The saved state keeps
/// naming the target, so the next start tries it again.
pub(crate) fn abandon_restore(ctx: &Ctx, app: &App) {
    // The music player lands where its Back would go.
    let music_to_settings = crate::music::restore_fallback_is_settings(&lock(&ctx.shared));
    crate::music::abandon(ctx);
    if music_to_settings {
        crate::navigation::finish(app);
        crate::music::land_on_settings(ctx, app);
        return;
    }
    let parent = {
        let mut shared = lock(&ctx.shared);
        shared.games.fill_task.cancel();
        shared.games.ticket = shared.games.ticket.wrapping_add(1);
        shared.games.loading = false;
        shared.games.loading_more = false;
        shared.systems_model.fill_task.cancel();
        shared.systems_model.transition_seq += 1;
        shared.systems_model.loading = false;
        // A restored Games screen has its category filled beneath it.
        let under_games = shared.persist.active_screen == crate::Screen::Games.token()
            && !shared.persist.games.entered_from_hub
            && shared.systems_model.mode == crate::SystemsMode::Category
            && !shared.systems_model.rows.is_empty();
        if under_games {
            crate::Screen::Systems
        } else {
            crate::Screen::Hub
        }
    };
    crate::navigation::finish(app);
    if parent == crate::Screen::Hub {
        return_to_hub(ctx, app);
    } else {
        transition_to_screen(app, parent, -1);
    }
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
            tracing::debug!("accept dropped: a press is already pending");
            return;
        }
        // Interrupt an undispatched Accept or lift a pending operation's
        // feedback. Back still belongs to the current surface, not the cue.
        tracing::debug!(action, "pending press cancelled by another action");
        crate::press_feedback::cancel(app);
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
    if crate::core_ui::picker_open(app) {
        return app.global::<crate::CoreUiView>().get_index();
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
        crate::Screen::Systems
        | crate::Screen::FavoriteSystems
        | crate::Screen::TopPlayedSystems => app.global::<crate::SystemsView>().get_current_index(),
        _ => app.global::<crate::GamesView>().get_current_index(),
    }
}

fn dispatch_with_focus(ctx: &Ctx, app: &App, action: &str) {
    let before = focus_index(app);
    dispatch_action(ctx, app, action);
    if !matches!(
        app.global::<crate::Shell>().get_active_screen(),
        crate::Screen::Hub
            | crate::Screen::Systems
            | crate::Screen::FavoriteSystems
            | crate::Screen::TopPlayedSystems
            | crate::Screen::Decks
            | crate::Screen::TopPlayed
            | crate::Screen::Games
            | crate::Screen::Favorites
            | crate::Screen::Recents
            | crate::Screen::SearchResults
            | crate::Screen::Deck
    ) {
        ctx.logos.request([]);
    }
    let after = focus_index(app);
    // A wrap can look geometrically adjacent on a two-row/two-column grid.
    // Input direction disambiguates it; never glide backward across that wrap.
    let wrapped = match action {
        actions::UP | actions::LEFT => after > before,
        actions::DOWN | actions::RIGHT => after < before,
        // A grid page is a new set of tiles: nothing glides onto it. A list
        // page is the same list a page further on, and glides there.
        actions::PAGE_PREV | actions::PAGE_NEXT => !list_layout_active(app),
        _ => false,
    };
    if wrapped {
        let motion = app.global::<crate::Motion>();
        motion.set_epoch(motion.get_epoch().wrapping_add(1));
    }
}

/// The active screen is a browse screen in its list layout.
fn list_layout_active(app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    match shell.get_active_screen() {
        crate::Screen::Systems
        | crate::Screen::FavoriteSystems
        | crate::Screen::TopPlayedSystems => shell.get_systems_list_layout(),
        crate::Screen::Games
        | crate::Screen::Favorites
        | crate::Screen::Recents
        | crate::Screen::SearchResults
        | crate::Screen::Deck
        | crate::Screen::Decks
        | crate::Screen::TopPlayed => shell.get_browse_list_layout(),
        _ => false,
    }
}

fn dispatch_action(ctx: &Ctx, app: &App, action: &str) {
    // Screensaver eats the waking press whole: disarm, restart the idle
    // clock, swallow.
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
        overlay_action(ctx, app, action, dialog_action);
        return;
    }
    // Core's picker: Core asked for an answer, so it sits above every
    // other surface but the decision dialog.
    if crate::core_ui::picker_open(app) {
        crate::core_ui::handle_action(ctx, app, action);
        return;
    }
    // Cold-launch curtain swallows input except Cancel, which still
    // quits from the (hidden) hub root so a frontend that can't reach
    // Core is never a trap on desktop.
    if app.global::<crate::Shell>().get_boot_curtain() {
        if action != actions::CANCEL {
            return;
        }
        // Once the catalog is in, the curtain is covering a restore:
        // Cancel gives that up instead of reaching the hidden screen.
        if restoring(app) {
            abandon_restore(ctx, app);
            return;
        }
    }
    // Single input gate during forward transitions.
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
            return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::QrCode);
        }
        return;
    }
    // Pairing: the panel owns input while a PIN is live, which is what
    // makes Back the only way out and so the only path that has to call
    // the pairing off.
    if app.global::<crate::Overlays>().get_pair_open() {
        crate::pairing::handle_action(ctx, app, action);
        return;
    }
    // The Online link panel, on the same terms: Back is the way out and
    // calls a live code off.
    if app.global::<crate::Overlays>().get_online_open() {
        crate::online::handle_action(ctx, app, action);
        return;
    }
    // The Online page's cloud backup list and activity log. A restore
    // confirmation is a dialog and takes input before this is reached.
    if app.global::<crate::Overlays>().get_online_list_open() {
        crate::online_list::handle_action(ctx, app, action);
        return;
    }
    // Settings > Library's launcher preference. A refused change raises an
    // alert, which takes input before this is reached.
    if app.global::<crate::Overlays>().get_launcher_prefs_open() {
        crate::launcher_prefs::handle_action(ctx, app, action);
        return;
    }
    if app.global::<crate::Overlays>().get_card_write_open() {
        // Accept commits the focused Cancel button; Back cancels directly.
        if action == actions::CANCEL || action == actions::ACCEPT {
            crate::card_write::cancel(ctx, app);
            return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::TokenWrite);
        }
        return;
    }
    if app.global::<crate::Overlays>().get_letter_open() {
        overlay_action(ctx, app, action, letter_action);
        return;
    }
    if app.global::<crate::Overlays>().get_list_open() {
        overlay_action(ctx, app, action, list_action);
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
        } else {
            tracing::debug!(action, "action dropped: a route is pending");
        }
        return;
    }
    // A fresh press always keeps (or restores) the live grid; only the
    // repeat path may set the flag.
    if !crate::input::rapid_page(ctx) {
        crate::input::note_rapid(ctx, app, action, false);
    }
    screen_action(ctx, app, action);
}

/// An overlay's own handler, then where focus goes if it just closed: back
/// to the Menu chip that opened it, when it was backed out of.
fn overlay_action(ctx: &Ctx, app: &App, action: &str, handler: fn(&Ctx, &App, &str)) {
    handler(ctx, app, action);
    crate::page_bar::overlay_settled(app, action);
}

/// The screen on show takes the action, or its Menu chip row while the
/// Menu button has put focus there.
fn screen_action(ctx: &Ctx, app: &App, action: &str) {
    if crate::page_bar::focused(app) {
        crate::page_bar::handle_action(ctx, app, action);
        return;
    }
    match app.global::<crate::Shell>().get_active_screen() {
        crate::Screen::Hub => crate::hub::handle_action(ctx, app, action),
        crate::Screen::Systems
        | crate::Screen::FavoriteSystems
        | crate::Screen::TopPlayedSystems => {
            crate::systems::handle_action(ctx, app, action);
        }
        crate::Screen::Games
        | crate::Screen::Favorites
        | crate::Screen::Recents
        | crate::Screen::SearchResults
        | crate::Screen::Deck
        | crate::Screen::Decks
        | crate::Screen::TopPlayed => {
            crate::games::handle_action(ctx, app, action);
        }
        crate::Screen::Search => crate::search::handle_action(ctx, app, action),
        crate::Screen::Rename => crate::rename::handle_action(ctx, app, action),
        crate::Screen::RaLogin => crate::ra_login::handle_action(ctx, app, action),
        crate::Screen::FileBrowser => crate::file_browser::handle_action(ctx, app, action),
        crate::Screen::Music => crate::music::handle_action(ctx, app, action),
        crate::Screen::Settings => crate::settings::handle_action(ctx, app, action),
        crate::Screen::About => about_action(ctx, app, action),
        crate::Screen::Update => crate::update::handle_action(ctx, app, action),
        crate::Screen::None => {}
    }
}

// ---------- Settings editor ----------
//
// Settings is a root category grid over per-domain field pages; the
// registries below mirror the field ids and option lists whose canonical
// value sets live in `zaparoo_app::settings`.
// Display includes live orientation plus restart-applied MiSTer HDMI
// resolution and native-CRT controls.

/// Current media-database state, read synchronously off the store's
/// watch resource; it gates the index and scrape entries.
pub(crate) fn media_state(ctx: &Ctx) -> zaparoo_core::store::MediaStatusState {
    let rx = ctx.store.media_status().subscribe();
    let state = rx.borrow().clone();
    state
}

/// About screen: enter from Settings, Back returns there. The screen
/// token persists so a kill on the About page restores to it.
pub fn enter_about(ctx: &Ctx, app: &App, entry: EntryMode) {
    let position = {
        let mut shared = lock(&ctx.shared);
        shared.persist.active_screen = "about".to_string();
        if entry == EntryMode::Fresh {
            shared.persist.about_scroll_milli = 0;
        }
        shared.persist.about_scroll_milli.min(1000)
    };
    app.global::<crate::AboutView>()
        .set_scroll_milli(i32::try_from(position).unwrap_or(0));
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

/// Ask before removing the Online account link; No is focused.
pub(crate) fn confirm_unlink_online(app: &App) {
    open_dialog(
        app,
        DialogKind::UnlinkOnline,
        "",
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
}

/// Ask before removing the `RetroAchievements` login; No is focused.
pub(crate) fn confirm_ra_log_out(app: &App) {
    open_dialog(
        app,
        DialogKind::RaLogOut,
        "",
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
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
    let (h, v, w) = {
        let shared = lock(&ctx.shared);
        (
            shared.persist.settings.crt_h_offset,
            shared.persist.settings.crt_v_offset,
            shared.persist.settings.crt_h_size,
        )
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_crt_h_offset(h);
    overlays.set_crt_v_offset(v);
    overlays.set_crt_h_size(w);
    overlays.set_crt_size_mode(false);
    overlays.set_crt_calibration_open(true);
}

fn crt_calibration_action(ctx: &Ctx, app: &App, action: &str) {
    let overlays = app.global::<crate::Overlays>();
    let size_mode = overlays.get_crt_size_mode();
    let (mut h, mut v) = (overlays.get_crt_h_offset(), overlays.get_crt_v_offset());
    let mut w = overlays.get_crt_h_size();
    match action {
        // One selector, two adjustments (master plan packet 7): the
        // context-menu button flips between position and size.
        actions::CONTEXT_MENU => {
            overlays.set_crt_size_mode(!size_mode);
            return;
        }
        // Width stretch: left widens (negative steps), right narrows,
        // matching the picture edge the arrow pushes.
        actions::LEFT if size_mode => w -= 1,
        actions::RIGHT if size_mode => w += 1,
        actions::UP | actions::DOWN if size_mode => return,
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
    if size_mode {
        let w = zaparoo_core::config::clamp_crt_h_size(w);
        lock(&ctx.shared).persist.settings.crt_h_size = w;
        overlays.set_crt_h_size(w);
        crate::set_live_crt_h_size(w);
        return;
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

/// The metadata setup panel opened on a scope a menu picked: the same
/// panel Settings opens, with the Systems row preset and still editable.
pub(crate) fn open_scrape_setup(ctx: &Ctx, app: &App, scope: zaparoo_app::media_setup::Scope) {
    crate::media_setup::open(ctx, app, zaparoo_app::media_setup::Kind::Scrape, scope);
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
    crate::page_bar::sync(app);
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
    crate::file_browser::relayout(ctx, app);
    crate::music::relayout(ctx, app);
}

/// Clock format or language changed: re-decide 12/24 hour and repaint.
pub(crate) fn apply_clock_setting(ctx: &Ctx, app: &App) {
    let twelve = {
        let guard = lock(&ctx.shared);
        crate::clock_twelve_hour(&guard.persist.settings)
    };
    ctx.clock_twelve_hour.store(twelve, Ordering::Relaxed);
    crate::push_clock(app, twelve);
}

/// Open the games screen's Menu list (the page-scoped operations, the
/// counterpart to North's item-scoped Options), where the Menu chip row
/// is not on screen. Go to... is pre-focused so the common path is a
/// fixed Menu-then-Accept chord, then the filter rows. The letter facet
/// and the filter's tag list are fetched here so both are likely ready by
/// the time the user gets into them.
pub(crate) fn open_view_menu(ctx: &Ctx, app: &App) {
    ensure_letter_index(ctx, app);
    crate::browse_filter::begin(ctx, app);
    let mut entries = vec![menu_row("jump_letter"), menu_row("search_here")];
    entries.extend(crate::browse_filter::view_rows(&lock(&ctx.shared)));
    present_list(ctx, app, ListContext::ViewMenu, "title:menu", entries);
}

/// Whether the focused row of the open list is a slider.
fn current_row_is_slider(app: &App) -> bool {
    let overlays = app.global::<crate::Overlays>();
    usize::try_from(overlays.get_list_index())
        .ok()
        .and_then(|i| overlays.get_list_entries().row_data(i))
        .is_some_and(|row| row.role == crate::MenuRole::Slider && row.enabled)
}

/// What each row of the open list is, for the cursor rules.
fn list_roles(app: &App) -> Vec<zaparoo_app::form_list::Role> {
    use zaparoo_app::form_list::Role;
    app.global::<crate::Overlays>()
        .get_list_entries()
        .iter()
        .map(|entry| match entry.role {
            // A row that opens a page, a picker's current choice and a
            // setting switched in place are all picked like any other.
            crate::MenuRole::Option
            | crate::MenuRole::Submenu
            | crate::MenuRole::CheckOff
            | crate::MenuRole::CheckOn
            | crate::MenuRole::Chosen
            | crate::MenuRole::ToggleOff
            | crate::MenuRole::ToggleOn
            | crate::MenuRole::Slider => Role::Option,
            crate::MenuRole::Header => Role::Header,
            crate::MenuRole::Action => Role::Action,
        })
        .collect()
}

/// A Menu list pick, by the list it was made on.
fn list_accept(ctx: &Ctx, app: &App, context: ListContext, id: &str) {
    match context {
        ListContext::ViewMenu => match id {
            "jump_letter" => open_go_to(ctx, app),
            "search_here" => crate::search::enter_scoped(ctx, app),
            "filter" => crate::browse_filter::open(ctx, app),
            "filter_clear" => crate::browse_filter::clear(ctx, app),
            _ => {}
        },
        ListContext::FilterCategories | ListContext::FilterValues(_) => {
            crate::browse_filter::accept(ctx, app, &context, id);
        }
        ListContext::SearchSystem => crate::search::system_picked(ctx, app, id),
        ListContext::HubPageMenu => crate::hub::page_menu_accept(ctx, app, id),
        ListContext::FileBrowserMenu => crate::file_browser::menu_accept(ctx, app, id),
        ListContext::HubAdd => crate::hub::add_picked(ctx, app, id),
        ListContext::FavoritesPageMenu => favorites_page_menu_accept(ctx, app, id),
        ListContext::SearchResultsPageMenu => match id {
            "edit_search" => crate::search::edit(ctx, app),
            "add_to_hub" => crate::search::toggle_hub(ctx, app),
            _ => {}
        },
        ListContext::DecksPageMenu | ListContext::DeckPageMenu => {
            crate::decks::page_menu_accept(ctx, app, id);
        }
        ListContext::FavoritesGrouping => {
            hold_menu_through(app, HeldMenu::List, || {
                favorites_grouping_picked(ctx, app, id);
            });
        }
        ListContext::FavoritesSort => favorites_sort_picked(ctx, app, id),
        ListContext::TopPlayedPageMenu => {
            if id == "top_grouping" {
                open_top_played_grouping_menu(ctx, app);
            }
        }
        ListContext::TopPlayedGrouping => {
            hold_menu_through(app, HeldMenu::List, || {
                top_played_grouping_picked(ctx, app, id);
            });
        }
        ListContext::SettingsPicker(field) => {
            crate::settings::picker_selected(ctx, app, &field, id);
        }
        ListContext::SystemLauncher(system_id) => {
            crate::launchers::set_system_launcher(ctx, app, &system_id, id);
        }
        ListContext::GameLauncher(system_id, path) => {
            crate::launchers::set_game_launcher(ctx, app, &system_id, &path, id);
        }
        ListContext::MusicMenu
        | ListContext::MusicMore
        | ListContext::MusicVolume
        | ListContext::MusicLibraryMenu
        | ListContext::MusicFolderOptions(_) => crate::music::list_accept(ctx, app, &context, id),
        ListContext::SettingsTileOptions(page) => {
            if id == "hub_toggle" {
                crate::hub::toggle_setting(ctx, app, &page);
            }
        }
    }
}

fn list_action(ctx: &Ctx, app: &App, action: &str) {
    // A pick's route is on its way: Back calls it off, which closes the
    // list; nothing else acts until it lands.
    if held_menu() == HeldMenu::List {
        if action == actions::CANCEL {
            cancel_held(ctx, app);
        }
        return;
    }
    if app.global::<crate::Overlays>().get_launcher_saving() {
        // A save Core has not answered holds the picker still, but never
        // hostage: Cancel gives up on it and closes as usual.
        if action != actions::CANCEL {
            return;
        }
        crate::launchers::abandon_save(ctx, app);
    }
    let roles = list_roles(app);
    let index = app.global::<crate::Overlays>().get_list_index().max(0) as usize;
    match action {
        // Wraps at the ends and passes over section headers.
        actions::UP | actions::DOWN if !roles.is_empty() => {
            let next = zaparoo_app::form_list::step(&roles, index, action == actions::DOWN);
            app.global::<crate::Overlays>()
                .set_list_index(i32::try_from(next).unwrap_or(0));
        }
        // Sideways on a slider row adjusts it, a step at a time.
        actions::LEFT | actions::RIGHT if current_row_is_slider(app) => {
            crate::music::slide(ctx, app, if action == actions::RIGHT { 1 } else { -1 });
        }
        // Sideways and the shoulder buttons jump a section in a list that
        // has them.
        actions::LEFT | actions::RIGHT | actions::PAGE_PREV | actions::PAGE_NEXT
            if matches!(lock(&ctx.shared).list_context, ListContext::SearchSystem) =>
        {
            let forward = matches!(action, actions::RIGHT | actions::PAGE_NEXT);
            let next = zaparoo_app::form_list::jump(&roles, index, forward);
            app.global::<crate::Overlays>()
                .set_list_index(i32::try_from(next).unwrap_or(0));
        }
        actions::ACCEPT => {
            let id = app
                .global::<crate::Overlays>()
                .get_list_entries()
                .row_data(index)
                .map(|e| e.id.to_string());
            if let Some(id) = id {
                let context = lock(&ctx.shared).list_context.clone();
                // These keep the panel open: the launcher pickers while they
                // save, and the filter, which swaps pages inside it.
                let stays_open = match context {
                    ListContext::SystemLauncher(_)
                    | ListContext::GameLauncher(_, _)
                    | ListContext::FilterCategories
                    | ListContext::FilterValues(_) => true,
                    ListContext::ViewMenu => id == "filter",
                    ListContext::MusicMenu | ListContext::MusicMore => {
                        crate::music::menu_stays_open(&id)
                    }
                    _ => false,
                };
                // A pick that opens another screen keeps the list up until
                // that screen is ready (`hold_menu_through`).
                let routes = matches!(
                    context,
                    ListContext::FavoritesGrouping | ListContext::TopPlayedGrouping
                );
                if !stays_open && !routes {
                    app.global::<crate::Overlays>().set_list_open(false);
                }
                list_accept(ctx, app, context, &id);
            }
        }
        actions::LEFT | actions::RIGHT | actions::PAGE_PREV | actions::PAGE_NEXT
            if matches!(lock(&ctx.shared).list_context, ListContext::FilterValues(_)) =>
        {
            crate::browse_filter::page(app, matches!(action, actions::RIGHT | actions::PAGE_NEXT));
        }
        actions::CANCEL | actions::PAGE_MENU => {
            let context = lock(&ctx.shared).list_context.clone();
            if matches!(
                context,
                ListContext::FilterCategories | ListContext::FilterValues(_)
            ) {
                crate::browse_filter::back(ctx, app, &context, action == actions::PAGE_MENU);
            } else {
                app.global::<crate::Overlays>().set_list_open(false);
                if action == actions::CANCEL {
                    return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::Picker);
                }
            }
        }
        _ => {}
    }
}

/// Fetch the browse facet for the current scope (`media.browse.index`,
/// non-empty buckets in sort order) and publish it to the picker
/// properties and the fast-scroll rail as it lands; the seq ticket drops
/// stale responses.
pub(crate) fn fetch_letter_index(ctx: &Ctx, app: &App) {
    // Core has no letter index for a folder read from disk.
    if lock(&ctx.shared).games.live {
        return;
    }
    let (browse_path, system_id, ticket, include_hidden, tags) = {
        let mut guard = lock(&ctx.shared);
        guard.letter_seq += 1;
        guard.letter_buckets.clear();
        guard.letter_scope = None;
        (
            guard.games.browse_path.clone(),
            guard.games.system_id.clone(),
            guard.letter_seq,
            crate::games::include_hidden(&guard),
            crate::browse_filter::active_tags(&guard),
        )
    };
    app.global::<crate::Overlays>()
        .set_letter_buckets(ModelRc::new(VecModel::from(
            Vec::<crate::LetterBucket>::new(),
        )));
    app.global::<crate::Overlays>().set_letter_index(0);
    app.global::<crate::Overlays>()
        .set_letter_state(crate::ContentState::Loading);

    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let scope = (system_id.clone(), browse_path.clone());
    ctx.handle.spawn(async move {
        let outcome = client
            .media_browse_index(zaparoo_core::media_types::MediaBrowseIndexParams {
                root_view: zaparoo_core::media_types::merged_root_view(
                    &browse_path,
                    std::slice::from_ref(&system_id),
                ),
                path: browse_path,
                systems: vec![system_id],
                include_hidden: Some(include_hidden),
                tags,
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
                    {
                        let mut guard = lock(&shared);
                        guard.letter_directories = result.indexes_directories();
                        guard.letter_buckets = result.groups;
                        guard.letter_scope = Some(scope);
                    }
                    let state = if rows.is_empty() {
                        crate::ContentState::Empty
                    } else {
                        crate::ContentState::Ready
                    };
                    app.global::<crate::Overlays>()
                        .set_letter_buckets(ModelRc::new(VecModel::from(rows)));
                    app.global::<crate::Overlays>().set_letter_state(state);
                    crate::games::render(&ctx2, &app);
                }
                Err(e) => {
                    // The picker stays (or opens) on the failure: closing
                    // it, or calling the list empty, would hide that the
                    // sections exist and could not be fetched.
                    tracing::warn!("letter index fetch failed: {}", e.message);
                    app.global::<crate::Overlays>()
                        .set_letter_state(crate::ContentState::Error);
                }
            }
        });
    });
}

/// Fetch the browse facet unless the folder on screen already has it.
pub(crate) fn ensure_letter_index(ctx: &Ctx, app: &App) {
    let fetched = {
        let shared = lock(&ctx.shared);
        shared.letter_scope.as_ref().is_some_and(|(system, path)| {
            *system == shared.games.system_id && *path == shared.games.browse_path
        })
    };
    if !fetched {
        fetch_letter_index(ctx, app);
    }
}

/// Open the jump-to-letter picker (Go to) over whatever facet state the
/// prefetch has reached (loading or filled), on the letter the selection
/// is in.
pub(crate) fn open_go_to(ctx: &Ctx, app: &App) {
    ensure_letter_index(ctx, app);
    let at = crate::games::current_letter_bucket(&lock(&ctx.shared)).unwrap_or(0);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_letter_index(i32::try_from(at).unwrap_or(0));
    overlays.set_letter_open(true);
}

fn close_letter_jump(_ctx: &Ctx, app: &App) {
    crate::press_feedback::cancel(app);
    // The index in flight is still the scope's: the rail keeps it, and a
    // reopened picker fetches again under a newer ticket.
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

/// Position jump on this page architecture: the absolute target is
/// leading dirs + bucket offset; pages already fetched are kept
/// (`PAGE_PREV` keeps working), and the gap up to the target is fetched
/// in bulk chunks sized to whole pages (batches always derive from
/// `page_size`), rounding the last chunk up.
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

/// QR write deep-link for the focused row: the scanning device opens
/// zaparoo.app, which hands the zapscript back to a Core/frontend
/// pairing.
/// The Frontend guide as a scannable link (the Documentation row).
pub(crate) fn open_documentation_qr(app: &App) {
    const DOCS_URL: &str = "https://zaparoo.org/docs/frontend/";
    if let Some((image, modules)) = crate::qr::qr_image(DOCS_URL, crate::qr::code_colors(app)) {
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
    let Some((image, modules)) =
        crate::qr::qr_image(&crate::qr::write_url(&text), crate::qr::code_colors(app))
    else {
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
    menu_row_full(id, "", label, true, "")
}

/// A menu row whose text comes from the `Labels.menu` vocabulary. The
/// key is the row id unless the copy depends on state (a favorite that
/// is already set, a hub item that hides rather than removes), and
/// `name` fills the one placeholder a row can carry.
pub(crate) fn menu_row(id: &str) -> crate::MenuEntry {
    menu_row_keyed(id, id, "")
}

pub(crate) fn menu_row_keyed(id: &str, key: &str, name: &str) -> crate::MenuEntry {
    menu_row_full(id, key, name, true, "")
}

/// The general form every other `menu_*` helper reduces to: a row that
/// can additionally be unusable right now, with a short worded reason
/// (a `Labels.menu` key) instead of hiding it — the picker is still the
/// place to pick it for later, once it stops being true.
pub(crate) fn menu_row_full(
    id: &str,
    key: &str,
    name: &str,
    enabled: bool,
    reason_key: &str,
) -> crate::MenuEntry {
    crate::MenuEntry {
        role: crate::MenuRole::default(),
        detail_key: SharedString::default(),
        id: SharedString::from(id),
        label: SharedString::from(name),
        label_key: SharedString::from(key),
        enabled,
        reason_key: SharedString::from(reason_key),
        detail: SharedString::default(),
    }
}

/// The same row with its role set: a picker's current choice (`Chosen`) or
/// a setting switched in place (`ToggleOn`, `ToggleOff`).
pub(crate) fn with_role(mut entry: crate::MenuEntry, role: crate::MenuRole) -> crate::MenuEntry {
    entry.role = role;
    entry
}

/// A picker's rows with the one that is the current choice marked.
pub(crate) fn mark_chosen(
    mut entries: Vec<crate::MenuEntry>,
    current: usize,
) -> Vec<crate::MenuEntry> {
    if let Some(entry) = entries.get_mut(current) {
        entry.role = crate::MenuRole::Chosen;
    }
    entries
}

/// Hub helpers the driver in `crate::hub` calls back into.
pub(crate) fn has_indexable(shared: &Shared, category: &str) -> bool {
    zaparoo_core::systems_catalog::systems_in_category(&shared.systems, category)
        .iter()
        .any(|s| s.zap_script.trim().is_empty())
}

fn indexable_system_ids(ctx: &Ctx, category: &str) -> Vec<String> {
    let guard = lock(&ctx.shared);
    zaparoo_core::systems_catalog::systems_in_category(&guard.systems, category)
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
    if !indexable_system_ids(ctx, category).is_empty() {
        open_scrape_setup(
            ctx,
            app,
            zaparoo_app::media_setup::Scope::Category(category.to_string()),
        );
    }
}

pub(crate) fn present_hub_context_menu(ctx: &Ctx, app: &App, menu: OptionsMenu) {
    present_context_menu(ctx, app, ContextOwner::Hub, 0, menu);
}

pub(crate) fn present_systems_context_menu(ctx: &Ctx, app: &App, target: usize, menu: OptionsMenu) {
    present_context_menu(ctx, app, ContextOwner::Systems, target, menu);
}

/// The row or tile a context menu is about: where it is, and what its
/// painted silhouette looks like. The menu panel and the scrim's hole both
/// follow this.
pub(crate) struct ContextAnchor {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// The anchored thing's own corner radius. A square hole around a
    /// rounded tile leaves a bright notch past each arc.
    pub radius: f32,
    /// Whether the focus zoom is on it. A focused tile paints larger than
    /// its cell rect, so the hole has to grow the same way or the tile
    /// spills past the bright area.
    pub zoomed: bool,
}

pub(crate) fn set_context_anchor(app: &App, anchor: &ContextAnchor) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_anchor_x(anchor.x);
    overlays.set_context_anchor_y(anchor.y);
    overlays.set_context_anchor_w(anchor.w);
    overlays.set_context_anchor_h(anchor.h);
    overlays.set_context_anchor_radius(anchor.radius);
    overlays.set_context_anchor_zoomed(anchor.zoomed);
}

pub(crate) fn present_games_context_menu(ctx: &Ctx, app: &App, target: usize, menu: OptionsMenu) {
    present_context_menu(ctx, app, ContextOwner::Games, target, menu);
}

pub(crate) fn present_list(
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
    overlays.set_list_form(false);
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(0);
    overlays.set_list_open(true);
}

/// The same panel as a form list: rows named on the left with their value
/// on the right, section headers and footer actions. Focus starts on
/// `index`, which must not be a header.
pub(crate) fn present_form_list(
    ctx: &Ctx,
    app: &App,
    context: ListContext,
    title: &str,
    entries: Vec<crate::MenuEntry>,
    index: usize,
) {
    present_list(ctx, app, context, title, entries);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_form(true);
    overlays.set_list_index(i32::try_from(index).unwrap_or(0));
}

/// A section header row.
pub(crate) fn menu_header(key: &str, name: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        role: crate::MenuRole::Header,
        ..menu_row_full("", key, name, false, "")
    }
}

/// A footer action row.
pub(crate) fn menu_action(id: &str) -> crate::MenuEntry {
    crate::MenuEntry {
        role: crate::MenuRole::Action,
        ..menu_row(id)
    }
}

/// A form row with a value: `detail` as data, or `detail_key` as a word of
/// the menu vocabulary when the value is ours to translate.
pub(crate) fn menu_value(
    id: &str,
    key: &str,
    name: &str,
    detail: &str,
    detail_key: &str,
) -> crate::MenuEntry {
    crate::MenuEntry {
        detail: SharedString::from(detail),
        detail_key: SharedString::from(detail_key),
        ..menu_row_keyed(id, key, name)
    }
}

pub(crate) fn present_hub_page_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubPageMenu, "title:menu", entries);
}

pub(crate) fn present_file_browser_menu(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(
        ctx,
        app,
        ListContext::FileBrowserMenu,
        "title:menu",
        entries,
    );
}

pub(crate) fn present_hub_add_picker(ctx: &Ctx, app: &App, entries: Vec<crate::MenuEntry>) {
    present_list(ctx, app, ListContext::HubAdd, "title:add_item", entries);
}

/// The favorites list's Menu list: order, grouping, a random favorite, a
/// search of the favorites, and the way back to the Hub.
pub(crate) fn open_favorites_page_menu(ctx: &Ctx, app: &App) {
    let (sort, grouping) = {
        let shared = lock(&ctx.shared);
        (
            crate::page_bar::favorites_sort_label(&shared),
            crate::page_bar::favorites_grouping_label(&shared),
        )
    };
    let entries = vec![
        menu_row_keyed("favorites_sort", "favorites_sort", sort),
        menu_row_keyed("favorites_grouping", "favorites_grouping", grouping),
        menu_row("launch_random_favorite"),
        menu_row("search_here"),
        menu_row("back_to_hub"),
    ];
    present_list(
        ctx,
        app,
        ListContext::FavoritesPageMenu,
        "title:menu",
        entries,
    );
}

/// The favorite systems' Menu list: the grouping switch and a search of
/// the favorites.
pub(crate) fn open_favorite_systems_page_menu(ctx: &Ctx, app: &App) {
    let grouping = crate::page_bar::favorites_grouping_label(&lock(&ctx.shared));
    let entries = vec![
        menu_row_keyed("favorites_grouping", "favorites_grouping", grouping),
        menu_row("search_here"),
    ];
    present_list(
        ctx,
        app,
        ListContext::FavoritesPageMenu,
        "title:menu",
        entries,
    );
}

/// The Search results' Menu list: change the search, keep it on the Hub,
/// or take its tile back off.
pub(crate) fn open_search_results_page_menu(ctx: &Ctx, app: &App) {
    let on_hub = crate::search::on_hub(&lock(&ctx.shared));
    let entries = vec![
        menu_row("edit_search"),
        menu_row_keyed(
            "add_to_hub",
            if on_hub { "hub:remove" } else { "save_to_hub" },
            "",
        ),
    ];
    present_list(
        ctx,
        app,
        ListContext::SearchResultsPageMenu,
        "title:menu",
        entries,
    );
}

pub(crate) fn favorites_page_menu_accept(ctx: &Ctx, app: &App, id: &str) {
    match id {
        "favorites_sort" => open_favorites_sort_menu(ctx, app),
        "favorites_grouping" => open_favorites_grouping_menu(ctx, app),
        "launch_random_favorite" => {
            let (list, scope) = {
                let shared = lock(&ctx.shared);
                (
                    crate::user_lists::active(&shared),
                    shared.games.list_system.clone(),
                )
            };
            launch(
                ctx,
                app,
                zaparoo_app::user_lists::random_launch_text(list, &scope),
                "",
            );
        }
        "search_here" => crate::search::enter_scoped(ctx, app),
        "back_to_hub" => go_home(ctx, app),
        _ => {}
    }
}

/// A built-in list from the Hub or a restore: the systems that hold its
/// games when it is grouped by system, otherwise the list itself.
pub(crate) fn enter_list(ctx: &Ctx, app: &App, list: Flag, entry: EntryMode) {
    if crate::user_lists::grouped(&lock(&ctx.shared), list) {
        crate::systems::enter_list(ctx, app, list, entry);
    } else {
        crate::games::enter_list(ctx, app, list, entry);
    }
}

/// Top played's Menu list (where the chip row is not on screen): its
/// grouping switch.
pub(crate) fn open_top_played_page_menu(ctx: &Ctx, app: &App) {
    let grouping = crate::page_bar::top_played_grouping_label(&lock(&ctx.shared));
    let entries = vec![menu_row_keyed(
        "top_grouping",
        "favorites_grouping",
        grouping,
    )];
    present_list(
        ctx,
        app,
        ListContext::TopPlayedPageMenu,
        "title:menu",
        entries,
    );
}

/// Top played's grouping page.
pub(crate) fn open_top_played_grouping_menu(ctx: &Ctx, app: &App) {
    let grouped = zaparoo_app::top_played::grouped(&lock(&ctx.shared).persist.top_played.grouping);
    let entries = mark_chosen(
        vec![
            menu_row_keyed("none", "group:none", ""),
            menu_row_keyed("system", "group:system", ""),
        ],
        usize::from(grouped),
    );
    present_list(
        ctx,
        app,
        ListContext::TopPlayedGrouping,
        "title:group_by",
        entries,
    );
    app.global::<crate::Overlays>()
        .set_list_index(i32::from(grouped));
}

/// Switching Top played's grouping re-enters it at the new level.
fn top_played_grouping_picked(ctx: &Ctx, app: &App, id: &str) {
    if !matches!(id, "none" | "system") {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let value = if id == "system" {
            zaparoo_app::user_lists::GROUP_SYSTEM
        } else {
            "none"
        };
        if shared.persist.top_played.grouping == value {
            return;
        }
        value.clone_into(&mut shared.persist.top_played.grouping);
    }
    save_persist(&ctx.shared);
    if id == "system" {
        crate::systems::enter_top_played(ctx, app, EntryMode::Restore);
    } else {
        crate::games::enter_top_played(ctx, app, EntryMode::Restore);
    }
}

/// The grouping page, reachable from either of a list's screens.
pub(crate) fn open_favorites_grouping_menu(ctx: &Ctx, app: &App) {
    let grouped = {
        let shared = lock(&ctx.shared);
        crate::user_lists::grouped(&shared, crate::user_lists::active(&shared))
    };
    let entries = mark_chosen(
        vec![
            menu_row_keyed("none", "group:none", ""),
            menu_row_keyed("system", "group:system", ""),
        ],
        usize::from(grouped),
    );
    present_list(
        ctx,
        app,
        ListContext::FavoritesGrouping,
        "title:group_by",
        entries,
    );
    app.global::<crate::Overlays>()
        .set_list_index(i32::from(grouped));
}

/// Switching the grouping re-enters the list at the new level.
fn favorites_grouping_picked(ctx: &Ctx, app: &App, id: &str) {
    if !matches!(id, "none" | "system") {
        return;
    }
    let list = {
        let mut shared = lock(&ctx.shared);
        let list = crate::user_lists::active(&shared);
        if !crate::user_lists::set_grouped(&mut shared, list, id == "system") {
            return;
        }
        list
    };
    if list == Flag::Favorite {
        crate::settings::save(ctx, app);
    } else {
        save_persist(&ctx.shared);
    }
    if id == "system" {
        crate::systems::enter_list(ctx, app, list, EntryMode::Restore);
    } else {
        crate::games::enter_list(ctx, app, list, EntryMode::Restore);
    }
}

pub(crate) fn open_favorites_sort_menu(ctx: &Ctx, app: &App) {
    use zaparoo_app::user_lists::ListSort;
    let current = {
        let shared = lock(&ctx.shared);
        crate::user_lists::sort(&shared, crate::user_lists::active(&shared))
    };
    let index = ListSort::ALL
        .iter()
        .position(|sort| *sort == current)
        .unwrap_or(0);
    let entries = mark_chosen(
        ListSort::ALL
            .iter()
            .map(|sort| menu_row_keyed(sort.token(), sort.label_key(), ""))
            .collect(),
        index,
    );
    present_list(ctx, app, ListContext::FavoritesSort, "title:sort", entries);
    app.global::<crate::Overlays>()
        .set_list_index(i32::try_from(index).unwrap_or(0));
}

/// Favorites' order is a durable preference (`frontend.toml`); the other
/// lists keep theirs with their screen state.
fn favorites_sort_picked(ctx: &Ctx, app: &App, id: &str) {
    let Some(sort) = zaparoo_app::user_lists::ListSort::from_token(id) else {
        return;
    };
    let (list, favorites_sort) = {
        let mut shared = lock(&ctx.shared);
        let list = crate::user_lists::active(&shared);
        if !crate::user_lists::set_sort(&mut shared, list, sort) {
            return;
        }
        (list, shared.favorites_sort.clone())
    };
    if list == Flag::Favorite {
        if config_writable(ctx) {
            if let Err(e) =
                zaparoo_core::config::save_favorites_sort(&ctx.config_path, &favorites_sort)
            {
                tracing::warn!("saving the favorites sort failed: {e}");
                report_action_error(ctx, app, "setting", "");
            }
        }
    } else {
        save_persist(&ctx.shared);
    }
    crate::games::refresh_list(ctx, app);
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
/// Whether `frontend.toml` may be written. False for the whole session once
/// the file failed to load; the startup alert has already said so, and each
/// later change stays in memory without raising its own alert.
pub(crate) fn config_writable(ctx: &Ctx) -> bool {
    lock(&ctx.shared).config_fault.is_none()
}

pub(crate) fn report_action_error(ctx: &Ctx, app: &App, kind: &str, context: &str) {
    // An error ends on the screen, never back in a menu.
    forget_menu_return(ctx);
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

/// A dialog closed without an answer of the user's (Core resolved its
/// confirmation elsewhere): hand the surface to whatever alert waited.
pub(crate) fn dialog_surface_freed(ctx: &Ctx, app: &App) {
    let next = lock(&ctx.shared).errors.take_next();
    if let Some(entry) = next {
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
    // `dialog_arg` carries the plain context for every other kind, and
    // `launch_repair`'s own fallback message for this one; its reason and
    // display names ride separately so `DialogLabels` can pick per-reason
    // wording instead of decoding JSON itself.
    let arg = if error == ErrorKind::LaunchRepair {
        let [reason, launcher, plugin, message] =
            RepairContext::decode(&entry.context).unwrap_or_default();
        let overlays = app.global::<crate::Overlays>();
        overlays.set_dialog_repair_reason(SharedString::from(reason));
        overlays.set_dialog_repair_launcher(SharedString::from(launcher));
        overlays.set_dialog_repair_plugin(SharedString::from(plugin));
        message
    } else {
        let overlays = app.global::<crate::Overlays>();
        overlays.set_dialog_repair_reason(SharedString::default());
        overlays.set_dialog_repair_launcher(SharedString::default());
        overlays.set_dialog_repair_plugin(SharedString::default());
        entry.context.clone()
    };
    open_dialog(app, DialogKind::ActionError, "", &arg, &[button], 0);
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
/// while it does. Read off the same status line the header shows.
pub(crate) fn media_busy(app: &App) -> bool {
    app.global::<crate::Status>().get_show_track()
}

/// The menu's own rows as the view takes them. A row that turns the menu
/// to a page is marked, so the view can draw where it leads.
pub(crate) fn root_entries(menu: &OptionsMenu) -> Vec<crate::MenuEntry> {
    menu.root
        .iter()
        .map(|row| {
            let mut entry = menu_row_keyed(row.id, row.key, "");
            if menu.page_for(row.id).is_some() || crate::context_page::asks_core(row.id) {
                entry.role = crate::MenuRole::Submenu;
            }
            entry
        })
        .collect()
}

fn present_context_menu(
    ctx: &Ctx,
    app: &App,
    owner: ContextOwner,
    target: usize,
    menu: OptionsMenu,
) {
    if menu.is_empty() {
        return;
    }
    let entries = root_entries(&menu);
    let strip = menu.strip;
    {
        let mut guard = lock(&ctx.shared);
        guard.menu_return = None;
        guard.context_owner = owner;
        guard.context_target = target;
        guard.context_menu = menu;
        guard.context_seq += 1;
        guard.context_strip_at = 0;
        guard.context_page.reset();
    }
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_page(crate::ContextPage::Root);
    overlays.set_context_index(0);
    publish_strip(app, strip);
    overlays.set_context_strip_index(-1);
    overlays.set_context_open(true);
}

/// Where the Options menu was when a row opened something in its place.
#[derive(Debug, Clone)]
pub struct MenuReturn {
    opens: zaparoo_app::options_menu::Opens,
    owner: ContextOwner,
    screen: crate::Screen,
    page: Option<zaparoo_app::options_menu::Page>,
    row: String,
}

/// A row is about to open something: note where the menu is, so Back
/// there brings it back. Rows that act at once leave nothing to return to.
fn remember_menu_return(ctx: &Ctx, app: &App, id: &str) {
    let mut shared = lock(&ctx.shared);
    let page = match shared.context_page.showing {
        Some(crate::context_page::Page::Menu(page)) => Some(page),
        _ => None,
    };
    // Write to token with one way to write opens it straight from its row.
    let opens = OptionsMenu::opens(id).or_else(|| {
        shared
            .context_page
            .showing
            .is_none()
            .then(|| OptionsMenu::opens(OptionsMenu::direct_action(id)))
            .flatten()
    });
    shared.menu_return = opens.map(|opens| MenuReturn {
        opens,
        owner: shared.context_owner,
        screen: app.global::<crate::Shell>().get_active_screen(),
        page,
        row: id.to_string(),
    });
}

/// The menu Back would bring back, with its rows and owner, taken out of
/// the way while something else (a tag search from Game Info) borrows the
/// screen.
#[derive(Debug, Clone)]
pub struct HeldReturn {
    back: MenuReturn,
    owner: ContextOwner,
    menu: zaparoo_app::options_menu::Menu,
}

pub(crate) fn take_menu_return(shared: &mut Shared) -> Option<HeldReturn> {
    let back = shared.menu_return.take()?;
    Some(HeldReturn {
        back,
        owner: shared.context_owner,
        menu: shared.context_menu.clone(),
    })
}

/// Puts a taken menu return back, rows and all, or clears it.
pub(crate) fn restore_menu_return(shared: &mut Shared, held: Option<HeldReturn>) {
    shared.menu_return = None;
    if let Some(held) = held {
        shared.menu_return = Some(held.back);
        shared.context_owner = held.owner;
        shared.context_menu = held.menu;
    }
}

/// What a row opened has finished (Start, Save, a pick): it ends on the
/// screen, so Back from anything later does not bring the menu back.
pub(crate) fn forget_menu_return(ctx: &Ctx) {
    lock(&ctx.shared).menu_return = None;
}

/// Back from what an Options row opened: the menu again, on the page it
/// was on, with that row focused. Its rows are the ones it had: Back means
/// nothing changed. False when `opens` is not what the menu opened, or the
/// screen has moved on, and the caller ends on the screen as before.
pub(crate) fn return_to_menu(
    ctx: &Ctx,
    app: &App,
    opens: zaparoo_app::options_menu::Opens,
) -> bool {
    let back = {
        let mut shared = lock(&ctx.shared);
        if shared
            .menu_return
            .as_ref()
            .is_none_or(|back| back.opens != opens)
        {
            return false;
        }
        shared.menu_return.take()
    };
    let Some(back) = back else {
        return false;
    };
    if app.global::<crate::Shell>().get_active_screen() != back.screen {
        return false;
    }
    let anchor = match back.owner {
        ContextOwner::Games => crate::games::context_anchor(ctx, app),
        ContextOwner::Systems => crate::systems::context_anchor(ctx, app),
        ContextOwner::Hub => crate::hub::context_anchor(ctx, app),
    };
    set_context_anchor(app, &anchor);
    let (entries, strip) = {
        let mut shared = lock(&ctx.shared);
        shared.context_page.reset();
        // A reopened menu is a new menu to a collection change in flight.
        shared.context_seq += 1;
        shared.context_strip_at = 0;
        (
            root_entries(&shared.context_menu),
            shared.context_menu.strip,
        )
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_page(crate::ContextPage::Root);
    publish_strip(app, strip);
    overlays.set_context_strip_index(-1);
    if let Some(page) = back.page {
        crate::context_page::turn(ctx, app, page);
    }
    let index = overlays
        .get_context_entries()
        .iter()
        .position(|entry| entry.id.as_str() == back.row)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0);
    overlays.set_context_index(index);
    overlays.set_context_open(true);
    true
}

pub(crate) fn user_flag(flag: Flag) -> crate::UserFlag {
    match flag {
        Flag::Favorite => crate::UserFlag::Favorite,
        Flag::Like => crate::UserFlag::Like,
        Flag::Dislike => crate::UserFlag::Dislike,
        Flag::PlayLater => crate::UserFlag::PlayLater,
    }
}

/// The strip's buttons, or none for a menu without one.
fn publish_strip(app: &App, flags: Option<Flags>) {
    let buttons: Vec<crate::FlagButton> = flags
        .map(|flags| {
            Flag::ALL
                .iter()
                .map(|&flag| crate::FlagButton {
                    flag: user_flag(flag),
                    on: flags.has(flag),
                })
                .collect()
        })
        .unwrap_or_default();
    app.global::<crate::Overlays>()
        .set_context_strip(ModelRc::new(VecModel::from(buttons)));
}

/// The open menu, for a collection change to find again when Core answers.
pub(crate) fn context_seq(ctx: &Ctx) -> u64 {
    lock(&ctx.shared).context_seq
}

/// Show `flags` on the strip of menu `seq`, if that menu is still open.
pub(crate) fn show_strip_flags(ctx: &Ctx, app: &App, seq: u64, flags: Flags) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.context_seq != seq || shared.context_menu.strip.is_none() {
            return;
        }
        shared.context_menu.strip = Some(flags);
    }
    if !app.global::<crate::Overlays>().get_context_open() {
        return;
    }
    // Change each button in place: only the toggled icons repaint.
    let model = app.global::<crate::Overlays>().get_context_strip();
    for (index, &flag) in Flag::ALL.iter().enumerate() {
        let button = crate::FlagButton {
            flag: user_flag(flag),
            on: flags.has(flag),
        };
        if model.row_data(index).is_some_and(|old| old != button) {
            model.set_row_data(index, button);
        }
    }
}

/// Whether the strip is on screen: a game's menu, on its own rows.
fn strip_shown(ctx: &Ctx, app: &App) -> bool {
    app.global::<crate::Overlays>()
        .get_context_strip()
        .row_count()
        > 0
        && !crate::context_page::showing(ctx)
}

pub(crate) fn close_context_menu(ctx: &Ctx, app: &App) {
    crate::press_feedback::cancel(app);
    // Bumping the seq abandons any in-flight card write (its result is
    // ignored on arrival) and any page still looking for a menu to fill.
    {
        let mut shared = lock(&ctx.shared);
        shared.card_write.cancel();
        shared.context_page.reset();
    }
    lock(&ctx.shared).context_seq += 1;
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_open(false);
    overlays.set_context_page(crate::ContextPage::Root);
    overlays.set_context_strip_index(-1);
    publish_strip(app, None);
}

/// Pointer input on the context menu's rows: hover moves focus, a click
/// accepts.
pub fn bind_context_input(ctx: &Arc<Ctx>, app: &App) {
    crate::about::bind(ctx, app);
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        app.global::<crate::Overlays>()
            .on_slider_step(move |direction| {
                let Some(app) = weak.upgrade() else { return };
                if lock(&ctx.shared).persist.settings.mouse_enabled && current_row_is_slider(&app) {
                    crate::music::slide(&ctx, &app, direction);
                }
            });
    }
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
                            && ov
                                .get_list_entries()
                                .row_data(row)
                                .is_some_and(|entry| entry.role != crate::MenuRole::Header) =>
                    {
                        ov.set_list_index(index);
                    }
                    crate::PressOwner::OnlineList
                        if !ov.get_dialog_open()
                            && ov.get_online_list_open()
                            && crate::online_list::focus(&ctx, &app, row) => {}
                    crate::PressOwner::LauncherPrefs
                        if !ov.get_dialog_open()
                            && ov.get_launcher_prefs_open()
                            && crate::launcher_prefs::focus(&ctx, &app, row) => {}
                    crate::PressOwner::CorePicker
                        if !ov.get_dialog_open() && crate::core_ui::picker_open(&app) =>
                    {
                        crate::core_ui::focus(&ctx, &app, row);
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
            let overlays = app.global::<crate::Overlays>();
            overlays.set_context_strip_index(-1);
            overlays.set_context_index(index);
        });
    }
    bind_strip_input(ctx, app);
    bind_row_clicked(ctx, app);
}

/// A click on a context-menu row focuses it and accepts.
fn bind_row_clicked(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<crate::ContextInput>();
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
        let overlays = app.global::<crate::Overlays>();
        overlays.set_context_strip_index(-1);
        overlays.set_context_index(index);
        handle_action(&ctx, &app, actions::ACCEPT);
    });
}

/// Pointer input on the collections strip: hover moves focus to a button,
/// a click toggles it.
fn bind_strip_input(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<crate::ContextInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_strip_hovered(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if crate::press_feedback::pending(&app)
                || !lock(&ctx.shared).persist.settings.mouse_enabled
            {
                return;
            }
            focus_strip(&ctx, &app, index);
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_strip_clicked(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if crate::press_feedback::pending(&app)
                || !lock(&ctx.shared).persist.settings.mouse_enabled
            {
                return;
            }
            focus_strip(&ctx, &app, index);
            handle_action(&ctx, &app, actions::ACCEPT);
        });
    }
}

/// Put focus on strip button `index`, taking it off the rows.
fn focus_strip(ctx: &Ctx, app: &App, index: i32) {
    let Ok(at) = usize::try_from(index) else {
        return;
    };
    if !strip_shown(ctx, app) || at >= Flag::ALL.len() {
        return;
    }
    lock(&ctx.shared).context_strip_at = at;
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_index(-1);
    overlays.set_context_strip_index(index);
}

/// Put focus back on row `index`, off the strip.
fn focus_row(app: &App, index: usize) {
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_strip_index(-1);
    overlays.set_context_index(i32::try_from(index).unwrap_or(0));
}

/// A key while the strip has focus. Left and Right walk the buttons and
/// stop at the ends; Down returns to the first row and Up wraps to the
/// last, as the rows themselves wrap.
fn strip_action(ctx: &Ctx, app: &App, action: &str, at: usize, rows: usize) {
    match action {
        actions::LEFT if at > 0 => focus_strip(ctx, app, (at - 1) as i32),
        actions::RIGHT if at + 1 < Flag::ALL.len() => focus_strip(ctx, app, (at + 1) as i32),
        actions::DOWN if rows > 0 => focus_row(app, 0),
        actions::UP if rows > 0 => focus_row(app, rows - 1),
        actions::ACCEPT => {
            let owner = lock(&ctx.shared).context_owner;
            let Some(&flag) = Flag::ALL.get(at) else {
                return;
            };
            match owner {
                ContextOwner::Games => crate::games::strip_toggle(ctx, app, flag),
                ContextOwner::Hub => crate::hub::strip_toggle(ctx, app, flag),
                ContextOwner::Systems => {}
            }
        }
        actions::CANCEL | actions::CONTEXT_MENU => close_context_menu(ctx, app),
        _ => {}
    }
}

fn context_action(ctx: &Ctx, app: &App, action: &str) {
    let len = app
        .global::<crate::Overlays>()
        .get_context_entries()
        .row_count();
    let index = app.global::<crate::Overlays>().get_context_index().max(0) as usize;
    // A row's route or wait is on its way: Back calls it off, which closes
    // the menu; nothing else acts until it lands.
    if held_menu() == HeldMenu::Context {
        if action == actions::CANCEL {
            cancel_held(ctx, app);
        }
        return;
    }
    let strip_at = app.global::<crate::Overlays>().get_context_strip_index();
    if strip_shown(ctx, app) {
        if let Ok(at) = usize::try_from(strip_at) {
            strip_action(ctx, app, action, at, len);
            return;
        }
    }
    match action {
        // The strip sits between the last row and the first: Up from the
        // first row and Down from the last reach it.
        actions::UP if len > 0 && index == 0 && strip_shown(ctx, app) => {
            let at = lock(&ctx.shared).context_strip_at;
            focus_strip(ctx, app, at as i32);
        }
        actions::DOWN if len > 0 && index + 1 == len && strip_shown(ctx, app) => {
            let at = lock(&ctx.shared).context_strip_at;
            focus_strip(ctx, app, at as i32);
        }
        actions::UP | actions::DOWN if len > 0 => {
            let step_ms = lock(&ctx.shared).input.modal_follow();
            let overlays = app.global::<crate::Overlays>();
            overlays.set_context_step_ms(step_ms);
            overlays.set_context_index(if action == actions::UP {
                ((index + len - 1) % len) as i32
            } else {
                ((index + 1) % len) as i32
            });
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
            // A page is a page of this menu, not a menu of its own: Back
            // returns to the rows it replaced.
            if crate::context_page::showing(ctx) {
                crate::context_page::leave(ctx, app);
            } else {
                close_context_menu(ctx, app);
            }
        }
        _ => {}
    }
}

fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    use crate::context_page::Page;
    remember_menu_return(ctx, app, id);
    let (owner, showing, opens) = {
        let shared = lock(&ctx.shared);
        let showing = shared.context_page.showing;
        // One level of pages: a row on a page never opens another.
        let opens = if showing.is_none() {
            shared.context_menu.page_for(id)
        } else {
            None
        };
        (shared.context_owner, showing, opens)
    };
    if matches!(
        showing,
        Some(Page::Alternates | Page::Discs | Page::Versions | Page::AddToDeck)
    ) {
        crate::context_page::accept(ctx, app, id);
        return;
    }
    if let Some(page) = opens {
        crate::context_page::turn(ctx, app, page);
        return;
    }
    // Write to token with one way to write runs that way.
    let id = if showing.is_none() {
        OptionsMenu::direct_action(id)
    } else {
        id
    };
    // A page Core has to be asked for keeps the menu open: its rows
    // become the page's once Core answers.
    let began = match owner {
        ContextOwner::Games => crate::games::begin_context_page(ctx, app, id),
        ContextOwner::Hub => crate::hub::begin_context_page(ctx, app, id),
        ContextOwner::Systems => false,
    };
    if began {
        return;
    }
    if OptionsMenu::holds_open(id) {
        run_held_row(ctx, app, owner, id);
        return;
    }
    close_context_menu(ctx, app);
    match owner {
        ContextOwner::Games => crate::games::context_accept(ctx, app, id),
        ContextOwner::Systems => crate::systems::context_accept(ctx, app, id),
        ContextOwner::Hub => crate::hub::context_accept(ctx, app, id),
    }
}

/// A row that opens something that may not be ready at once: the menu
/// stays up, its row held, until it is (`hold_menu_through`).
fn run_held_row(ctx: &Ctx, app: &App, owner: ContextOwner, id: &str) {
    hold_menu_through(app, HeldMenu::Context, || match owner {
        ContextOwner::Games => crate::games::context_accept(ctx, app, id),
        ContextOwner::Systems => crate::systems::context_accept(ctx, app, id),
        ContextOwner::Hub => crate::hub::context_accept(ctx, app, id),
    });
    if !app.global::<crate::Overlays>().get_context_open() {
        // The rest of the menu's own state goes with it.
        close_context_menu(ctx, app);
    }
}

/// Kick a scoped media-database rebuild; progress lands in the header
/// via the store's media-status resource, same as the full rebuild.
pub(crate) fn start_index(ctx: &Ctx, app: &App, systems: Option<Vec<String>>) {
    use zaparoo_core::media_types::MediaIndexParams;
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    // Core's own progress takes the header once the job is running; until
    // it answers, this is the only sign the request went anywhere.
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Starting, "", "");
    ctx.handle.spawn(async move {
        let result = client.media_generate(MediaIndexParams { systems }).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            if let Err(e) = result {
                tracing::warn!("start_index failed: {}", e.message);
                report_action_error(&ctx2, &app, "media_index", "");
            }
        });
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
    launch_first(ctx, app, vec![text], name);
}

/// Launch the first of `candidates` Core can resolve. A Hub tile carries
/// several identifiers for one game, most portable first; the next is tried
/// only when Core reports the previous one did not resolve, so nothing is
/// ever launched twice. The last failure is the one reported.
pub(crate) fn launch_first(ctx: &Ctx, app: &App, candidates: Vec<String>, name: &str) {
    if candidates.is_empty() {
        return;
    }
    crate::perf::mark("launch-press", "");
    crate::hub_covers::record_browse_page(ctx);
    // The tile the user pressed stays pressed until Core answers, so the
    // feedback is where the eye already is. The header line is the second,
    // worded cue and only appears if the wait becomes one.
    let hold = crate::press_feedback::keep_held(app);
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Launching, name, "");
    let store = ctx.store.clone();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let name = name.to_string();
    ctx.handle.spawn(async move {
        let result = run_first(
            candidates,
            |text| {
                let store = store.clone();
                async move { store.run_mutation::<RunMutation>(RunParams { text }).await }
            },
            |e: &zaparoo_core::client::ClientError| {
                tracing::warn!("launch failed for {name}: {e}");
                zaparoo_app::hub::should_try_next(e.category.as_deref())
            },
        )
        .await;
        let outcome = match result {
            Ok(()) => {
                crate::perf::mark("launch-reply", "ok=true");
                LaunchOutcome::Ok
            }
            Err(e) => {
                crate::perf::mark("launch-reply", "ok=false");
                LaunchOutcome::from_error(&e)
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            finish_launch(&ctx2, &app, hold, outcome, &name);
        });
    });
}

/// Run `candidates` in order until one succeeds, returning the last
/// failure otherwise. `try_next` sees each failure and decides whether the
/// next candidate may run; it is not asked about the last one.
async fn run_first<E, F, Fut>(
    candidates: Vec<String>,
    mut run: F,
    try_next: impl Fn(&E) -> bool,
) -> Result<(), E>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), E>>,
{
    let mut candidates = candidates.into_iter().peekable();
    let mut result = Ok(());
    while let Some(text) = candidates.next() {
        result = run(text).await;
        match &result {
            Ok(()) => break,
            Err(e) => {
                let more = candidates.peek().is_some();
                if !try_next(e) || !more {
                    break;
                }
            }
        }
    }
    result
}

pub(crate) fn finish_launch(
    ctx: &Ctx,
    app: &App,
    hold: crate::press_feedback::Hold,
    outcome: LaunchOutcome,
    name: &str,
) {
    crate::press_feedback::release(app, hold);
    match outcome {
        LaunchOutcome::Ok => {}
        LaunchOutcome::Failed => report_action_error(ctx, app, "launch", name),
        LaunchOutcome::Repair(context) => {
            report_action_error(ctx, app, "launch_repair", &context.encode());
        }
    }
}

/// What a launch attempt produced, decided once (in the async task) so the
/// event-loop closure only has to act on it.
pub(crate) enum LaunchOutcome {
    Ok,
    Failed,
    Repair(RepairContext),
}

impl LaunchOutcome {
    fn from_error(e: &zaparoo_core::client::ClientError) -> Self {
        if !e.is_launch_repair() {
            return Self::Failed;
        }
        Self::Repair(RepairContext::new(
            e.reason.as_deref(),
            e.params.as_deref(),
            &e.message,
        ))
    }
}

/// A `launch_repair` error's `reason` and the display names Core sent,
/// carried through the alert queue as one JSON string (the same idiom
/// `launchers.rs`'s retry payload uses) so a second failure queued behind
/// the first survives with its own reason intact.
pub(crate) struct RepairContext {
    reason: String,
    launcher: String,
    plugin: String,
    message: String,
}

impl RepairContext {
    /// Built from an already-confirmed `launch_repair` error's own fields
    /// (plain data, not `ClientError` itself, so this is testable without
    /// a live client). An absent `reason` or param reads as empty, which
    /// `DialogLabels.launch-repair-body` treats the same as `unspecified`.
    fn new(
        reason: Option<&str>,
        params: Option<&std::collections::HashMap<String, String>>,
        message: &str,
    ) -> Self {
        let param = |key: &str| {
            params
                .and_then(|params| params.get(key))
                .cloned()
                .unwrap_or_default()
        };
        Self {
            reason: reason.unwrap_or_default().to_string(),
            launcher: param("launcher"),
            plugin: param("plugin"),
            message: message.to_string(),
        }
    }

    fn encode(&self) -> String {
        serde_json::to_string(&[&self.reason, &self.launcher, &self.plugin, &self.message])
            .unwrap_or_default()
    }

    /// The inverse of `encode`, tolerant of a malformed payload (an older
    /// build's queued alert surviving a hot-reload, say): every field
    /// falls back to the generic-failure treatment rather than panicking.
    fn decode(payload: &str) -> Option<[String; 4]> {
        serde_json::from_str(payload).ok()
    }
}

#[cfg(test)]
mod tests {
    use zaparoo_core::media_types::SystemInfo;
    use zaparoo_core::systems_catalog::systems_in_category as systems_for_category;

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

    /// Run the fallback chain against a scripted Core: each candidate maps
    /// to the error category it fails with, or succeeds when absent.
    async fn run_chain(
        candidates: &[&str],
        failures: &[(&'static str, &'static str)],
    ) -> (Result<(), &'static str>, Vec<String>) {
        let tried = std::sync::Mutex::new(Vec::new());
        let result = super::run_first(
            candidates.iter().map(ToString::to_string).collect(),
            |text| {
                let failure = failures
                    .iter()
                    .find(|(candidate, _)| *candidate == text)
                    .map(|(_, category)| *category);
                tried
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(text);
                async move { failure.map_or(Ok(()), Err) }
            },
            |category: &&'static str| zaparoo_app::hub::should_try_next(Some(category)),
        )
        .await;
        (
            result,
            tried
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    #[tokio::test]
    async fn a_launch_stops_at_the_first_candidate_core_resolves() {
        let (result, tried) =
            run_chain(&["NES/Zelda.nes", "@NES/Zelda", "/g/Zelda.nes"], &[]).await;
        assert_eq!(result, Ok(()));
        assert_eq!(tried, ["NES/Zelda.nes"]);
    }

    #[tokio::test]
    async fn an_unresolved_candidate_falls_through_to_the_next() {
        let (result, tried) = run_chain(
            &["NES/Zelda.nes", "@NES/Zelda", "/g/Zelda.nes"],
            &[
                ("NES/Zelda.nes", "media_not_found"),
                ("@NES/Zelda", "invalid_script"),
            ],
        )
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(tried, ["NES/Zelda.nes", "@NES/Zelda", "/g/Zelda.nes"]);
    }

    #[tokio::test]
    async fn a_failure_that_is_not_about_the_identifier_ends_the_launch() {
        for category in [
            "busy",
            "blocked",
            "playtime_limit",
            "launch_repair",
            "timeout",
        ] {
            let (result, tried) = run_chain(
                &["NES/Zelda.nes", "@NES/Zelda"],
                &[("NES/Zelda.nes", category)],
            )
            .await;
            assert_eq!(result, Err(category));
            assert_eq!(tried, ["NES/Zelda.nes"], "{category}");
        }
    }

    #[tokio::test]
    async fn the_last_failure_is_the_one_reported() {
        let (result, tried) = run_chain(
            &["NES/Zelda.nes", "/g/Zelda.nes"],
            &[
                ("NES/Zelda.nes", "media_not_found"),
                ("/g/Zelda.nes", "execution_failed"),
            ],
        )
        .await;
        assert_eq!(result, Err("execution_failed"));
        assert_eq!(tried, ["NES/Zelda.nes", "/g/Zelda.nes"]);
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

    fn params(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_repair_context_round_trips_through_its_encoded_payload() {
        let context = super::RepairContext::new(
            Some("launcher_plugin_missing"),
            Some(&params(&[("launcher", "RetroArch"), ("plugin", "Mesen")])),
            "this launcher's plugin for this system is not installed",
        );
        let decoded = super::RepairContext::decode(&context.encode());
        assert_eq!(
            decoded,
            Some([
                "launcher_plugin_missing".to_string(),
                "RetroArch".to_string(),
                "Mesen".to_string(),
                "this launcher's plugin for this system is not installed".to_string(),
            ])
        );
    }

    #[test]
    fn an_absent_reason_or_param_encodes_as_empty_not_missing() {
        let context = super::RepairContext::new(None, None, "fallback message");
        let decoded = super::RepairContext::decode(&context.encode());
        assert_eq!(
            decoded,
            Some([
                String::new(),
                String::new(),
                String::new(),
                "fallback message".to_string(),
            ])
        );
    }

    #[test]
    fn a_malformed_payload_decodes_to_none_rather_than_panicking() {
        assert_eq!(super::RepairContext::decode("not json"), None);
        assert_eq!(super::RepairContext::decode(""), None);
        assert_eq!(
            super::RepairContext::decode(r#"["only", "three", "of four"]"#),
            None
        );
    }
}
