// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Settings rows that are Core's own: Readers and its pages, Profiles,
// System's pairing and security rows, and Library's preferred game
// versions. Core is the only store; what is here is its last answer, held
// in memory while Settings is open and fetched again as each page opens.
// A change is shown at once and sent; a refusal reports and fetches
// Core's answer again.

use std::collections::BTreeMap;
use std::future::Future;

use zaparoo_app::readers::{self as readers_rules, Mode};
use zaparoo_app::settings::{self as rules, Row};
use zaparoo_core::client::ClientError;
use zaparoo_core::media_types::{ReaderInfo, SettingsResult, UpdateSettingsParams};
use zaparoo_core::profile_types::{
    NewProfileParams, PairedClient, PlaytimeLimits, Profile, UpdatePlaytimeLimitsParams,
    UpdateProfileParams,
};
use zaparoo_core::readers_types::{DriverConfig, DriverInfo, Mapping, PortInfo, ReaderConnection};

use crate::router::{lock, Ctx, Shared};
use crate::{App, DialogButton, DialogKind, SettingsPage, SettingsRow};
use slint::{ComponentHandle, SharedString};

/// A reader being added: its type, then where it is, then its options.
#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub driver: String,
    pub path: String,
    pub id_source: String,
}

/// A deletion waiting on its confirmation dialog.
#[derive(Debug, Clone)]
enum Confirm {
    Revoke(String),
    DeleteProfile(String),
    DeleteMapping(i64),
}

/// What a keyboard entry on the rename screen is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextKind {
    /// A profile's new name, by its id.
    ProfileName(String),
    /// The name of a profile to make.
    NewProfile,
    /// The MQTT broker of the reader being added.
    ReaderAddress,
    /// The file the file reader being added watches.
    ReaderPath,
}

impl TextKind {
    /// The rename screen's persisted purpose for this entry.
    pub fn token(&self) -> String {
        match self {
            Self::ProfileName(id) => format!("profile_name:{id}"),
            Self::NewProfile => "new_profile".to_string(),
            Self::ReaderAddress => "reader_address".to_string(),
            Self::ReaderPath => "reader_path".to_string(),
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        if let Some(id) = token.strip_prefix("profile_name:") {
            return Some(Self::ProfileName(id.to_string()));
        }
        Some(match token {
            "new_profile" => Self::NewProfile,
            "reader_address" => Self::ReaderAddress,
            "reader_path" => Self::ReaderPath,
            _ => return None,
        })
    }

    /// The page and row the entry returns to.
    pub fn origin(&self) -> (SettingsPage, &'static str) {
        match self {
            Self::ProfileName(_) => (SettingsPage::ProfileEdit, "profileName"),
            Self::NewProfile => (SettingsPage::Profiles, "newProfile"),
            Self::ReaderAddress | Self::ReaderPath => (SettingsPage::AddReader, "addReaderType"),
        }
    }

    /// Where a cold start lands instead: a page anyone may open, since a
    /// gated page is never restored unlocked.
    pub fn restore_page(&self) -> (SettingsPage, &'static str) {
        match self {
            Self::ProfileName(_) | Self::NewProfile => (SettingsPage::Profiles, "newProfile"),
            Self::ReaderAddress | Self::ReaderPath => (SettingsPage::Readers, "addReader"),
        }
    }
}

/// Core's answers the Settings pages show.
#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent load flags, one per Core list"
)]
pub struct Model {
    pub settings: Option<SettingsResult>,
    pub network_name: String,
    /// Core's reader types, in the order the pages list them.
    pub drivers: Vec<DriverInfo>,
    /// Ports found for the reader being added.
    pub ports: Vec<PortInfo>,
    pub mappings: Option<Vec<Mapping>>,
    pub profiles: Option<Vec<Profile>>,
    pub clients: Vec<PairedClient>,
    pub limits: Option<PlaytimeLimits>,
    /// The reader the detail page is about.
    pub detail: Option<ReaderInfo>,
    pub draft: Draft,
    /// The profile the edit page is about.
    pub editing: Option<String>,
    /// An admin unlocked the gate, until Settings goes back to its root.
    pub unlocked: bool,
    /// The next gated action was let through by the gate: it runs once.
    pass: bool,
    /// The reader type whose picker is open.
    picking: Option<usize>,
    confirm: Option<Confirm>,
    pub inspector: crate::token_inspector::Model,
    /// Bumped by every local settings edit, so a `settings` answer asked
    /// for before it cannot put the old value back.
    settings_rev: u64,
    /// Settings edits Core has not answered yet.
    settings_inflight: u32,
    /// The same pair for profile edits and Core's profile list.
    profiles_rev: u64,
    profiles_inflight: u32,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("core_settings::Model")
            .field("unlocked", &self.unlocked)
            .field("editing", &self.editing)
            .finish_non_exhaustive()
    }
}

impl Model {
    /// The profile being edited is the only admin: Core will not delete it.
    fn editing_last_admin(&self) -> bool {
        let admins = self
            .profiles
            .as_ref()
            .map_or(0, |list| list.iter().filter(|p| p.is_admin()).count());
        self.editing_profile()
            .is_some_and(|p| rules::last_admin(p.is_admin(), admins))
    }

    fn editing_profile(&self) -> Option<&Profile> {
        let id = self.editing.as_deref()?;
        self.profiles.as_ref()?.iter().find(|p| p.profile_id == id)
    }

    fn defaults(driver: &DriverInfo) -> readers_rules::Defaults {
        readers_rules::Defaults {
            enabled: driver.default_enabled,
            auto_detect: driver.default_auto_detect,
            can_auto_detect: driver.can_auto_detect,
        }
    }

    fn config(&self, driver: &str) -> readers_rules::Config {
        let config = self
            .settings
            .as_ref()
            .and_then(|s| s.readers_drivers.as_ref())
            .and_then(|map| map.get(driver))
            .copied()
            .unwrap_or_default();
        readers_rules::Config {
            enabled: config.enabled,
            auto_detect: config.auto_detect,
        }
    }

    fn driver_mode(&self, driver: &DriverInfo) -> Mode {
        readers_rules::mode(self.config(&driver.id), Self::defaults(driver))
    }

    fn auto_detect_on(&self) -> bool {
        self.settings
            .as_ref()
            .and_then(|s| s.readers_auto_detect)
            .unwrap_or(true)
    }

    fn draft_driver(&self) -> Option<&DriverInfo> {
        self.drivers.iter().find(|d| d.id == self.draft.driver)
    }

    fn connections(&self) -> Vec<ReaderConnection> {
        self.settings
            .as_ref()
            .and_then(|s| s.readers_connect.clone())
            .unwrap_or_default()
    }

    fn regions(&self) -> Vec<String> {
        self.settings
            .as_ref()
            .and_then(|s| s.media_default_regions.clone())
            .unwrap_or_default()
    }

    fn langs(&self) -> Vec<String> {
        self.settings
            .as_ref()
            .and_then(|s| s.media_default_langs.clone())
            .unwrap_or_default()
    }

    fn ignored_systems(&self) -> Vec<String> {
        self.settings
            .as_ref()
            .and_then(|s| s.readers_scan_ignore_systems.clone())
            .unwrap_or_default()
    }
}

fn region_choices(model: &Model) -> Vec<String> {
    rules::game_version_choices(rules::GAME_REGIONS, &model.regions())
}

fn lang_choices(model: &Model) -> Vec<String> {
    rules::game_version_choices(rules::GAME_LANGS, &model.langs())
}

// ---------- What the rows show ----------

/// Fill the rules' inputs that come from Core.
pub(crate) fn fill_inputs(shared: &Shared, inputs: &mut rules::Inputs) {
    let core = &shared.core;
    inputs.core_settings = core.settings.is_some();
    inputs.paired_devices = core.clients.len();
    inputs.profiles = core.profiles.as_ref().map_or(0, Vec::len);
    inputs.active_profile = shared.active_profile.is_some();
    if let Some(profile) = core.editing_profile() {
        inputs.profile_has_pin = profile.has_pin;
        inputs.profile_limits_off = profile.limits_enabled == Some(false);
    }
    inputs.playtime_enabled = core.limits.as_ref().is_some_and(|l| l.enabled);
    inputs.game_regions = region_choices(core).len();
    inputs.game_langs = lang_choices(core).len();
    let settings = core.settings.as_ref();
    let detail = core.detail.as_ref().map(|r| readers_rules::Detail {
        source: if r.source == "connect" {
            readers_rules::Source::Manual
        } else {
            readers_rules::Source::Auto
        },
        has_path: !r.path.is_empty(),
        has_error: r.error.as_deref().is_some_and(|e| !e.is_empty()),
    });
    let draft = core.draft_driver();
    inputs.readers = readers_rules::Inputs {
        loaded: settings.is_some(),
        entries: shared.readers.len(),
        hold_mode: settings
            .and_then(|s| s.readers_scan_mode.as_deref())
            .is_some_and(|m| m == "hold"),
        drivers: core.drivers.len(),
        display_drivers: core.drivers.iter().filter(|d| d.display).count(),
        ports: core.ports.len(),
        draft_path: !core.draft.path.is_empty(),
        id_sources: draft.is_some_and(|d| !d.id_sources.is_empty()),
        detail: detail.unwrap_or_default(),
        guard_on: settings
            .and_then(|s| s.launch_guard_enabled)
            .unwrap_or(false),
        guard_scan_again: !settings
            .and_then(|s| s.launch_guard_require_confirm)
            .unwrap_or(false),
        mappings_loaded: core.mappings.is_some(),
        mappings: core.mappings.as_ref().map_or(0, Vec::len),
        systems: shared.systems.len(),
        ..readers_rules::Inputs::default()
    };
    crate::token_inspector::fill_inputs(shared, &mut inputs.readers);
}

/// A toggle row that is Core's: whether it is on.
pub(crate) fn checked(shared: &Shared, id: &str) -> Option<bool> {
    let core = &shared.core;
    let s = core.settings.as_ref();
    let flag = |f: fn(&SettingsResult) -> Option<bool>, default: bool| {
        Some(s.and_then(f).unwrap_or(default))
    };
    match id {
        "debugLogging" => Some(rules::debug_logging_on(
            shared.persist.settings.debug_logging,
            s.and_then(|s| s.debug_logging),
        )),
        "encryption" => flag(|s| s.encryption, false),
        "errorReporting" => flag(|s| s.error_reporting, false),
        "readersAutoDetect" => Some(core.auto_detect_on()),
        "readersScanIgnoreOnConnect" => flag(|s| s.readers_scan_ignore_on_connect, false),
        "audioScanFeedback" => flag(|s| s.audio_scan_feedback, true),
        "launchGuardEnabled" => flag(|s| s.launch_guard_enabled, false),
        // Core's setting is the opposite: it turns the second scan off.
        "launchGuardScanAgain" => Some(!flag(|s| s.launch_guard_require_confirm, false)?),
        "profilesRequireForLaunch" => flag(|s| s.profiles_require_for_launch, false),
        "playtimeEnabled" => Some(core.limits.as_ref().is_some_and(|l| l.enabled)),
        _ => None,
    }
}

/// A picker or info row that is Core's: its canonical value.
pub(crate) fn current_value(shared: &Shared, id: &str) -> Option<String> {
    let core = &shared.core;
    let s = core.settings.as_ref();
    let seconds = |v: Option<f32>, default: f32| readers_rules::seconds_token(v.unwrap_or(default));
    let profile = core.editing_profile();
    Some(match id {
        "readersScanMode" => s
            .and_then(|s| s.readers_scan_mode.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "tap".to_string()),
        "readersScanExitDelay" => seconds(s.and_then(|s| s.readers_scan_exit_delay), 0.0),
        "launchGuardTimeout" => seconds(s.and_then(|s| s.launch_guard_timeout), 15.0),
        "launchGuardDelay" => seconds(s.and_then(|s| s.launch_guard_delay), 0.0),
        "profileRole" => profile
            .map_or("member", |p| if p.is_admin() { "admin" } else { "member" })
            .to_string(),
        "profileLimits" => rules::limit_mode_token(profile.and_then(|p| p.limits_enabled)).into(),
        "profileDailyLimit" => rules::limit_token(profile.and_then(|p| p.daily_limit.as_deref())),
        "profileSessionLimit" => {
            rules::limit_token(profile.and_then(|p| p.session_limit.as_deref()))
        }
        "playtimeDaily" => rules::limit_token(Some(
            core.limits
                .as_ref()
                .and_then(|l| l.daily.as_deref())
                .unwrap_or("0"),
        )),
        "playtimeSession" => rules::limit_token(Some(
            core.limits
                .as_ref()
                .and_then(|l| l.session.as_deref())
                .unwrap_or("0"),
        )),
        "profilePin" => if profile.is_some_and(|p| p.has_pin) {
            "set"
        } else {
            "none"
        }
        .into(),
        "addReaderIdSource" => {
            let sources = core
                .draft_driver()
                .map(|d| d.id_sources.clone())
                .unwrap_or_default();
            if sources.contains(&core.draft.id_source) {
                core.draft.id_source.clone()
            } else {
                sources.first().cloned().unwrap_or_default()
            }
        }
        "addReaderDriver" => core.draft.driver.clone(),
        "readerDriver" => core
            .detail
            .as_ref()
            .map(|r| r.driver.clone())
            .unwrap_or_default(),
        "readerWrite" => if core
            .detail
            .as_ref()
            .is_some_and(|r| r.capabilities.iter().any(|c| c == "write"))
        {
            "yes"
        } else {
            "no"
        }
        .into(),
        "readerScanMode" => core
            .detail
            .as_ref()
            .map(|r| r.scan_mode.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "tap".to_string()),
        _ => return crate::token_inspector::current_value(shared, id),
    })
}

/// The text a Core row names its value with, as written: a name, a path,
/// an error.
/// A row Core would refuse: deleting the only admin profile or making it
/// a member, removing an admin's PIN, or making a profile without a PIN
/// an admin. Drawn muted with its reason, and no input acts on it.
pub(crate) fn refused(shared: &Shared, id: &str) -> bool {
    let core = &shared.core;
    match id {
        "deleteProfile" => core.editing_last_admin(),
        "profileRole" => {
            core.editing_last_admin()
                || core
                    .editing_profile()
                    .is_some_and(|p| !p.is_admin() && !p.has_pin)
        }
        "profileClearPin" => core.editing_profile().is_some_and(Profile::is_admin),
        _ => false,
    }
}

/// The driver a field row is about, so the view can name it in the
/// user's language; Core's description stays the fallback.
pub(crate) fn field_driver(shared: &Shared, id: &str) -> Option<String> {
    let core = &shared.core;
    match id {
        "addReaderDriver" => core.draft_driver().map(|d| d.id.clone()),
        "readerDriver" => core.detail.as_ref().map(|r| r.driver.clone()),
        _ => crate::token_inspector::field_driver(shared, id),
    }
}

pub(crate) fn value_name(shared: &Shared, id: &str) -> Option<SharedString> {
    let core = &shared.core;
    Some(match id {
        "pairDevice" => core.network_name.as_str().into(),
        "profileName" => core
            .editing_profile()
            .map_or("", |p| p.name.as_str())
            .into(),
        "addReaderPath" => core.draft.path.as_str().into(),
        "addReaderDriver" => core
            .draft_driver()
            .map_or("", |d| d.description.as_str())
            .into(),
        "readerDriver" => {
            let driver = core.detail.as_ref().map_or("", |r| r.driver.as_str());
            core.drivers
                .iter()
                .find(|d| d.id == driver)
                .map_or("", |d| d.description.as_str())
                .into()
        }
        "readerPort" => core.detail.as_ref().map_or("", |r| r.path.as_str()).into(),
        "readerError" => core
            .detail
            .as_ref()
            .and_then(|r| r.error.as_deref())
            .unwrap_or("")
            .into(),
        _ => return crate::token_inspector::value_name(shared, id),
    })
}

/// A picker row whose options are Core's.
pub(crate) fn options(shared: &Shared, id: &str) -> Option<Vec<String>> {
    match id {
        "addReaderIdSource" => Some(
            shared
                .core
                .draft_driver()
                .map(|d| d.id_sources.clone())
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// An item row of one of Core's lists.
#[allow(clippy::too_many_lines, reason = "one arm per kind of Core list row")]
pub(crate) fn fill_item(shared: &Shared, row: Row, out: &mut SettingsRow) {
    let Some(index) = row.item() else { return };
    let core = &shared.core;
    out.item = i32::try_from(index).unwrap_or(0);
    let text = |s: &str| SharedString::from(s);
    match row.id() {
        "connectedReader" => {
            let Some(reader) = shared.readers.get(index) else {
                return;
            };
            out.driver = text(&reader.driver);
            out.title = text(
                core.drivers
                    .iter()
                    .find(|d| d.id == reader.driver)
                    .map_or(reader.driver.as_str(), |d| d.description.as_str()),
            );
            if reader.connected {
                out.value = "connected".into();
                // The port says which one it is; Core's info mostly
                // repeats the driver the title already names.
                out.text = text(if reader.path.is_empty() {
                    &reader.info
                } else {
                    &reader.path
                });
            } else {
                out.value = "notConnected".into();
            }
        }
        "addReaderType" => {
            let Some(driver) = core.drivers.get(index) else {
                return;
            };
            out.driver = text(&driver.id);
            out.title = text(&driver.description);
        }
        "addReaderPort" => {
            let Some(port) = core.ports.get(index) else {
                return;
            };
            out.title = text(&port.path);
            out.text = text(port.description.as_deref().unwrap_or(""));
        }
        "readerType" => {
            let Some(driver) = core.drivers.get(index) else {
                return;
            };
            out.driver = text(&driver.id);
            out.title = text(&driver.description);
            out.value = core.driver_mode(driver).token().into();
        }
        "mapping" => {
            let Some(mapping) = core.mappings.as_ref().and_then(|m| m.get(index)) else {
                return;
            };
            out.title = text(if mapping.label.is_empty() {
                &mapping.pattern
            } else {
                &mapping.label
            });
            if mapping.enabled {
                out.text = text(&mapping.r#override);
            } else {
                out.value = "off".into();
            }
            if mapping.read_only {
                out.value = "file".into();
            }
        }
        "ignoredSystem" => {
            let Some(system) = shared.systems.get(index) else {
                return;
            };
            out.title = text(&system.name);
            out.checked = core.ignored_systems().contains(&system.id);
        }
        "pairedClient" => {
            let Some(client) = core.clients.get(index) else {
                return;
            };
            out.title = text(&client.client_name);
            let admins = core.clients.iter().filter(|c| c.role == "admin").count();
            out.value = if rules::last_admin(client.role == "admin", admins) {
                "lastAdmin".into()
            } else {
                text(&client.role)
            };
        }
        "profile" => {
            let Some(profile) = core.profiles.as_ref().and_then(|p| p.get(index)) else {
                return;
            };
            out.title = text(&profile.name);
            let active = shared
                .active_profile
                .as_ref()
                .is_some_and(|a| a.profile_id == profile.profile_id);
            out.value = if active {
                "active"
            } else if profile.is_admin() {
                "admin"
            } else {
                ""
            }
            .into();
        }
        "gameRegion" | "gameLang" => {
            let (choices, current, defaults) = if row.id() == "gameRegion" {
                (
                    region_choices(core),
                    core.regions(),
                    rules::DEFAULT_GAME_REGIONS,
                )
            } else {
                (lang_choices(core), core.langs(), rules::DEFAULT_GAME_LANGS)
            };
            let Some(value) = choices.get(index) else {
                return;
            };
            out.value = text(value);
            out.checked = rules::effective_versions(&current, defaults).contains(value);
            if rules::last_version(&current, defaults, value) {
                out.enabled = false;
            }
        }
        _ => {}
    }
}

// ---------- Fetching ----------

/// Ask Core for something, then store it and redraw the page.
fn fetch<T, Fut>(
    ctx: &Ctx,
    app: &App,
    what: &'static str,
    call: Fut,
    apply: impl FnOnce(&mut Shared, T) + Send + 'static,
) where
    T: Send + 'static,
    Fut: Future<Output = Result<T, ClientError>> + Send + 'static,
{
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        match call.await {
            Ok(value) => {
                apply(&mut lock(&ctx.shared), value);
                let _ = weak.upgrade_in_event_loop(move |app| changed(&ctx, &app));
            }
            Err(error) => tracing::warn!("{what} failed: {}", error.message),
        }
    });
}

/// Core's answer landed: redraw, keeping the cursor on a row it can use.
fn changed(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_active_screen() != crate::Screen::Settings {
        return;
    }
    crate::settings::reseat(ctx, app);
    crate::settings::refresh(ctx, app);
}

pub(crate) fn fetch_settings(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let rev = lock(&ctx.shared).core.settings_rev;
    fetch(
        ctx,
        app,
        "settings",
        async move { client.settings().await },
        move |s, r| {
            // An edit made since, or one still on its way, is newer than
            // this answer: the page keeps showing it.
            if s.core.settings_rev == rev && s.core.settings_inflight == 0 {
                s.core.settings = Some(r);
            }
        },
    );
}

/// Core's settings from the start-up read.
pub(crate) fn observe_settings(shared: &mut Shared, result: &SettingsResult) {
    shared.core.settings = Some(result.clone());
}

fn fetch_drivers(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    fetch(
        ctx,
        app,
        "readers.drivers",
        async move { client.readers_drivers().await },
        |s, r| {
            let mut drivers = r.drivers;
            drivers.retain(|d| readers_rules::listed(&d.id));
            readers_rules::sort_drivers(&mut drivers, |d| d.id.as_str(), |d| d.display);
            s.core.drivers = drivers;
        },
    );
}

fn fetch_readers(ctx: &Ctx, app: &App) {
    crate::router::refresh_readers(ctx, app);
}

pub(crate) fn fetch_profiles(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let rev = lock(&ctx.shared).core.profiles_rev;
    fetch(
        ctx,
        app,
        "profiles",
        async move { client.profiles().await },
        move |s, r| {
            if s.core.profiles_rev == rev && s.core.profiles_inflight == 0 {
                s.core.profiles = Some(r.profiles);
            }
        },
    );
}

fn fetch_limits(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    fetch(
        ctx,
        app,
        "playtime limits",
        async move { client.playtime_limits().await },
        |s, r| {
            s.core.limits = Some(r);
        },
    );
}

fn fetch_clients(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    fetch(
        ctx,
        app,
        "clients",
        async move { client.clients().await },
        |s, r| {
            s.core.clients = r.clients;
        },
    );
}

pub(crate) fn fetch_mappings(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    fetch(
        ctx,
        app,
        "mappings",
        async move { client.mappings().await },
        |s, r| {
            s.core.mappings = Some(r.mappings);
        },
    );
}

fn fetch_version(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    fetch(
        ctx,
        app,
        "version",
        async move { client.version().await },
        |s, r| {
            s.core.network_name = r.network_name;
        },
    );
}

/// A page opens: ask Core for what it shows.
pub(crate) fn on_open(ctx: &Ctx, app: &App, page: SettingsPage) {
    match page {
        SettingsPage::Root => {
            {
                let mut shared = lock(&ctx.shared);
                shared.core.unlocked = false;
                shared.core.pass = false;
            }
            crate::pin::cancel_pending();
        }
        SettingsPage::Readers => {
            fetch_settings(ctx, app);
            fetch_readers(ctx, app);
            fetch_drivers(ctx, app);
        }
        SettingsPage::ReaderTypes | SettingsPage::AddReader => {
            fetch_settings(ctx, app);
            fetch_drivers(ctx, app);
        }
        SettingsPage::Library
        | SettingsPage::GameVersions
        | SettingsPage::LaunchGuard
        | SettingsPage::IgnoredSystems => fetch_settings(ctx, app),
        SettingsPage::About => {
            fetch_settings(ctx, app);
            fetch_version(ctx, app);
        }
        SettingsPage::PairedDevices => fetch_clients(ctx, app),
        SettingsPage::Profiles | SettingsPage::PlaytimeLimits => {
            fetch_profiles(ctx, app);
            fetch_limits(ctx, app);
            fetch_settings(ctx, app);
        }
        SettingsPage::ProfileEdit => fetch_profiles(ctx, app),
        SettingsPage::Mappings => fetch_mappings(ctx, app),
        SettingsPage::TokenInspector => crate::token_inspector::open(ctx, app),
        _ => {}
    }
}

/// Leaving a page: the inspector lets `ZapScript` run again.
pub(crate) fn on_leave(ctx: &Ctx, page: SettingsPage) {
    if page == SettingsPage::TokenInspector {
        crate::token_inspector::close(ctx);
    }
}

/// Settings went away: the gate locks again.
pub(crate) fn left_settings(ctx: &Ctx) {
    let mut shared = lock(&ctx.shared);
    shared.core.unlocked = false;
    shared.core.pass = false;
    drop(shared);
    crate::pin::cancel_pending();
}

// ---------- The admin gate ----------

/// Whether an action on a gated row or page may run now. When it may not,
/// the PIN panel opens and `retry` runs once an admin unlocks it.
pub(crate) fn admitted(ctx: &Ctx, app: &App, retry: impl FnOnce(&Ctx, &App) + 'static) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        // The one-shot pass is spent whatever else lets this action in,
        // so it can never carry over to a later one.
        let pass = std::mem::take(&mut shared.core.pass);
        if shared.core.unlocked || pass {
            return true;
        }
    }
    crate::pin::gate(ctx, app, move |ctx, app| {
        lock(&ctx.shared).core.pass = true;
        retry(ctx, app);
    });
    false
}

// ---------- Changing Core's settings ----------

/// Show a change at once and send it. A refusal reports and fetches
/// Core's settings again, so the row shows what Core really holds.
fn update(
    ctx: &Ctx,
    app: &App,
    params: UpdateSettingsParams,
    local: impl FnOnce(&mut SettingsResult),
) {
    {
        let mut shared = lock(&ctx.shared);
        let core = &mut shared.core;
        core.settings_rev = core.settings_rev.wrapping_add(1);
        core.settings_inflight += 1;
        if let Some(settings) = core.settings.as_mut() {
            local(settings);
        }
    }
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.settings_update(params).await;
        {
            let mut shared = lock(&ctx2.shared);
            shared.core.settings_inflight = shared.core.settings_inflight.saturating_sub(1);
        }
        if let Err(error) = result {
            tracing::warn!("settings.update failed: {}", error.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                crate::router::report_action_error(&ctx2, &app, "setting", "");
                fetch_settings(&ctx2, &app);
            });
        }
    });
}

/// Core's own debug logging, which the one Debug logging row turns on
/// and off with the frontend's.
pub(crate) fn set_debug_logging(ctx: &Ctx, app: &App, on: bool) {
    if lock(&ctx.shared).core.settings.is_none() {
        return;
    }
    update(
        ctx,
        app,
        UpdateSettingsParams {
            debug_logging: Some(on),
            ..UpdateSettingsParams::default()
        },
        |s| s.debug_logging = Some(on),
    );
}

/// Flip a toggle row that is Core's. False when the row is not one.
pub(crate) fn toggle(ctx: &Ctx, app: &App, id: &str) -> bool {
    let Some(on) = checked(&lock(&ctx.shared), id).map(|v| !v) else {
        return false;
    };
    let mut p = UpdateSettingsParams::default();
    let local: fn(&mut SettingsResult, bool) = match id {
        "encryption" => {
            p.encryption = Some(on);
            |s, on| s.encryption = Some(on)
        }
        "errorReporting" => {
            p.error_reporting = Some(on);
            |s, on| s.error_reporting = Some(on)
        }
        "readersAutoDetect" => {
            p.readers_auto_detect = Some(on);
            |s, on| s.readers_auto_detect = Some(on)
        }
        "readersScanIgnoreOnConnect" => {
            p.readers_scan_ignore_on_connect = Some(on);
            |s, on| s.readers_scan_ignore_on_connect = Some(on)
        }
        "audioScanFeedback" => {
            p.audio_scan_feedback = Some(on);
            |s, on| s.audio_scan_feedback = Some(on)
        }
        "launchGuardEnabled" => {
            p.launch_guard_enabled = Some(on);
            |s, on| s.launch_guard_enabled = Some(on)
        }
        "launchGuardScanAgain" => {
            p.launch_guard_require_confirm = Some(!on);
            |s, on| s.launch_guard_require_confirm = Some(!on)
        }
        "profilesRequireForLaunch" => {
            p.profiles_require_for_launch = Some(on);
            |s, on| s.profiles_require_for_launch = Some(on)
        }
        "playtimeEnabled" => {
            update_limits(
                ctx,
                app,
                UpdatePlaytimeLimitsParams {
                    enabled: Some(on),
                    ..UpdatePlaytimeLimitsParams::default()
                },
                move |l| l.enabled = on,
            );
            return true;
        }
        // Debug logging is the frontend's own row too; settings.rs owns it.
        _ => return false,
    };
    update(ctx, app, p, move |s| local(s, on));
    true
}

fn update_limits(
    ctx: &Ctx,
    app: &App,
    params: UpdatePlaytimeLimitsParams,
    local: impl FnOnce(&mut PlaytimeLimits),
) {
    if let Some(limits) = lock(&ctx.shared).core.limits.as_mut() {
        local(limits);
    }
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.playtime_limits_update(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = result {
                tracing::warn!("playtime limits update failed: {}", error.message);
                crate::router::report_action_error(&ctx2, &app, "setting", "");
            }
            fetch_limits(&ctx2, &app);
        });
    });
}

/// A picker row that is Core's was picked. False when it is not one.
pub(crate) fn picker_selected(ctx: &Ctx, app: &App, id: &str, value: &str) -> bool {
    let seconds = readers_rules::seconds_value(value);
    let mut p = UpdateSettingsParams::default();
    match id {
        "readersScanMode" => {
            p.readers_scan_mode = Some(value.to_string());
            let v = value.to_string();
            update(ctx, app, p, move |s| s.readers_scan_mode = Some(v));
        }
        "readersScanExitDelay" => {
            p.readers_scan_exit_delay = seconds;
            update(ctx, app, p, move |s| s.readers_scan_exit_delay = seconds);
        }
        "launchGuardTimeout" => {
            p.launch_guard_timeout = seconds;
            update(ctx, app, p, move |s| s.launch_guard_timeout = seconds);
        }
        "launchGuardDelay" => {
            p.launch_guard_delay = seconds;
            update(ctx, app, p, move |s| s.launch_guard_delay = seconds);
        }
        "readerType" => {
            let index = lock(&ctx.shared).core.picking.take();
            if let (Some(index), Some(mode)) = (index, Mode::parse(value)) {
                set_driver_mode(ctx, app, index, mode);
            }
        }
        "addReaderIdSource" => {
            lock(&ctx.shared).core.draft.id_source = value.to_string();
            crate::settings::refresh(ctx, app);
        }
        "playtimeDaily" | "playtimeSession" => {
            let duration = rules::limit_duration(value);
            let mut params = UpdatePlaytimeLimitsParams::default();
            if id == "playtimeDaily" {
                params.daily = Some(duration.clone());
            } else {
                params.session = Some(duration.clone());
            }
            let daily = id == "playtimeDaily";
            update_limits(ctx, app, params, move |l| {
                if daily {
                    l.daily = Some(duration);
                } else {
                    l.session = Some(duration);
                }
            });
        }
        "profileRole" | "profileLimits" | "profileDailyLimit" | "profileSessionLimit" => {
            update_profile_field(ctx, app, id, value);
        }
        _ => return false,
    }
    true
}

fn set_driver_mode(ctx: &Ctx, app: &App, index: usize, mode: Mode) {
    let Some(driver) = lock(&ctx.shared).core.drivers.get(index).cloned() else {
        return;
    };
    let config = readers_rules::config_for(mode, Model::defaults(&driver));
    let config = DriverConfig {
        enabled: config.enabled,
        auto_detect: config.auto_detect,
    };
    let id = driver.id.clone();
    let params = UpdateSettingsParams {
        readers_drivers: Some(BTreeMap::from([(id.clone(), config)])),
        ..UpdateSettingsParams::default()
    };
    update(ctx, app, params, move |s| {
        s.readers_drivers
            .get_or_insert_with(BTreeMap::new)
            .insert(id, config);
    });
}

/// The reader type picker: Off, Manual only and Auto-detect, the default
/// marked, Auto-detect unavailable where it cannot work.
fn open_reader_type(ctx: &Ctx, app: &App, index: usize) {
    let (entries, current) = {
        let mut shared = lock(&ctx.shared);
        let core = &mut shared.core;
        let Some(driver) = core.drivers.get(index).cloned() else {
            return;
        };
        core.picking = Some(index);
        let defaults = Model::defaults(&driver);
        let auto_on = core.auto_detect_on();
        let current = core.driver_mode(&driver);
        let default = readers_rules::default_mode(defaults);
        let entries: Vec<crate::MenuEntry> = Mode::ALL
            .iter()
            .map(|&mode| {
                let enabled = readers_rules::mode_available(mode, defaults, auto_on);
                let reason = if enabled {
                    ""
                } else if defaults.can_auto_detect {
                    "auto_detect_off"
                } else {
                    "auto_detect_unsupported"
                };
                crate::MenuEntry {
                    id: mode.token().into(),
                    detail_key: if mode == default {
                        "setting:default"
                    } else {
                        ""
                    }
                    .into(),
                    enabled,
                    reason_key: reason.into(),
                    ..crate::MenuEntry::default()
                }
            })
            .collect();
        let current = Mode::ALL.iter().position(|&m| m == current).unwrap_or(0);
        (entries, current)
    };
    crate::settings::present_picker(ctx, app, "readerType", entries, current);
}

// ---------- Profiles ----------

fn update_profile(
    ctx: &Ctx,
    app: &App,
    params: UpdateProfileParams,
    then: impl FnOnce(&Ctx, &App) + Send + 'static,
) {
    {
        let mut shared = lock(&ctx.shared);
        let core = &mut shared.core;
        core.profiles_rev = core.profiles_rev.wrapping_add(1);
        core.profiles_inflight += 1;
    }
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.profiles_update(params).await;
        {
            let mut shared = lock(&ctx2.shared);
            shared.core.profiles_inflight = shared.core.profiles_inflight.saturating_sub(1);
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            match result {
                Ok(()) => then(&ctx2, &app),
                Err(error) => {
                    tracing::warn!("profiles.update failed: {}", error.message);
                    crate::router::report_action_error(&ctx2, &app, "setting", "");
                }
            }
            fetch_profiles(&ctx2, &app);
        });
    });
}

/// Create a profile. The first one comes with its admin PIN and the PIN
/// panel's ticket, and Core's refusal shows in that panel.
pub(crate) fn create_profile(ctx: &Ctx, app: &App, name: String, pin: Option<(String, u64)>) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let (pin, ticket) = pin.unzip();
        let result = client
            .profiles_new(NewProfileParams {
                name,
                pin,
                ..NewProfileParams::default()
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let error = result.err().map(|e| {
                tracing::warn!("profiles.new failed: {}", e.message);
                e.message
            });
            match ticket {
                Some(ticket) => crate::pin::pin_saved(&ctx2, &app, ticket, error),
                None if error.is_some() => {
                    crate::router::report_action_error(&ctx2, &app, "setting", "");
                }
                None => {}
            }
            fetch_profiles(&ctx2, &app);
        });
    });
}

/// The PIN panel's new PIN for a profile: Core keeps it, and the panel
/// shows Core's refusal in place if it has one.
pub(crate) fn save_pin(ctx: &Ctx, app: &App, profile_id: String, pin: String, ticket: u64) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client
            .profiles_update(UpdateProfileParams {
                profile_id,
                pin: Some(pin),
                ..UpdateProfileParams::default()
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let error = result.err().map(|e| {
                tracing::warn!("profiles.update failed: {}", e.message);
                e.message
            });
            crate::pin::pin_saved(&ctx2, &app, ticket, error);
            fetch_profiles(&ctx2, &app);
        });
    });
}

/// A profile's role or limits picked. A limit set back to Default clears
/// the profile's overrides and sends the ones it keeps: Core clears first,
/// then applies what is given.
fn update_profile_field(ctx: &Ctx, app: &App, id: &str, value: &str) {
    let Some(profile) = lock(&ctx.shared).core.editing_profile().cloned() else {
        return;
    };
    let mut params = UpdateProfileParams {
        profile_id: profile.profile_id.clone(),
        ..UpdateProfileParams::default()
    };
    let mut limits_enabled = profile.limits_enabled;
    let mut daily = profile.daily_limit.clone();
    let mut session = profile.session_limit.clone();
    let limit = |token: &str| (token != "default").then(|| rules::limit_duration(token));
    match id {
        "profileRole" => {
            params.role = Some(value.to_string());
            update_profile(ctx, app, params, |_, _| {});
            return;
        }
        "profileLimits" => {
            limits_enabled = match value {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            };
        }
        "profileDailyLimit" => daily = limit(value),
        "profileSessionLimit" => session = limit(value),
        _ => return,
    }
    let cleared = (profile.limits_enabled.is_some() && limits_enabled.is_none())
        || (profile.daily_limit.is_some() && daily.is_none())
        || (profile.session_limit.is_some() && session.is_none());
    // The next edit builds on this one, not on the list Core last sent.
    if let Some(edited) = lock(&ctx.shared)
        .core
        .profiles
        .as_mut()
        .and_then(|list| list.iter_mut().find(|p| p.profile_id == profile.profile_id))
    {
        edited.limits_enabled = limits_enabled;
        edited.daily_limit.clone_from(&daily);
        edited.session_limit.clone_from(&session);
    }
    params.clear_limits = cleared;
    params.limits_enabled = limits_enabled;
    params.daily_limit = daily;
    params.session_limit = session;
    crate::settings::refresh(ctx, app);
    update_profile(ctx, app, params, |_, _| {});
}

fn open_profile(ctx: &Ctx, app: &App, index: usize) {
    let id = lock(&ctx.shared)
        .core
        .profiles
        .as_ref()
        .and_then(|p| p.get(index))
        .map(|p| p.profile_id.clone());
    if let Some(id) = id {
        lock(&ctx.shared).core.editing = Some(id);
        crate::settings::navigate_page(ctx, app, SettingsPage::ProfileEdit);
    }
}

/// A name typed on the rename screen, or `None` when it was left.
pub(crate) fn text_entered(ctx: &Ctx, app: &App, kind: &TextKind, text: Option<String>) {
    let (page, row) = kind.origin();
    let Some(text) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
        crate::settings::show_return(ctx, app, page, row);
        return;
    };
    match kind {
        TextKind::ProfileName(id) => {
            let params = UpdateProfileParams {
                profile_id: id.clone(),
                name: Some(text),
                ..UpdateProfileParams::default()
            };
            update_profile(ctx, app, params, |_, _| {});
            crate::settings::show_return(ctx, app, page, row);
        }
        TextKind::NewProfile => {
            let first = lock(&ctx.shared)
                .core
                .profiles
                .as_ref()
                .is_none_or(Vec::is_empty);
            crate::settings::show_return(ctx, app, page, row);
            if first {
                crate::pin::new_admin(ctx, app, text);
            } else {
                create_profile(ctx, app, text, None);
            }
        }
        TextKind::ReaderAddress | TextKind::ReaderPath => {
            lock(&ctx.shared).core.draft.path = text;
            crate::settings::show_return(ctx, app, SettingsPage::AddReaderOptions, "saveReader");
        }
    }
}

/// Where a rename-screen entry restored after a kill lands.
pub(crate) fn restore_text(ctx: &Ctx, app: &App, kind: &TextKind) {
    let (page, row) = kind.restore_page();
    crate::settings::show_return(ctx, app, page, row);
}

// ---------- Readers ----------

fn choose_driver(ctx: &Ctx, app: &App, index: usize) {
    let Some(driver) = lock(&ctx.shared).core.drivers.get(index).cloned() else {
        return;
    };
    {
        let mut shared = lock(&ctx.shared);
        shared.core.draft = Draft {
            driver: driver.id.clone(),
            id_source: driver.id_sources.first().cloned().unwrap_or_default(),
            ..Draft::default()
        };
        shared.core.ports.clear();
    }
    match readers_rules::first_step(readers_rules::PathKind::parse(&driver.path_kind)) {
        readers_rules::AddStep::Port => {
            let client = ctx.store.client();
            let id = driver.id.clone();
            fetch(
                ctx,
                app,
                "readers.ports",
                async move { client.readers_ports(&id).await },
                |s, r| {
                    s.core.ports = r.ports;
                },
            );
            crate::settings::navigate_page(ctx, app, SettingsPage::AddReaderPort);
        }
        readers_rules::AddStep::Entry => {
            let kind = match readers_rules::entry_for(&driver.id) {
                readers_rules::Entry::Address => TextKind::ReaderAddress,
                readers_rules::Entry::Path => TextKind::ReaderPath,
            };
            crate::rename::open_text(ctx, app, kind, "");
        }
        readers_rules::AddStep::Options => {
            crate::settings::navigate_page(ctx, app, SettingsPage::AddReaderOptions);
        }
    }
}

fn save_reader(ctx: &Ctx, app: &App) {
    let (mut list, draft) = {
        let shared = lock(&ctx.shared);
        (shared.core.connections(), shared.core.draft.clone())
    };
    let entry = ReaderConnection {
        enabled: None,
        driver: draft.driver.clone(),
        path: draft.path.clone(),
        id_source: draft.id_source.clone(),
        scan_mode: String::new(),
    };
    readers_rules::add_connection(&mut list, entry, |c| (c.driver.as_str(), c.path.as_str()));
    set_connections(ctx, app, list);
    crate::settings::return_to_row(ctx, app, SettingsPage::Readers, "addReader");
    fetch_readers(ctx, app);
}

fn set_connections(ctx: &Ctx, app: &App, list: Vec<ReaderConnection>) {
    let params = UpdateSettingsParams {
        readers_connect: Some(list.clone()),
        ..UpdateSettingsParams::default()
    };
    update(ctx, app, params, move |s| s.readers_connect = Some(list));
}

fn remove_reader(ctx: &Ctx, app: &App) {
    let Some(reader) = lock(&ctx.shared).core.detail.clone() else {
        return;
    };
    if reader.source == "connect" {
        let mut list = lock(&ctx.shared).core.connections();
        readers_rules::remove_connection(&mut list, &reader.driver, &reader.path, |c| {
            (c.driver.as_str(), c.path.as_str())
        });
        set_connections(ctx, app, list);
    } else {
        // Stop detecting this type: Manual only, nothing else changes.
        let index = lock(&ctx.shared)
            .core
            .drivers
            .iter()
            .position(|d| d.id == reader.driver);
        if let Some(index) = index {
            set_driver_mode(ctx, app, index, Mode::Manual);
        }
    }
    {
        let mut shared = lock(&ctx.shared);
        shared
            .readers
            .retain(|r| !(r.driver == reader.driver && r.path == reader.path && r.id == reader.id));
    }
    crate::settings::return_to_row(ctx, app, SettingsPage::Readers, "connectedReader");
    fetch_readers(ctx, app);
}

// ---------- Input ----------

/// Accept on a Navigate or Action row that is Core's. False when it is not
/// one.
pub(crate) fn accept(ctx: &Ctx, app: &App, id: &str) -> bool {
    match id {
        "saveReader" => save_reader(ctx, app),
        "removeReader" | "stopDetecting" => remove_reader(ctx, app),
        "leaveProfile" => crate::pin::leave(ctx, app),
        "newProfile" => crate::rename::open_text(ctx, app, TextKind::NewProfile, ""),
        "profileName" => {
            let (id, name) = {
                let shared = lock(&ctx.shared);
                let Some(p) = shared.core.editing_profile() else {
                    return true;
                };
                (p.profile_id.clone(), p.name.clone())
            };
            crate::rename::open_text(ctx, app, TextKind::ProfileName(id), &name);
        }
        "profilePin" => {
            let profile = lock(&ctx.shared).core.editing_profile().cloned();
            if let Some(profile) = profile {
                crate::pin::set_pin(ctx, app, &profile);
            }
        }
        "profileClearPin" => {
            let Some(id) = lock(&ctx.shared).core.editing.clone() else {
                return true;
            };
            let params = UpdateProfileParams {
                profile_id: id,
                clear_pin: true,
                ..UpdateProfileParams::default()
            };
            update_profile(ctx, app, params, |ctx, app| {
                crate::settings::reseat_to(ctx, app, "profilePin");
            });
        }
        "writeSwitchCard" => {
            let card = lock(&ctx.shared)
                .core
                .editing_profile()
                .map(|p| zaparoo_core::profile_types::switch_card_script(&p.switch_id));
            if let Some(card) = card {
                crate::card_write::begin(ctx, app, card);
            }
        }
        "deleteProfile" => {
            if lock(&ctx.shared).core.editing_last_admin() {
                return true;
            }
            let Some(profile) = lock(&ctx.shared).core.editing_profile().cloned() else {
                return true;
            };
            lock(&ctx.shared).core.confirm = Some(Confirm::DeleteProfile(profile.profile_id));
            crate::router::open_dialog(
                app,
                DialogKind::DeleteProfile,
                &profile.name,
                "",
                &[DialogButton::No, DialogButton::Yes],
                0,
            );
        }
        _ => return crate::token_inspector::accept(ctx, app, id),
    }
    true
}

/// Accept on an item row of one of Core's lists.
pub(crate) fn accept_item(ctx: &Ctx, app: &App, id: &str, index: usize) {
    match id {
        "connectedReader" => {
            let reader = lock(&ctx.shared).readers.get(index).cloned();
            if let Some(reader) = reader {
                lock(&ctx.shared).core.detail = Some(reader);
                crate::settings::navigate_page(ctx, app, SettingsPage::ReaderDetail);
            }
        }
        "addReaderType" => choose_driver(ctx, app, index),
        "addReaderPort" => {
            let path = lock(&ctx.shared)
                .core
                .ports
                .get(index)
                .map(|p| p.path.clone());
            if let Some(path) = path {
                lock(&ctx.shared).core.draft.path = path;
                crate::settings::navigate_page(ctx, app, SettingsPage::AddReaderOptions);
            }
        }
        "readerType" => open_reader_type(ctx, app, index),
        "mapping" => crate::mappings::open_options(ctx, app, index),
        "pairedClient" => confirm_revoke(ctx, app, index),
        "profile" => {
            let (profile, active) = {
                let shared = lock(&ctx.shared);
                let profile = shared
                    .core
                    .profiles
                    .as_ref()
                    .and_then(|p| p.get(index))
                    .cloned();
                let active = shared.active_profile.as_ref().map(|a| a.profile_id.clone());
                (profile, active)
            };
            if let Some(profile) = profile {
                if active.as_deref() != Some(profile.profile_id.as_str()) {
                    crate::pin::switch_to(ctx, app, &profile);
                }
            }
        }
        "ignoredSystem" | "gameRegion" | "gameLang" => toggle_item(ctx, app, id, index),
        _ => {}
    }
}

/// Options on an item row.
pub(crate) fn options_item(ctx: &Ctx, app: &App, id: &str, index: usize) {
    match id {
        "profile" => {
            let retry = move |ctx: &Ctx, app: &App| open_profile(ctx, app, index);
            if admitted(ctx, app, retry) {
                open_profile(ctx, app, index);
            }
        }
        "mapping" => crate::mappings::open_options(ctx, app, index),
        "pairedClient" => confirm_revoke(ctx, app, index),
        _ => {}
    }
}

/// Whether Options does something on this item row, for the help bar.
pub(crate) fn has_options(id: &str) -> bool {
    matches!(id, "profile" | "mapping" | "pairedClient")
}

/// Left, Right or Accept on a toggle item row.
pub(crate) fn toggle_item(ctx: &Ctx, app: &App, id: &str, index: usize) {
    match id {
        "ignoredSystem" => {
            let (mut list, system) = {
                let shared = lock(&ctx.shared);
                let Some(system) = shared.systems.get(index).map(|s| s.id.clone()) else {
                    return;
                };
                (shared.core.ignored_systems(), system)
            };
            readers_rules::toggle_system(&mut list, &system);
            let params = UpdateSettingsParams {
                readers_scan_ignore_systems: Some(list.clone()),
                ..UpdateSettingsParams::default()
            };
            update(ctx, app, params, move |s| {
                s.readers_scan_ignore_systems = Some(list);
            });
        }
        "gameRegion" | "gameLang" => {
            let region = id == "gameRegion";
            let (current, value, on) = {
                let shared = lock(&ctx.shared);
                let core = &shared.core;
                let (choices, current, defaults) = if region {
                    (
                        region_choices(core),
                        core.regions(),
                        rules::DEFAULT_GAME_REGIONS,
                    )
                } else {
                    (lang_choices(core), core.langs(), rules::DEFAULT_GAME_LANGS)
                };
                let Some(value) = choices.get(index).cloned() else {
                    return;
                };
                let on = !rules::effective_versions(&current, defaults).contains(&value);
                (current, value, on)
            };
            let defaults = if region {
                rules::DEFAULT_GAME_REGIONS
            } else {
                rules::DEFAULT_GAME_LANGS
            };
            if !on && rules::last_version(&current, defaults, &value) {
                return;
            }
            let next = rules::toggle_version(&current, defaults, &value, on);
            let mut params = UpdateSettingsParams::default();
            if region {
                params.media_default_regions = Some(next.clone());
                update(ctx, app, params, move |s| {
                    s.media_default_regions = Some(next);
                });
            } else {
                params.media_default_langs = Some(next.clone());
                update(ctx, app, params, move |s| {
                    s.media_default_langs = Some(next);
                });
            }
        }
        _ => {}
    }
}

fn confirm_revoke(ctx: &Ctx, app: &App, index: usize) {
    let (client, admins) = {
        let shared = lock(&ctx.shared);
        let clients = &shared.core.clients;
        let admins = clients.iter().filter(|c| c.role == "admin").count();
        (clients.get(index).cloned(), admins)
    };
    let Some(client) = client else {
        return;
    };
    if rules::last_admin(client.role == "admin", admins) {
        return;
    }
    lock(&ctx.shared).core.confirm = Some(Confirm::Revoke(client.client_id));
    crate::router::open_dialog(
        app,
        DialogKind::RevokeDevice,
        &client.client_name,
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
}

/// Ask before deleting a mapping.
pub(crate) fn confirm_delete_mapping(ctx: &Ctx, app: &App, id: i64, name: &str) {
    lock(&ctx.shared).core.confirm = Some(Confirm::DeleteMapping(id));
    crate::router::open_dialog(
        app,
        DialogKind::DeleteMapping,
        name,
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
}

/// The answer to a revoke or delete dialog.
pub(crate) fn confirm_answered(ctx: &Ctx, app: &App, yes: bool) {
    let Some(confirm) = lock(&ctx.shared).core.confirm.take() else {
        return;
    };
    if !yes {
        return;
    }
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    match confirm {
        Confirm::Revoke(id) => {
            lock(&ctx.shared).core.clients.retain(|c| c.client_id != id);
            crate::settings::reseat(ctx, app);
            crate::settings::refresh(ctx, app);
            ctx.handle.spawn(async move {
                let result = client.clients_delete(&id).await;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if let Err(error) = result {
                        tracing::warn!("clients.delete failed: {}", error.message);
                        crate::router::report_action_error(&ctx2, &app, "setting", "");
                    }
                    fetch_clients(&ctx2, &app);
                });
            });
        }
        Confirm::DeleteProfile(id) => {
            {
                let mut shared = lock(&ctx.shared);
                if let Some(list) = shared.core.profiles.as_mut() {
                    list.retain(|p| p.profile_id != id);
                }
                shared.core.editing = None;
            }
            crate::settings::return_to_row(ctx, app, SettingsPage::Profiles, "newProfile");
            ctx.handle.spawn(async move {
                let result = client.profiles_delete(&id).await;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if let Err(error) = result {
                        tracing::warn!("profiles.delete failed: {}", error.message);
                        crate::router::report_action_error(&ctx2, &app, "setting", "");
                    }
                    fetch_profiles(&ctx2, &app);
                });
            });
        }
        Confirm::DeleteMapping(id) => crate::mappings::delete(ctx, app, id),
    }
}

/// Back from one of Core's pages: the page and row it returns to.
pub(crate) fn parent(page: SettingsPage) -> Option<(SettingsPage, &'static str)> {
    let (page, row) = match page {
        SettingsPage::Language => (SettingsPage::About, rules::LANGUAGE_PAGE),
        SettingsPage::PairedDevices => (SettingsPage::About, rules::PAIRED_DEVICES_PAGE),
        SettingsPage::GameVersions => (SettingsPage::Library, rules::GAME_VERSIONS_PAGE),
        SettingsPage::ProfileEdit => (SettingsPage::Profiles, "profile"),
        SettingsPage::PlaytimeLimits => (SettingsPage::Profiles, rules::PLAYTIME_LIMITS_PAGE),
        page => {
            let (parent, row) = readers_rules::parent(page.token())?;
            (SettingsPage::try_from(parent).ok()?, row)
        }
    };
    Some((page, row))
}

/// The item a page returns to, when it is one entry of a list.
pub(crate) fn parent_item(ctx: &Ctx, page: SettingsPage) -> Option<usize> {
    let shared = lock(&ctx.shared);
    match page {
        SettingsPage::ProfileEdit => {
            let id = shared.core.editing.as_deref()?;
            shared
                .core
                .profiles
                .as_ref()?
                .iter()
                .position(|p| p.profile_id == id)
        }
        SettingsPage::ReaderDetail => {
            let reader = shared.core.detail.as_ref()?;
            shared
                .readers
                .iter()
                .position(|r| r.id == reader.id && r.path == reader.path)
        }
        _ => None,
    }
}
