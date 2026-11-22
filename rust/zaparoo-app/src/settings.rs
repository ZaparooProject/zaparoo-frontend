// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Settings registry: the root categories, the rows each page
//! shows at the current runtime, what control each row carries, the
//! canonical option lists, and the cursor rules over rows that include
//! non-navigable headers. Every user-visible string stays a key the view
//! translates.

/// The control a row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// A value the user cycles or picks from a list.
    Picker,
    /// An on/off switch.
    Toggle,
    /// A one-shot the row performs (its label says what).
    Action,
    /// Opens another page or screen.
    Navigate,
    /// Reads as off, mixed, or fully on across several settings at once;
    /// Accept always drives it to fully on or fully off. Today, only the
    /// Online page's "All" features row uses this.
    TriToggle,
    /// Shows a live value and nothing else: there is nothing to change, so
    /// it never takes focus.
    Info,
    /// Reads like an info row, but takes focus, because its description
    /// says what to do about what it reports.
    Notice,
    /// A value on a scale: Left and Right step it, holding repeats.
    Slider,
    /// One of a live list of things (a Bluetooth device on the pairing
    /// page): it reads like an info row, but it takes focus, because
    /// Accept or Options acts on the thing it stands for.
    Device,
}

/// A row on a settings page: a group header, a field, or one entry of a
/// live list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A non-navigable group label; the id is its vocabulary key.
    Header(&'static str),
    Field {
        id: &'static str,
        control: Control,
    },
    /// Entry `index` of a list the host holds (the connected controllers,
    /// the Bluetooth devices). Every entry of one list shares the id, its
    /// kind; the index says which one, in the host's own order, so a
    /// page's static rows never move when the list grows or shrinks.
    Item {
        id: &'static str,
        index: usize,
        control: Control,
    },
}

impl Row {
    pub fn id(self) -> &'static str {
        match self {
            Self::Header(id) | Self::Field { id, .. } | Self::Item { id, .. } => id,
        }
    }

    /// A row that is a setting or an entry, not a group heading.
    pub fn is_field(self) -> bool {
        matches!(self, Self::Field { .. } | Self::Item { .. })
    }

    /// The row's control; a heading has none.
    pub fn control(self) -> Option<Control> {
        match self {
            Self::Header(_) => None,
            Self::Field { control, .. } | Self::Item { control, .. } => Some(control),
        }
    }

    /// Which entry of its list an item row is.
    pub fn item(self) -> Option<usize> {
        match self {
            Self::Item { index, .. } => Some(index),
            _ => None,
        }
    }

    /// A row the cursor can rest on: a field with something to do.
    pub fn is_navigable(self) -> bool {
        self.control()
            .is_some_and(|control| control != Control::Info)
    }
}

/// One root category tile: the page it opens and its glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub id: &'static str,
    pub glyph: &'static str,
}

/// Every root category tile, in the order the console-style menu shows
/// it: what the collection holds and how it is browsed, how the interface
/// looks and the output it draws on, input, the readers that scan tokens,
/// who is playing, the accounts the frontend links to, then System, always
/// last.
/// The `RetroAchievements` tile shows only where `MiSTer_RA` can be set up; see
/// `root_pages`.
pub const PAGES: &[Page] = &[
    Page {
        id: "pageLibraryData",
        glyph: "icons/Library",
    },
    Page {
        id: BROWSING_PAGE,
        glyph: "icons/Browsing",
    },
    Page {
        id: "pageAppearance",
        glyph: "icons/Appearance",
    },
    Page {
        id: "pageDisplayInterface",
        glyph: "icons/Display",
    },
    Page {
        id: "pageControlsInput",
        glyph: "icons/Controls",
    },
    Page {
        id: READERS_PAGE,
        glyph: "status/NFC",
    },
    Page {
        id: PROFILES_PAGE,
        glyph: "icons/Users",
    },
    Page {
        id: "pageOnline",
        glyph: "icons/Online",
    },
    Page {
        id: RETRO_ACHIEVEMENTS_PAGE,
        glyph: "icons/Trophy",
    },
    Page {
        id: SYSTEM_PAGE,
        glyph: "icons/Settings",
    },
];

/// The readers Core scans tokens with, and what a scan does.
pub const READERS_PAGE: &str = "pageReaders";

/// Who is playing: the profile list, switching, and managing them.
pub const PROFILES_PAGE: &str = "pageProfiles";

/// System: pairing, security, language, logs, documentation and identity.
/// Its id is the old About page's, so a pinned Hub tile keeps opening it.
pub const SYSTEM_PAGE: &str = "pageSupportAbout";

/// Language and region, a row of System. No longer a root tile, but a
/// pinned Hub tile keeps opening it and the Hub still offers it.
pub const LANGUAGE_PAGE: &str = "pageLanguage";

/// Library's preferred game versions: Core's default regions and
/// languages for a game with several versions.
pub const GAME_VERSIONS_PAGE: &str = "pageGameVersions";

/// The devices paired with Core, a page of System.
pub const PAIRED_DEVICES_PAGE: &str = "pagePairedDevices";

/// One profile's settings, opened from its entry on the Profiles page.
pub const PROFILE_EDIT_PAGE: &str = "pageProfileEdit";

/// The playtime limits every profile inherits, a page of Profiles.
pub const PLAYTIME_LIMITS_PAGE: &str = "pagePlaytimeLimits";

/// The browse lists' presentation: layouts, artwork, what a list shows.
pub const BROWSING_PAGE: &str = "pageBrowsing";

/// The `RetroAchievements` account and `MiSTer_RA`'s options, a root tile
/// only where the frontend can set `MiSTer_RA` up.
pub const RETRO_ACHIEVEMENTS_PAGE: &str = "pageRetroAchievements";

/// The root tiles this machine shows, in `PAGES` order.
pub fn root_pages(retro_achievements: bool) -> impl Iterator<Item = &'static Page> {
    PAGES
        .iter()
        .filter(move |p| retro_achievements || p.id != RETRO_ACHIEVEMENTS_PAGE)
}

/// The music player's old root tile. It is a Hub action now; a tile pinned
/// while it was a Settings page keeps opening the player.
pub const MUSIC_PAGE: &str = "pageMusic";

/// The glyph of a root tile, for a pinned Hub tile that stands for it,
/// and of the pages a pinned tile can open that are not root tiles.
pub fn page_glyph(id: &str) -> Option<&'static str> {
    match id {
        MUSIC_PAGE => Some("icons/Music"),
        LANGUAGE_PAGE => Some("icons/Language"),
        _ => PAGES.iter().find(|p| p.id == id).map(|p| p.glyph),
    }
}

/// What the registry needs to know about the machine it runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent runtime capabilities"
)]
pub struct Inputs {
    /// An embedding host can grant access through its own folder chooser.
    pub can_pick_folder: bool,
    /// An embedding host can scan a folder for installed launchers.
    pub can_scan_launchers: bool,
    /// An embedding host can hand off to the system setting that lets it
    /// measure foreground playtime exactly.
    pub can_request_playtime_access: bool,
    /// Core lets this client link an Online account and set play history
    /// consent: it answered `playtimeSyncEnabled`, which it only does for
    /// local and admin clients.
    pub can_link_online: bool,
    /// This device is currently linked to an Online account: the account
    /// row shows status/unlink instead of a link prompt, and the Warp row
    /// joins it. Every other row on the page still shows either way
    /// (linking again resets them, so there is always something to see).
    pub online_linked: bool,
    /// `MiSTer`: the resolution and analog-video rows only exist there.
    pub is_mister: bool,
    /// The frontend is already running the native CRT path.
    pub crt_enabled: bool,
    /// A debug build offers the one-second screensaver for testing.
    pub debug_build: bool,
    /// Something can post the log bundle: curl when standalone, or the
    /// embedding host's own uploader.
    pub log_upload: bool,
    /// Core has a global launcher preference to edit (v2.21.0 and later).
    pub launcher_preference: bool,
    /// The frontend runs on a `MiSTer` and can set up `MiSTer_RA` there.
    pub retro_achievements: bool,
    /// A background image can show: a custom image is set, or Game
    /// background is on. The rows that only shape an image show then.
    pub background_active: bool,
    /// A background image or folder is set: the row that picks it shows
    /// then.
    pub background_custom: bool,
    /// The header logo setting is a custom image: the row that picks the
    /// image shows then.
    pub header_logo_custom: bool,
    /// `retroachievements.cfg` holds a username and password.
    pub ra_logged_in: bool,
    /// `MiSTer_RA`, a `RetroAchievements` core, and the MiSTer.ini section
    /// that switches to it are all there.
    pub ra_installed: bool,
    /// The preference names `RetroAchievements`, but some system with a
    /// `RetroAchievements` core has a default of its own that Core takes
    /// first.
    pub ra_conflict: bool,
    /// Core reports the controllers connected to it; an older Core, or a
    /// device that cannot, does not, and the Controls page has no
    /// Controllers row.
    pub controllers_listed: bool,
    /// Core can pair a Bluetooth controller for this client
    /// (`controllers::pairing_available`).
    pub can_pair_controllers: bool,
    /// How many devices the Controllers page's search has found so far
    /// (`controllers::PairSession`, which only ever appends), and how many
    /// the Paired controllers page lists.
    pub bluetooth_found: usize,
    pub bluetooth_paired: usize,
    /// The screensaver style is Bouncing or Random: the row that picks
    /// what bounces shows then.
    pub screensaver_bounces: bool,
    /// Core answered `settings`: the rows that are Core's own show then.
    pub core_settings: bool,
    /// How many devices are paired with Core (`clients`).
    pub paired_devices: usize,
    /// How many profiles Core has, and whether one is active.
    pub profiles: usize,
    pub active_profile: bool,
    /// The profile being edited has a PIN: the row that removes it shows.
    pub profile_has_pin: bool,
    /// The profile being edited has limits turned off: its own daily and
    /// session limits mean nothing then.
    pub profile_limits_off: bool,
    /// Playtime limits are on: the global limits show then.
    pub playtime_enabled: bool,
    /// How many regions and languages the preferred game versions page
    /// lists (`game_version_choices`).
    pub game_regions: usize,
    pub game_langs: usize,
    /// What the Readers pages show (`readers::Inputs`).
    pub readers: crate::readers::Inputs,
}

const TOGGLES: &[&str] = &[
    "mouseEnabled",
    "showHidden",
    "showOriginalFilenames",
    "showTags",
    "misterNames",
    "debugLogging",
    "reduceMotion",
    "chooseProfileAtStartup",
    "showMenuRow",
    "showHelpBar",
    "crtEnabled",
    "swapConfirmCancel",
    "swapOptionsView",
    "playtimeSync",
    "onlineRemoteControl",
    "onlineLibrarySync",
    "onlineCloudBackup",
    "raLaunchers",
    "raHardcore",
    "raProgressPopups",
    "raChallengePopups",
    "raLeaderboardPopups",
    "raListHotkey",
    "encryption",
    "errorReporting",
    "playtimeEnabled",
    "profilesRequireForLaunch",
];

const NAVIGATES: &[&str] = &[
    "aboutLicense",
    "credits",
    "documentation",
    "crtCalibration",
    "pageAppearance",
    "pageDisplayInterface",
    "pageLanguage",
    "pageControlsInput",
    "pageLibraryData",
    "pageBrowsing",
    "pageRetroAchievements",
    "pageOnline",
    "pageSupportAbout",
    "launcherPreference",
    "backgroundImage",
    "controllers",
    "pairedControllers",
    "headerLogoImage",
    "onlineManageBackups",
    "onlineRemoteActivity",
    "screensaver",
    LANGUAGE_PAGE,
    GAME_VERSIONS_PAGE,
    PAIRED_DEVICES_PAGE,
    PLAYTIME_LIMITS_PAGE,
    "profileName",
    "profilePin",
];

const ACTIONS: &[&str] = &[
    "detectLaunchers",
    "addGameFolder",
    "updateMediaDb",
    "runScraper",
    "pairDevice",
    "uploadLog",
    "playtimeAccess",
    "onlineLinkAccount",
    "onlineUnlinkAccount",
    "raLogIn",
    "raLogOut",
    "leaveProfile",
    "newProfile",
    "profileClearPin",
    "writeSwitchCard",
    "deleteProfile",
];

/// Rows whose current value sweeps between fully off and fully on across
/// several settings at once; see `Control::TriToggle`.
const TRI_TOGGLES: &[&str] = &["onlineAllFeatures"];

/// Rows on a scale; see `Control::Slider`.
const SLIDERS: &[&str] = &["backgroundBlur", "backgroundDim"];

/// Rows that only report a live value; see `Control::Info`.
const INFOS: &[&str] = &[
    "noPairedControllers",
    "onlineStatus",
    "onlineWarp",
    "onlineRemoteStatus",
    "raAccount",
    "noPairedDevices",
    "noProfiles",
];

/// Rows that report something to act on elsewhere; see `Control::Notice`.
const NOTICES: &[&str] = &["raInstall", "raConflict"];

/// The control a row id carries: everything that is not a toggle, a
/// navigation, a one-shot, a tri-toggle or an info row is a picker.
pub fn control(id: &str) -> Control {
    if TOGGLES.contains(&id) {
        Control::Toggle
    } else if NAVIGATES.contains(&id) {
        Control::Navigate
    } else if ACTIONS.contains(&id) {
        Control::Action
    } else if TRI_TOGGLES.contains(&id) {
        Control::TriToggle
    } else if INFOS.contains(&id) {
        Control::Info
    } else if NOTICES.contains(&id) {
        Control::Notice
    } else if SLIDERS.contains(&id) {
        Control::Slider
    } else {
        Control::Picker
    }
}

fn field(id: &'static str) -> Row {
    Row::Field {
        id,
        control: control(id),
    }
}

/// Library: getting games in and keeping the media database current.
/// Maintenance leads: these are rows a user comes here to do, not one-time
/// preferences. The launcher rows close the page.
fn library_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = Vec::new();
    if inputs.can_pick_folder {
        rows.push(field("addGameFolder"));
    }
    if inputs.can_request_playtime_access {
        rows.push(field("playtimeAccess"));
    }
    rows.extend([
        Row::Header("maintenance"),
        field("updateMediaDb"),
        field("runScraper"),
    ]);
    // Core's own preference, so it waits for Core's settings.
    if inputs.core_settings {
        rows.extend([Row::Header("preferences"), field(GAME_VERSIONS_PAGE)]);
    }
    if inputs.can_scan_launchers || inputs.launcher_preference {
        rows.push(Row::Header("launchers"));
        if inputs.can_scan_launchers {
            rows.push(field("detectLaunchers"));
        }
        if inputs.launcher_preference {
            rows.push(field("launcherPreference"));
        }
    }
    rows
}

/// Browsing: how the browse screens lay out and label what they list.
/// Layouts first, then what Left and Right do in the list a layout picks,
/// the artwork, then what a list shows.
fn browsing_rows() -> Vec<Row> {
    vec![
        field("systemsLayout"),
        field("gamesLayout"),
        field("listLeftRight"),
        field("mediaImageType"),
        field("showTags"),
        field("showOriginalFilenames"),
        field("showHidden"),
    ]
}

/// The `RetroAchievements` page, laid out like Online's. The account
/// leads: nothing else here does anything without it. Setup follows (the
/// launcher switch and what stands in its way), then the options
/// `MiSTer_RA` reads as a game starts, the popups grouped last. A section
/// with nothing to show keeps no header.
fn retro_achievements_rows(inputs: &Inputs) -> Vec<Row> {
    if !inputs.retro_achievements {
        return Vec::new();
    }
    let mut rows = vec![
        Row::Header("account"),
        field("raAccount"),
        field(if inputs.ra_logged_in {
            "raLogOut"
        } else {
            "raLogIn"
        }),
    ];
    let mut setup = Vec::new();
    if inputs.launcher_preference {
        setup.push(field("raLaunchers"));
    }
    if !inputs.ra_installed {
        setup.push(field("raInstall"));
    } else if inputs.ra_conflict {
        setup.push(field("raConflict"));
    }
    if !setup.is_empty() {
        rows.push(Row::Header("setup"));
        rows.extend(setup);
    }
    rows.extend([
        Row::Header("gameplay"),
        field("raHardcore"),
        field("raListHotkey"),
        Row::Header("popups"),
        field("raProgressPopups"),
        field("raChallengePopups"),
        field("raLeaderboardPopups"),
        field("raPopupPosition"),
    ]);
    rows
}

/// The page the Controls page's Controllers row opens, and the page its
/// Paired controllers row opens in turn: pages of the same panel, never
/// root tiles.
pub const CONTROLLERS_PAGE: &str = "pageControllers";
pub const PAIRED_PAGE: &str = "pagePairedControllers";

/// The page Appearance's Screensaver row opens: a page of the same panel,
/// never a root tile.
pub const SCREENSAVER_PAGE: &str = "pageScreensaver";

/// Screensaver. When it starts, what plays, and, for the styles that
/// bounce something, what bounces.
fn screensaver_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![field("screensaverTimeout"), field("screensaverStyle")];
    if inputs.screensaver_bounces {
        rows.push(field("screensaverBouncing"));
    }
    rows
}

/// Controls. The way to the controllers Core sees and to pairing another
/// leads, the row a user comes here to do; the input preferences follow.
fn controls_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = Vec::new();
    if inputs.controllers_listed || inputs.can_pair_controllers {
        rows.push(field("controllers"));
    }
    rows.extend([
        field("buttonLayout"),
        field("swapConfirmCancel"),
        field("swapOptionsView"),
        field("mouseEnabled"),
    ]);
    rows
}

/// Controllers. The connected cards are drawn by the page itself and take
/// no focus; the rows are what the search found, in the order it found
/// them and only ever appended, then the fixed Paired controllers row. A
/// find lands after the last one and before that row, so neither the
/// rows already listed nor the cursor move. Without pairing the page is
/// the cards alone.
fn controllers_rows(inputs: &Inputs) -> Vec<Row> {
    if !inputs.can_pair_controllers {
        return Vec::new();
    }
    let mut rows: Vec<Row> = (0..inputs.bluetooth_found)
        .map(|index| Row::Item {
            id: "foundDevice",
            index,
            control: Control::Device,
        })
        .collect();
    rows.push(field("pairedControllers"));
    rows
}

/// Paired controllers: every device Core has paired, in a stable order.
fn paired_rows(inputs: &Inputs) -> Vec<Row> {
    if inputs.bluetooth_paired == 0 {
        return vec![field("noPairedControllers")];
    }
    (0..inputs.bluetooth_paired)
        .map(|index| Row::Item {
            id: "pairedDevice",
            index,
            control: Control::Device,
        })
        .collect()
}

/// System. Pairing leads, with the devices already paired and the
/// encryption they must use; then language, then help in task order (turn
/// on debug logging, report errors, upload the log, read the guide), and
/// identity closes the page. Core's own rows wait for its settings.
fn system_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![field("pairDevice"), field(PAIRED_DEVICES_PAGE)];
    if inputs.core_settings {
        rows.push(field("encryption"));
    }
    rows.extend([field(LANGUAGE_PAGE), field("debugLogging")]);
    if inputs.core_settings {
        rows.push(field("errorReporting"));
    }
    if inputs.log_upload {
        rows.push(field("uploadLog"));
    }
    rows.extend([
        field("documentation"),
        field("aboutLicense"),
        field("credits"),
    ]);
    rows
}

fn items(id: &'static str, count: usize, control: Control) -> impl Iterator<Item = Row> {
    (0..count).map(move |index| Row::Item { id, index, control })
}

/// Paired devices: every client Core has paired, or a line saying there
/// are none.
fn paired_device_rows(inputs: &Inputs) -> Vec<Row> {
    if inputs.paired_devices == 0 {
        return vec![field("noPairedDevices")];
    }
    items("pairedClient", inputs.paired_devices, Control::Device).collect()
}

/// Preferred game versions: a switch per region, then per language.
fn game_version_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![Row::Header("regions")];
    rows.extend(items("gameRegion", inputs.game_regions, Control::Toggle));
    rows.push(Row::Header("languages"));
    rows.extend(items("gameLang", inputs.game_langs, Control::Toggle));
    rows
}

/// Profiles: who is playing, picked from the list, and a way to leave;
/// then the rows that manage them, and whether a start asks who is
/// playing.
fn profiles_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows: Vec<Row> = if inputs.profiles == 0 {
        vec![field("noProfiles")]
    } else {
        items("profile", inputs.profiles, Control::Device).collect()
    };
    if inputs.active_profile {
        rows.push(field("leaveProfile"));
    }
    rows.extend([
        Row::Header("manage"),
        field("newProfile"),
        field(PLAYTIME_LIMITS_PAGE),
        field("chooseProfileAtStartup"),
    ]);
    rows
}

/// One profile: who it is and how it unlocks, its own limits, then its
/// switch card and deleting it.
fn profile_edit_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![field("profileName"), field("profilePin")];
    if inputs.profile_has_pin {
        rows.push(field("profileClearPin"));
    }
    rows.extend([
        field("profileRole"),
        Row::Header("limits"),
        field("profileLimits"),
    ]);
    if !inputs.profile_limits_off {
        rows.extend([field("profileDailyLimit"), field("profileSessionLimit")]);
    }
    rows.extend([
        Row::Header("manage"),
        field("writeSwitchCard"),
        field("deleteProfile"),
    ]);
    rows
}

/// Playtime limits: the switch, the limits every profile inherits while
/// it is on, and whether a profile must be picked to play at all.
fn playtime_limit_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![field("playtimeEnabled")];
    if inputs.playtime_enabled {
        rows.extend([field("playtimeDaily"), field("playtimeSession")]);
    }
    rows.push(field("profilesRequireForLaunch"));
    rows
}

/// The Language page: `names.txt` sits under Region on a `MiSTer`.
fn language_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![field("language"), field("region")];
    // `Main_MiSTer`'s own names.txt only exists on a `MiSTer`.
    if inputs.is_mister {
        rows.push(field("misterNames"));
    }
    rows.push(field("clockFormat"));
    rows
}

/// The Appearance page's rows.
fn appearance_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![
        field("colorScheme"),
        field("colorIntensity"),
        field("systemLogoStyle"),
        field("headerLogo"),
    ];
    if inputs.header_logo_custom {
        rows.push(field("headerLogoImage"));
    }
    rows.extend([
        field("showMenuRow"),
        field("showHelpBar"),
        field("reduceMotion"),
        field("screensaver"),
        Row::Header("background"),
        field("backgroundSource"),
    ]);
    if inputs.background_custom {
        rows.push(field("backgroundImage"));
    }
    rows.push(field("gameBackground"));
    // Blur, dimming and the list card only shape an image; with
    // none that can show they would change nothing.
    if inputs.background_active {
        rows.extend([
            field("backgroundBlur"),
            field("backgroundDim"),
            field("backgroundListStyle"),
        ]);
    }
    rows
}

/// The rows of one page; the empty page id is the root category grid.
pub fn page_rows(page: &str, inputs: &Inputs) -> Vec<Row> {
    match page {
        "" => root_pages(inputs.retro_achievements)
            .map(|p| field(p.id))
            .collect(),
        "pageAppearance" => appearance_rows(inputs),
        "pageDisplayInterface" => {
            let mut rows = Vec::new();
            // The interface resolution is the HDMI path's own; under the
            // CRT path the video standard sets it instead.
            if inputs.is_mister && !inputs.crt_enabled {
                rows.push(field("resolution"));
            }
            rows.push(field("interfaceProfile"));
            rows.push(field("orientation"));
            if inputs.is_mister {
                // The toggle is always offered so an HDMI user can switch
                // in; the standard and the calibration pattern only mean
                // something once the frontend runs with --crt.
                rows.push(Row::Header("analogVideo"));
                rows.push(field("crtEnabled"));
                if inputs.crt_enabled {
                    rows.push(field("crtVideoStandard"));
                    rows.push(field("crtCalibration"));
                }
            }
            rows
        }
        LANGUAGE_PAGE => language_rows(inputs),
        GAME_VERSIONS_PAGE => game_version_rows(inputs),
        PAIRED_DEVICES_PAGE => paired_device_rows(inputs),
        PROFILES_PAGE => profiles_rows(inputs),
        PROFILE_EDIT_PAGE => profile_edit_rows(inputs),
        PLAYTIME_LIMITS_PAGE => playtime_limit_rows(inputs),
        "pageControlsInput" => controls_rows(inputs),
        CONTROLLERS_PAGE => controllers_rows(inputs),
        PAIRED_PAGE => paired_rows(inputs),
        SCREENSAVER_PAGE => screensaver_rows(inputs),
        "pageLibraryData" => library_rows(inputs),
        BROWSING_PAGE => browsing_rows(),
        RETRO_ACHIEVEMENTS_PAGE => retro_achievements_rows(inputs),
        // Account first: everything else on the page needs it. Status and
        // Warp only report (info rows, never focused); the one thing to do
        // there is link or unlink. Features lead with the one switch that
        // covers all four, then each on its own; Remote and Backup close
        // the page, in the features' order, with the one action each
        // actually needs doing from here.
        "pageOnline" => {
            if !inputs.can_link_online {
                // Core has not answered the online settings: it is not
                // connected yet, or too old to offer Online, or this client
                // is not a local one. Say so instead of an empty page.
                return vec![Row::Header("onlineUnavailable")];
            }
            let mut rows = vec![Row::Header("account"), field("onlineStatus")];
            if inputs.online_linked {
                rows.push(field("onlineWarp"));
                rows.push(field("onlineUnlinkAccount"));
            } else {
                rows.push(field("onlineLinkAccount"));
            }
            rows.extend([
                Row::Header("features"),
                field("onlineAllFeatures"),
                field("onlineRemoteControl"),
                field("playtimeSync"),
                field("onlineLibrarySync"),
                field("onlineCloudBackup"),
                Row::Header("remote"),
                field("onlineRemoteStatus"),
                field("onlineRemoteActivity"),
                Row::Header("backup"),
                field("onlineBackupSchedule"),
                field("onlineManageBackups"),
            ]);
            rows
        }
        SYSTEM_PAGE => system_rows(inputs),
        page => crate::readers::page_rows(page, &inputs.readers).unwrap_or_default(),
    }
}

/// The page a row opens, for the navigation rows that are category tiles
/// or pages inside one.
pub fn opens_page(id: &str) -> Option<&'static str> {
    const SUB_PAGES: &[&str] = &[
        LANGUAGE_PAGE,
        GAME_VERSIONS_PAGE,
        PAIRED_DEVICES_PAGE,
        PLAYTIME_LIMITS_PAGE,
    ];
    PAGES
        .iter()
        .map(|p| p.id)
        .chain(SUB_PAGES.iter().copied())
        .find(|page| *page == id)
}

pub const LANGUAGES: &[&str] = &[
    "auto", "ar", "eu", "zh_CN", "zh_TW", "nl", "en", "fr", "de", "el", "he", "hi", "it_IT", "ja",
    "ko", "ro", "sk", "es", "uk",
];
pub const CLOCK_FORMATS: &[&str] = &["auto", "12h", "24h", "off"];
pub const REGIONS: &[&str] = &["auto", "us", "eu", "jp"];
pub const INTERFACE_PROFILES: &[&str] = &["device", "standard", "handheld"];
pub const ORIENTATIONS: &[&str] = &["horizontal", "cw", "ccw"];
pub const BROWSE_LAYOUTS: &[&str] = &["grid", "list", "list-only"];
/// What Left and Right do in the detailed list: turn a page, or step the
/// detail pane through the game's images.
pub const LIST_LEFT_RIGHT: &[&str] = &["page", "image"];
pub const SYSTEM_LOGO_STYLES: &[&str] = &["tinted", "color"];
pub const COLOR_INTENSITIES: &[&str] = &["subtle", "vivid"];
pub const BUTTON_LAYOUTS: &[&str] = &[
    "auto", "style_a", "style_b", "style_c", "style_d", "style_e",
];
pub const SCREENSAVER_TIMEOUTS: &[&str] = &["off", "60", "120", "300", "600", "900", "1800"];
pub const MEDIA_IMAGE_TYPES: &[&str] = &[
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
pub const CRT_VIDEO_STANDARDS: &[&str] = &["ntsc", "pal"];
pub const BACKUP_SCHEDULES: &[&str] = &["daily", "weekly", "manual"];
pub const POPUP_POSITIONS: &[&str] = &["left", "center", "right"];
pub const PROFILE_ROLES: &[&str] = &["member", "admin"];
/// A profile's own limits: what every profile inherits, or on or off for
/// this one.
pub const PROFILE_LIMIT_MODES: &[&str] = &["default", "on", "off"];
/// A playtime limit in minutes; "0" is no limit.
pub const PLAYTIME_LIMITS: &[&str] = &["0", "30", "60", "90", "120", "180", "240"];
/// A profile's own limit, which can also inherit the global one.
pub const PROFILE_PLAYTIME_LIMITS: &[&str] =
    &["default", "0", "30", "60", "90", "120", "180", "240"];

/// The option list of a picker row, when it is a fixed one. Resolutions
/// and color schemes come from the host (the output modes it detected,
/// and the palette table), so they answer `None`.
pub fn options(id: &str, inputs: &Inputs) -> Option<Vec<&'static str>> {
    let fixed: &[&'static str] = match id {
        "language" => LANGUAGES,
        "clockFormat" => CLOCK_FORMATS,
        "region" => REGIONS,
        "interfaceProfile" => INTERFACE_PROFILES,
        "orientation" => ORIENTATIONS,
        "systemsLayout" | "gamesLayout" => BROWSE_LAYOUTS,
        "listLeftRight" => LIST_LEFT_RIGHT,
        "systemLogoStyle" => SYSTEM_LOGO_STYLES,
        "colorIntensity" => COLOR_INTENSITIES,
        "buttonLayout" => BUTTON_LAYOUTS,
        "mediaImageType" => MEDIA_IMAGE_TYPES,
        "crtVideoStandard" => CRT_VIDEO_STANDARDS,
        "onlineBackupSchedule" => BACKUP_SCHEDULES,
        "raPopupPosition" => POPUP_POSITIONS,
        "profileRole" => PROFILE_ROLES,
        "profileLimits" => PROFILE_LIMIT_MODES,
        "profileDailyLimit" | "profileSessionLimit" => PROFILE_PLAYTIME_LIMITS,
        "playtimeDaily" | "playtimeSession" => PLAYTIME_LIMITS,
        "gameBackground" => crate::background::GAME_BACKGROUND_TYPES,
        "backgroundListStyle" => crate::background::LIST_STYLES,
        "headerLogo" => crate::branding::HEADER_LOGOS,
        "backgroundSource" => crate::background::SOURCES,
        "screensaverStyle" => crate::screensaver::STYLES,
        "screensaverBouncing" => crate::screensaver::BOUNCINGS,
        id if crate::readers::options(id).is_some() => crate::readers::options(id)?,
        "screensaverTimeout" => {
            let mut values = SCREENSAVER_TIMEOUTS.to_vec();
            // A debug build can watch the screensaver arm immediately.
            if inputs.debug_build {
                values.insert(1, "1");
            }
            return Some(values);
        }
        _ => return None,
    };
    Some(fixed.to_vec())
}

/// The label an action row's control shows (`focusedActionLabel`), as a
/// vocabulary key.
pub fn action_label_key(id: &str, busy: bool) -> &'static str {
    match id {
        "updateMediaDb" | "runScraper" => {
            if busy {
                "cancel"
            } else {
                "configure"
            }
        }
        "uploadLog" => "upload",
        "detectLaunchers" => "detect",
        "pairDevice" => "pair",
        "onlineLinkAccount" => "link",
        "onlineUnlinkAccount" => "unlink",
        "raLogIn" => "logIn",
        "raLogOut" => "logOut",
        "leaveProfile" => "leave",
        "newProfile" => "create",
        "profileClearPin" => "remove",
        "writeSwitchCard" => "write",
        "deleteProfile" => "delete",
        _ => "open",
    }
}

/// A media job the row would start is already running.
pub fn action_busy(id: &str, index_busy: bool, scrape_busy: bool) -> bool {
    match id {
        "updateMediaDb" => index_busy,
        "runScraper" => scrape_busy,
        _ => false,
    }
}

/// The other media job is running, so this row cannot start its own.
pub fn action_disabled(id: &str, index_busy: bool, scrape_busy: bool) -> bool {
    match id {
        "updateMediaDb" => scrape_busy,
        "runScraper" => index_busy,
        _ => false,
    }
}

/// Online rows that do nothing without a linked account. They stay on the
/// page, dimmed, and say to link first, the same as Core's own TUI: Core
/// refuses every one of them for an unlinked device.
pub fn needs_online_link(id: &str) -> bool {
    matches!(
        id,
        "onlineAllFeatures"
            | "onlineRemoteControl"
            | "playtimeSync"
            | "onlineLibrarySync"
            | "onlineCloudBackup"
            | "onlineBackupSchedule"
            | "onlineManageBackups"
    )
}

/// The `RetroAchievements` launcher switch does nothing until
/// `RetroAchievements` is installed; it stays on the page, dimmed.
pub fn needs_ra_install(id: &str) -> bool {
    id == "raLaunchers"
}

/// Rows whose change only takes effect after the frontend restarts; the
/// view says so in their description and the host stages a confirm.
pub fn restarts(id: &str) -> bool {
    matches!(
        id,
        "resolution" | "crtEnabled" | "crtVideoStandard" | "language" | "debugLogging"
    )
}

/// The first row a cursor may sit on.
pub fn first_navigable(rows: &[Row]) -> usize {
    rows.iter().position(|r| r.is_navigable()).unwrap_or(0)
}

/// Where a remembered row lands on a page as it is now: the same row
/// while it is still a field, otherwise the page's first field.
pub fn restore_seat(rows: &[Row], index: usize) -> usize {
    if rows.get(index).copied().is_some_and(Row::is_navigable) {
        index
    } else {
        first_navigable(rows)
    }
}

/// Where a row the cursor should land on sits on the page now, or the
/// page's first field when it is not there.
pub fn seat_of(rows: &[Row], id: &str) -> usize {
    rows.iter()
        .position(|r| r.is_navigable() && r.id() == id)
        .unwrap_or_else(|| first_navigable(rows))
}

/// Where the cursor goes when a live list changes under it. `key` says
/// what each row is (an item row's entry, not its index), so the cursor
/// stays on the same device as entries come and go around it; when that
/// one is gone it keeps its place on the page, on the nearest row it can
/// rest on.
pub fn follow_seat<K: PartialEq>(
    rows: &[Row],
    key: impl Fn(usize) -> K,
    focused: &K,
    previous: usize,
) -> usize {
    if let Some(i) = (0..rows.len()).find(|&i| rows[i].is_navigable() && key(i) == *focused) {
        return i;
    }
    let from = previous.min(rows.len().saturating_sub(1));
    if rows.get(from).is_some_and(|r| r.is_navigable()) {
        return from;
    }
    let up = (0..from).rev().find(|&i| rows[i].is_navigable());
    let down = (from..rows.len()).find(|&i| rows[i].is_navigable());
    up.or(down).unwrap_or_else(|| first_navigable(rows))
}

/// The next navigable row in `dir`, wrapping and skipping headers.
pub fn seek_navigable(rows: &[Row], from: usize, dir: i64) -> usize {
    let len = rows.len();
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
        if rows[i as usize].is_navigable() {
            return i as usize;
        }
    }
    from
}

// ---------- Admin gate ----------

/// Pages only an admin may open while Core has profiles.
pub fn gated_page(id: &str) -> bool {
    matches!(
        id,
        "pageOnline"
            | GAME_VERSIONS_PAGE
            | PAIRED_DEVICES_PAGE
            | PROFILE_EDIT_PAGE
            | PLAYTIME_LIMITS_PAGE
            | "pageReaderDetail"
            | "pageAddReader"
            | "pageAddReaderPort"
            | "pageAddReaderOptions"
            | "pageReaderTypes"
            | "pageLaunchGuard"
            | "pageMappings"
            | "pageIgnoredSystems"
    )
}

/// The pages whose focused row survives a kill. A page under one of them
/// comes back as the row on it that opens that page.
pub fn kept_page(page: &str) -> bool {
    matches!(page, READERS_PAGE | PROFILES_PAGE | SYSTEM_PAGE)
}

/// Rows only an admin may act on (Accept, a change, or Options) while
/// Core has profiles, on pages anyone can open.
pub fn gated_row(id: &str) -> bool {
    matches!(
        id,
        "updateMediaDb"
            | "runScraper"
            | "launcherPreference"
            | "raLogIn"
            | "pairDevice"
            | "encryption"
            | "errorReporting"
            | "newProfile"
            | "chooseProfileAtStartup"
            | "readersScanMode"
            | "readersScanExitDelay"
            | "readersScanIgnoreSystems"
            | "readersScanIgnoreOnConnect"
            | "audioScanFeedback"
            | "connectedReader"
            | "mapToken"
            | "removeMapping"
    )
}

/// An admin must unlock first. Never while Core has no profiles, nor
/// while none of them is an admin (nobody could unlock it then), nor once
/// an admin has unlocked it.
pub fn admin_required(profiles: usize, admins: usize, unlocked: bool) -> bool {
    profiles > 0 && admins > 0 && !unlocked
}

/// A profile Core verified unlocks the gate only as an admin.
pub fn verified_admin(role: &str) -> bool {
    role == "admin"
}

/// Core refuses to remove the last admin, whether a paired device or a
/// profile, so that one is never offered for removal. `admins` counts
/// every admin in the list `this` belongs to.
pub fn last_admin(this_is_admin: bool, admins: usize) -> bool {
    this_is_admin && admins == 1
}

/// A PIN is 4 to 8 digits.
pub const PIN_MIN: usize = 4;
pub const PIN_MAX: usize = 8;

/// The PIN typed so far can be sent.
pub fn pin_complete(pin: &str) -> bool {
    (PIN_MIN..=PIN_MAX).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

// ---------- System ----------

/// Debug logging is one row for the frontend's flag and Core's: on only
/// while both are. Before Core answers, the frontend's alone. A mixed
/// state reads as off, so choosing the row turns both on.
pub fn debug_logging_on(frontend: bool, core: Option<bool>) -> bool {
    frontend && core.unwrap_or(true)
}

// ---------- Playtime limits ----------

/// Minutes in a Go duration string ("1h30m0s", "90m", "0s", "0").
/// Fractions of a minute round to the nearest one. Anything else is
/// `None`.
pub fn duration_minutes(value: &str) -> Option<u64> {
    let value = value.trim();
    if value == "0" {
        return Some(0);
    }
    if value.is_empty() {
        return None;
    }
    let mut seconds = 0.0_f64;
    let mut rest = value;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if digits == 0 {
            return None;
        }
        let amount: f64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let scale = match &rest[..unit] {
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            "ms" => 1e-3,
            "us" | "\u{b5}s" => 1e-6,
            "ns" => 1e-9,
            _ => return None,
        };
        seconds += amount * scale;
        rest = &rest[unit..];
    }
    Some((seconds / 60.0).round() as u64)
}

/// A limit's picker token: minutes, or "default" while a profile inherits
/// the global one.
pub fn limit_token(value: Option<&str>) -> String {
    match value {
        None => "default".to_string(),
        Some(value) => duration_minutes(value).unwrap_or(0).to_string(),
    }
}

/// The duration Core takes for a picker token of minutes.
pub fn limit_duration(token: &str) -> String {
    format!("{token}m")
}

/// A profile's limits switch as a picker token.
pub fn limit_mode_token(enabled: Option<bool>) -> &'static str {
    match enabled {
        None => "default",
        Some(true) => "on",
        Some(false) => "off",
    }
}

// ---------- Preferred game versions ----------

/// The regions Core knows, in the order the page lists them.
pub const GAME_REGIONS: &[&str] = &[
    "world",
    "us",
    "eu",
    "jp",
    "asia",
    "au",
    "br",
    "ca",
    "cn",
    "fr",
    "de",
    "hk",
    "it",
    "kr",
    "es",
    "scandinavia",
    "tw",
];

/// The languages Core knows, in the order the page lists them.
pub const GAME_LANGS: &[&str] = &[
    "en", "fr", "de", "es", "it", "ja", "ko", "zh", "pt", "nl", "sv", "no", "da", "fi", "pl", "ru",
    "ar",
];

/// What Core uses while its list is empty.
pub const DEFAULT_GAME_REGIONS: &[&str] = &["us", "world"];
pub const DEFAULT_GAME_LANGS: &[&str] = &["en"];

/// The values Core uses: its list, or its defaults while the list is
/// empty.
pub fn effective_versions(current: &[String], defaults: &[&str]) -> Vec<String> {
    if current.is_empty() {
        defaults.iter().map(|v| (*v).to_string()).collect()
    } else {
        current.to_vec()
    }
}

/// Every value the page lists: the known ones, then any other value
/// already in Core's list, so it can be seen and turned off.
pub fn game_version_choices(known: &[&str], current: &[String]) -> Vec<String> {
    let mut choices: Vec<String> = known.iter().map(|v| (*v).to_string()).collect();
    for value in current {
        if !choices.contains(value) {
            choices.push(value.clone());
        }
    }
    choices
}

/// `value` is the only one on. It stays on: an empty list is Core's
/// defaults, so turning it off would bring the defaults back on.
pub fn last_version(current: &[String], defaults: &[&str], value: &str) -> bool {
    let on = effective_versions(current, defaults);
    on.len() == 1 && on[0] == value
}

/// Core's list after one value is turned on (added last) or off.
pub fn toggle_version(current: &[String], defaults: &[&str], value: &str, on: bool) -> Vec<String> {
    let mut list = effective_versions(current, defaults);
    list.retain(|v| v != value);
    if on {
        list.push(value.to_string());
    }
    list
}

/// One rendered line of text at `size`, the way Slint lays it out: Noto
/// Sans has a 1.362 em hhea line height and no line gap. Shared by the
/// action status line and the group heading so they stay in step with the
/// type ladder instead of with a magic percentage.
pub(crate) fn line_height(size: i32) -> i32 {
    (f64::from(size) * 1.362).ceil() as i32
}

/// One line of the hint band. The bitmap face has no leading: a line is
/// exactly its cell, so the scaled figure would hold a third of a line of
/// blank band open under every description.
pub(crate) fn hint_line_height(inputs: &crate::sizing::Inputs, size: i32) -> i32 {
    if inputs.bitmap_type {
        size
    } else {
        line_height(size)
    }
}

/// A settings page card in whole pixels: where it sits in the scene, the
/// bands stacked inside it, and the heights its rows stack with. The view
/// places the card from these values instead of deriving its own, so the
/// rows band it clips is the viewport the scroll was solved against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageGeometry {
    /// The card is centered: as wide as the scene less 3% either side, and
    /// never wider than 70% of it.
    pub card_x: i32,
    pub card_width: i32,
    pub card_y: i32,
    pub card_height: i32,
    /// The card's lip: above the first row, under the last, either side of
    /// the hint rule and under the hint text. One inset, used everywhere
    /// on the surface (`docs/style.md`, "Surface containment").
    pub pad: i32,
    pub hint_height: i32,
    /// What is left for rows once the lips, the hint band, its rule and the
    /// clear line over the rule are taken out.
    pub rows_viewport: i32,
    /// A field is one line tall.
    pub row_height: i32,
    /// What a row with a status line adds under its label.
    pub action_band_height: i32,
    /// A group heading's own short row.
    pub header_height: i32,
}

pub fn page_geometry(inputs: &crate::sizing::Inputs) -> PageGeometry {
    use crate::layouts::{self, ThemeId, View};
    let derived = crate::sizing::derive(inputs);
    let profile = layouts::profile(ThemeId::current(inputs), View::GamesGrid, inputs);
    let card_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + inputs.pct_h(4.0);
    let bottom = derived.help_bar_height + inputs.pct_h(4.0);
    let card_height = (inputs.screen_height as i32 - card_y - bottom).max(0);
    // Descriptions are authored against the 240p card's line budget, so
    // only that tier needs two lines held open; every wider one fits them
    // on one and a second reserved line just holds the rows up. Fixed per
    // tier, never per row: this is the rows viewport, and a band that grew
    // with the focused description would reflow the list under the cursor.
    let hint_lines = if derived.tier == crate::sizing::Tier::T240 || inputs.bitmap_type {
        2
    } else {
        1
    };
    let hint_height = hint_lines * hint_line_height(inputs, derived.font_body);
    let pad = inputs.pct_min(2.0);
    // Four lips and two hairlines: the hint rule, and one clear line over
    // it where the rows band stops, so a row gliding out never abuts the
    // rule.
    let rows_viewport = (card_height - 4 * pad - 2 - hint_height).max(0);
    let screen_width = inputs.screen_width as i32;
    let card_width = (screen_width - 2 * inputs.pct_w(3.0))
        .min(inputs.pct_w(70.0))
        .max(0);
    PageGeometry {
        card_x: (screen_width - card_width) / 2,
        card_width,
        card_y,
        card_height,
        pad,
        hint_height,
        rows_viewport,
        row_height: inputs.pct_h(8.0),
        // Exactly the status line it makes room for, plus the gap over it.
        // A flat percentage reserved a second *body* line for a caption
        // and left the pair floating in a row taller than its content.
        action_band_height: line_height(derived.font_caption) + inputs.pct_h(0.3),
        // `SectionHeader`'s own anatomy: top gap, the label, the rule gap
        // and the hairline. The heading is a shared component, so the row
        // Rust stacks has to be the box that component draws.
        header_height: inputs.pct_h(1.5) + line_height(derived.font_body) + inputs.pct_h(0.8) + 1,
    }
}

/// The root category grid: two rows of three, transposed when the scene
/// is rotated.
/// How far a settings rows band scrolls to keep the focused row in view.
/// `rows` is each row's (top, height) in order, measured from the first
/// row's top. `lip` is the inset the card keeps above the first row and
/// below the last; it scrolls with the rows, so the band clips against the
/// card's own edges rather than against a margin inside them.
///
/// The band is a fixed height and its rows are not all the same height.
/// Its bottom edge is fixed (the hint rule), so a row may be cut there,
/// which reads as "there is more below" because the rows glide under it.
/// The top is the edge that must stay clean: a scrolled band always starts
/// on a row top, so no sliver of the row before it hangs under the card's
/// frame. The focused row keeps one lip of air above the bottom edge.
///
/// Returns the scroll offset in lip-inclusive space (0, or `lip` plus a
/// row top) and the height the band paints (`viewport` plus both lips).
pub fn band_extent(rows: &[(f32, f32)], index: usize, viewport: f32, lip: f32) -> (f32, f32) {
    let shown = viewport + 2.0 * lip;
    let Some(&(focused_top, focused_height)) = rows.get(index) else {
        return (0.0, shown);
    };
    // The least scrolling that brings the focused row fully into view with
    // its lip under it, rounded up to a row top.
    let needed = focused_top + focused_height - viewport - lip;
    if needed <= 0.0 {
        return (0.0, shown);
    }
    let snapped = rows
        .iter()
        .map(|(top, _)| *top)
        .filter(|top| *top >= needed && *top <= focused_top)
        .fold(f32::INFINITY, f32::min);
    // No row top frames a row taller than the band; sit on its own top.
    let top = if snapped.is_finite() {
        snapped
    } else {
        focused_top
    };
    if top <= 0.0 {
        (0.0, shown)
    } else {
        (lip + top, shown)
    }
}

pub fn root_grid_shape(count: usize, rotated: bool) -> (usize, usize) {
    let rows = 2;
    let columns = count.div_ceil(rows).max(1);
    if rotated {
        (rows, columns)
    } else {
        (columns, rows)
    }
}

/// A cardinal move on the root grid: horizontal wraps inside its row,
/// vertical wraps around the column.
pub fn root_grid_move(index: usize, count: usize, columns: usize, dx: i64, dy: i64) -> usize {
    if count == 0 {
        return index;
    }
    let columns = columns.max(1);
    let row = index / columns;
    let col = index % columns;
    if dx != 0 {
        let row_start = row * columns;
        let row_end = (row_start + columns - 1).min(count - 1);
        let next = index as i64 + dx;
        return if next < row_start as i64 {
            row_end
        } else if next > row_end as i64 {
            row_start
        } else {
            next as usize
        };
    }
    if dy != 0 {
        let next = index as i64 + dy * columns as i64;
        return if next < 0 {
            let last_row = (count - 1) / columns;
            (last_row * columns + col).min(count - 1)
        } else if next >= count as i64 {
            col.min(count - 1)
        } else {
            next as usize
        };
    }
    index
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a missing option list is a test failure either way"
)]
mod tests {
    use super::*;

    fn mister(crt: bool) -> Inputs {
        Inputs {
            is_mister: true,
            crt_enabled: crt,
            log_upload: true,
            ..Inputs::default()
        }
    }

    #[test]
    fn the_names_txt_row_is_mister_only() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageLanguage", inputs)
                .into_iter()
                .map(Row::id)
                .collect()
        };
        assert_eq!(
            ids(&mister(false)),
            ["language", "region", "misterNames", "clockFormat"]
        );
        assert_eq!(
            ids(&Inputs::default()),
            ["language", "region", "clockFormat"]
        );
        assert_eq!(control("misterNames"), Control::Toggle);
    }

    #[test]
    fn the_upload_row_needs_an_uploader() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageSupportAbout", inputs)
                .into_iter()
                .map(Row::id)
                .collect()
        };
        assert!(ids(&mister(false)).contains(&"uploadLog"));
        assert!(!ids(&Inputs::default()).contains(&"uploadLog"));
    }

    #[test]
    fn online_page_needs_core_consent_capability_and_is_not_on_library() {
        let ids = |page: &str, inputs: &Inputs| -> Vec<&str> {
            page_rows(page, inputs).iter().map(|row| row.id()).collect()
        };
        assert!(!ids("pageLibraryData", &Inputs::default()).contains(&"playtimeSync"));
        assert!(!ids("pageLibraryData", &Inputs::default()).contains(&"onlineLinkAccount"));
        assert_eq!(
            page_rows("pageOnline", &Inputs::default()),
            vec![Row::Header("onlineUnavailable")],
            "a client Core never grants Online consent to gets an honest page, not a crash"
        );

        let inputs = Inputs {
            can_link_online: true,
            ..Inputs::default()
        };
        let rows = ids("pageOnline", &inputs);
        assert!(rows.contains(&"onlineLinkAccount"), "{rows:?}");
        assert!(!rows.contains(&"onlineUnlinkAccount"), "{rows:?}");
        assert!(!rows.contains(&"onlineWarp"), "{rows:?}");
        assert!(rows.contains(&"playtimeSync"), "{rows:?}");
        assert!(rows.contains(&"onlineAllFeatures"), "{rows:?}");

        let linked_rows = ids(
            "pageOnline",
            &Inputs {
                can_link_online: true,
                online_linked: true,
                ..Inputs::default()
            },
        );
        assert!(
            !linked_rows.contains(&"onlineLinkAccount"),
            "{linked_rows:?}"
        );
        assert!(
            linked_rows.contains(&"onlineUnlinkAccount"),
            "{linked_rows:?}"
        );
        assert!(linked_rows.contains(&"onlineWarp"), "{linked_rows:?}");

        // Everything that changes or reads account data waits for a link;
        // linking itself, and the rows that only report, do not.
        for row in &page_rows("pageOnline", &inputs) {
            let waits = needs_online_link(row.id());
            match row.id() {
                "onlineLinkAccount"
                | "onlineStatus"
                | "onlineRemoteStatus"
                | "onlineRemoteActivity" => assert!(!waits, "{}", row.id()),
                _ if row.is_field() => assert!(waits, "{}", row.id()),
                _ => {}
            }
        }

        assert_eq!(control("onlineLinkAccount"), Control::Action);
        assert_eq!(control("onlineUnlinkAccount"), Control::Action);
        assert_eq!(control("onlineAllFeatures"), Control::TriToggle);
        assert_eq!(control("onlineStatus"), Control::Info);
        assert_eq!(control("onlineWarp"), Control::Info);
        assert_eq!(control("onlineRemoteStatus"), Control::Info);
        assert_eq!(control("onlineBackupSchedule"), Control::Picker);
        assert_eq!(control("onlineManageBackups"), Control::Navigate);
        assert_eq!(control("onlineRemoteActivity"), Control::Navigate);
        assert_eq!(control("onlineRemoteControl"), Control::Toggle);
        assert_eq!(control("onlineLibrarySync"), Control::Toggle);
        assert_eq!(control("onlineCloudBackup"), Control::Toggle);
        assert_eq!(control("playtimeSync"), Control::Toggle);
        assert_eq!(
            options("onlineBackupSchedule", &Inputs::default()).unwrap(),
            BACKUP_SCHEDULES
        );
        // An info row only reports: the cursor starts on the first row
        // there is something to do on and steps over the rest.
        let page = page_rows("pageOnline", &inputs);
        let seat = first_navigable(&page);
        assert_eq!(page[seat].id(), "onlineLinkAccount");
        let next = seek_navigable(&page, seat, 1);
        assert_eq!(
            page[next].id(),
            "onlineAllFeatures",
            "the all-features switch leads the Features group"
        );
        let last = seek_navigable(&page, seat, -1);
        assert_eq!(page[last].id(), "onlineManageBackups");
        let activity = seat_of(&page, "onlineRemoteActivity");
        assert_eq!(
            page[seek_navigable(&page, activity, -1)].id(),
            "onlineCloudBackup",
            "up from Activity skips the remote status row"
        );
        assert!(page.iter().all(|row| row.is_navigable()
            || !row.is_field()
            || matches!(
                row.id(),
                "onlineStatus" | "onlineWarp" | "onlineRemoteStatus"
            )));
    }

    #[test]
    fn playtime_access_requires_a_host_handoff_and_remains_an_action() {
        let mut inputs = Inputs::default();
        assert!(!page_rows("pageLibraryData", &inputs)
            .iter()
            .any(|row| row.id() == "playtimeAccess"));
        inputs.can_request_playtime_access = true;
        assert!(page_rows("pageLibraryData", &inputs)
            .iter()
            .any(|row| row.id() == "playtimeAccess"));
        assert_eq!(control("playtimeAccess"), Control::Action);
    }

    #[test]
    fn folder_action_requires_host_capability_and_leads_library() {
        let mut inputs = Inputs::default();
        assert!(!page_rows("pageLibraryData", &inputs)
            .iter()
            .any(|row| row.id() == "addGameFolder"));
        inputs.can_pick_folder = true;
        assert_eq!(
            page_rows("pageLibraryData", &inputs)[0],
            Row::Field {
                id: "addGameFolder",
                control: Control::Action
            }
        );
        assert_eq!(control("addGameFolder"), Control::Action);
    }

    #[test]
    fn launcher_scan_requires_host_capability_and_is_an_action() {
        let mut inputs = Inputs::default();
        assert!(!page_rows("pageLibraryData", &inputs)
            .iter()
            .any(|row| row.id() == "detectLaunchers"));
        inputs.can_scan_launchers = true;
        let rows = page_rows("pageLibraryData", &inputs);
        let mut after = rows
            .iter()
            .skip_while(|r| **r != Row::Header("launchers"))
            .skip(1);
        assert_eq!(
            after.next(),
            Some(&Row::Field {
                id: "detectLaunchers",
                control: Control::Action
            })
        );
        assert_eq!(control("detectLaunchers"), Control::Action);
    }

    #[test]
    fn field_control_covers_every_row_kind() {
        assert_eq!(control("showHidden"), Control::Toggle);
        assert_eq!(control("reduceMotion"), Control::Toggle);
        assert_eq!(control("aboutLicense"), Control::Navigate);
        assert_eq!(control("credits"), Control::Navigate);
        assert_eq!(control("documentation"), Control::Navigate);
        assert_eq!(control("crtCalibration"), Control::Navigate);
        assert_eq!(control("pageDisplayInterface"), Control::Navigate);
        assert_eq!(control("updateMediaDb"), Control::Action);
        assert_eq!(control("runScraper"), Control::Action);
        assert_eq!(control("uploadLog"), Control::Action);
        assert_eq!(control("pairDevice"), Control::Action);
        assert_eq!(control("resolution"), Control::Picker);
        assert_eq!(control("colorScheme"), Control::Picker);
    }

    #[test]
    fn root_page_lists_the_categories() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("", inputs).into_iter().map(Row::id).collect()
        };
        let rows = page_rows("", &Inputs::default());
        assert!(rows.iter().all(|r| r.is_field()));
        assert_eq!(
            ids(&Inputs::default()),
            [
                "pageLibraryData",
                BROWSING_PAGE,
                "pageAppearance",
                "pageDisplayInterface",
                "pageControlsInput",
                READERS_PAGE,
                PROFILES_PAGE,
                "pageOnline",
                SYSTEM_PAGE,
            ]
        );
        assert_eq!(root_grid_shape(rows.len(), false), (5, 2));
        // RetroAchievements is a tile only where MiSTer_RA can be set up,
        // between Online and About.
        let mister = ids(&Inputs {
            retro_achievements: true,
            ..Inputs::default()
        });
        assert_eq!(mister.len(), 10);
        assert_eq!(
            &mister[7..],
            ["pageOnline", RETRO_ACHIEVEMENTS_PAGE, "pageSupportAbout"]
        );
        assert_eq!(root_grid_shape(mister.len(), false), (5, 2));
        assert_eq!(page_glyph(RETRO_ACHIEVEMENTS_PAGE), Some("icons/Trophy"));
        assert_eq!(page_glyph(BROWSING_PAGE), Some("icons/Browsing"));
        assert_eq!(control(BROWSING_PAGE), Control::Navigate);
        assert_eq!(control(RETRO_ACHIEVEMENTS_PAGE), Control::Navigate);
        assert_eq!(page_glyph(MUSIC_PAGE), Some("icons/Music"));
        assert_eq!(page_glyph("colorScheme"), None);
        assert_eq!(opens_page("pageLibraryData"), Some("pageLibraryData"));
        assert_eq!(opens_page("pageOnline"), Some("pageOnline"));
        assert_eq!(opens_page("colorScheme"), None);
    }

    #[test]
    fn display_page_gates_the_mister_and_crt_rows() {
        let desktop = page_rows("pageDisplayInterface", &Inputs::default());
        let ids: Vec<&str> = desktop.iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec!["interfaceProfile", "orientation"]);

        let hdmi: Vec<&str> = page_rows("pageDisplayInterface", &mister(false))
            .iter()
            .map(|r| r.id())
            .collect();
        assert_eq!(
            hdmi,
            vec![
                "resolution",
                "interfaceProfile",
                "orientation",
                "analogVideo",
                "crtEnabled"
            ]
        );

        let crt: Vec<&str> = page_rows("pageDisplayInterface", &mister(true))
            .iter()
            .map(|r| r.id())
            .collect();
        assert_eq!(
            crt,
            vec![
                "interfaceProfile",
                "orientation",
                "analogVideo",
                "crtEnabled",
                "crtVideoStandard",
                "crtCalibration"
            ]
        );
    }

    #[test]
    fn library_page_leads_with_maintenance() {
        let rows = page_rows("pageLibraryData", &Inputs::default());
        let ids: Vec<&str> = rows.iter().map(|r| r.id()).collect();
        assert_eq!(ids, ["maintenance", "updateMediaDb", "runScraper"]);
        let hosted: Vec<&str> = page_rows(
            "pageLibraryData",
            &Inputs {
                can_pick_folder: true,
                can_scan_launchers: true,
                can_request_playtime_access: true,
                launcher_preference: true,
                ..Inputs::default()
            },
        )
        .iter()
        .map(|r| r.id())
        .collect();
        assert_eq!(
            hosted,
            [
                "addGameFolder",
                "playtimeAccess",
                "maintenance",
                "updateMediaDb",
                "runScraper",
                "launchers",
                "detectLaunchers",
                "launcherPreference",
            ]
        );
        // Round 10 retired the arcade alternate-versions toggle.
        assert!(!rows
            .iter()
            .any(|r| r.id() == "discoverArcadeAlternateVersions"));
        assert!(!rows.iter().any(|r| r.id() == "browseLayout"));
    }

    #[test]
    fn browsing_page_lists_the_list_preferences() {
        let ids: Vec<&str> = page_rows(BROWSING_PAGE, &Inputs::default())
            .iter()
            .map(|r| r.id())
            .collect();
        assert_eq!(
            ids,
            [
                "systemsLayout",
                "gamesLayout",
                "listLeftRight",
                "mediaImageType",
                "showTags",
                "showOriginalFilenames",
                "showHidden",
            ]
        );
    }

    #[test]
    fn library_page_adds_the_launcher_section_and_ra_has_its_own_page() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageLibraryData", inputs)
                .into_iter()
                .map(Row::id)
                .collect()
        };
        let plain = ids(&Inputs::default());
        assert!(!plain.contains(&"launcherPreference"));
        assert!(!plain.contains(&"launchers"));

        let core = ids(&Inputs {
            launcher_preference: true,
            ..Inputs::default()
        });
        assert_eq!(
            &core[core.len() - 2..],
            &["launchers", "launcherPreference"]
        );

        let fresh = Inputs {
            launcher_preference: true,
            retro_achievements: true,
            ..Inputs::default()
        };
        assert!(!ids(&fresh).iter().any(|id| id.starts_with("ra")));
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows(RETRO_ACHIEVEMENTS_PAGE, inputs)
                .into_iter()
                .map(Row::id)
                .collect()
        };
        assert!(ids(&Inputs::default()).is_empty());
        assert_eq!(
            ids(&fresh),
            [
                "account",
                "raAccount",
                "raLogIn",
                "setup",
                "raLaunchers",
                "raInstall",
                "gameplay",
                "raHardcore",
                "raListHotkey",
                "popups",
                "raProgressPopups",
                "raChallengePopups",
                "raLeaderboardPopups",
                "raPopupPosition",
            ]
        );
        let ready = ids(&Inputs {
            ra_logged_in: true,
            ra_installed: true,
            ra_conflict: true,
            ..fresh
        });
        assert!(ready.contains(&"raLogOut") && !ready.contains(&"raLogIn"));
        assert!(ready.contains(&"raConflict") && !ready.contains(&"raInstall"));
        let old_core = ids(&Inputs {
            launcher_preference: false,
            ..fresh
        });
        assert!(!old_core.contains(&"raLaunchers"));
        let bare = ids(&Inputs {
            launcher_preference: false,
            ra_installed: true,
            ..fresh
        });
        assert!(!bare.contains(&"setup"), "an empty section keeps no header");

        assert_eq!(control("raAccount"), Control::Info);
        assert_eq!(control("raInstall"), Control::Notice);
        assert_eq!(control("raConflict"), Control::Notice);
        assert_eq!(control("raLogIn"), Control::Action);
        assert_eq!(control("raLogOut"), Control::Action);
        assert_eq!(control("raLaunchers"), Control::Toggle);
        assert_eq!(control("raLeaderboardPopups"), Control::Toggle);
        assert_eq!(control("raPopupPosition"), Control::Picker);
        assert_eq!(control("launcherPreference"), Control::Navigate);
        assert_eq!(options("raPopupPosition", &fresh).unwrap(), POPUP_POSITIONS);
        assert_eq!(action_label_key("raLogIn", false), "logIn");
        assert!(needs_ra_install("raLaunchers"));
        assert!(!needs_ra_install("raHardcore"));

        let page = page_rows(RETRO_ACHIEVEMENTS_PAGE, &fresh);
        assert_eq!(page[seat_of(&page, "raLogIn")].id(), "raLogIn");
        assert_eq!(seat_of(&page, "raLogOut"), first_navigable(&page));
        assert_eq!(
            seat_of(&page, "raAccount"),
            first_navigable(&page),
            "an info row is no seat"
        );
    }

    #[test]
    fn ra_notices_take_the_cursor() {
        let fresh = Inputs {
            retro_achievements: true,
            ..Inputs::default()
        };
        let navigable = |inputs: &Inputs, id: &str| {
            page_rows(RETRO_ACHIEVEMENTS_PAGE, inputs)
                .into_iter()
                .any(|row| row.id() == id && row.is_navigable())
        };
        assert!(navigable(&fresh, "raInstall"));
        assert!(navigable(
            &Inputs {
                ra_installed: true,
                ra_conflict: true,
                ..fresh
            },
            "raConflict"
        ));
    }

    #[test]
    fn the_system_page_leads_with_pairing() {
        let rows = page_rows(
            "pageSupportAbout",
            &Inputs {
                log_upload: true,
                ..Inputs::default()
            },
        );
        assert_eq!(
            rows[0],
            Row::Field {
                id: "pairDevice",
                control: Control::Action
            }
        );
        assert_eq!(first_navigable(&rows), 0);
        let ids: Vec<&str> = rows.iter().map(|r| r.id()).collect();
        assert_eq!(
            ids,
            vec![
                "pairDevice",
                PAIRED_DEVICES_PAGE,
                LANGUAGE_PAGE,
                "debugLogging",
                "uploadLog",
                "documentation",
                "aboutLicense",
                "credits",
            ]
        );
        // Pairing has nothing to do with a media job, so neither job can
        // gate it and it is never the busy one.
        assert!(!action_busy("pairDevice", true, true));
        assert!(!action_disabled("pairDevice", true, true));
        assert_eq!(action_label_key("pairDevice", false), "pair");
    }

    #[test]
    fn the_controls_page_leads_with_one_controllers_row() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageControlsInput", inputs)
                .into_iter()
                .map(Row::id)
                .collect()
        };
        // An older Core reports no controllers and cannot pair: the page
        // is the input settings alone.
        let input = [
            "buttonLayout",
            "swapConfirmCancel",
            "swapOptionsView",
            "mouseEnabled",
        ];
        assert_eq!(ids(&Inputs::default()), input);
        for inputs in [
            Inputs {
                controllers_listed: true,
                ..Inputs::default()
            },
            Inputs {
                can_pair_controllers: true,
                ..Inputs::default()
            },
        ] {
            let got = ids(&inputs);
            assert_eq!(&got[..1], &["controllers"]);
            assert_eq!(&got[1..], &input);
        }
        // It opens a page of its own, so it reads as one, chevron and all.
        assert_eq!(control("controllers"), Control::Navigate);
        assert!(opens_page(CONTROLLERS_PAGE).is_none(), "not a root tile");
        assert!(opens_page(PAIRED_PAGE).is_none(), "not a root tile");
    }

    #[test]
    fn finds_land_before_the_paired_controllers_row() {
        let page = |found| {
            page_rows(
                CONTROLLERS_PAGE,
                &Inputs {
                    can_pair_controllers: true,
                    bluetooth_found: found,
                    ..Inputs::default()
                },
            )
        };
        // Nothing found yet: the cursor opens on Paired controllers.
        let empty = page(0);
        assert_eq!(empty.len(), 1);
        assert_eq!(first_navigable(&empty), 0);
        assert_eq!(empty[0].id(), "pairedControllers");
        assert_eq!(control("pairedControllers"), Control::Navigate);
        let two = page(2);
        let three = page(3);
        assert_eq!(
            two[1],
            Row::Item {
                id: "foundDevice",
                index: 1,
                control: Control::Device
            }
        );
        // The rows already there keep their indices as one is appended,
        // and the cursor follows Paired controllers by what it is.
        assert_eq!(&three[..2], &two[..2]);
        let key = |rows: &[Row]| {
            let rows = rows.to_vec();
            move |i: usize| rows[i].id()
        };
        assert_eq!(follow_seat(&three, key(&three), &"pairedControllers", 2), 3);
        // Up from Paired controllers reaches the newest find; Down from
        // the last find comes back.
        assert_eq!(seek_navigable(&three, 3, -1), 2);
        assert_eq!(seek_navigable(&three, 2, 1), 3);
        // Without pairing the page is its cards alone.
        assert!(page_rows(CONTROLLERS_PAGE, &Inputs::default()).is_empty());
    }

    #[test]
    fn the_paired_page_lists_devices_or_says_there_are_none() {
        let page = |paired| {
            page_rows(
                PAIRED_PAGE,
                &Inputs {
                    bluetooth_paired: paired,
                    ..Inputs::default()
                },
            )
        };
        let none = page(0);
        assert_eq!(none, vec![field("noPairedControllers")]);
        assert!(!none[0].is_navigable());
        let two = page(2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[1].item(), Some(1));
        assert!(two[0].is_navigable(), "a device row takes focus");
    }

    #[test]
    fn the_cursor_follows_its_entry_when_one_before_it_goes() {
        const BEFORE: [&str; 3] = ["a", "b", "c"];
        const AFTER: [&str; 2] = ["a", "c"];
        let rows = |n: usize| {
            page_rows(
                PAIRED_PAGE,
                &Inputs {
                    bluetooth_paired: n,
                    ..Inputs::default()
                },
            )
        };
        let key = |list: &'static [&'static str]| move |row: usize| list.get(row).copied();
        let focused = key(&BEFORE)(2);
        assert_eq!(follow_seat(&rows(2), key(&AFTER), &focused, 2), 1);
        // The focused one went: the nearest row it can rest on.
        assert_eq!(follow_seat(&rows(2), key(&AFTER), &Some("b"), 1), 1);
    }

    #[test]
    fn unknown_page_has_no_rows() {
        assert!(page_rows("pageNope", &Inputs::default()).is_empty());
    }

    #[test]
    fn appearance_offers_the_logo_image_only_for_a_custom_logo() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageAppearance", inputs)
                .iter()
                .map(|r| r.id())
                .collect()
        };
        let plain = ids(&Inputs::default());
        let at = plain.iter().position(|id| *id == "headerLogo").unwrap();
        assert_eq!(plain[at - 1], "systemLogoStyle");
        assert_eq!(plain[at + 1], "showMenuRow");
        let motion = plain.iter().position(|id| *id == "reduceMotion").unwrap();
        assert_eq!(plain[motion + 1], "screensaver");
        assert!(!plain.contains(&"headerLogoImage"));
        let custom = ids(&Inputs {
            header_logo_custom: true,
            ..Inputs::default()
        });
        assert_eq!(custom[at + 1], "headerLogoImage");
        assert_eq!(control("headerLogo"), Control::Picker);
        assert_eq!(control("headerLogoImage"), Control::Navigate);
        assert_eq!(
            options("headerLogo", &Inputs::default()).unwrap(),
            crate::branding::HEADER_LOGOS
        );
    }

    #[test]
    fn appearance_offers_the_background_image_only_once_one_is_set() {
        let ids = |inputs: &Inputs| -> Vec<&'static str> {
            page_rows("pageAppearance", inputs)
                .iter()
                .map(|r| r.id())
                .collect()
        };
        let plain = ids(&Inputs::default());
        let at = plain
            .iter()
            .position(|id| *id == "backgroundSource")
            .unwrap();
        assert_eq!(plain[at - 1], "background");
        assert_eq!(plain[at + 1], "gameBackground");
        assert!(!plain.contains(&"backgroundImage"));
        assert!(!plain.contains(&"backgroundBlur"));
        let custom = ids(&Inputs {
            background_custom: true,
            background_active: true,
            ..Inputs::default()
        });
        assert_eq!(custom[at + 1], "backgroundImage");
        assert_eq!(custom[at + 2], "gameBackground");
        assert!(custom.contains(&"backgroundBlur"));
        assert_eq!(control("backgroundSource"), Control::Picker);
        assert_eq!(control("backgroundImage"), Control::Navigate);
        assert_eq!(
            options("backgroundSource", &Inputs::default()).unwrap(),
            ["none", "custom"]
        );
        assert_eq!(crate::background::source(""), "none");
        assert_eq!(crate::background::source("/media/fat/a.png"), "custom");
    }

    #[test]
    fn option_lists_are_the_canonical_ones() {
        assert_eq!(
            options("clockFormat", &Inputs::default()).unwrap(),
            CLOCK_FORMATS
        );
        assert_eq!(
            options("gamesLayout", &Inputs::default()).unwrap(),
            BROWSE_LAYOUTS
        );
        assert_eq!(options("resolution", &Inputs::default()), None);
        assert_eq!(options("colorScheme", &Inputs::default()), None);
        assert_eq!(options("showHidden", &Inputs::default()), None);
    }

    #[test]
    fn the_screensaver_page_offers_bouncing_only_for_styles_that_bounce() {
        let ids = |bounces| -> Vec<&'static str> {
            page_rows(
                SCREENSAVER_PAGE,
                &Inputs {
                    screensaver_bounces: bounces,
                    ..Inputs::default()
                },
            )
            .iter()
            .map(|r| r.id())
            .collect()
        };
        assert_eq!(ids(false), ["screensaverTimeout", "screensaverStyle"]);
        assert_eq!(
            ids(true),
            [
                "screensaverTimeout",
                "screensaverStyle",
                "screensaverBouncing"
            ]
        );
        assert_eq!(control("screensaver"), Control::Navigate);
        assert!(opens_page(SCREENSAVER_PAGE).is_none(), "not a root tile");
        assert_eq!(control("screensaverStyle"), Control::Picker);
        assert_eq!(
            options("screensaverStyle", &Inputs::default()).unwrap(),
            crate::screensaver::STYLES
        );
        assert_eq!(
            options("screensaverBouncing", &Inputs::default()).unwrap(),
            crate::screensaver::BOUNCINGS
        );
        assert!(!page_rows("pageAppearance", &Inputs::default())
            .iter()
            .any(|r| r.id() == "screensaverTimeout"));
    }

    #[test]
    fn a_debug_build_offers_the_one_second_screensaver() {
        let release = options("screensaverTimeout", &Inputs::default()).unwrap();
        assert_eq!(release, SCREENSAVER_TIMEOUTS);
        let debug = options(
            "screensaverTimeout",
            &Inputs {
                debug_build: true,
                ..Inputs::default()
            },
        )
        .unwrap();
        assert_eq!(debug[0], "off");
        assert_eq!(debug[1], "1");
        assert_eq!(debug.len(), SCREENSAVER_TIMEOUTS.len() + 1);
    }

    #[test]
    fn media_jobs_share_their_row_labels_and_gates() {
        assert_eq!(action_label_key("updateMediaDb", false), "configure");
        assert_eq!(action_label_key("updateMediaDb", true), "cancel");
        assert_eq!(action_label_key("runScraper", true), "cancel");
        assert_eq!(action_label_key("uploadLog", false), "upload");
        assert_eq!(action_label_key("aboutLicense", false), "open");
        assert!(action_busy("updateMediaDb", true, false));
        assert!(!action_busy("updateMediaDb", false, true));
        assert!(action_disabled("updateMediaDb", false, true));
        assert!(action_disabled("runScraper", true, false));
        assert!(!action_disabled("uploadLog", true, true));
    }

    #[test]
    fn restart_rows_are_the_documented_ones() {
        for id in [
            "resolution",
            "crtEnabled",
            "crtVideoStandard",
            "language",
            "debugLogging",
        ] {
            assert!(restarts(id), "{id} should restart");
        }
        for id in ["orientation", "colorScheme", "showHidden"] {
            assert!(!restarts(id), "{id} should not restart");
        }
    }

    #[test]
    fn the_cursor_skips_headers_and_wraps() {
        let rows = page_rows(
            "pageLibraryData",
            &Inputs {
                launcher_preference: true,
                ..Inputs::default()
            },
        );
        assert_eq!(first_navigable(&rows), 1);
        // Down off the last maintenance row lands past the next header.
        assert_eq!(seek_navigable(&rows, 2, 1), 4);
        // Up off the first field wraps to the last.
        assert_eq!(seek_navigable(&rows, 1, -1), rows.len() - 1);
        assert_eq!(seek_navigable(&rows, rows.len() - 1, 1), 1);
        assert_eq!(seek_navigable(&[], 0, 1), 0);
        assert_eq!(first_navigable(&[Row::Header("x")]), 0);
    }

    #[test]
    fn root_grid_is_two_rows_transposed_when_rotated() {
        assert_eq!(root_grid_shape(6, false), (3, 2));
        assert_eq!(root_grid_shape(6, true), (2, 3));
    }

    #[test]
    fn root_grid_moves_wrap_within_the_row_and_around_the_column() {
        // 6 tiles, 3 columns: 0 1 2 / 3 4 5.
        assert_eq!(root_grid_move(0, 6, 3, 1, 0), 1);
        assert_eq!(root_grid_move(2, 6, 3, 1, 0), 0);
        assert_eq!(root_grid_move(3, 6, 3, -1, 0), 5);
        assert_eq!(root_grid_move(1, 6, 3, 0, 1), 4);
        assert_eq!(root_grid_move(4, 6, 3, 0, 1), 1);
        assert_eq!(root_grid_move(1, 6, 3, 0, -1), 4);
        assert_eq!(root_grid_move(0, 0, 3, 1, 0), 0);
        // A short last row: the column with a tile below steps into it,
        // the one without wraps back to the top of its own column.
        assert_eq!(root_grid_move(0, 5, 3, 0, 1), 3);
        assert_eq!(root_grid_move(2, 5, 3, 0, 1), 2);
    }

    /// The CRT scene: 352x240 less the 5% safe area, on the bitmap face.
    fn crt_scene() -> crate::sizing::Inputs {
        crate::sizing::Inputs {
            screen_width: 316.0,
            screen_height: 216.0,
            crt_native_path: true,
            bitmap_type: true,
            ..crate::sizing::Inputs::default()
        }
    }

    #[test]
    fn a_bitmap_hint_line_is_the_font_size() {
        use super::page_geometry;
        // Two 8px lines, not two 11px ones: the face has no leading.
        let crt = page_geometry(&crt_scene());
        assert_eq!(crt.hint_height, 16);
        // The scaled face keeps its 1.362 em line: 10px body at 240p is
        // 14px a line and two lines, 19px body at 720p is 26px and one.
        let scaled_240 = page_geometry(&crate::sizing::Inputs {
            screen_width: 320.0,
            screen_height: 240.0,
            ..crate::sizing::Inputs::default()
        });
        assert_eq!(scaled_240.hint_height, 28);
        let hd = page_geometry(&crate::sizing::Inputs {
            screen_width: 1280.0,
            screen_height: 720.0,
            ..crate::sizing::Inputs::default()
        });
        assert_eq!(hd.hint_height, 26);
        // Headings and status lines are not hint lines and keep the
        // scaled figure on both faces.
        assert_eq!(crt.header_height, 3 + 11 + 2 + 1);
        assert_eq!(crt.action_band_height, 11 + 1);
    }

    #[test]
    fn the_page_card_is_whole_pixels_that_add_up() {
        use super::page_geometry;
        let g = page_geometry(&crt_scene());
        assert_eq!((g.card_y, g.card_height, g.pad), (33, 154, 4));
        assert_eq!(g.row_height, 17);
        assert_eq!(g.rows_viewport, 120);
        // Top to bottom: lip, rows, lip, the clear line, the rule, lip,
        // hint, lip.
        for inputs in [
            crt_scene(),
            crate::sizing::Inputs {
                help_bar_two_rows: true,
                ..crt_scene()
            },
            crate::sizing::Inputs {
                screen_width: 1920.0,
                screen_height: 1080.0,
                ..crate::sizing::Inputs::default()
            },
        ] {
            let g = page_geometry(&inputs);
            assert_eq!(
                g.pad + g.rows_viewport + g.pad + 1 + 1 + g.pad + g.hint_height + g.pad,
                g.card_height,
                "{inputs:?}"
            );
        }
    }

    #[test]
    fn a_wrapped_help_bar_takes_its_rows_from_the_card() {
        use super::page_geometry;
        let one = page_geometry(&crt_scene());
        let two = page_geometry(&crate::sizing::Inputs {
            help_bar_two_rows: true,
            ..crt_scene()
        });
        // The bar grows from 20px to 38px and the card gives up exactly
        // that; nothing above it moves.
        assert_eq!(two.card_y, one.card_y);
        assert_eq!(one.card_height - two.card_height, 18);
        assert_eq!(one.rows_viewport - two.rows_viewport, 18);
    }

    #[test]
    fn band_scrolls_to_a_row_top_and_keeps_a_fixed_height() {
        use super::band_extent;
        // Six 60px rows in a band that fits 3.5 of them, with a 10px lip.
        let rows: Vec<(f32, f32)> = (0..6).map(|i| (i as f32 * 60.0, 60.0)).collect();
        // Focus at the top: no scroll. The band paints its full height
        // every time, so its bottom edge never moves.
        assert_eq!(band_extent(&rows, 0, 210.0, 10.0), (0.0, 230.0));
        assert_eq!(band_extent(&rows, 2, 210.0, 10.0), (0.0, 230.0));
        // Focus below the fold: it scrolls the least it can, to a row top,
        // and the top lip has scrolled away with the rows above it.
        assert_eq!(band_extent(&rows, 4, 210.0, 10.0), (130.0, 230.0));
        assert_eq!(band_extent(&rows, 5, 210.0, 10.0), (190.0, 230.0));
        // Coming back up returns to the top rather than hanging mid-row.
        assert_eq!(band_extent(&rows, 1, 210.0, 10.0), (0.0, 230.0));
    }

    #[test]
    fn the_focused_row_keeps_a_lip_above_the_bottom_edge() {
        use super::band_extent;
        // A header (30) then 60px rows; the band shows 200 plus two 10px
        // lips. Row 4 ends at 270, so 70 must scroll away; the first row
        // top at or past that is 90.
        let rows = [
            (0.0_f32, 30.0_f32),
            (30.0, 60.0),
            (90.0, 60.0),
            (150.0, 60.0),
            (210.0, 60.0),
        ];
        let (scroll, shown) = band_extent(&rows, 4, 200.0, 10.0);
        assert_eq!((scroll, shown), (100.0, 220.0));
        // In lip-inclusive space the focused row's bottom sits at
        // 10 + 270 - 100 = 180, at least one lip above the 220 edge.
        assert!(10.0 + 270.0 - scroll <= shown - 10.0);
    }

    #[test]
    fn a_row_taller_than_the_band_sits_on_its_own_top() {
        use super::band_extent;
        let rows = [(0.0_f32, 500.0_f32)];
        assert_eq!(band_extent(&rows, 0, 200.0, 10.0), (0.0, 220.0));
        let rows = [(0.0_f32, 60.0_f32), (60.0, 500.0)];
        assert_eq!(band_extent(&rows, 1, 200.0, 10.0), (70.0, 220.0));
    }

    #[test]
    fn an_out_of_range_index_does_not_collapse_the_band() {
        use super::band_extent;
        let rows = [(0.0_f32, 60.0_f32)];
        assert_eq!(band_extent(&rows, 9, 200.0, 10.0), (0.0, 220.0));
    }
}

#[cfg(test)]
mod profile_rules_tests {
    use super::*;

    #[test]
    fn the_gate_is_free_without_profiles_or_admins_and_after_an_unlock() {
        assert!(!admin_required(0, 0, false));
        assert!(!admin_required(2, 0, false));
        assert!(admin_required(2, 1, false));
        assert!(!admin_required(2, 1, true));
        assert!(verified_admin("admin"));
        assert!(!verified_admin("member"));
    }

    #[test]
    fn only_the_last_admin_is_kept_from_removal() {
        assert!(last_admin(true, 1));
        assert!(!last_admin(true, 2));
        assert!(!last_admin(false, 1));
    }

    #[test]
    fn gated_pages_and_rows_are_the_management_ones() {
        for page in [
            "pageOnline",
            GAME_VERSIONS_PAGE,
            PAIRED_DEVICES_PAGE,
            PROFILE_EDIT_PAGE,
            PLAYTIME_LIMITS_PAGE,
            "pageMappings",
        ] {
            assert!(gated_page(page), "{page}");
        }
        for page in [PROFILES_PAGE, SYSTEM_PAGE, READERS_PAGE, LANGUAGE_PAGE, ""] {
            assert!(!gated_page(page), "{page}");
        }
        for row in [
            "updateMediaDb",
            "pairDevice",
            "encryption",
            "newProfile",
            "chooseProfileAtStartup",
            "mapToken",
        ] {
            assert!(gated_row(row), "{row}");
        }
        for row in ["readToken", "debugLogging", "leaveProfile", "language"] {
            assert!(!gated_row(row), "{row}");
        }
    }

    #[test]
    fn only_the_ungated_core_pages_keep_their_row() {
        for page in [READERS_PAGE, PROFILES_PAGE, SYSTEM_PAGE] {
            assert!(kept_page(page) && !gated_page(page), "{page}");
        }
        for page in [
            "pageMappings",
            PROFILE_EDIT_PAGE,
            LANGUAGE_PAGE,
            "pageAppearance",
            "",
        ] {
            assert!(!kept_page(page), "{page}");
        }
    }

    #[test]
    fn a_pin_is_four_to_eight_digits() {
        assert!(!pin_complete("123"));
        assert!(pin_complete("1234"));
        assert!(pin_complete("12345678"));
        assert!(!pin_complete("123456789"));
        assert!(!pin_complete("12a4"));
    }

    #[test]
    fn debug_logging_is_on_only_when_both_are() {
        assert!(debug_logging_on(true, Some(true)));
        assert!(!debug_logging_on(true, Some(false)));
        assert!(!debug_logging_on(false, Some(true)));
        assert!(debug_logging_on(true, None));
        assert!(!debug_logging_on(false, None));
    }

    #[test]
    fn go_durations_read_as_minutes() {
        assert_eq!(duration_minutes("0"), Some(0));
        assert_eq!(duration_minutes("0s"), Some(0));
        assert_eq!(duration_minutes("1h30m0s"), Some(90));
        assert_eq!(duration_minutes("45m"), Some(45));
        assert_eq!(duration_minutes("2h"), Some(120));
        assert_eq!(duration_minutes("1.5h"), Some(90));
        assert_eq!(duration_minutes("90s"), Some(2));
        assert_eq!(duration_minutes(""), None);
        assert_eq!(duration_minutes("soon"), None);
        assert_eq!(duration_minutes("5x"), None);
        assert_eq!(limit_token(None), "default");
        assert_eq!(limit_token(Some("1h0m0s")), "60");
        assert_eq!(limit_duration("90"), "90m");
        assert_eq!(limit_mode_token(None), "default");
        assert_eq!(limit_mode_token(Some(false)), "off");
    }

    #[test]
    fn game_versions_toggle_against_core_defaults() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(
            effective_versions(&empty, DEFAULT_GAME_REGIONS),
            ["us", "world"]
        );
        assert_eq!(
            toggle_version(&empty, DEFAULT_GAME_REGIONS, "jp", true),
            ["us", "world", "jp"]
        );
        assert_eq!(
            toggle_version(&empty, DEFAULT_GAME_REGIONS, "us", false),
            ["world"]
        );
        let one = vec!["en".to_string()];
        assert!(last_version(&one, DEFAULT_GAME_LANGS, "en"));
        assert!(last_version(&empty, DEFAULT_GAME_LANGS, "en"));
        assert!(!last_version(&empty, DEFAULT_GAME_REGIONS, "us"));
        assert!(!last_version(&one, DEFAULT_GAME_LANGS, "fr"));
        let odd = vec!["xx".to_string(), "us".to_string()];
        let choices = game_version_choices(GAME_REGIONS, &odd);
        assert_eq!(choices.len(), GAME_REGIONS.len() + 1);
        assert_eq!(choices.last().map(String::as_str), Some("xx"));
    }

    fn with(f: impl FnOnce(&mut Inputs)) -> Inputs {
        let mut inputs = Inputs::default();
        f(&mut inputs);
        inputs
    }

    #[test]
    fn system_page_lists_pairing_security_language_help_then_identity() {
        let ids = |inputs: &Inputs| -> Vec<&str> {
            page_rows(SYSTEM_PAGE, inputs)
                .iter()
                .map(|r| r.id())
                .collect()
        };
        assert_eq!(
            ids(&with(|i| {
                i.core_settings = true;
                i.log_upload = true;
            })),
            [
                "pairDevice",
                PAIRED_DEVICES_PAGE,
                "encryption",
                LANGUAGE_PAGE,
                "debugLogging",
                "errorReporting",
                "uploadLog",
                "documentation",
                "aboutLicense",
                "credits",
            ]
        );
        assert_eq!(
            ids(&Inputs::default()),
            [
                "pairDevice",
                PAIRED_DEVICES_PAGE,
                LANGUAGE_PAGE,
                "debugLogging",
                "documentation",
                "aboutLicense",
                "credits",
            ]
        );
        assert_eq!(opens_page(LANGUAGE_PAGE), Some(LANGUAGE_PAGE));
        assert_eq!(control(PAIRED_DEVICES_PAGE), Control::Navigate);
    }

    #[test]
    fn profile_pages_follow_what_core_has() {
        let none = page_rows(PROFILES_PAGE, &Inputs::default());
        assert_eq!(none[0].id(), "noProfiles");
        assert_eq!(none[first_navigable(&none)].id(), "newProfile");
        let some = page_rows(
            PROFILES_PAGE,
            &with(|i| {
                i.profiles = 2;
                i.active_profile = true;
            }),
        );
        assert_eq!(some[1].item(), Some(1));
        assert_eq!(some[2].id(), "leaveProfile");
        let last = some.last().copied();
        assert_eq!(last.map(Row::id), Some("chooseProfileAtStartup"));
        assert_eq!(last.and_then(Row::control), Some(Control::Toggle));

        let edit = |inputs: &Inputs| -> Vec<&str> {
            page_rows(PROFILE_EDIT_PAGE, inputs)
                .iter()
                .map(|r| r.id())
                .collect()
        };
        let full = edit(&with(|i| i.profile_has_pin = true));
        assert!(full.contains(&"profileClearPin"));
        assert!(full.contains(&"profileDailyLimit"));
        let off = edit(&with(|i| i.profile_limits_off = true));
        assert!(!off.contains(&"profileClearPin"));
        assert!(!off.contains(&"profileSessionLimit"));

        let limits = |inputs: &Inputs| page_rows(PLAYTIME_LIMITS_PAGE, inputs).len();
        assert_eq!(limits(&Inputs::default()), 2);
        assert_eq!(limits(&with(|i| i.playtime_enabled = true)), 4);
    }

    #[test]
    fn game_versions_and_paired_devices_list_their_entries() {
        let rows = page_rows(
            GAME_VERSIONS_PAGE,
            &with(|i| {
                i.game_regions = 3;
                i.game_langs = 2;
            }),
        );
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[4], Row::Header("languages"));
        assert_eq!(rows[1].control(), Some(Control::Toggle));
        let none = page_rows(PAIRED_DEVICES_PAGE, &Inputs::default());
        assert_eq!(
            none,
            [Row::Field {
                id: "noPairedDevices",
                control: Control::Info
            }]
        );
        let two = page_rows(PAIRED_DEVICES_PAGE, &with(|i| i.paired_devices = 2));
        assert_eq!(two.len(), 2);
        assert!(
            page_rows("pageLibraryData", &with(|i| i.core_settings = true))
                .iter()
                .any(|r| r.id() == GAME_VERSIONS_PAGE)
        );
    }

    #[test]
    fn music_and_language_keep_their_glyphs_off_the_root() {
        assert_eq!(page_glyph(MUSIC_PAGE), Some("icons/Music"));
        assert_eq!(page_glyph(LANGUAGE_PAGE), Some("icons/Language"));
        assert_eq!(page_glyph(PROFILES_PAGE), Some("icons/Users"));
        assert!(!PAGES
            .iter()
            .any(|p| p.id == MUSIC_PAGE || p.id == LANGUAGE_PAGE));
    }
}
