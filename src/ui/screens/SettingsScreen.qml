// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so qinvokable calls on
// Zaparoo.Browse singletons still trip qmllint's "Member can be shadowed"
// check. Until the schema grows method-level finality, suppress the compiler
// category file-wide.
// qmllint disable compiler

// Settings screen — gamepad-driven vertical form. Button style selects the
// help-bar icon style (Automatic, or a fixed Style A/B/C/D/E →
// resources/images/buttons/<id>/). Automatic follows the connected
// controller via Browse.ControllerReport; the ids stay the same neutral
// letters this picker always used — never named after a controller maker.
// Mouse support is cross-platform and controls cursor visibility plus mouse
// hit targets.
//
// Pure input dispatcher: root rows open category subpages; subpage
// rows open pickers, toggle values, or emit router actions. Escape
// returns from subpage to root, then from root to Hub.
Item {
    id: settings

    Component.onCompleted: console.debug("startup/qml component SettingsScreen completed")

    // Bound by MainLayout to `root.pendingTransition !== ""`. Settings
    // is a destination, never a source, so this is currently always
    // false when the screen is visible — kept for parity with the
    // other screens so the convention holds when a future routing
    // change adds a Settings-as-source path.
    property bool transitioning: false
    property bool optimisticLoading: false

    signal requestHubScreen
    // Forward signal carrying the focused action row's id. The router
    // decides what the payload means — `"uploadLog"` opens the log-
    // upload modal, `"aboutLicense"` navigates to the About screen.
    signal requestAccept(actionId: string)
    // Picker request. The router mounts `ListPickerModal` with these
    // properties and dispatches the user's selection back through the
    // matching `Browse.Settings` setter (keyed off `fieldId`).
    signal requestListPicker(title: string, entries: var, initialId: string, fieldId: string)

    readonly property string pageRoot: "root"
    readonly property string pageAppearance: "appearance"
    readonly property string pageDisplayInterface: "displayInterface"
    readonly property string pageLanguage: "language"
    readonly property string pageControlsInput: "controlsInput"
    readonly property string pageLibraryData: "libraryData"
    readonly property string pageSupportAbout: "supportAbout"
    property string currentPage: settings.pageRoot
    readonly property bool showingRootGrid: settings.currentPage === settings.pageRoot
    property var _pageIndexes: ({})
    // Incremented when the user accepts a category tile so it plays the
    // push-in animation. Forwarded to all category TileLoaders.
    property int activatePulse: 0
    // Sibling of `activatePulse` for the in-page SettingsField rows: bumped on
    // a field accept so the focused non-toggle row plays its push-in tap.
    // Toggle rows ignore it (their knob slide is the feedback).
    property int fieldActivatePulse: 0
    // True for one event-loop tick during a page switch. Passed as
    // `animateChanges: false` to SettingsField delegates so a reused delegate
    // does not animate its toggle-knob slide when the new page's field model
    // lands. Normal user navigation animates because `_pageSwitching` is false
    // at that point.
    property bool _pageSwitching: false

    // Page-aware field registries. The root mirrors console settings
    // menus: stable domain categories first, short subpages second. The
    // six domains are grouped by what the user is trying to do, not by
    // which Rust module or QML file backs the setting — see
    // docs/content-style.md's "Adding a setting" checklist before adding
    // a new row. Appearance = how it looks; Library = the game
    // collection, browsing and maintenance; Display = video output
    // geometry; Controls = input; Language = locale; About = identity,
    // legal, and diagnostics. Future Core features should land in one of
    // these six rather than a vague Advanced bucket.
    readonly property var categoryFields: [
        {
            kind: "field",
            id: "pageAppearance",
            label: qsTr("Appearance"),
            coverKey: "icons/Appearance"
        },
        {
            kind: "field",
            id: "pageLibraryData",
            label: qsTr("Library"),
            coverKey: "icons/Library"
        },
        {
            kind: "field",
            id: "pageDisplayInterface",
            label: qsTr("Display"),
            coverKey: "icons/Display"
        },
        {
            kind: "field",
            id: "pageControlsInput",
            label: qsTr("Controls"),
            coverKey: "icons/Controls"
        },
        {
            kind: "field",
            id: "pageLanguage",
            label: qsTr("Language"),
            coverKey: "icons/Language"
        },
        {
            kind: "field",
            id: "pageSupportAbout",
            label: qsTr("About"),
            coverKey: "icons/Support"
        }
    ]
    // Appearance = how the interface looks: theme, logos, motion, screensaver.
    readonly property var appearanceFields: [
        {
            kind: "field",
            id: "colorScheme",
            label: qsTr("Color scheme"),
            description: qsTr("Changes colors across screens, including text and highlights.")
        },
        {
            kind: "field",
            id: "colorIntensity",
            label: qsTr("Color intensity"),
            description: qsTr("Controls how strongly accent colors tint tiles and backgrounds.")
        },
        {
            kind: "field",
            id: "systemLogoStyle",
            label: qsTr("System logos"),
            description: qsTr("Shows system logos in original colors or tinted to match colors.")
        },
        {
            kind: "field",
            id: "reduceMotion",
            label: qsTr("Reduce motion"),
            description: qsTr("Turns off animation throughout the interface.")
        },
        {
            kind: "field",
            id: "screensaverTimeout",
            label: qsTr("Screensaver"),
            description: qsTr("Sets idle time before the screensaver starts. Off disables it.")
        }
    ]
    // Display = video output geometry — resolution, orientation, analog video.
    readonly property var displayInterfaceFields: {
        const out = [];
        if (Browse.Settings.is_mister && !Browse.CrtVideo.crt_enabled) {
            out.push({
                kind: "field",
                id: "resolution",
                label: qsTr("Interface resolution"),
                description: qsTr("The resolution the interface renders at. Restarts the frontend.")
            });
        }
        out.push({
            kind: "field",
            id: "orientation",
            label: qsTr("Orientation"),
            description: qsTr("Rotates the interface for a sideways screen or tate cabinet.")
        });
        if (Browse.Settings.is_mister) {
            // Native CRT video path (Menu fork DDR writer). The toggle is
            // always offered on MiSTer so HDMI users can switch in; the
            // standard picker and calibration screen only matter once the
            // frontend is running with --crt.
            out.push({
                kind: "header",
                label: qsTr("Analog video")
            });
            out.push({
                kind: "field",
                id: "crtEnabled",
                label: qsTr("CRT mode"),
                description: qsTr("Outputs 15 kHz analog video through MiSTer. Restarts frontend.")
            });
            if (Browse.CrtVideo.crt_enabled) {
                out.push({
                    kind: "field",
                    id: "crtVideoStandard",
                    label: qsTr("Video standard"),
                    description: qsTr("Matches analog output to NTSC or PAL. Restarts the frontend.")
                });
                out.push({
                    kind: "field",
                    id: "crtCalibration",
                    label: qsTr("Screen position"),
                    description: qsTr("Opens a test pattern to position the analog picture with arrows.")
                });
            }
        }
        return out;
    }
    // Language = locale/regional preferences.
    readonly property var languageFields: [
        {
            kind: "field",
            id: "language",
            label: qsTr("Language"),
            description: qsTr("The language used for interface text. Restarts the frontend.")
        },
        {
            kind: "field",
            id: "region",
            label: qsTr("Region"),
            description: qsTr("Selects regional system and game names, such as Mega Drive.")
        },
        {
            kind: "field",
            id: "clockFormat",
            label: qsTr("Clock format"),
            description: qsTr("12-hour or 24-hour time in the top bar.")
        }
    ]
    readonly property var controlsInputFields: [
        {
            kind: "field",
            id: "buttonLayout",
            label: qsTr("Button style"),
            description: qsTr("Matches on-screen prompt icons to your controller.")
        },
        {
            kind: "field",
            id: "swapConfirmCancel",
            label: qsTr("Swap controller confirm/cancel"),
            description: qsTr("Flips which controller button accepts and which cancels.")
        },
        {
            kind: "field",
            id: "swapOptionsView",
            label: qsTr("Swap controller options/view"),
            description: qsTr("Swaps which controller button opens Options and View.")
        },
        {
            kind: "field",
            id: "mouseEnabled",
            label: qsTr("Mouse support"),
            description: qsTr("Lets a mouse click tiles and menu rows.")
        }
    ]
    // Library = the game collection: how it's browsed, and how it's scanned.
    // Round 10: Maintenance moved above Browsing -- these are rows a user
    // actively comes here to DO (add new games, fix missing art), not a
    // one-time preference, unlike everything under Browsing. Kept on this
    // page rather than promoted to its own root tile so soon after round
    // 9's consolidation; see docs/content-style.md's "Adding a setting"
    // checklist for the description-vs-label tradeoff behind every row
    // below now carrying one.
    readonly property var libraryDataFields: [
        {
            kind: "header",
            label: qsTr("Maintenance")
        },
        {
            kind: "field",
            id: "updateMediaDb",
            label: qsTr("Update media database"),
            description: qsTr("Rescans game folders for added or removed files. Choose systems.")
        },
        {
            kind: "field",
            id: "runScraper",
            label: qsTr("Update metadata"),
            description: qsTr("Imports artwork and details from your files. Downloads nothing.")
        },
        {
            kind: "header",
            label: qsTr("Browsing")
        },
        {
            kind: "field",
            id: "systemsLayout",
            label: qsTr("Systems layout"),
            description: qsTr("Shows systems as a cover grid or a list with details.")
        },
        {
            kind: "field",
            id: "gamesLayout",
            label: qsTr("Games layout"),
            description: qsTr("Sets grid or list layout for Games, Favorites, and Recents.")
        },
        {
            kind: "field",
            id: "mediaImageType",
            label: qsTr("Preferred artwork"),
            description: qsTr("Prefers an artwork type when games have multiple images.")
        },
        {
            kind: "field",
            id: "showHidden",
            label: qsTr("Show hidden items"),
            description: qsTr("Shows hidden systems and categories so you can unhide them.")
        },
        {
            kind: "field",
            id: "showOriginalFilenames",
            label: qsTr("Show original filenames"),
            description: qsTr("Shows real filenames instead of cleaned-up display titles.")
        }
    ]
    readonly property var supportAboutFields: [
        {
            kind: "field",
            id: "aboutLicense",
            label: qsTr("About / License"),
            description: qsTr("Version, build date, license terms, and contributors.")
        },
        {
            kind: "field",
            id: "documentation",
            label: qsTr("Documentation"),
            description: qsTr("Shows a code for opening the Frontend guide on your phone.")
        },
        {
            kind: "field",
            id: "debugLogging",
            label: qsTr("Debug logging"),
            description: qsTr("Adds troubleshooting detail to logs. Restarts the frontend.")
        },
        {
            kind: "field",
            id: "uploadLog",
            label: qsTr("Upload log file"),
            description: qsTr("Uploads the log and provides a link for problem reports.")
        }
    ]
    readonly property var fields: {
        if (settings.currentPage === settings.pageAppearance)
            return settings.appearanceFields;
        if (settings.currentPage === settings.pageDisplayInterface)
            return settings.displayInterfaceFields;
        if (settings.currentPage === settings.pageLanguage)
            return settings.languageFields;
        if (settings.currentPage === settings.pageControlsInput)
            return settings.controlsInputFields;
        if (settings.currentPage === settings.pageLibraryData)
            return settings.libraryDataFields;
        if (settings.currentPage === settings.pageSupportAbout)
            return settings.supportAboutFields;
        return settings.categoryFields;
    }
    readonly property string pageTitle: {
        if (settings.currentPage === settings.pageAppearance)
            return qsTr("Appearance");
        if (settings.currentPage === settings.pageDisplayInterface)
            return qsTr("Display");
        if (settings.currentPage === settings.pageLanguage)
            return qsTr("Language");
        if (settings.currentPage === settings.pageControlsInput)
            return qsTr("Controls");
        if (settings.currentPage === settings.pageLibraryData)
            return qsTr("Library");
        if (settings.currentPage === settings.pageSupportAbout)
            return qsTr("About");
        return qsTr("Settings");
    }

    // Live-state caption helpers for the action rows. While the matching
    // operation is in flight we paint the same vocabulary as the Core TUI
    // (Optimizing / In progress / Paused). When idle, fall back to a
    // count summary so the user can see at a glance how much is indexed
    // / scraped without having to start a job. The fields used here
    // mirror the TUI's `formatDBMenuLabel` and `formatScrapeMenuLabel`:
    // `total_media` is the populated-when-idle indexed count;
    // `scrape_total_scraped` is the cumulative scraped count, seeded
    // via `media.scrape.status` on connect.
    function _indexActionStatus(): string {
        if (Browse.MediaStatus.optimizing)
            return qsTr("Optimizing");
        if (Browse.MediaStatus.indexing)
            return Browse.MediaStatus.paused ? qsTr("Paused") : qsTr("In progress");
        const total = Browse.MediaStatus.total_media;
        if (total > 0)
            return qsTr("%1 indexed").arg(Format.count(total));
        return "";
    }

    function _scrapeActionStatus(): string {
        if (Browse.MediaStatus.scraping)
            return Browse.MediaStatus.scrape_paused ? qsTr("Paused") : qsTr("In progress");
        const total = Browse.MediaStatus.scrape_total_scraped;
        if (total > 0)
            return qsTr("%1 imported").arg(Format.count(total));
        return "";
    }

    // Index and scrape can't run concurrently — Core serialises them.
    // While one is in flight the *other* row is non-actionable so we
    // don't queue a request that Core will reject.
    readonly property bool _indexBusy: Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing
    readonly property bool _scrapeBusy: Browse.MediaStatus.scraping

    // Drive the top/bottom scroll chevrons. Ignore the spacer-only
    // overflow at the form edges: the arrows should mean another row
    // is hidden, not that there is padding past the last visible row.
    // The 1-px epsilon swallows sub-pixel rounding so the chevrons
    // don't flicker on exact-fit content. Use the Column geometry
    // rather than Repeater.itemAt() so the binding re-evaluates after
    // layout settles; itemAt() returning null during construction made
    // the bottom chevron miss overflowing pages.
    readonly property bool _hasContentAbove: settings._firstFieldTop() >= 0 && flickable.contentY > settings._firstFieldTop() + 1
    readonly property bool _hasContentBelow: settings._lastFieldBottom() >= 0 && flickable.contentY + flickable.height < settings._lastFieldBottom() - 1

    function _firstFieldTop(): real {
        if (settings.fieldCount <= 0)
            return -1;
        return leadingSpacer.height + form.spacing;
    }

    function _lastFieldBottom(): real {
        if (settings.fieldCount <= 0)
            return -1;
        return Math.max(0, form.implicitHeight - trailingSpacer.height - form.spacing);
    }

    // Round 11: idle Accept now opens the index setup modal (system scope
    // + Start, see IndexSetupModal.qml) rather than indexing every system
    // directly — same treatment `_triggerScrape` got in round 10. Busy
    // Accept still cancels in place; no need to route that through a modal.
    function _triggerIndex(): void {
        if (settings._scrapeBusy)
            return;
        if (settings._indexBusy)
            Browse.MediaStatus.cancel_index();
        else
            settings.requestAccept("updateMediaDb");
    }

    // Round 10: idle Accept now opens the scrape setup modal (scraper
    // choice + re-scrape toggle + Start, see ScrapeSetupModal.qml) rather
    // than starting a scrape directly with a hardcoded scraper id — the
    // modal itself owns the force-flag state that used to live here as
    // `rescrapeExisting`. Busy Accept still cancels in place; no need to
    // route that through a modal.
    function _triggerScrape(): void {
        if (settings._indexBusy)
            return;
        if (settings._scrapeBusy)
            Browse.MediaStatus.cancel_scrape();
        else
            settings.requestAccept("runScraper");
    }

    function _fieldEnabled(id: string): bool {
        if (id === "updateMediaDb")
            return !settings._scrapeBusy;
        if (id === "runScraper")
            return !settings._indexBusy;
        return true;
    }

    function _fieldValue(id: string): string {
        if (id === "resolution")
            return settings._resolutionDisplay(Browse.Settings.current_resolution);
        if (id === "language")
            return settings._languageDisplay(Browse.Settings.current_language);
        if (id === "orientation")
            return settings._orientationDisplay(Browse.Settings.current_orientation);
        if (id === "systemsLayout")
            return settings._browseLayoutDisplay(Browse.Settings.current_systems_browse_layout);
        if (id === "gamesLayout")
            return settings._browseLayoutDisplay(Browse.Settings.current_games_browse_layout);
        if (id === "systemLogoStyle")
            return settings._systemLogoStyleDisplay(Browse.Settings.current_system_logo_style);
        if (id === "colorScheme")
            return settings._colorSchemeDisplay(Browse.Settings.current_color_scheme);
        if (id === "colorIntensity")
            return settings._colorIntensityDisplay(Browse.Settings.current_color_intensity);
        if (id === "buttonLayout")
            return settings._buttonLayoutDisplay(Browse.Settings.current_button_layout);
        if (id === "screensaverTimeout")
            return settings._screensaverTimeoutDisplay(Browse.Settings.current_screensaver_timeout);
        if (id === "clockFormat")
            return settings._clockFormatDisplay(Browse.Settings.current_clock_format);
        if (id === "region")
            return settings._regionDisplay(Browse.Settings.current_region);
        if (id === "mediaImageType")
            return settings._mediaImageTypeDisplay(Browse.Settings.current_media_image_type);
        if (id === "crtVideoStandard")
            return settings._videoStandardDisplay(Browse.CrtVideo.current_video_standard);
        return "";
    }

    function _fieldControl(id: string): string {
        if (id === "mouseEnabled" || id === "showHidden" || id === "showOriginalFilenames" || id === "debugLogging" || id === "reduceMotion" || id === "crtEnabled" || id === "swapConfirmCancel" || id === "swapOptionsView")
            return "toggle";
        if (id === "aboutLicense" || id === "documentation" || id === "pageAppearance" || id === "pageDisplayInterface" || id === "pageLanguage" || id === "pageControlsInput" || id === "pageLibraryData" || id === "pageSupportAbout" || id === "crtCalibration")
            return "navigate";
        if (id === "updateMediaDb" || id === "runScraper" || id === "uploadLog")
            return "action";
        return "picker";
    }

    function _fieldChecked(id: string): bool {
        if (id === "debugLogging")
            return Browse.Settings.current_debug_logging;
        if (id === "showHidden")
            return Browse.Settings.current_show_hidden;
        if (id === "showOriginalFilenames")
            return Browse.Settings.current_show_original_filenames;
        if (id === "reduceMotion")
            return Browse.Settings.current_reduce_motion;
        if (id === "swapConfirmCancel")
            return Browse.Settings.current_swap_confirm_cancel;
        if (id === "swapOptionsView")
            return Browse.Settings.current_swap_options_view;
        if (id === "crtEnabled")
            return Browse.CrtVideo.crt_enabled;
        return Browse.Settings.current_mouse_enabled;
    }

    readonly property int fieldCount: settings.fields.length

    // True iff `idx` points at a focusable field row (not a header,
    // not out of bounds). All `focused*` derivations early-return on
    // header indices so a defensive out-of-band write to currentIndex
    // can't mis-light the help bar.
    function _isField(idx: int): bool {
        if (idx < 0 || idx >= settings.fieldCount)
            return false;
        return settings.fields[idx].kind === "field";
    }

    // First focusable row in the registry. Used to seed `currentIndex`
    // at construction; returns -1 only if every entry is a header
    // (registry mistake — shouldn't happen).
    function _firstNavigableIndex(): int {
        for (let i = 0; i < settings.fieldCount; i++)
            if (settings.fields[i].kind === "field")
                return i;
        return -1;
    }

    // Last focusable row in the registry. Symmetric with
    // `_firstNavigableIndex`, used so the scroll-into-view logic can snap
    // exactly to the bottom edge on the last field. -1 only if every
    // entry is a header.
    function _lastNavigableIndex(): int {
        for (let i = settings.fieldCount - 1; i >= 0; i--)
            if (settings.fields[i].kind === "field")
                return i;
        return -1;
    }

    // Walk from `from` in `direction` (±1) until we hit a focusable
    // row. Headers are transparent, and edges wrap so Up on the first
    // field lands on the last field (and vice versa).
    function _seekNavigable(from: int, direction: int): int {
        if (settings.fieldCount <= 0)
            return from;
        let i = from;
        for (let steps = 0; steps < settings.fieldCount; steps++) {
            i += direction;
            if (i < 0)
                i = settings.fieldCount - 1;
            else if (i >= settings.fieldCount)
                i = 0;
            if (settings.fields[i].kind === "field")
                return i;
        }
        return from;
    }

    readonly property int _rootGridLandscapeRows: 2
    readonly property int _rootGridLandscapeColumns: Math.ceil(settings.categoryFields.length / settings._rootGridLandscapeRows)
    readonly property bool _rootGridRotated: Browse.Settings.current_orientation !== "horizontal"
    // Keep the known category grid balanced, then transpose it with the scene.
    // Six categories therefore render 3x2 horizontally and 2x3 in CW/CCW.
    readonly property int rootGridColumns: settings._rootGridRotated ? settings._rootGridLandscapeRows : settings._rootGridLandscapeColumns
    readonly property int rootGridRows: settings._rootGridRotated ? settings._rootGridLandscapeColumns : settings._rootGridLandscapeRows

    function _moveRootGrid(dx: int, dy: int): void {
        if (settings.fieldCount <= 0)
            return;
        const columns = settings.rootGridColumns;
        const row = Math.floor(settings.currentIndex / columns);
        const col = settings.currentIndex % columns;
        if (dx !== 0) {
            const rowStart = row * columns;
            const rowEnd = Math.min(settings.fieldCount - 1, rowStart + columns - 1);
            let next = settings.currentIndex + dx;
            if (next < rowStart)
                next = rowEnd;
            else if (next > rowEnd)
                next = rowStart;
            settings.currentIndex = next;
            return;
        }
        if (dy !== 0) {
            let next = settings.currentIndex + dy * columns;
            if (next < 0) {
                const lastRow = Math.floor((settings.fieldCount - 1) / columns);
                next = Math.min(lastRow * columns + col, settings.fieldCount - 1);
            } else if (next >= settings.fieldCount) {
                next = Math.min(col, settings.fieldCount - 1);
            }
            settings.currentIndex = next;
        }
    }

    function _focusRootIndex(index: int): void {
        if (index < 0 || index >= settings.fieldCount)
            return;
        settings.currentIndex = index;
    }

    readonly property bool focusedFieldIsToggle: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return id === "mouseEnabled" || id === "showHidden" || id === "showOriginalFilenames" || id === "debugLogging" || id === "reduceMotion" || id === "crtEnabled" || id === "swapConfirmCancel" || id === "swapOptionsView";
    }
    // True when the focused field is a list-picker row (Accept opens a
    // modal; left/right is a no-op — pickers don't cycle inline). Drives
    // the help-bar A: Open hint.
    readonly property bool focusedFieldIsPicker: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return id === "resolution" || id === "language" || id === "clockFormat" || id === "region" || id === "orientation" || id === "systemsLayout" || id === "gamesLayout" || id === "systemLogoStyle" || id === "colorScheme" || id === "colorIntensity" || id === "buttonLayout" || id === "screensaverTimeout" || id === "mediaImageType" || id === "crtVideoStandard";
    }
    // True when focused row accepts A without left/right cycling:
    // pickers, jobs, modal/navigation rows, and root category rows.
    // Drives help-bar Accept hint and suppresses left/right Change cue.
    readonly property bool focusedFieldIsAction: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return settings.focusedFieldIsPicker || id === "updateMediaDb" || id === "runScraper" || id === "uploadLog" || id === "aboutLicense" || id === "documentation" || id === "pageAppearance" || id === "pageDisplayInterface" || id === "pageLanguage" || id === "pageControlsInput" || id === "pageLibraryData" || id === "pageSupportAbout" || id === "crtCalibration";
    }
    // Verb shown on the help-bar Accept hint for the focused action row.
    // updateMediaDb flips between Start and Cancel because the press
    // toggles the in-flight operation directly; runScraper reads
    // "Configure" while idle (round 10 — the press opens the scrape
    // setup modal rather than starting a scrape directly) but still
    // "Cancel" while busy, since a busy press still cancels in place;
    // uploadLog reads "Upload" because the press opens the upload-flow
    // modal rather than kicking off an in-row job; aboutLicense reads
    // "Open" because the press navigates.
    readonly property string focusedActionLabel: {
        if (!settings._isField(settings.currentIndex))
            return "";
        const id = settings.fields[settings.currentIndex].id;
        if (id === "updateMediaDb")
            return settings.focusedActionBusy ? qsTr("Cancel") : qsTr("Configure");
        if (id === "runScraper")
            return settings.focusedActionBusy ? qsTr("Cancel") : qsTr("Configure");
        if (id === "uploadLog")
            return qsTr("Upload");
        return qsTr("Open");
    }
    // True when the focused action's matching operation is currently
    // running, so the help bar can label Accept as "Cancel" rather
    // than "Start".
    readonly property bool focusedActionBusy: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        if (id === "updateMediaDb")
            return settings._indexBusy;
        if (id === "runScraper")
            return settings._scrapeBusy;
        return false;
    }
    // True when the focused action can't run right now because the
    // *other* media operation has the bus. Drives the dimmed-row
    // visual and lets the help bar drop the Accept hint instead of
    // promising a press that will silently no-op.
    readonly property bool focusedActionDisabled: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        if (id === "updateMediaDb")
            return settings._scrapeBusy;
        if (id === "runScraper")
            return settings._indexBusy;
        return false;
    }

    // Initial focus: first navigable row. The binding evaluates once
    // (no reactive dependencies inside `_firstNavigableIndex`) and is
    // broken the first time the user moves focus — handleAction's
    // up/down branches assign to `currentIndex` directly. Falling back
    // to 0 covers the all-headers degenerate case; helpers below
    // early-return on `_isField(0) === false` if it ever lands there.
    property int currentIndex: {
        const idx = settings._firstNavigableIndex();
        return idx >= 0 ? idx : 0;
    }

    function _resolutionList(): list<string> {
        const raw = Browse.Settings.available_resolutions;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _resolutionDisplay(value: string): string {
        if (value === "")
            return qsTr("Automatic");
        const parts = value.split("x");
        if (parts.length !== 2)
            return value;
        return qsTr("%1 × %2").arg(parts[0]).arg(parts[1]);
    }

    function _resolutionPickerDisplay(value: string): string {
        if (value === "")
            return qsTr("Automatic (Recommended)");
        const label = settings._resolutionDisplay(value);
        const parts = value.split("x");
        if (parts.length === 2 && Number(parts[1]) >= 1080)
            return qsTr("%1 (Animations off)").arg(label);
        return label;
    }

    function _colorSchemeList(): list<string> {
        const raw = Browse.Settings.available_color_schemes;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _colorIntensityList(): list<string> {
        const raw = Browse.Settings.available_color_intensities;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _buttonLayoutList(): list<string> {
        const raw = Browse.Settings.available_button_layouts;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _browseLayoutList(): list<string> {
        const raw = Browse.Settings.available_browse_layouts;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _systemLogoStyleList(): list<string> {
        const raw = Browse.Settings.available_system_logo_styles;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _languageList(): list<string> {
        const raw = Browse.Settings.available_languages;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _clockFormatList(): list<string> {
        const raw = Browse.Settings.available_clock_formats;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _orientationList(): list<string> {
        const raw = Browse.Settings.available_orientations;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _languageDisplay(value: string): string {
        if (value === "en" || value === "en_US" || value === "en_GB")
            return qsTr("English");
        if (value === "it" || value === "it_IT")
            return qsTr("Italian");
        if (value === "es" || value === "es_ES")
            return qsTr("Spanish");
        if (value === "eu" || value === "eu_ES")
            return qsTr("Basque");
        if (value === "de" || value === "de_DE")
            return qsTr("German");
        if (value === "el" || value === "el_GR")
            return qsTr("Greek");
        if (value === "ja" || value === "ja_JP")
            return qsTr("Japanese");
        if (value === "ko" || value === "ko_KR")
            return qsTr("Korean");
        if (value === "nl" || value === "nl_NL")
            return qsTr("Dutch");
        if (value === "ro" || value === "ro_RO")
            return qsTr("Romanian");
        if (value === "sk" || value === "sk_SK")
            return qsTr("Slovak");
        if (value === "uk" || value === "uk_UA")
            return qsTr("Ukrainian");
        if (value === "zh_CN")
            return qsTr("Chinese (Simplified)");
        if (value === "zh_TW" || value === "zh_HK")
            return qsTr("Chinese (Traditional)");
        if (value === "he" || value === "he_IL")
            return qsTr("Hebrew");
        if (value === "ar" || value === "ar_SA")
            return qsTr("Arabic");
        if (value === "hi" || value === "hi_IN")
            return qsTr("Hindi");
        if (value === "fr" || value === "fr_FR")
            return qsTr("French");
        return qsTr("Automatic");
    }

    function _clockFormatDisplay(value: string): string {
        if (value === "12h")
            return qsTr("12-hour");
        if (value === "24h")
            return qsTr("24-hour");
        return qsTr("Automatic");
    }

    function _regionList(): list<string> {
        const raw = Browse.Settings.available_regions;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _regionDisplay(value: string): string {
        if (value === "us")
            return qsTr("Americas");
        if (value === "eu")
            return qsTr("Europe");
        if (value === "jp")
            return qsTr("Japan");
        return qsTr("Automatic");
    }

    function _orientationDisplay(value: string): string {
        if (value === "cw")
            return qsTr("Rotated CW");
        if (value === "ccw")
            return qsTr("Rotated CCW");
        return qsTr("Horizontal");
    }

    function _browseLayoutDisplay(value: string): string {
        if (value === "list")
            return qsTr("Detailed list view");
        return qsTr("Grid view");
    }

    function _systemLogoStyleDisplay(value: string): string {
        if (value === "color")
            return qsTr("Full color");
        return qsTr("Tinted");
    }

    // A lookup table rather than an if-chain — the table form keeps every
    // label a literal qsTr() call so lupdate can harvest it (a computed/
    // templated string would not be translator-visible), and it stays one
    // edit for a future preset add/remove. 19 presets ship as of round 7;
    // round 10 reordered the catalog into family blocks (see
    // ColorSchemes.qml's `ids` and docs/style.md -> "Preset catalog") —
    // this table's own key order follows suit for readability, though
    // lookup here doesn't depend on it.
    function _colorSchemeDisplay(value: string): string {
        const names = {
            "zaparoo-dark": qsTr("Zaparoo Dark"),
            "zaparoo-light": qsTr("Zaparoo Light"),
            "classic-purple": qsTr("Classic Purple"),
            "amber-phosphor": qsTr("Amber Phosphor"),
            "game-boy": qsTr("Game Boy"),
            "green-phosphor": qsTr("Green Phosphor"),
            "neo-geo": qsTr("Neo Geo"),
            "nes": qsTr("NES"),
            "virtual-boy": qsTr("Virtual Boy"),
            "dracula": qsTr("Dracula"),
            "everforest": qsTr("Everforest"),
            "gruvbox": qsTr("Gruvbox"),
            "nord": qsTr("Nord"),
            "oxocarbon": qsTr("Oxocarbon"),
            "rose-pine": qsTr("Rosé Pine"),
            "solarized-dark": qsTr("Solarized Dark"),
            "synthwave-84": qsTr("Synthwave '84"),
            "flexoki-paper": qsTr("Flexoki Paper"),
            "solarized-light": qsTr("Solarized Light")
        };
        return names[value] !== undefined ? names[value] : names["zaparoo-dark"];
    }

    // Two values only. "Subtle" is what ships and what every existing
    // install already sees; "Vivid" lets the high-chroma presets (NES,
    // Virtual Boy, Game Boy, Synthwave '84) read as vividly as their name
    // suggests. A middle rung was considered and dropped -- the difference
    // is too soft to perceive reliably, and it would triple what has to be
    // checked across 19 presets.
    function _colorIntensityDisplay(value: string): string {
        if (value === "vivid")
            return qsTr("Vivid");
        return qsTr("Subtle");
    }

    function _buttonLayoutDisplay(value: string): string {
        if (value === "style_b")
            return qsTr("Style B");
        if (value === "style_c")
            return qsTr("Style C");
        if (value === "style_d")
            return qsTr("Style D");
        if (value === "style_e")
            return qsTr("Style E");
        if (value === "style_a")
            return qsTr("Style A");
        return qsTr("Automatic");
    }

    function _screensaverTimeoutList(): list<string> {
        const raw = Browse.Settings.available_screensaver_timeouts;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _mediaImageTypeList(): list<string> {
        const raw = Browse.Settings.available_media_image_types;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _screensaverTimeoutDisplay(value: string): string {
        if (value === "off")
            return qsTr("Off");
        if (value === "1")
            return qsTr("1 second (testing)");
        if (value === "60")
            return qsTr("1 minute");
        if (value === "120")
            return qsTr("2 minutes");
        if (value === "300")
            return qsTr("5 minutes");
        if (value === "600")
            return qsTr("10 minutes");
        if (value === "900")
            return qsTr("15 minutes");
        if (value === "1800")
            return qsTr("30 minutes");
        return qsTr("%1 seconds").arg(value);
    }

    function _videoStandardList(): list<string> {
        const raw = Browse.CrtVideo.available_video_standards;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _videoStandardDisplay(value: string): string {
        if (value === "pal")
            return qsTr("PAL (50 Hz)");
        if (value === "480i")
            return qsTr("480i (60 Hz)");
        return qsTr("NTSC (60 Hz)");
    }

    function _mediaImageTypeDisplay(value: string): string {
        if (value === "auto")
            return qsTr("Automatic");
        if (value === "image")
            return qsTr("Image");
        if (value === "thumbnail")
            return qsTr("Thumbnail");
        if (value === "boxart")
            return qsTr("Box art");
        if (value === "boxart3d")
            return qsTr("3D box art");
        if (value === "screenshot")
            return qsTr("Screenshot");
        if (value === "wheel")
            return qsTr("Wheel");
        if (value === "titleshot")
            return qsTr("Title screen");
        if (value === "map")
            return qsTr("Map");
        if (value === "marquee")
            return qsTr("Marquee");
        if (value === "fanart")
            return qsTr("Fan art");
        if (value === "boxartside")
            return qsTr("Box side");
        if (value === "boxartback")
            return qsTr("Box back");
        return value;
    }

    // Build the picker entry list for a field. Each entry is
    // `{ id: string, label: string }` — `id` is the canonical value
    // the model stores, `label` is the localised display string.
    // The router emits `requestListPicker` and `Main.qml` mounts the
    // shared `ListPickerModal` with these.
    function _openPickerForField(id: string): void {
        let title = "";
        let entries = [];
        let initialId = "";
        if (id === "resolution") {
            title = qsTr("Interface resolution");
            const list = settings._resolutionList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._resolutionPickerDisplay(list[i])
                });
            initialId = Browse.Settings.current_resolution;
        } else if (id === "language") {
            title = qsTr("Language");
            const list = settings._languageList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._languageDisplay(list[i])
                });
            initialId = Browse.Settings.current_language;
        } else if (id === "clockFormat") {
            title = qsTr("Clock format");
            const list = settings._clockFormatList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._clockFormatDisplay(list[i])
                });
            initialId = Browse.Settings.current_clock_format;
        } else if (id === "region") {
            title = qsTr("Region");
            const list = settings._regionList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._regionDisplay(list[i])
                });
            initialId = Browse.Settings.current_region;
        } else if (id === "orientation") {
            title = qsTr("Orientation");
            const list = settings._orientationList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._orientationDisplay(list[i])
                });
            initialId = Browse.Settings.current_orientation;
        } else if (id === "systemsLayout") {
            title = qsTr("Systems layout");
            const list = settings._browseLayoutList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._browseLayoutDisplay(list[i])
                });
            initialId = Browse.Settings.current_systems_browse_layout;
        } else if (id === "gamesLayout") {
            title = qsTr("Games layout");
            const list = settings._browseLayoutList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._browseLayoutDisplay(list[i])
                });
            initialId = Browse.Settings.current_games_browse_layout;
        } else if (id === "systemLogoStyle") {
            title = qsTr("System logos");
            const list = settings._systemLogoStyleList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._systemLogoStyleDisplay(list[i])
                });
            initialId = Browse.Settings.current_system_logo_style;
        } else if (id === "colorScheme") {
            title = qsTr("Color scheme");
            const list = settings._colorSchemeList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._colorSchemeDisplay(list[i]),
                    // Precomputed once here rather than per-delegate in
                    // ListPickerModal's Repeater -- ColorSchemes.previewColors()
                    // is cheap, but there's no reason to re-run it on every
                    // binding re-evaluation. Undefined for every other picker's
                    // entries, which is the flag ListPickerModal uses to fall
                    // back to its plain centered-label row.
                    swatch: ColorSchemes.previewColors(list[i])
                });
            initialId = Browse.Settings.current_color_scheme;
        } else if (id === "colorIntensity") {
            title = qsTr("Color intensity");
            const list = settings._colorIntensityList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._colorIntensityDisplay(list[i])
                });
            initialId = Browse.Settings.current_color_intensity;
        } else if (id === "buttonLayout") {
            title = qsTr("Button style");
            const list = settings._buttonLayoutList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._buttonLayoutDisplay(list[i])
                });
            initialId = Browse.Settings.current_button_layout;
        } else if (id === "screensaverTimeout") {
            title = qsTr("Screensaver");
            const list = settings._screensaverTimeoutList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._screensaverTimeoutDisplay(list[i])
                });
            initialId = Browse.Settings.current_screensaver_timeout;
        } else if (id === "mediaImageType") {
            title = qsTr("Preferred artwork");
            const list = settings._mediaImageTypeList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._mediaImageTypeDisplay(list[i])
                });
            initialId = Browse.Settings.current_media_image_type;
        } else if (id === "crtVideoStandard") {
            title = qsTr("Video standard");
            const list = settings._videoStandardList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._videoStandardDisplay(list[i])
                });
            initialId = Browse.CrtVideo.current_video_standard;
        } else {
            return;
        }
        if (entries.length === 0)
            return;
        settings.requestListPicker(title, entries, initialId, id);
    }

    function _reprojectBrowseModels(): void {
        Browse.SystemsModel.reproject();
        Browse.CategoriesModel.reproject();
    }

    function _setShowHidden(direction: int): void {
        const showHidden = direction > 0;
        if (Browse.Settings.current_show_hidden === showHidden)
            return;
        Browse.Settings.set_show_hidden(showHidden);
        settings._reprojectBrowseModels();
    }

    function _toggleShowHidden(): void {
        Browse.Settings.set_show_hidden(!Browse.Settings.current_show_hidden);
        settings._reprojectBrowseModels();
    }

    function _setShowOriginalFilenames(direction: int): void {
        Browse.Settings.set_show_original_filenames(direction > 0);
    }

    function _toggleShowOriginalFilenames(): void {
        Browse.Settings.set_show_original_filenames(!Browse.Settings.current_show_original_filenames);
    }

    function _setMouseEnabled(direction: int): void {
        Browse.Settings.set_mouse_enabled(direction > 0);
    }

    function _toggleMouseEnabled(): void {
        Browse.Settings.set_mouse_enabled(!Browse.Settings.current_mouse_enabled);
    }

    // Debug logging is confirm-gated like CRT mode: the pill never flips
    // locally. The request routes to Main.qml, which stages the
    // restart-confirm modal; the actual flip happens once the frontend
    // relaunches (the tracing subscriber is only built once at startup).
    function _requestDebugLogging(enable: bool): void {
        if (enable === Browse.Settings.current_debug_logging)
            return;
        settings.requestAccept(enable ? "debugLoggingEnable" : "debugLoggingDisable");
    }

    function _setReduceMotion(direction: int): void {
        Browse.Settings.set_reduce_motion(direction > 0);
    }

    function _toggleReduceMotion(): void {
        Browse.Settings.set_reduce_motion(!Browse.Settings.current_reduce_motion);
    }

    function _setSwapConfirmCancel(direction: int): void {
        Browse.Settings.set_swap_confirm_cancel(direction > 0);
    }

    function _toggleSwapConfirmCancel(): void {
        Browse.Settings.set_swap_confirm_cancel(!Browse.Settings.current_swap_confirm_cancel);
    }

    function _setSwapOptionsView(direction: int): void {
        Browse.Settings.set_swap_options_view(direction > 0);
    }

    function _toggleSwapOptionsView(): void {
        Browse.Settings.set_swap_options_view(!Browse.Settings.current_swap_options_view);
    }

    // CRT mode is confirm-gated: the pill never flips locally. The
    // request routes to Main.qml, which stages the restart-confirm
    // modal; the actual flip happens on the post-exit-42 respawn.
    function _requestCrtEnabled(enable: bool): void {
        if (enable === Browse.CrtVideo.crt_enabled)
            return;
        settings.requestAccept(enable ? "crtEnable" : "crtDisable");
    }

    function _cycleFocused(direction: int): void {
        if (!settings._isField(settings.currentIndex))
            return;
        const id = settings.fields[settings.currentIndex].id;
        // Picker fields ignore left/right - accept opens the
        // list-picker modal instead. Only toggles still respond to
        // direction presses (left = off, right = on).
        if (id === "mouseEnabled")
            settings._setMouseEnabled(direction);
        else if (id === "showHidden")
            settings._setShowHidden(direction);
        else if (id === "showOriginalFilenames")
            settings._setShowOriginalFilenames(direction);
        else if (id === "debugLogging")
            settings._requestDebugLogging(direction > 0);
        else if (id === "reduceMotion")
            settings._setReduceMotion(direction);
        else if (id === "swapConfirmCancel")
            settings._setSwapConfirmCancel(direction);
        else if (id === "swapOptionsView")
            settings._setSwapOptionsView(direction);
        else if (id === "crtEnabled")
            settings._requestCrtEnabled(direction > 0);
    }

    function _rememberPageFocus(): void {
        settings._pageIndexes[settings.currentPage] = settings.currentIndex;
    }

    function _restorePageFocus(): void {
        const first = settings._firstNavigableIndex();
        const fallback = first >= 0 ? first : 0;
        const remembered = settings._pageIndexes[settings.currentPage];
        const idx = remembered === undefined ? fallback : Math.max(0, Math.min(settings.fieldCount - 1, remembered));
        settings.currentIndex = settings._isField(idx) ? idx : fallback;
        // Call directly rather than relying on onCurrentIndexChanged: the
        // assignment above is a no-op when the new page's remembered
        // index happens to equal the old numeric currentIndex, but the
        // Flickable's content just changed pages regardless, so the
        // viewport needs re-framing either way. `_scrollFocusedIntoView`
        // itself no-ops safely if the Repeater hasn't rebuilt its
        // delegates for the new page yet; the `form.implicitHeightChanged`
        // connection above re-triggers it once that settles.
        settings._scrollFocusedIntoView();
    }

    function _switchPage(page: string): void {
        // Disable SettingsField Behaviors for this synchronous block so
        // reused delegates don't animate focus-border or toggle-position
        // changes when the new page's field model lands. The flag is
        // cleared on the next event-loop tick so subsequent user moves
        // still animate normally.
        settings._pageSwitching = true;
        settings._rememberPageFocus();
        settings.currentPage = page;
        settings._restorePageFocus();
        Qt.callLater(() => {
            settings._pageSwitching = false;
        });
    }

    function _openPage(id: string): bool {
        // Resolve the target page first so we can return false quickly for
        // non-page IDs. Then fire the pulse (cue plays on the still-visible
        // tile) and defer _switchPage so the push-in's downward leg is
        // fully visible before the page swaps out.
        let page = "";
        if (id === "pageAppearance")
            page = settings.pageAppearance;
        else if (id === "pageDisplayInterface")
            page = settings.pageDisplayInterface;
        else if (id === "pageLanguage")
            page = settings.pageLanguage;
        else if (id === "pageControlsInput")
            page = settings.pageControlsInput;
        else if (id === "pageLibraryData")
            page = settings.pageLibraryData;
        else if (id === "pageSupportAbout")
            page = settings.pageSupportAbout;
        else
            return false;
        settings.activatePulse++;
        pressCommit._page = page;
        pressCommit.arm();
        return true;
    }

    function _goBack(): void {
        // Disarm pending accepts so a press-then-back inside the deferred
        // window cannot drill into a subpage / open a picker after backing out.
        pressCommit.stop();
        fieldCommit.stop();
        if (settings.currentPage !== settings.pageRoot) {
            settings._switchPage(settings.pageRoot);
            return;
        }
        // Leaving Settings entirely -- not just popping a sub-page. The
        // `SettingsScreen` instance survives this (see MainLayout.qml's
        // `settingsScreenLoader`: `active` is never set back to false, only
        // `visible` toggles), so without an explicit reset here
        // `_pageIndexes` would still hold every sub-page's remembered row
        // the next time Settings opens, landing back on whichever sub-page
        // was left rather than the category grid. Sub-page <-> parent
        // memory stays intact for the rest of a visit (see
        // `_rememberPageFocus`/`_restorePageFocus` above) -- this only
        // clears it on the way out, matching a cold start's own behavior
        // (which also always lands on the root grid).
        settings.currentPage = settings.pageRoot;
        settings._pageIndexes = ({});
        settings.currentIndex = settings._firstNavigableIndex();
        settings.requestHubScreen();
    }

    function handleAction(action: string): void {
        if (settings.optimisticLoading) {
            if (action === "cancel")
                settings._goBack();
            return;
        }
        if (action === "up") {
            if (settings.showingRootGrid)
                settings._moveRootGrid(0, -1);
            else
                settings.currentIndex = settings._seekNavigable(settings.currentIndex, -1);
        } else if (action === "down") {
            if (settings.showingRootGrid)
                settings._moveRootGrid(0, 1);
            else
                settings.currentIndex = settings._seekNavigable(settings.currentIndex, 1);
        } else if (action === "left") {
            if (settings.showingRootGrid)
                settings._moveRootGrid(-1, 0);
            else
                settings._cycleFocused(-1);
        } else if (action === "right") {
            if (settings.showingRootGrid)
                settings._moveRootGrid(1, 0);
            else
                settings._cycleFocused(1);
        } else if (action === "accept") {
            if (!settings._isField(settings.currentIndex))
                return;
            const id = settings.fields[settings.currentIndex].id;
            if (settings._openPage(id))
                return;
            // Toggles flip in place — the knob slide is their cue, so act now
            // and skip the push-in.
            if (settings._fieldControl(id) === "toggle") {
                if (id === "mouseEnabled")
                    settings._toggleMouseEnabled();
                else if (id === "showHidden")
                    settings._toggleShowHidden();
                else if (id === "showOriginalFilenames")
                    settings._toggleShowOriginalFilenames();
                else if (id === "debugLogging")
                    settings._requestDebugLogging(!Browse.Settings.current_debug_logging);
                else if (id === "reduceMotion")
                    settings._toggleReduceMotion();
                else if (id === "swapConfirmCancel")
                    settings._toggleSwapConfirmCancel();
                else if (id === "swapOptionsView")
                    settings._toggleSwapOptionsView();
                else if (id === "crtEnabled")
                    settings._requestCrtEnabled(!Browse.CrtVideo.crt_enabled);
                return;
            }
            // Picker / action / about either open a modal or navigate away,
            // which would cover or replace the row before its push-in could
            // show. Play the cue, then run the action deferred (the same
            // deferred-flip the tiles use) so the press is visible on the
            // still-static settings screen first.
            settings.fieldActivatePulse++;
            fieldCommit._id = id;
            fieldCommit.arm();
        } else if (action === "cancel") {
            settings._goBack();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    DeferredAction {
        id: pressCommit
        property string _page: ""
        onDeferred: {
            const p = _page;
            _page = "";
            settings._switchPage(p);
        }
    }

    // Deferred-flip for non-toggle field activations: the focused row's push-in
    // plays on the still-visible settings screen, then this fires `pressMs`
    // later to open the modal / navigate. Without the defer the modal scrim or
    // screen change covers the row before the cue can render.
    DeferredAction {
        id: fieldCommit
        property string _id: ""
        onDeferred: {
            const id = fieldCommit._id;
            fieldCommit._id = "";
            if (id === "updateMediaDb")
                settings._triggerIndex();
            else if (id === "runScraper")
                settings._triggerScrape();
            else if (id === "uploadLog")
                settings.requestAccept("uploadLog");
            else if (id === "aboutLicense")
                settings.requestAccept("aboutLicense");
            else if (id === "documentation")
                settings.requestAccept("documentation");
            else if (id === "crtCalibration")
                settings.requestAccept("crtCalibration");
            else
                settings._openPickerForField(id);
        }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: settings._goBack()
    }

    TopStatusStrip {
        id: topStrip
        visible: !settings.optimisticLoading && Sizing.tier !== "240"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom
        height: Sizing.tier === "240" ? 0 : Sizing.pctH(7)
        title: settings.pageTitle
        currentPage: 0
        totalPages: 0
        totalText: ""
    }

    // Scroll focused row into view if it sits outside the Flickable's
    // current viewport, row-by-row rather than by pixel: the first/last
    // focusable row always lands exactly at 0 / the max scroll (so the
    // card's own top/bottom breathing room is fully visible at either
    // extreme), every other move keeps a `form.spacing` gap above/below
    // the row instead of flush-cutting the neighbouring row at its
    // boundary, and a section header scrolls in together with the first
    // field under it rather than being left off-screen above it. No-op
    // for header indices (they aren't focusable and currentIndex never
    // lands on one in normal flow). Bound to `onCurrentIndexChanged`
    // below plus viewport/content-size changes and page mount, since a
    // resize or a page switch can leave a stale contentY that no longer
    // frames the (possibly unchanged) focused row correctly. No
    // animation — software-renderer budget can't pay for a moving
    // column behind a focus border.
    function _scrollFocusedIntoView(): void {
        if (settings.showingRootGrid || !settings._isField(settings.currentIndex))
            return;
        const idx = settings.currentIndex;
        const row = rowRepeater.itemAt(idx);
        if (row === null)
            return;
        const maxY = Math.max(0, flickable.contentHeight - flickable.height);
        if (idx === settings._firstNavigableIndex()) {
            flickable.contentY = 0;
            return;
        }
        if (idx === settings._lastNavigableIndex()) {
            flickable.contentY = maxY;
            return;
        }
        // Pull a section header along with the field immediately below
        // it, so the header title doesn't scroll off above a field that
        // just became focused.
        let topBound = row.y;
        const prevRow = idx > 0 ? rowRepeater.itemAt(idx - 1) : null;
        // itemAt() returns the generic QQuickItem type statically, so the
        // static analyzer can't see the delegate's own `isHeader`
        // property — same pattern as PagedGrid.qml's own itemAt()-typed
        // accesses.
        // qmllint disable missing-property
        if (prevRow !== null && prevRow.isHeader)
            topBound = prevRow.y;
        // qmllint enable missing-property
        const top = Math.max(0, topBound - form.spacing);
        const bottom = Math.min(maxY, row.y + row.height + form.spacing - flickable.height);
        if (flickable.contentY > top)
            flickable.contentY = top;
        else if (flickable.contentY < bottom)
            flickable.contentY = bottom;
    }

    onCurrentIndexChanged: settings._scrollFocusedIntoView()
    Connections {
        target: flickable
        function onHeightChanged(): void {
            settings._scrollFocusedIntoView();
        }
    }
    Connections {
        target: form
        function onImplicitHeightChanged(): void {
            settings._scrollFocusedIntoView();
        }
    }

    Item {
        id: categoryGrid

        visible: !settings.optimisticLoading && settings.showingRootGrid && settings.fieldCount > 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.tier === "240" ? Sizing.pctH(1) : Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.tier === "240" ? Sizing.helpBarHeight + Sizing._hubActiveLabelHeight : Sizing.pctH(15)

        readonly property int columns: settings.rootGridColumns
        readonly property int rows: settings.rootGridRows
        readonly property int leftInset: Sizing._hubGridSideInset
        readonly property int rightInset: Sizing._hubGridSideInset
        readonly property int topInset: Sizing._hubGridTopInset
        readonly property int bottomInset: Sizing._hubGridBottomInset
        readonly property int cellSpacingX: Sizing._hubGridColumnGap
        readonly property int cellSpacingY: Sizing._hubGridRowGap
        readonly property int _availableWidth: Math.max(0, width - leftInset - rightInset)
        readonly property int _availableHeight: Math.max(0, height - topInset - bottomInset)
        // Matches the Hub's resolved tile dimensions so Settings tiles read
        // as the same physical object. Low-resolution Hub tiles are slightly
        // taller than wide to fill their two-row band; larger tiers remain
        // square. Settings' own fits stay as safety ceilings.
        readonly property int cellWidth: Math.max(0, Math.min(Sizing.hubTileWidth, Math.floor((_availableWidth - (columns - 1) * cellSpacingX) / columns)))
        readonly property int cellHeight: Math.max(0, Math.min(Sizing.hubTileHeight, Math.floor((_availableHeight - (rows - 1) * cellSpacingY) / rows)))
        readonly property int visibleColumns: Math.max(1, Math.min(columns, settings.fieldCount))
        readonly property int visibleRows: Math.min(rows, Math.max(1, Math.ceil(settings.fieldCount / columns)))
        readonly property int contentWidth: visibleColumns * cellWidth + (visibleColumns - 1) * cellSpacingX
        readonly property int contentHeight: visibleRows * cellHeight + (visibleRows - 1) * cellSpacingY
        readonly property int originX: leftInset + Sizing.center(_availableWidth, contentWidth)
        readonly property int originY: topInset + Sizing.center(_availableHeight, contentHeight)

        Component {
            id: categoryTileDelegate
            Tile {
                // Matches the Hub's own tightened padding so these tiles
                // read as the same physical object as the Hub's, not
                // just the same outer cell size — see docs/style.md's
                // "Tile aspect and grid blocks" and Tile.qml's
                // `compactPadding` doc comment.
                compactPadding: true
            }
        }

        Repeater {
            model: settings.fields

            Item {
                id: categoryCell

                required property int index
                required property var modelData

                readonly property int cellRow: Math.floor(index / categoryGrid.columns)
                readonly property int cellCol: index % categoryGrid.columns
                readonly property bool isSelected: index === settings.currentIndex

                x: categoryGrid.originX + cellCol * (categoryGrid.cellWidth + categoryGrid.cellSpacingX)
                y: categoryGrid.originY + cellRow * (categoryGrid.cellHeight + categoryGrid.cellSpacingY)
                width: categoryGrid.cellWidth
                height: categoryGrid.cellHeight
                z: isSelected ? 1 : 0

                TileLoader {
                    anchors.fill: parent
                    sourceComponent: categoryTileDelegate
                    isSelected: categoryCell.isSelected
                    isFocused: true
                    name: categoryCell.modelData.label
                    coverKey: categoryCell.modelData.coverKey
                    activatePulse: settings.activatePulse
                }

                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    cursorShape: Qt.PointingHandCursor

                    onEntered: settings._focusRootIndex(categoryCell.index)
                    onClicked: mouse => {
                        if (mouse.button === Qt.RightButton) {
                            settings._goBack();
                            return;
                        }
                        settings._focusRootIndex(categoryCell.index);
                        settings.handleAction("accept");
                    }
                }
            }
        }
    }

    ActiveLabel {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.tier === "240" ? Sizing.helpBarHeight : Sizing.pctH(8)
        height: Sizing.tier === "240" ? Sizing._hubActiveLabelHeight : Sizing.pctH(7)
        text: settings.showingRootGrid && settings._isField(settings.currentIndex) ? settings.fields[settings.currentIndex].label : ""
        visible: !settings.optimisticLoading && settings.showingRootGrid && settings.fieldCount > 0
    }

    // Card frame: static geometry, unlike the old design where the card
    // Rectangle scrolled with the row Column. The card's own top/bottom
    // edges are never scrollable content now, so they can't be scrolled
    // out of view — see docs/style.md's Settings hint-band note. Width
    // capped so the rows don't stretch edge-to-edge on widescreen; bottom
    // margin clears the help bar plus a small gap.
    Item {
        id: settingsCard
        visible: !settings.optimisticLoading && !settings.showingRootGrid

        // topMargin and bottomMargin are sized to leave a clear band for
        // the scroll chevrons to sit outside the card (chevron pctH(3) +
        // breathing room). bottomMargin also has to clear the help bar
        // plus a small gap.
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(4)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.helpBarHeight + Sizing.pctH(4)
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(parent.width - Sizing.pctW(6), Sizing.pctW(70))

        readonly property bool _hasContent: settings.fieldCount > 0
        // Two lines of Sizing.fontBody reserved for hintText below,
        // regardless of whether the focused row has a description — an
        // appearing/disappearing band would reflow the row column under
        // the cursor. Driven off `hintLineMetrics.lineSpacing`, not a
        // fixed multiplier — round 8 shipped `Sizing.fontBody * 1.35`, but
        // Noto Sans (the app's own `Theme.fontUi`) has an hhea line
        // spacing of ~1.362 em, not 1.35. With an explicit `height`,
        // `wrapMode`, and `elide != ElideNone`, `Text` drops whichever
        // trailing line doesn't fit inside that height and elides the
        // last one that does — so the 1.35 reservation was consistently
        // ~1-2px short of a real two-line layout at every tier, and the
        // second line never rendered. A real measurement is exact
        // (and self-corrects under the bitmap face) instead of needing a
        // second constant tuned to match.
        readonly property int _hintTextHeight: 2 * Math.ceil(hintLineMetrics.lineSpacing)

        FontMetrics {
            id: hintLineMetrics
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontBody
        }

        // One card behind every section so the rows read as lines of text
        // cut into something, matching the inverse-video vocabulary each
        // row itself uses (see SettingsField.qml / SelectionBar.qml)
        // rather than floating loose on the screen background. Sized to
        // this static frame, not to the scrolling Column, so the card's
        // own edges never move.
        Rectangle {
            anchors.fill: parent
            visible: settingsCard._hasContent
            color: Theme.surfaceCard
            border.width: Sizing.cardBorderWidth
            border.color: Theme.borderMid
            radius: Sizing.radiusMd
            antialiasing: Sizing.cornerAntialiasing
        }

        // Focused-row description, replacing the old per-row description
        // line (illegible at 540p/CRT — see docs/style.md's Settings
        // hint-band note and docs/content-style.md's "Adding a setting"
        // checklist). Always reserved at two lines so the row column
        // never reflows under the cursor when the description changes.
        Text {
            id: hintText
            objectName: "settingsHintText"
            visible: settingsCard._hasContent
            anchors.left: parent.left
            anchors.leftMargin: flickable.cardPadding
            anchors.right: parent.right
            anchors.rightMargin: flickable.cardPadding
            anchors.bottom: parent.bottom
            anchors.bottomMargin: Sizing.pctH(1)
            height: settingsCard._hintTextHeight
            verticalAlignment: Text.AlignTop
            wrapMode: Text.WordWrap
            maximumLineCount: 2
            elide: Text.ElideRight
            text: settings._isField(settings.currentIndex) ? (settings.fields[settings.currentIndex].description ?? "") : ""
            color: Theme.textLabel
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontBody
            renderType: Text.NativeRendering
        }

        Rectangle {
            id: hintDivider
            visible: settingsCard._hasContent
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: hintText.top
            anchors.bottomMargin: Sizing.pctH(0.5)
            height: Sizing.stroke(1)
            color: Theme.borderSubtle
        }

        // Form lives in a Flickable so the section bands can grow past
        // the card's own height without dropping off-frame.
        Flickable {
            id: flickable

            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: hintDivider.top
            anchors.bottomMargin: Sizing.pctH(0.5)
            contentWidth: width
            contentHeight: form.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            // Inset applied to every row mount below so the card keeps a
            // visible lip on both sides, matching BrowseList's row inset
            // (see the "Row edge consistency" note in docs/style.md).
            readonly property int cardPadding: Sizing.pctW(2)

            Column {
                id: form

                width: parent.width
                spacing: Sizing.pctH(1.5)
                visible: settings.fieldCount > 0

                // Leading spacer — keeps the first field clear of the top
                // scroll chevron and gives the cut-off edge a breath of
                // whitespace instead of clipping mid-row.
                Item {
                    id: leadingSpacer

                    width: form.width
                    height: Sizing.pctH(2)
                }

                Repeater {
                    id: rowRepeater
                    model: settings.fields

                    // Wrapper row — both potential children exist but only
                    // the kind-matching one paints. A Loader would also
                    // work, but binding-through-`parent.modelData` adds
                    // static-analysis friction under
                    // ComponentBehavior:Bound; the wrapper Item is cheap
                    // (≤ 3 headers + ≤ 7 fields) and keeps every
                    // field-row binding readable in place.
                    Item {
                        id: row

                        required property int index
                        required property var modelData

                        readonly property bool isHeader: modelData.kind === "header"

                        width: form.width
                        implicitHeight: row.isHeader ? header.implicitHeight : field.implicitHeight

                        SectionHeader {
                            id: header
                            visible: row.isHeader
                            // Same card padding as every field row: the
                            // heading's label lines up with the field
                            // labels and its rule runs the row width.
                            anchors.left: parent.left
                            anchors.leftMargin: flickable.cardPadding
                            anchors.right: parent.right
                            anchors.rightMargin: flickable.cardPadding
                            label: row.modelData.label
                        }

                        SettingsField {
                            id: field
                            visible: !row.isHeader
                            anchors.left: parent.left
                            anchors.leftMargin: flickable.cardPadding
                            anchors.right: parent.right
                            anchors.rightMargin: flickable.cardPadding
                            isFocused: row.index === settings.currentIndex
                            animateChanges: !settings._pageSwitching
                            activatePulse: settings.fieldActivatePulse
                            // Index and scrape can't run together; while
                            // one operation is in flight the other row
                            // dims and its MouseArea stops responding.
                            // Keyboard Accept is separately gated in
                            // `_triggerIndex`/`_triggerScrape`.
                            enabled: settings._fieldEnabled(row.modelData.id)
                            label: row.modelData.label
                            value: settings._fieldValue(row.modelData.id)
                            control: settings._fieldControl(row.modelData.id)
                            checked: settings._fieldChecked(row.modelData.id)
                            actionStatus: row.modelData.id === "updateMediaDb" ? settings._indexActionStatus() : row.modelData.id === "runScraper" ? settings._scrapeActionStatus() : ""
                            onHovered: settings.currentIndex = row.index
                            onClicked: {
                                settings.currentIndex = row.index;
                                if (row.modelData.id === "mouseEnabled")
                                    settings._toggleMouseEnabled();
                                else if (row.modelData.id === "showHidden")
                                    settings._toggleShowHidden();
                                else if (row.modelData.id === "showOriginalFilenames")
                                    settings._toggleShowOriginalFilenames();
                                else if (row.modelData.id === "debugLogging")
                                    settings._requestDebugLogging(!Browse.Settings.current_debug_logging);
                                else if (row.modelData.id === "reduceMotion")
                                    settings._toggleReduceMotion();
                                else if (row.modelData.id === "swapConfirmCancel")
                                    settings._toggleSwapConfirmCancel();
                                else if (row.modelData.id === "swapOptionsView")
                                    settings._toggleSwapOptionsView();
                                else if (row.modelData.id === "crtEnabled")
                                    settings._requestCrtEnabled(!Browse.CrtVideo.crt_enabled);
                            }
                            onRightClicked: settings._goBack()
                            // Picker, action, and navigate rows route
                            // through `onAccepted` (see SettingsField's
                            // MouseArea), so the focus commit lives here
                            // too — clicking commits focus before firing
                            // the action.
                            onAccepted: {
                                settings.currentIndex = row.index;
                                if (settings._openPage(row.modelData.id))
                                    return;
                                // Non-toggle rows route here (toggles use onClicked).
                                // Defer like the keyboard path so the push-in shows
                                // before the modal opens / the screen navigates.
                                settings.fieldActivatePulse++;
                                fieldCommit._id = row.modelData.id;
                                fieldCommit.arm();
                            }
                        }
                    }
                }

                // Trailing spacer — symmetric with the leading spacer, so
                // the last field clears the bottom chevron and the cut-off
                // edge sits in whitespace.
                Item {
                    id: trailingSpacer

                    width: form.width
                    height: Sizing.pctH(2)
                }
            }
        }
    }

    // Top/bottom scroll chevrons — mirror the PagedGrid/BrowseList
    // recipe (same SVG icons, `PreserveAspectFit` + `smooth: true` +
    // `sourceSize` pinned to the painted size) but centered on the
    // viewport in the chrome gap *above* and *below* the Flickable, not
    // inside its visible band. Sitting outside the scrolled area means
    // the chevrons never overlap moving content as the user scrolls.
    // Visible only when content extends past the matching edge.
    //
    // Anchored to `settingsCard`, not `flickable`, deliberately —
    // `flickable` is a grandchild of `settings` (nested inside
    // `settingsCard`, see above), and QML anchors only resolve against a
    // parent or a sibling. Round 8 introduced `settingsCard` and left
    // these anchored to `flickable`, so every anchor line here was
    // silently rejected at runtime and both Images fell back to `x: 0,
    // y: 0` — rendering in the screen's top-left corner regardless of
    // where the card actually sat. `settingsCard` is a sibling of these
    // Images (both direct children of `settings`), and its own
    // top/bottomMargin (see its comment above) already reserves exactly
    // this chevron band outside the card.
    // Round 9: dims (Theme.textLabel) rather than hides when there's
    // nothing to scroll to in that direction, matching PageIndicator.qml's
    // treatment -- see that file's own comment. `visible` still gates on
    // whether the card itself is even showing; only the content-direction
    // half of the old condition moved to the source colour. Round 10:
    // `visible` also requires actual overflow in EITHER direction, so a
    // short page (everything fits, nothing to scroll) shows neither
    // chevron at all instead of two permanently-dim arrows.
    Image {
        source: Resources.iconUrl("ScrollUp", settings._hasContentAbove ? Theme.textPrimary : Theme.textLabel)
        width: Sizing.pctH(3)
        height: width
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.bottom: settingsCard.top
        anchors.bottomMargin: Sizing.pctH(0.5)
        anchors.horizontalCenter: settingsCard.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: !settings.optimisticLoading && !settings.showingRootGrid && (settings._hasContentAbove || settings._hasContentBelow)
    }

    Image {
        source: Resources.iconUrl("ScrollDown", settings._hasContentBelow ? Theme.textPrimary : Theme.textLabel)
        width: Sizing.pctH(3)
        height: width
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.top: settingsCard.bottom
        anchors.topMargin: Sizing.pctH(0.5)
        anchors.horizontalCenter: settingsCard.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: !settings.optimisticLoading && !settings.showingRootGrid && (settings._hasContentAbove || settings._hasContentBelow)
    }

    // Empty-state placeholder shown on runtimes with no settings to
    // expose. Centered in the body so it doesn't compete with the
    // top strip or help bar.
    Text {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        visible: !settings.optimisticLoading && settings.fieldCount === 0
        text: qsTr("No settings available on this platform")
        color: Theme.textLabel
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        renderType: Text.NativeRendering
    }

    ScreenStateOverlay {
        anchors.fill: parent
        // Fills `settings` exactly, so this is the same point the default
        // (`overlay.height / 2`) already resolves to — spelled out
        // explicitly so all three loading-capable screens wire the cue
        // anchor the same way rather than two doing it and one relying on
        // a default that happens to match.
        cueCenterY: settings.height / 2 - y
        enabled: settings.optimisticLoading
        loading: settings.optimisticLoading
        count: 0
        loadingText: qsTr("Loading settings…")
    }
}
