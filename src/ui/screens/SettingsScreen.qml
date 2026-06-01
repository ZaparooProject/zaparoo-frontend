// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton (set_resolution) still trips
// qmllint's "Member can be shadowed" check. Until the schema grows
// method-level finality, suppress the compiler category file-wide.
// qmllint disable compiler

// Settings screen — gamepad-driven vertical form. Resolution is MiSTer-only
// because it changes frontend startup video config and applies on restart.
// Button style is cross-platform and selects the resource directory for
// help-bar button glyphs (Style A/B/C/D → resources/images/buttons/{a,b,c,d}/).
// Mouse support is cross-platform and controls cursor visibility plus mouse
// hit targets.
//
// Pure input dispatcher: emits `requestHubScreen()` on Escape; left/
// right cycle the focused field's value via the model singleton.
Item {
    id: settings

    // Bound by MainLayout to `root.pendingTransition !== ""`. Settings
    // is a destination, never a source, so this is currently always
    // false when the screen is visible — kept for parity with the
    // other screens so the convention holds when a future routing
    // change adds a Settings-as-source path.
    property bool transitioning: false

    signal requestHubScreen
    // Forward signal carrying the focused action row's id. The router
    // decides what the payload means — `"uploadLog"` opens the log-
    // upload modal, `"aboutLicense"` navigates to the About screen.
    signal requestAccept(actionId: string)
    // Picker request. The router mounts `ListPickerModal` with these
    // properties and dispatches the user's selection back through the
    // matching `Browse.Settings` setter (keyed off `fieldId`).
    signal requestListPicker(title: string, entries: var, initialId: string, fieldId: string)

    // Field registry. Each entry's `kind` is `"header"` (non-focusable
    // group label) or `"field"` (a navigable row). Field entries also
    // carry an `id` that handleAction routes to the right model setter.
    // Keeping this as data (rather than a Repeater of typed children)
    // makes adding rows a one-line edit and keeps navigation uniform.
    //
    // Section grouping mirrors the Settings menus on Switch/Xbox/
    // Playnite/Pegasus: a single page with non-focusable headers
    // splitting commonly-used controls (General, Library) from rarer
    // diagnostics-flavoured ones (Advanced).
    readonly property var fields: {
        const out = [];
        out.push({
            kind: "header",
            label: qsTr("General")
        });
        if (Browse.Settings.is_mister) {
            out.push({
                kind: "field",
                id: "resolution",
                label: qsTr("Resolution")
            });
        }
        out.push({
            kind: "field",
            id: "language",
            label: qsTr("Language")
        });
        out.push({
            kind: "field",
            id: "orientation",
            label: qsTr("Orientation")
        });
        out.push({
            kind: "field",
            id: "browseLayout",
            label: qsTr("Browsing layout")
        });
        out.push({
            kind: "field",
            id: "buttonLayout",
            label: qsTr("Button style")
        });
        out.push({
            kind: "field",
            id: "screensaverTimeout",
            label: qsTr("Screensaver")
        });
        out.push({
            kind: "header",
            label: qsTr("Library")
        });
        out.push({
            kind: "field",
            id: "discoverArcadeAlternateVersions",
            label: qsTr("Discover arcade alternate versions")
        });
        out.push({
            kind: "field",
            id: "mediaImageType",
            label: qsTr("Preferred artwork")
        });
        out.push({
            kind: "field",
            id: "updateMediaDb",
            label: qsTr("Update media database")
        });
        out.push({
            kind: "field",
            id: "runScraper",
            label: qsTr("Scrape metadata")
        });
        out.push({
            kind: "field",
            id: "rescrapeExisting",
            label: qsTr("Re-scrape existing")
        });
        out.push({
            kind: "header",
            label: qsTr("Advanced")
        });
        out.push({
            kind: "field",
            id: "mouseEnabled",
            label: qsTr("Mouse support")
        });
        out.push({
            kind: "field",
            id: "debugLogging",
            label: qsTr("Debug logging")
        });
        out.push({
            kind: "field",
            id: "uploadLog",
            label: qsTr("Upload log file")
        });
        out.push({
            kind: "field",
            id: "aboutLicense",
            label: qsTr("About / License")
        });
        return out;
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
            return qsTr("%1 indexed").arg(total);
        return "";
    }

    function _scrapeActionStatus(): string {
        if (Browse.MediaStatus.scraping)
            return Browse.MediaStatus.scrape_paused ? qsTr("Paused") : qsTr("In progress");
        const total = Browse.MediaStatus.scrape_total_scraped;
        if (total > 0)
            return qsTr("%1 scraped").arg(total);
        return "";
    }

    // Index and scrape can't run concurrently — Core serialises them.
    // While one is in flight the *other* row is non-actionable so we
    // don't queue a request that Core will reject.
    readonly property bool _indexBusy: Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing
    readonly property bool _scrapeBusy: Browse.MediaStatus.scraping
    property bool rescrapeExisting: false

    // Drive the top/bottom scroll chevrons. Mirrors PagedGrid's
    // `hasPagesAbove`/`hasPagesBelow` recipe, but for a continuous
    // Flickable rather than a paginated grid. The 1-px epsilon
    // swallows sub-pixel rounding so the chevrons don't flicker on
    // exact-fit content.
    readonly property bool _hasContentAbove: flickable.contentY > 1
    readonly property bool _hasContentBelow: flickable.contentY + flickable.height < flickable.contentHeight - 1

    function _triggerIndex(): void {
        if (settings._scrapeBusy)
            return;
        if (settings._indexBusy)
            Browse.MediaStatus.cancel_index();
        else
            Browse.MediaStatus.start_index();
    }

    function _triggerScrape(): void {
        if (settings._indexBusy)
            return;
        if (settings._scrapeBusy)
            Browse.MediaStatus.cancel_scrape();
        else {
            Browse.MediaStatus.start_scrape(settings.rescrapeExisting);
            settings.rescrapeExisting = false;
        }
    }

    function _fieldEnabled(id: string): bool {
        if (id === "updateMediaDb")
            return !settings._scrapeBusy;
        if (id === "runScraper")
            return !settings._indexBusy;
        if (id === "rescrapeExisting")
            return !settings._indexBusy && !settings._scrapeBusy;
        return true;
    }

    function _fieldValue(id: string): string {
        if (id === "resolution")
            return settings._resolutionDisplay(Browse.Settings.current_resolution);
        if (id === "language")
            return settings._languageDisplay(Browse.Settings.current_language);
        if (id === "orientation")
            return settings._orientationDisplay(Browse.Settings.current_orientation);
        if (id === "browseLayout")
            return settings._browseLayoutDisplay(Browse.Settings.current_browse_layout);
        if (id === "buttonLayout")
            return settings._buttonLayoutDisplay(Browse.Settings.current_button_layout);
        if (id === "screensaverTimeout")
            return settings._screensaverTimeoutDisplay(Browse.Settings.current_screensaver_timeout);
        if (id === "mediaImageType")
            return settings._mediaImageTypeDisplay(Browse.Settings.current_media_image_type);
        return "";
    }

    function _fieldControl(id: string): string {
        if (id === "mouseEnabled" || id === "discoverArcadeAlternateVersions" || id === "debugLogging" || id === "rescrapeExisting")
            return "toggle";
        if (id === "aboutLicense")
            return "navigate";
        if (id === "updateMediaDb" || id === "runScraper" || id === "uploadLog")
            return "action";
        return "picker";
    }

    function _fieldChecked(id: string): bool {
        if (id === "debugLogging")
            return Browse.Settings.current_debug_logging;
        if (id === "discoverArcadeAlternateVersions")
            return Browse.Settings.current_discover_arcade_alternate_versions;
        if (id === "rescrapeExisting")
            return settings.rescrapeExisting;
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

    // Walk from `from` in `direction` (±1) until we hit a focusable
    // row or run off the registry. Headers are transparent — Up/Down
    // skip across them so the user feels a single flat list.
    function _seekNavigable(from: int, direction: int): int {
        let i = from + direction;
        while (i >= 0 && i < settings.fieldCount) {
            if (settings.fields[i].kind === "field")
                return i;
            i += direction;
        }
        return from;
    }

    readonly property bool focusedFieldIsToggle: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return id === "mouseEnabled" || id === "discoverArcadeAlternateVersions" || id === "debugLogging" || id === "rescrapeExisting";
    }
    // True when the focused field is a list-picker row (Accept opens a
    // modal; left/right is a no-op — pickers don't cycle inline). Drives
    // the help-bar A: Open hint.
    readonly property bool focusedFieldIsPicker: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return id === "language" || id === "orientation" || id === "browseLayout" || id === "buttonLayout" || id === "resolution" || id === "screensaverTimeout" || id === "mediaImageType";
    }
    // True when the focused field is an action button (updateMediaDb,
    // runScraper, uploadLog, aboutLicense). Drives the help-bar Accept
    // hint and the SettingsField chevron.
    readonly property bool focusedFieldIsAction: {
        if (!settings._isField(settings.currentIndex))
            return false;
        const id = settings.fields[settings.currentIndex].id;
        return id === "updateMediaDb" || id === "runScraper" || id === "uploadLog" || id === "aboutLicense";
    }
    // Verb shown on the help-bar Accept hint for the focused action
    // row. Index/scrape flip between Start and Cancel because the press
    // toggles the in-flight operation; uploadLog reads "Upload" because
    // the press opens the upload-flow modal rather than kicking off an
    // in-row job; aboutLicense reads "Open" because the press navigates.
    readonly property string focusedActionLabel: {
        if (!settings._isField(settings.currentIndex))
            return "";
        const id = settings.fields[settings.currentIndex].id;
        if (id === "updateMediaDb" || id === "runScraper")
            return settings.focusedActionBusy ? qsTr("Cancel") : qsTr("Start");
        if (id === "uploadLog")
            return qsTr("Upload");
        if (id === "aboutLicense")
            return qsTr("Open");
        return "";
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
        // Empty resolution means "fall back to frontend.toml defaults",
        // which the Settings model treats as the platform default. Render
        // it as a translated label rather than an empty cell so the user
        // sees something selectable.
        return value === "" ? qsTr("Default") : value;
    }

    function _buttonLayoutList(): list<string> {
        const raw = Browse.Settings.available_button_layouts;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _browseLayoutList(): list<string> {
        const raw = Browse.Settings.available_browse_layouts;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _languageList(): list<string> {
        const raw = Browse.Settings.available_languages;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _orientationList(): list<string> {
        const raw = Browse.Settings.available_orientations;
        return raw === undefined || raw === null ? [] : raw;
    }

    function _languageDisplay(value: string): string {
        if (value === "en")
            return qsTr("English");
        if (value === "it_IT")
            return qsTr("Italian");
        if (value === "de")
            return qsTr("German");
        if (value === "el")
            return qsTr("Greek");
        if (value === "ja")
            return qsTr("Japanese");
        if (value === "ko")
            return qsTr("Korean");
        if (value === "nl")
            return qsTr("Dutch");
        if (value === "ro")
            return qsTr("Romanian");
        if (value === "sk")
            return qsTr("Slovak");
        if (value === "uk")
            return qsTr("Ukrainian");
        if (value === "zh_CN")
            return qsTr("Chinese (Simplified)");
        if (value === "he")
            return qsTr("Hebrew");
        if (value === "ar")
            return qsTr("Arabic");
        if (value === "hi")
            return qsTr("Hindi");
        return qsTr("Auto");
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

    function _buttonLayoutDisplay(value: string): string {
        if (value === "b")
            return qsTr("Style B");
        if (value === "c")
            return qsTr("Style C");
        if (value === "d")
            return qsTr("Style D");
        return qsTr("Style A");
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

    function _mediaImageTypeDisplay(value: string): string {
        if (value === "auto")
            return qsTr("Auto");
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
            title = qsTr("Resolution");
            const list = settings._resolutionList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._resolutionDisplay(list[i])
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
        } else if (id === "orientation") {
            title = qsTr("Orientation");
            const list = settings._orientationList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._orientationDisplay(list[i])
                });
            initialId = Browse.Settings.current_orientation;
        } else if (id === "browseLayout") {
            title = qsTr("Browsing layout");
            const list = settings._browseLayoutList();
            for (let i = 0; i < list.length; i++)
                entries.push({
                    id: list[i],
                    label: settings._browseLayoutDisplay(list[i])
                });
            initialId = Browse.Settings.current_browse_layout;
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
        } else {
            return;
        }
        if (entries.length === 0)
            return;
        settings.requestListPicker(title, entries, initialId, id);
    }

    function _setMouseEnabled(direction: int): void {
        Browse.Settings.set_mouse_enabled(direction > 0);
    }

    function _toggleMouseEnabled(): void {
        Browse.Settings.set_mouse_enabled(!Browse.Settings.current_mouse_enabled);
    }

    function _setDebugLogging(direction: int): void {
        Browse.Settings.set_debug_logging(direction > 0);
    }

    function _toggleDebugLogging(): void {
        Browse.Settings.set_debug_logging(!Browse.Settings.current_debug_logging);
    }

    function _setDiscoverArcadeAlternateVersions(direction: int): void {
        Browse.Settings.set_discover_arcade_alternate_versions(direction > 0);
    }

    function _toggleDiscoverArcadeAlternateVersions(): void {
        Browse.Settings.set_discover_arcade_alternate_versions(!Browse.Settings.current_discover_arcade_alternate_versions);
    }

    function _setRescrapeExisting(direction: int): void {
        if (settings._indexBusy || settings._scrapeBusy)
            return;
        settings.rescrapeExisting = direction > 0;
    }

    function _toggleRescrapeExisting(): void {
        if (settings._indexBusy || settings._scrapeBusy)
            return;
        settings.rescrapeExisting = !settings.rescrapeExisting;
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
        else if (id === "discoverArcadeAlternateVersions")
            settings._setDiscoverArcadeAlternateVersions(direction);
        else if (id === "debugLogging")
            settings._setDebugLogging(direction);
        else if (id === "rescrapeExisting")
            settings._setRescrapeExisting(direction);
    }

    function handleAction(action: string): void {
        if (action === "up") {
            settings.currentIndex = settings._seekNavigable(settings.currentIndex, -1);
        } else if (action === "down") {
            settings.currentIndex = settings._seekNavigable(settings.currentIndex, 1);
        } else if (action === "left") {
            settings._cycleFocused(-1);
        } else if (action === "right") {
            settings._cycleFocused(1);
        } else if (action === "accept") {
            if (!settings._isField(settings.currentIndex))
                return;
            const id = settings.fields[settings.currentIndex].id;
            if (id === "mouseEnabled")
                settings._toggleMouseEnabled();
            else if (id === "discoverArcadeAlternateVersions")
                settings._toggleDiscoverArcadeAlternateVersions();
            else if (id === "debugLogging")
                settings._toggleDebugLogging();
            else if (id === "rescrapeExisting")
                settings._toggleRescrapeExisting();
            else if (id === "updateMediaDb")
                settings._triggerIndex();
            else if (id === "runScraper")
                settings._triggerScrape();
            else if (id === "uploadLog")
                settings.requestAccept("uploadLog");
            else if (id === "aboutLicense")
                settings.requestAccept("aboutLicense");
            else
                settings._openPickerForField(id);
        } else if (action === "cancel") {
            settings.requestHubScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: settings.requestHubScreen()
    }

    TopStatusStrip {
        id: topStrip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom
        height: Sizing.pctH(7)
        title: qsTr("Settings")
        currentPage: 0
        totalPages: 0
        totalText: ""
    }

    // Scroll focused row into view if it sits outside the Flickable's
    // current viewport. No-op for header indices (they aren't focusable
    // and currentIndex never lands on one in normal flow). Bind via
    // onCurrentIndexChanged below; no animation — software-renderer
    // budget can't pay for a moving column behind a focus border.
    function _scrollFocusedIntoView(): void {
        if (!settings._isField(settings.currentIndex))
            return;
        const row = rowRepeater.itemAt(settings.currentIndex);
        if (row === null)
            return;
        const top = row.y;
        const bottom = top + row.height;
        if (top < flickable.contentY)
            flickable.contentY = top;
        else if (bottom > flickable.contentY + flickable.height)
            flickable.contentY = bottom - flickable.height;
    }

    onCurrentIndexChanged: settings._scrollFocusedIntoView()

    // Form lives in a Flickable so the section bands can grow past
    // a single screen without dropping off-frame. Width capped so
    // the rows don't stretch edge-to-edge on widescreen; bottom
    // margin clears the help bar (pctH(6)) plus a small gap.
    Flickable {
        id: flickable

        // topMargin and bottomMargin are sized to leave a clear band
        // for the scroll chevrons to sit outside the scrollable area
        // (chevron pctH(3) + breathing room). bottomMargin also has to
        // clear the help bar (pctH(6)) plus a small gap.
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(4)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(10)
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(parent.width - Sizing.pctW(10), Sizing.pctW(70))
        contentWidth: width
        contentHeight: form.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: form

            width: parent.width
            spacing: Sizing.pctH(1.5)
            visible: settings.fieldCount > 0

            // Leading spacer — keeps the first field clear of the top
            // scroll chevron and gives the cut-off edge a breath of
            // whitespace instead of clipping mid-row.
            Item {
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

                    SettingsSectionHeader {
                        id: header
                        visible: row.isHeader
                        anchors.left: parent.left
                        anchors.right: parent.right
                        label: row.modelData.label
                    }

                    SettingsField {
                        id: field
                        visible: !row.isHeader
                        anchors.left: parent.left
                        anchors.right: parent.right
                        isFocused: row.index === settings.currentIndex
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
                            else if (row.modelData.id === "discoverArcadeAlternateVersions")
                                settings._toggleDiscoverArcadeAlternateVersions();
                            else if (row.modelData.id === "debugLogging")
                                settings._toggleDebugLogging();
                            else if (row.modelData.id === "rescrapeExisting")
                                settings._toggleRescrapeExisting();
                        }
                        onRightClicked: settings.requestHubScreen()
                        // Picker, action, and navigate rows route
                        // through `onAccepted` (see SettingsField's
                        // MouseArea), so the focus commit lives here
                        // too — clicking commits focus before firing
                        // the action.
                        onAccepted: {
                            settings.currentIndex = row.index;
                            if (row.modelData.id === "updateMediaDb")
                                settings._triggerIndex();
                            else if (row.modelData.id === "runScraper")
                                settings._triggerScrape();
                            else if (row.modelData.id === "uploadLog")
                                settings.requestAccept("uploadLog");
                            else if (row.modelData.id === "aboutLicense")
                                settings.requestAccept("aboutLicense");
                            else
                                settings._openPickerForField(row.modelData.id);
                        }
                    }
                }
            }

            // Trailing spacer — symmetric with the leading spacer, so
            // the last field clears the bottom chevron and the cut-off
            // edge sits in whitespace.
            Item {
                width: form.width
                height: Sizing.pctH(2)
            }
        }
    }

    // Top/bottom scroll chevrons — mirror the PagedGrid/BrowseList
    // recipe (same SVG icons, `PreserveAspectFit` + `smooth: true`)
    // but centered on the viewport in the chrome gap *above* and
    // *below* the Flickable, not inside its visible band. Sitting
    // outside the scrolled area means the chevrons never overlap
    // moving content as the user scrolls. Visible only when content
    // extends past the matching edge.
    Image {
        source: Resources.iconUrl("ScrollUp")
        width: Sizing.pctH(3)
        height: width
        anchors.bottom: flickable.top
        anchors.bottomMargin: Sizing.pctH(0.5)
        anchors.horizontalCenter: flickable.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: settings._hasContentAbove
    }

    Image {
        source: Resources.iconUrl("ScrollDown")
        width: Sizing.pctH(3)
        height: width
        anchors.top: flickable.bottom
        anchors.topMargin: Sizing.pctH(0.5)
        anchors.horizontalCenter: flickable.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: settings._hasContentBelow
    }

    // Empty-state placeholder shown on runtimes with no settings to
    // expose. Centered in the body so it doesn't compete with the
    // top strip or help bar.
    Text {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        visible: settings.fieldCount === 0
        text: qsTr("No settings available on this platform")
        color: Theme.textLabel
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.6)
        renderType: Text.NativeRendering
    }
}
