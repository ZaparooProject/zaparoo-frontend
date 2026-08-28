// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import QtQuick.Controls
import Zaparoo.Ui
import Zaparoo.Theme
import Zaparoo.Screens
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton still trips qmllint's "Member can
// be shadowed" check. Until the schema grows method-level finality,
// suppress the compiler category file-wide.
// qmllint disable compiler

// Visual tree. Edit this file in Qt Design Studio; the state machine
// and side-effects live in Main.qml which extends this layout. Keep
// this file declarative — property bindings and child objects only,
// no imperative JS or signal-handler bodies, so the designer sees
// everything in the 2D view.
ApplicationWindow {
    id: root

    // Screen constants re-exported from the manager so tests and
    // Main.qml can reference them without importing Zaparoo.Screens.
    readonly property string screenHub: ScreenManager.screenHub
    readonly property string screenSystems: ScreenManager.screenSystems
    readonly property string screenGames: ScreenManager.screenGames
    readonly property string screenFavorites: ScreenManager.screenFavorites
    readonly property string screenFavoriteSystems: ScreenManager.screenFavoriteSystems
    readonly property string screenRecents: ScreenManager.screenRecents
    readonly property string screenUpdate: ScreenManager.screenUpdate
    readonly property string screenSettings: ScreenManager.screenSettings
    readonly property string screenAbout: ScreenManager.screenAbout

    // Runtime state. `activeScreen` mirrors ScreenManager's property
    // (two-way synced below so direct assignment from tests still
    // works).
    //
    // `fullScreen` defaults true so the embedded build's first binding
    // pass for `width`/`height`/`visibility` evaluates against the
    // correct branch — Qt's createWithInitialProperties runs bindings
    // BEFORE applying initialProperties, so a `false` default would
    // commit width=1280/height=720 for one pass, then re-bind. On
    // linuxfb that one pass is what the writer thread copies to the
    // CRT region (visible as a whole-frame size snap on first paint).
    // Desktop preview sets fullScreen=false via initialProperties.
    property bool fullScreen: true
    property bool crtNativePath: false
    property bool bitmapType: false
    property bool debugCrtSafeAreaOverlay: false
    property string activeScreen: ScreenManager.activeScreen
    readonly property bool updateEnabled: Browse.BuildInfo.update_enabled

    // Desktop CRT preview. When `crtPreview` is true and `videoWidth` /
    // `videoHeight` are nonzero, the visual scene renders at the
    // logical (videoWidth x videoHeight) size and is integer-upscaled
    // via a layered wrapper Item -- each logical pixel becomes an N x N
    // block of physical pixels with nearest-neighbour filtering on the
    // upscale step itself (so the preview faithfully shows the same
    // pixels MiSTer would copy to its CRT). Off-MiSTer trigger only;
    // the embedded build keeps these defaults so the binary still
    // draws straight to the framebuffer.
    property int videoWidth: 0
    property int videoHeight: 0
    property bool crtPreview: false
    // Explicit override from `ZAPAROO_CRT_PREVIEW_SCALE`. 0 = auto-pick
    // the largest integer scale that fits the primary screen.
    property int crtPreviewScale: 0
    readonly property int _crtPreviewMinScale: 3
    readonly property int _crtPreviewMaxScale: 5
    property bool _crtPreviewResizeGuard: false

    readonly property bool _crtPreviewActive: root.crtPreview && root.videoWidth > 0 && root.videoHeight > 0
    property bool _startupTraceActive: true
    property bool _statusIconsEnabled: false
    property bool _headerMediaActivityEnabled: false
    property bool _firstFrameSeen: false
    readonly property bool _debugCrtSafeAreaGuideVisible: root.debugCrtSafeAreaOverlay && root.crtNativePath && Sizing.screenHeight <= 300

    // Emitted for every presented frame. Main.qml uses this to close
    // responsiveness timings on the first frame containing a destination
    // screen; startup still uses `_firstFrameSeen` below.
    signal framePresented
    property bool systemsScreenRequested: false
    property bool gamesScreenRequested: false
    // Router-owned deep-page restoration gate. Keeps Games content hidden while
    // cursor pages are appended until the persisted parent selection exists.
    property bool gamesSelectionRestorePending: false
    property bool favoritesScreenRequested: false
    property bool favoriteSystemsScreenRequested: false
    property string favoritesSystemId: ""
    property bool recentsScreenRequested: false
    property bool settingsScreenRequested: false
    property bool aboutScreenRequested: false
    property bool cardWriteModalRequested: false
    property bool settingNeedsRestartModalRequested: false
    property bool contextMenuRequested: false
    property bool qrCodeModalRequested: false
    property bool gameInfoModalRequested: false
    property bool commercialNoticeModalRequested: false
    property bool coreVersionModalRequested: false
    property bool actionErrorModalRequested: false
    property bool randomFailedModalRequested: false
    property bool logUploadModalRequested: false
    property bool quitConfirmModalRequested: false
    property bool listPickerModalRequested: false
    property bool letterJumpModalRequested: false
    property bool crtCalibrationModalRequested: false
    property bool scrapeSetupModalRequested: false
    property bool indexSetupModalRequested: false

    function _startupTrace(): void {
        if (!root._startupTraceActive)
            return;
        root._trace(arguments);
    }

    // Cover load events, deliberately outside the `_startupTraceActive` gate.
    // That gate closes on the first Hub paint, which is before the Systems grid
    // and the Settings tiles have drawn a single icon -- exactly where pop-in
    // would still be hiding. `console.debug` is already silent unless
    // `[logging] debug = true`, so there is no second gate to add.
    // See docs/architecture.md -> "Measuring cover pop-in".
    function _coverTrace(): void {
        root._trace(arguments);
    }

    function _trace(args: var): void {
        const parts = [];
        for (let i = 0; i < args.length; i++)
            parts.push(String(args[i]));
        console.debug(parts.join(" "));
    }

    function _clampCrtPreviewScale(scale: int): int {
        return Math.max(root._crtPreviewMinScale, Math.min(root._crtPreviewMaxScale, scale));
    }

    // Largest integer scale that keeps the upscaled window inside
    // the primary screen with a 5% margin reserved for window
    // decoration / panel chrome. Falls back to 4 when Screen
    // metrics aren't available yet (very brief during construction
    // on some platforms). Clamped to the desktop resize band so the
    // preview always starts at one of the supported integer steps.
    readonly property int _crtPreviewInitialScale: {
        if (!_crtPreviewActive)
            return 1;
        if (root.crtPreviewScale > 0)
            return root._clampCrtPreviewScale(root.crtPreviewScale);
        const sw = Screen.width;
        const sh = Screen.height;
        if (sw <= 0 || sh <= 0)
            return 4;
        const sx = Math.floor((sw * 0.95) / root.videoWidth);
        const sy = Math.floor((sh * 0.95) / root.videoHeight);
        return root._clampCrtPreviewScale(Math.max(1, Math.min(sx, sy)));
    }

    readonly property int _crtPreviewEffectiveScale: {
        if (!_crtPreviewActive)
            return 1;
        if (root.crtPreviewScale > 0)
            return root._clampCrtPreviewScale(root.crtPreviewScale);
        const sx = Math.floor(root.width / root.videoWidth);
        const sy = Math.floor(root.height / root.videoHeight);
        return root._clampCrtPreviewScale(Math.max(1, Math.min(sx, sy)));
    }

    function applyCrtPreviewScale(scale: int): void {
        if (!root._crtPreviewActive)
            return;
        const clamped = root._clampCrtPreviewScale(scale);
        const targetWidth = root.videoWidth * clamped;
        const targetHeight = root.videoHeight * clamped;
        if (root.width === targetWidth && root.height === targetHeight)
            return;
        root._crtPreviewResizeGuard = true;
        root.width = targetWidth;
        root.height = targetHeight;
        root._crtPreviewResizeGuard = false;
    }

    // Defaults keep the design canvas at a sensible aspect for Design
    // Studio. Fullscreen embedded builds (MiSTer) need the screen
    // dims applied at construction so the first paint matches the
    // FB layout — Component.onCompleted fires after the first frame,
    // so an imperative override there leaves a wrong-size first
    // frame on screen (visible as a zoomed top-left slice on CRT,
    // where the frontend's writer thread copies that slice into the
    // FPGA's 320x240 scan-out region). For windowed/preview builds
    // the binding only evaluates once at construction (Screen.width
    // is constant per session) so it doesn't fight user resizes.
    width: root.fullScreen ? Screen.width : 1280
    height: root.fullScreen ? Screen.height : 720
    minimumWidth: _crtPreviewActive ? root.videoWidth * (root.crtPreviewScale > 0 ? root._clampCrtPreviewScale(root.crtPreviewScale) : root._crtPreviewMinScale) : 426
    minimumHeight: _crtPreviewActive ? root.videoHeight * (root.crtPreviewScale > 0 ? root._clampCrtPreviewScale(root.crtPreviewScale) : root._crtPreviewMinScale) : 240
    maximumWidth: _crtPreviewActive ? root.videoWidth * (root.crtPreviewScale > 0 ? root._clampCrtPreviewScale(root.crtPreviewScale) : root._crtPreviewMaxScale) : 16777215
    maximumHeight: _crtPreviewActive ? root.videoHeight * (root.crtPreviewScale > 0 ? root._clampCrtPreviewScale(root.crtPreviewScale) : root._crtPreviewMaxScale) : 16777215
    visible: true
    visibility: root.fullScreen ? Window.FullScreen : Window.Windowed
    title: qsTr("Zaparoo Frontend")

    onWidthChanged: {
        if (root._crtPreviewActive && root.crtPreviewScale === 0 && !root._crtPreviewResizeGuard)
            root.applyCrtPreviewScale(root._crtPreviewEffectiveScale);
    }
    onHeightChanged: {
        if (root._crtPreviewActive && root.crtPreviewScale === 0 && !root._crtPreviewResizeGuard)
            root.applyCrtPreviewScale(root._crtPreviewEffectiveScale);
    }
    onFrameSwapped: {
        root.framePresented();
        if (!root._firstFrameSeen) {
            root._firstFrameSeen = true;
            root._statusIconsEnabled = true;
            root._headerMediaActivityEnabled = true;
            root._startupTrace("startup/qml firstFrameSwapped", "statusIconsEnabled=" + root._statusIconsEnabled, "mediaActivityEnabled=" + root._headerMediaActivityEnabled);
        }
    }

    // When the window crosses to a different screen (e.g. dev drags
    // it from a 4K to a 1080p monitor), Qt updates Screen.width and
    // Screen.height. The previously-picked integer scale may no
    // longer fit the smaller screen, so recompute against the new
    // dimensions and shrink the window if needed. Auto-scale only;
    // an explicit ZAPAROO_CRT_PREVIEW_SCALE override is honored.
    readonly property real _crtPreviewScreenW: Screen.width
    readonly property real _crtPreviewScreenH: Screen.height
    on_CrtPreviewScreenWChanged: _maybeShrinkCrtPreviewToScreen()
    on_CrtPreviewScreenHChanged: _maybeShrinkCrtPreviewToScreen()

    function _maybeShrinkCrtPreviewToScreen(): void {
        if (!root._crtPreviewActive || root.crtPreviewScale > 0)
            return;
        const sw = Screen.width;
        const sh = Screen.height;
        if (sw <= 0 || sh <= 0)
            return;
        const sx = Math.floor((sw * 0.95) / root.videoWidth);
        const sy = Math.floor((sh * 0.95) / root.videoHeight);
        const fitScale = root._clampCrtPreviewScale(Math.max(1, Math.min(sx, sy)));
        if (root._crtPreviewEffectiveScale > fitScale)
            root.applyCrtPreviewScale(fitScale);
    }

    // Help-bar glyphs. "auto" defers the family to the connected controller
    // (via the MiSTer input report, projected by Browse.ControllerReport);
    // a manual pick pins the family. Confirm/cancel face glyphs always
    // follow the live report regardless of which family is active -- which
    // physical button accepts is a fact about the connected controller
    // (Main's own OK/Cancel swap included), not an aesthetic choice, so a
    // manually-pinned art style must not silently show the wrong button.
    readonly property bool _autoButtonLayout: Browse.Settings.current_button_layout === "auto"
    readonly property string _effectiveButtonLayout: root._autoButtonLayout ? Browse.ControllerReport.glyph_layout : Browse.Settings.current_button_layout
    // Whether the keyboard is the *live* active input source, per the
    // MiSTer input report -- deliberately read straight off
    // Browse.ControllerReport.glyph_layout, not _effectiveButtonLayout, so a
    // manual "Style E" pin (Settings lists it alongside A-D) doesn't get
    // treated as keyboard input just because a controller player likes its
    // look. Enter/Escape are fixed keys, so "Swap controller confirm/cancel"
    // (Settings > Controls & Input) never applies while a real keyboard is
    // active -- see Main.qml's `_swapConfirmCancelAction`, which uses this
    // same property to guard the action-dispatch side of the same swap.
    readonly property bool _keyboardActive: Browse.ControllerReport.glyph_layout === "style_e"
    readonly property bool _swapConfirmCancel: Browse.Settings.current_swap_confirm_cancel && !root._keyboardActive
    // "Swap controller options/view" -- same idea as confirm/cancel above,
    // but for ButtonX/ButtonY. Unlike confirm/cancel, Main_MiSTer never
    // reverse-maps these, so with the swap off they're always the fixed
    // FaceNorth/FaceWest position -- this setting exists purely to
    // compensate for a controller whose own X/Y mapping is backwards. Same
    // keyboard exemption as confirm/cancel (Tab/Space are fixed keys); see
    // Main.qml's `_swapOptionsViewAction`.
    readonly property bool _swapOptionsView: Browse.Settings.current_swap_options_view && !root._keyboardActive

    Binding {
        target: Resources
        property: "buttonLayout"
        value: root._effectiveButtonLayout
    }

    Binding {
        target: Resources
        property: "confirmButton"
        value: root._swapConfirmCancel ? Browse.ControllerReport.cancel_button : Browse.ControllerReport.accept_button
    }

    Binding {
        target: Resources
        property: "cancelButton"
        value: root._swapConfirmCancel ? Browse.ControllerReport.accept_button : Browse.ControllerReport.cancel_button
    }

    Binding {
        target: Resources
        property: "optionsButton"
        value: root._swapOptionsView ? "FaceWest" : "FaceNorth"
    }

    Binding {
        target: Resources
        property: "viewButton"
        value: root._swapOptionsView ? "FaceNorth" : "FaceWest"
    }

    Binding {
        target: Resources
        property: "systemLogoStyle"
        value: Browse.Settings.current_system_logo_style
    }

    Binding {
        target: Theme
        property: "crtNativePath"
        value: root.crtNativePath
    }

    Binding {
        target: Sizing
        property: "crtNativePath"
        value: root.crtNativePath
    }

    Binding {
        target: Theme
        property: "bitmapType"
        value: root.bitmapType
    }

    Binding {
        target: Sizing
        property: "bitmapType"
        value: root.bitmapType
    }

    Binding {
        target: Sizing
        property: "swapPercentageAxes"
        value: root._sceneRotated
    }

    // Screen plumbing exposed for Main.qml's orchestration. Anything
    // inside the screens (categories row, systems/games grids) is
    // reached via root.hubScreen.* / root.systemsScreen.* /
    // root.gamesScreen.* — no per-widget aliases here.
    property alias hubScreen: hubScreen
    property var systemsScreen: systemsScreenLoader.item
    property var gamesScreen: gamesScreenLoader.item
    property var favoritesScreen: favoritesScreenLoader.item
    property var favoriteSystemsScreen: favoriteSystemsScreenLoader.item
    property var recentsScreen: recentsScreenLoader.item
    property var updateScreen: updateScreenLoader.item
    property var settingsScreen: settingsScreenLoader.item
    property var aboutScreen: aboutScreenLoader.item
    property var cardWriteModal: cardWriteModalLoader.item
    property var contextMenu: contextMenuLoader.item
    property var qrCodeModal: qrCodeModalLoader.item
    property var commercialNoticeModal: commercialNoticeModalLoader.item
    property var coreVersionModal: coreVersionModalLoader.item
    property var actionErrorModal: actionErrorModalLoader.item
    property var gameInfoModal: gameInfoModalLoader.item
    property var logUploadModal: logUploadModalLoader.item
    property var quitConfirmModal: quitConfirmModalLoader.item
    property var settingNeedsRestartModal: settingNeedsRestartModalLoader.item
    property var listPickerModal: listPickerModalLoader.item
    property var letterJumpModal: letterJumpModalLoader.item
    property var crtCalibrationModal: crtCalibrationModalLoader.item
    property var scrapeSetupModal: scrapeSetupModalLoader.item
    property var indexSetupModal: indexSetupModalLoader.item
    property alias headerBar: headerBar
    property alias screensaverOverlay: screensaverOverlay
    // Exposed so Main.qml binds Sizing.screenWidth/Height to the
    // logical scene dimensions. In rotated mode this is the swapped
    // B x A layout space while the outer framebuffer still stays A x B.
    property alias scene: scene

    property bool cardWriteModalVisible: false
    property bool cardWriteFailed: false
    property bool qrCodeModalVisible: false
    property bool commercialNoticeModalVisible: false
    property bool coreVersionModalVisible: false
    property bool actionErrorModalVisible: false
    property string actionErrorKey: ""
    property string actionErrorTitle: ""
    property string actionErrorBody: ""
    property string actionErrorButtonLabel: qsTr("OK")
    property bool randomFailedModalVisible: false
    property bool gameInfoModalVisible: false
    // QR modal wording, set by Main.qml before opening. The shared
    // Browse.QrCode singleton is a single slot, so the payload and the
    // copy describing it are always set together at the call site.
    property string qrCodeModalTitle: ""
    property string qrCodeModalInstruction: ""
    property string qrCodeModalUrlText: ""
    property bool logUploadModalVisible: false
    property bool scrapeSetupModalVisible: false
    property bool indexSetupModalVisible: false
    property bool quitConfirmModalVisible: false
    property bool listPickerModalVisible: false
    property bool settingNeedsRestartModalVisible: false
    // Letter-jump grid. Entries are bound live from
    // Browse.GamesModel.letter_index_json so the grid fills in when the facet
    // lands; `letterJumpLoading` distinguishes "still fetching" from "no rail".
    property bool letterJumpModalVisible: false
    property var letterJumpEntries: []
    property bool letterJumpLoading: false
    property bool crtCalibrationModalVisible: false
    // Round-trip state for the list picker. The router writes these
    // when opening the modal (Settings emits requestListPicker with
    // fieldId so the accept handler can dispatch back to the right
    // Browse.Settings.set_X without re-parsing the title).
    property string listPickerTitle: ""
    property var listPickerEntries: []
    property string listPickerInitialId: ""
    property string listPickerFieldId: ""
    property bool contextMenuVisible: false
    property rect contextMenuAnchor: Qt.rect(0, 0, 0, 0)
    // Corner radius of the anchored tile/row, for ContextMenu's rounded
    // scrim hole. 0 keeps the square hole (e.g. hub_favorites' action
    // tile, which isn't yet known to be a PressableSurface).
    property int contextMenuAnchorRadius: 0
    // Owner-aware. Written by Main.qml at openContextMenu time; each entry
    // is `{ id: string, label: string }`. The router switches on `id`, not
    // position, so adding/removing entries can't silently re-map actions.
    // TODO: `Browse.SystemStatus.has_nfc` (used by Main.qml when building
    // the games-tile entries) is only updated when Core runs locally
    // (rust/frontend/src/models/system_status.rs:88). Remote-Core readers
    // aren't tracked yet — wire a Core-driven reader-status feed before
    // showing "Write to NFC token" in remote-Core configs.
    property var contextMenuEntries: []

    signal contextMenuAccepted(string id)
    signal contextMenuCloseRequested
    signal closeGameInfoRequested

    // Transition state owned by Main.qml. "" while idle; non-empty while
    // the router is waiting on a model fill or a delayed loading cue
    // before flipping `activeScreen` / rebrowsing. Declared here so the
    // source-screen content-hiding bindings (row/grid `visible`) resolve
    // statically in qmllint.
    property string pendingTransition: ""
    readonly property int loadingIndicatorDelayMs: 300
    readonly property int minimumLoadingVisibleMs: 200
    property bool transitionCueVisible: false
    // Games uses same progressive reveal for screen entry and in-screen folder
    // replacement: model/card frame first, raster covers from following frame.
    property bool gamesCoverRevealReady: true
    // Folder navigation timing spans user input through model readiness and
    // first presentation. Main.qml owns lifecycle; GamesScreen supplies input
    // timestamp before synchronous state persistence.
    property double gamesNavigationInputAt: 0
    property double gamesNavigationModelReadyAt: 0
    property string gamesNavigationAction: ""

    // Cold-launch boot gate. Non-Hub restores stay behind BootOverlay /
    // startupRestoreCurtain until the target can paint; Hub restores take the
    // optimistic path and show placeholder Hub immediately while Core/catalog
    // boot in the background.
    property bool bootComplete: false
    property bool startupRestoreCurtainVisible: Browse.AppState.active_screen !== "" && Browse.AppState.active_screen !== root.screenHub
    readonly property bool optimisticHubVisible: !root.bootComplete && !root.startupRestoreCurtainVisible && root.activeScreen === root.screenHub
    readonly property bool coreIndependentStartupVisible: !root.bootComplete && !root.startupRestoreCurtainVisible && (root.activeScreen === root.screenSettings || root.activeScreen === root.screenAbout)
    readonly property bool catalogStillBooting: !Browse.CategoriesModel.loaded && (Browse.CategoriesModel.error_message ?? "") === ""

    // Per-screen state derivation. Shape mirrors ScreenStateOverlay's
    // `state` ternary so the help bar and the in-screen overlay agree
    // on what state each screen is in. Hub has no Loading row —
    // CategoriesModel binds eagerly via bind_to_endpoint! and exposes
    // no `loading` qproperty, so a count-of-zero collapses straight
    // into Empty (matching the overlay's existing behavior on Hub).
    readonly property string systemsScreenState: root.activeScreen !== root.screenSystems ? "" : ((Browse.SystemsModel.loading || root.catalogStillBooting) ? "loading" : ((Browse.SystemsModel.error_message ?? "") !== "" ? "error" : (Browse.SystemsModel.count === 0 ? "empty" : "ready")))

    readonly property string gamesScreenState: root.activeScreen !== root.screenGames ? "" : ((Browse.GamesModel.loading || root.catalogStillBooting) ? "loading" : ((Browse.GamesModel.error_message ?? "") !== "" ? "error" : (Browse.GamesModel.count === 0 ? "empty" : "ready")))

    readonly property string favoritesScreenState: root.activeScreen !== root.screenFavorites ? "" : ((Browse.FavoritesModel.loading || root.catalogStillBooting) ? "loading" : ((Browse.FavoritesModel.error_message ?? "") !== "" ? "error" : (Browse.FavoritesModel.count === 0 ? "empty" : "ready")))

    readonly property string favoriteSystemsScreenState: root.activeScreen !== root.screenFavoriteSystems ? "" : ((Browse.FavoriteSystemsModel.loading || root.catalogStillBooting) ? "loading" : ((Browse.FavoriteSystemsModel.error_message ?? "") !== "" ? "error" : (Browse.FavoriteSystemsModel.count === 0 ? "empty" : "ready")))

    readonly property string hubScreenState: (Browse.CategoriesModel.error_message ?? "") !== "" ? "error" : (Browse.CategoriesModel.count === 0 ? "empty" : "ready")

    readonly property string recentsScreenState: root.activeScreen !== root.screenRecents ? "" : ((Browse.RecentsModel.loading || root.catalogStillBooting) ? "loading" : ((Browse.RecentsModel.error_message ?? "") !== "" ? "error" : (Browse.RecentsModel.count === 0 ? "empty" : "ready")))
    readonly property string displayOrientation: Browse.Settings.current_orientation
    readonly property bool _sceneRotated: root.displayOrientation === "cw" || root.displayOrientation === "ccw"
    // Round 10: split into per-family properties -- Systems/FavoriteSystems
    // follow the systems layout preference, Games/Favorites/Recents follow
    // the games one. `_browseViewId` below already branches on screen
    // family for exactly this reason; only the boolean it reads needed to
    // stop being shared.
    readonly property bool _systemsBrowseListLayout: Browse.Settings.current_systems_browse_layout === "list"
    readonly property bool _gamesBrowseListLayout: Browse.Settings.current_games_browse_layout === "list"
    readonly property bool _systemsBrowseTateListLayout: root._systemsBrowseListLayout && root.displayOrientation !== "horizontal"
    readonly property bool _gamesBrowseTateListLayout: root._gamesBrowseListLayout && root.displayOrientation !== "horizontal"
    readonly property string _browseViewId: {
        if (root.activeScreen === root.screenSystems || root.activeScreen === root.screenFavoriteSystems)
            return root._systemsBrowseListLayout ? (root._systemsBrowseTateListLayout ? "systemsListTate" : "systemsList") : "systemsGrid";
        if (root.activeScreen === root.screenGames || root.activeScreen === root.screenFavorites || root.activeScreen === root.screenRecents)
            return root._gamesBrowseListLayout ? (root._gamesBrowseTateListLayout ? "gamesListTate" : "gamesList") : "gamesGrid";
        return "gamesGrid";
    }
    // Whichever of the two layout preferences the CURRENT active screen's
    // family follows -- for callers like `browseHeaderTitle` below that
    // need "is the active screen in list layout" without re-deriving
    // `_browseViewId`'s own family branch.
    readonly property bool _activeBrowseListLayout: {
        if (root.activeScreen === root.screenSystems || root.activeScreen === root.screenFavoriteSystems)
            return root._systemsBrowseListLayout;
        if (root.activeScreen === root.screenGames || root.activeScreen === root.screenFavorites || root.activeScreen === root.screenRecents)
            return root._gamesBrowseListLayout;
        return false;
    }
    readonly property string _browseThemeId: BrowseLayouts.currentThemeId
    readonly property var _browseViewProfile: BrowseLayouts.themeProfile(root._browseThemeId, root._browseViewId)
    // Same resolution as GamesScreen's own `_systemDisplayName` -- see the
    // comment there for why the category-scoped `index_for_system_id` lookup
    // this used to do disagreed with the Systems grid, and why the two `void`
    // reads are load-bearing.
    readonly property string _crtGamesHeaderTitle: {
        if (root.activeScreen !== root.screenGames)
            return "";
        const sid = Browse.GamesModel.current_system_id;
        if (sid === "")
            return "";
        void Browse.SystemsModel.count;
        void Browse.Settings.current_region;
        const resolved = Browse.SystemsModel.system_name_for_id(sid);
        return resolved !== "" ? resolved : sid;
    }
    readonly property string browseHeaderTitle: {
        if (!root.crtNativePath)
            return "";
        if (root._activeBrowseListLayout)
            return "";
        if (root.activeScreen === root.screenSystems)
            return CategoryIds.displayName(Browse.SystemsModel.current_category);
        if (root.activeScreen === root.screenGames)
            return root._crtGamesHeaderTitle;
        if (root.activeScreen === root.screenFavorites)
            return qsTr("Favorites");
        if (root.activeScreen === root.screenFavoriteSystems)
            return qsTr("Favorites");
        if (root.activeScreen === root.screenRecents)
            return qsTr("Recently played");
        return "";
    }

    signal cancelCardWriteRequested
    signal closeQrCodeRequested
    signal closeCommercialNoticeRequested
    signal closeCoreVersionRequested
    signal actionErrorAccepted
    signal closeRandomFailedRequested
    signal closeLogUploadRequested
    signal closeScrapeSetupRequested
    signal closeIndexSetupRequested
    // Bubbled from ScrapeSetupModal's own scraper-picker row -- Main.qml
    // owns opening the shared ListPickerModal (the modal itself can't
    // instantiate a second top-level modal directly; see the Loader
    // pattern every other modal-with-nested-picker interaction uses).
    signal requestScraperPicker
    // Bubbled from either ScrapeSetupModal's or IndexSetupModal's own
    // Systems row -- shared because the two modals are mutually
    // exclusive (only one Settings Accept path opens either at a time),
    // so Main.qml's handler tells them apart by which one is currently
    // visible rather than needing two identical signals.
    signal requestSystemScopePicker
    signal closeQuitConfirmRequested
    signal quitConfirmAccepted
    signal listPickerAccepted(string fieldId, string selectedId)
    signal listPickerCloseRequested(string fieldId)
    signal letterJumpAccepted(int itemOffset)
    signal letterJumpCloseRequested
    signal acceptRestart
    signal cancelRestart
    signal closeCrtCalibrationRequested

    // Two-way sync between root.activeScreen and ScreenManager.activeScreen.
    // Binding-breaking assignments (tests setting root.activeScreen = "games")
    // still propagate to ScreenManager; ScreenManager changes (from the
    // screens) still update root.activeScreen. The `if (X !== Y)` guard
    // on each side prevents the obvious cycle. Adding any transformation
    // between the two sides would defeat the guard — see #24 for the
    // tracked single-source-of-truth refactor.
    onActiveScreenChanged: {
        root._startupTrace("startup/qml activeScreenChanged", "activeScreen=" + root.activeScreen, "pendingTransition=" + root.pendingTransition, "startupRestoreCurtainVisible=" + root.startupRestoreCurtainVisible);
        if (ScreenManager.activeScreen !== root.activeScreen)
            ScreenManager.activeScreen = root.activeScreen;
    }
    onStartupRestoreCurtainVisibleChanged: {
        root._startupTrace("startup/qml startupRestoreCurtainVisibleChanged", "visible=" + root.startupRestoreCurtainVisible, "activeScreen=" + root.activeScreen);
    }
    Connections {
        target: ScreenManager
        function onActiveScreenChanged(): void {
            if (root.activeScreen !== ScreenManager.activeScreen)
                root.activeScreen = ScreenManager.activeScreen;
        }
    }

    // CRT preview wrapper. Default (preview off): fills the parent
    // window 1:1, scale 1, no layer -- identical to pre-preview
    // rendering. Preview on: fixed (videoWidth x videoHeight) logical
    // size, scaled by crtPreviewScale around the top-left corner with
    // nearest-neighbour filtering, and layered so all the children
    // paint through one cached pixmap that gets the integer-upscale.
    // `smooth: false` and `layer.smooth: false` together preserve
    // the pixel grid; without both, Qt bilinear-filters the upscale
    // and the CRT artefacts the preview is meant to expose get
    // smeared out.
    //
    // The desktop preview pins Qt's high-DPI scaling to 1 in main.cpp
    // when --crt is set, so logical pixels map 1:1 to physical pixels
    // and the GL backend's final logical-to-physical present step is
    // a no-op (no bilinear filtering smearing the integer upscale).

    // Native CRT safe-area inset. The 352-px active line fills a
    // standard NTSC/PAL raster edge-to-edge, so the outer few percent
    // of the framebuffer is cropped on most tubes (broadcast
    // overscan). Shrinking `scene` by 5% per side keeps every
    // interactive element inside the SMPTE action-safe area - and
    // because Main.qml binds Sizing.screenWidth/Height to the scene,
    // every pct-derived size re-solves automatically. The background
    // items below restore full bleed with negative margins so the
    // cropped band still shows intentional content.
    readonly property int _crtInsetW: root.crtNativePath ? 2 * Math.round(framebufferScene.width * 0.05) : 0
    readonly property int _crtInsetH: root.crtNativePath ? 2 * Math.round(framebufferScene.height * 0.05) : 0

    Item {
        id: framebufferScene

        x: 0
        y: 0
        width: root._crtPreviewActive ? root.videoWidth : root.width
        height: root._crtPreviewActive ? root.videoHeight : root.height
        clip: false
        transformOrigin: Item.TopLeft
        scale: root._crtPreviewActive ? root._crtPreviewEffectiveScale : 1
        // smooth/layer.smooth control the integer-upscale sampling on
        // the wrapper itself, NOT the rendering of the children. Both
        // must stay false so each logical pixel maps to a clean
        // N x N physical block; otherwise the upscale would smear
        // genuine antialiasing artefacts and defeat the preview's
        // diagnostic value. Antialiasing inside the scene (font
        // hinting, line edges) is intentionally left untouched so
        // the preview matches what MiSTer would actually render.
        //
        // `layer.textureSize` is critical: without it, on a hi-DPI
        // screen Qt sizes the layer texture at the item's *physical*
        // pixel count (logical × devicePixelRatio), so children
        // rasterise at e.g. 768×448 with full AA before the layer is
        // captured. The nearest-neighbour upscale then samples that
        // already-antialiased high-rez source -- which is what makes
        // the preview look universally blurry instead of pixelated.
        // Pinning the texture to videoWidth × videoHeight forces
        // logical-pixel rasterisation, so the AA the preview shows is
        // *exactly* what MiSTer's framebuffer would capture.
        smooth: false
        layer.enabled: root._crtPreviewActive
        layer.smooth: false
        layer.textureSize: root._crtPreviewActive ? Qt.size(root.videoWidth, root.videoHeight) : Qt.size(0, 0)

        Item {
            id: scene

            x: Sizing.center(framebufferScene.width, width)
            y: Sizing.center(framebufferScene.height, height)
            // Subtract the safe-area inset on the framebuffer axis each
            // scene dimension spans (the swap mirrors the rotation).
            width: root._sceneRotated ? framebufferScene.height - root._crtInsetH : framebufferScene.width - root._crtInsetW
            height: root._sceneRotated ? framebufferScene.width - root._crtInsetW : framebufferScene.height - root._crtInsetH
            clip: false
            transformOrigin: Item.Center
            rotation: root.displayOrientation === "cw" ? 90 : root.displayOrientation === "ccw" ? -90 : 0

            // ── Background ────────────────────────────────────────────────────────────

            Rectangle {
                anchors.fill: parent
                // Full bleed: overscan back past the safe-area inset so
                // the background reaches the true framebuffer edge.
                anchors.margins: -Math.max(root._crtInsetW, root._crtInsetH)
                color: Theme.bgDeep
            }

            // ── Top header (logo + status row + status line) ───────────────────────────

            // Single component owning the brand mark, host status icons +
            // clock, and the Core/task status line. Height is fixed
            // (Sizing.headerHeight) so the status line's slot stays reserved
            // when idle and the logo always matches the stacked rows.
            // Screens clear `Sizing.headerBottom`.
            HeaderBar {
                id: headerBar

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.topMargin: Sizing.headerTopMargin
                layoutProfile: root._browseViewProfile
                browseTitle: root.browseHeaderTitle
                statusIconsEnabled: root._statusIconsEnabled
                mediaActivityEnabled: root._headerMediaActivityEnabled
                z: 200
            }

            // ── Screen containers ─────────────────────────────────────────────────────

            // Stacked container — only the active screen paints. Hub →
            // Systems → Games drill-downs (and Esc back) are instant cuts:
            // bind `visible` directly on the live `activeScreen` and let
            // Qt's scene graph swap which screen paints in one frame.
            //
            // Earlier iterations tried a horizontal slide, then a direct
            // opacity fade on the screen container, then an overlay-rectangle
            // fade. All three were structurally too expensive for Qt Quick's
            // Software adaptation when the destination screen is a dense
            // grid. Translucent overlays don't subtract from the renderer's
            // dirty region, so every cell underneath re-rasterises per frame
            // throughout the fade — text labels, cover images, card bodies.
            // Instant cuts paint the new screen exactly once. See
            // docs/qml-gotchas.md → "Software-renderer animation costs"
            // for the full reasoning.
            //
            // Transition feedback is a delayed static LoadingIndicator, not
            // an animated screen effect. Quick swaps cut directly; slower
            // model fills hide source rows/grids only after the loading cue is
            // visible. Bottom selection context and help stay frozen until the
            // destination cut so the source screen does not dismantle itself.
            //
            // The wrapper `Item` stays for grouping clarity; with no fade
            // machinery it carries no buffered state. Model bindings stay
            // live across deactivations so Esc back to systems doesn't
            // re-instantiate the whole delegate tree — Items with
            // `visible: false` skip painting but keep their scene graph
            // alive and their decoded covers warm.
            Item {
                id: stackedScreens

                anchors.fill: parent
                visible: !root.startupRestoreCurtainVisible && (root.bootComplete || root.optimisticHubVisible || root.coreIndependentStartupVisible)

                HubScreen {
                    id: hubScreen
                    anchors.fill: parent
                    visible: root.activeScreen === root.screenHub
                    transitioning: root.transitionCueVisible
                    resumeModelEnabled: root._firstFrameSeen
                    onVisibleChanged: {
                        if (!visible || !root._startupTraceActive)
                            return;
                        root._startupTrace("startup/qml firstHubVisible", "restoreCurtainVisible=" + root.startupRestoreCurtainVisible, "connectionState=" + Browse.AppStatus.connection_state, "categories=" + Browse.CategoriesModel.count);
                        root._startupTraceActive = false;
                    }
                }

                Loader {
                    id: systemsScreenLoader
                    anchors.fill: parent
                    active: root.systemsScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenSystems
                    onLoaded: console.debug("responsiveness systems screen mounted")
                    sourceComponent: Component {
                        SystemsScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            preparingTransition: root.pendingTransition === "systems"
                            active: root.activeScreen === root.screenSystems
                            optimisticLoading: root.activeScreen === root.screenSystems && root.catalogStillBooting
                        }
                    }
                }

                Loader {
                    id: gamesScreenLoader
                    anchors.fill: parent
                    active: root.gamesScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenGames
                    sourceComponent: Component {
                        GamesScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            active: root.activeScreen === root.screenGames
                            coverRevealReady: root.gamesCoverRevealReady
                            optimisticLoading: root.activeScreen === root.screenGames && (root.catalogStillBooting || root.gamesSelectionRestorePending)
                        }
                    }
                }

                Loader {
                    id: favoritesScreenLoader
                    anchors.fill: parent
                    active: root.favoritesScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenFavorites
                    sourceComponent: Component {
                        FavoritesScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            optimisticLoading: root.activeScreen === root.screenFavorites && root.catalogStillBooting
                            selectedSystemId: root.favoritesSystemId
                        }
                    }
                }

                Loader {
                    id: favoriteSystemsScreenLoader
                    anchors.fill: parent
                    active: root.favoriteSystemsScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenFavoriteSystems
                    sourceComponent: Component {
                        FavoriteSystemsScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            optimisticLoading: root.activeScreen === root.screenFavoriteSystems && root.catalogStillBooting
                        }
                    }
                }

                Loader {
                    id: recentsScreenLoader
                    anchors.fill: parent
                    active: root.recentsScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenRecents
                    sourceComponent: Component {
                        RecentsScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            optimisticLoading: root.activeScreen === root.screenRecents && root.catalogStillBooting
                        }
                    }
                }

                Loader {
                    id: updateScreenLoader
                    anchors.fill: parent
                    active: root.updateEnabled && root.activeScreen === root.screenUpdate
                    visible: status === Loader.Ready && root.activeScreen === root.screenUpdate
                    source: active ? "qrc:/qt/qml/Zaparoo/App/UpdateEntry.qml" : ""
                }

                Binding {
                    target: updateScreenLoader.item
                    property: "transitioning"
                    value: root.pendingTransition !== ""
                    when: updateScreenLoader.item !== null
                }

                Loader {
                    id: settingsScreenLoader
                    anchors.fill: parent
                    active: root.settingsScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenSettings
                    sourceComponent: Component {
                        SettingsScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                            optimisticLoading: root.activeScreen === root.screenSettings && root.catalogStillBooting
                        }
                    }
                }

                Loader {
                    id: aboutScreenLoader
                    anchors.fill: parent
                    active: root.aboutScreenRequested
                    visible: status === Loader.Ready && root.activeScreen === root.screenAbout
                    sourceComponent: Component {
                        AboutScreen {
                            anchors.fill: parent
                            transitioning: root.transitionCueVisible
                        }
                    }
                }
            }

            // ── Boot overlay ─────────────────────────────────────────────────────────
            // Mounted until Core/catalog boot reaches READY, except for the
            // optimistic Hub path. Non-Hub restores must not flash placeholder Hub
            // tiles or blank deferred icons underneath.
            Loader {
                anchors.fill: parent
                active: !root.bootComplete && !root.optimisticHubVisible && !root.coreIndependentStartupVisible
                z: 50
                sourceComponent: BootOverlay {}
            }

            // ── Card writer modal ────────────────────────────────────────────────────

            Loader {
                id: cardWriteModalLoader
                anchors.fill: parent
                // Round 10: every modal Loader here needs an explicit z
                // above HeaderBar's 200 -- `z` only resolves among
                // siblings, and each modal's own root Item declares a
                // z (300, or 310 for commercialNotice) that only applies
                // INSIDE its own Loader, never reaching `scene`'s actual
                // stacking context where it sits alongside HeaderBar.
                // Without this, a modal tall enough to reach into the
                // header band (GameInfoModal was the only one that did)
                // paints BEHIND the logo instead of in front of it.
                z: 300
                active: root.cardWriteModalRequested
                sourceComponent: Component {
                    Modal {
                        open: root.cardWriteModalVisible
                        kind: "transient"
                        failed: root.cardWriteFailed
                        title: root.cardWriteFailed ? qsTr("Writing failed") : qsTr("Hold a writable token near the reader")
                        onCancelRequested: root.cancelCardWriteRequested()
                    }
                }
            }

            // ── Setting restart prompt modal ────────────────────────────────────────────────────

            Loader {
                id: settingNeedsRestartModalLoader
                anchors.fill: parent
                z: 300
                active: root.settingNeedsRestartModalRequested
                sourceComponent: Component {
                    Modal {
                        open: root.settingNeedsRestartModalVisible
                        kind: "confirm"
                        title: qsTr("Quit and restart Zaparoo Frontend?")
                        body: qsTr("In order to apply this setting we need to restart the frontend.")
                        onConfirmed: root.acceptRestart()
                        onCancelRequested: root.cancelRestart()
                    }
                }
            }

            // Core version warning. Pushed by Main.qml on startup when the
            // connected Core is older than the frontend's minimum. Warn-only:
            // a single OK button dismisses it, nothing is locked out.
            Loader {
                id: coreVersionModalLoader
                anchors.fill: parent
                z: 300
                active: root.coreVersionModalRequested
                sourceComponent: Component {
                    Modal {
                        open: root.coreVersionModalVisible
                        kind: "action_error"
                        title: qsTr("Update Zaparoo Core")
                        body: qsTr("This frontend needs Zaparoo Core %1 or newer. You're running %2. Some features may not work until you update.").arg(Browse.AppStatus.min_core_version).arg(Browse.AppStatus.core_version)
                        buttonLabel: qsTr("OK")
                        onAccepted: root.closeCoreVersionRequested()
                    }
                }
            }

            Loader {
                id: actionErrorModalLoader
                anchors.fill: parent
                z: 300
                active: root.actionErrorModalRequested
                sourceComponent: Component {
                    Modal {
                        open: root.actionErrorModalVisible
                        kind: "action_error"
                        title: root.actionErrorTitle
                        body: root.actionErrorBody
                        buttonLabel: root.actionErrorButtonLabel
                        onAccepted: root.actionErrorAccepted()
                    }
                }
            }

            Loader {
                id: contextMenuLoader
                anchors.fill: parent
                z: 300
                active: root.contextMenuRequested
                sourceComponent: Component {
                    ContextMenu {
                        open: root.contextMenuVisible
                        anchorRect: root.contextMenuAnchor
                        anchorRadius: root.contextMenuAnchorRadius
                        entries: root.contextMenuEntries
                        bottomUnsafeHeight: BrowseLayouts.numberValue(root._browseViewProfile, "footer.bottomUnsafeHeight", Sizing.pctH(6) + Sizing.pctH(2))
                        onAccepted: id => root.contextMenuAccepted(id)
                        onCloseRequested: root.contextMenuCloseRequested()
                    }
                }
            }

            Loader {
                id: qrCodeModalLoader
                anchors.fill: parent
                z: 300
                active: root.qrCodeModalRequested
                sourceComponent: Component {
                    QrCodeModal {
                        anchors.fill: parent
                        open: root.qrCodeModalVisible
                        title: root.qrCodeModalTitle
                        instructionText: root.qrCodeModalInstruction
                        urlText: root.qrCodeModalUrlText
                    }
                }
            }

            Loader {
                id: gameInfoModalLoader
                anchors.fill: parent
                z: 300
                active: root.gameInfoModalRequested
                sourceComponent: Component {
                    GameInfoModal {
                        anchors.fill: parent
                        open: root.gameInfoModalVisible
                        onCloseRequested: root.closeGameInfoRequested()
                    }
                }
            }

            // Commercial-use notice. Sits above every other modal (z: 310) so
            // it always paints first on a fresh install. Once the user acks,
            // `Browse.Notice.commercial_ack` flips to true on disk and the
            // modal stays closed for the rest of this install. The Loader
            // itself (not just CommercialNoticeModal's own internal z)
            // needs to carry this -- see cardWriteModalLoader's comment.
            Loader {
                id: commercialNoticeModalLoader
                anchors.fill: parent
                z: 310
                active: root.commercialNoticeModalRequested
                sourceComponent: Component {
                    CommercialNoticeModal {
                        anchors.fill: parent
                        open: root.commercialNoticeModalVisible
                        onCloseRequested: root.closeCommercialNoticeRequested()
                    }
                }
            }

            // Log-upload modal. Pushed by Main.qml when the user triggers the
            // "Upload log" action in Settings. Owns its own three-phase view
            // (uploading / success / error) — the router only sees open / close.
            Loader {
                id: logUploadModalLoader
                anchors.fill: parent
                z: 300
                active: root.logUploadModalRequested
                sourceComponent: Component {
                    LogUploadModal {
                        anchors.fill: parent
                        open: root.logUploadModalVisible
                        onCloseRequested: root.closeLogUploadRequested()
                    }
                }
            }

            // Scrape setup modal (round 10). Pushed by Main.qml when the
            // user triggers the "Scrape metadata" action in Settings while
            // idle. Scraper choice + re-scrape toggle + Start, replacing
            // the hardcoded "gamelist.xml" every other scrape call site
            // still uses.
            Loader {
                id: scrapeSetupModalLoader
                anchors.fill: parent
                z: 300
                active: root.scrapeSetupModalRequested
                sourceComponent: Component {
                    ScrapeSetupModal {
                        anchors.fill: parent
                        open: root.scrapeSetupModalVisible
                        onCloseRequested: root.closeScrapeSetupRequested()
                        onRequestScraperPicker: root.requestScraperPicker()
                        onRequestSystemScopePicker: root.requestSystemScopePicker()
                    }
                }
            }

            // Update-media-database setup modal (round 11) — same shell as
            // ScrapeSetupModal, trimmed to Systems + Start. Pushed by
            // Main.qml when the user triggers "Update media database" in
            // Settings while idle.
            Loader {
                id: indexSetupModalLoader
                anchors.fill: parent
                z: 300
                active: root.indexSetupModalRequested
                sourceComponent: Component {
                    IndexSetupModal {
                        anchors.fill: parent
                        open: root.indexSetupModalVisible
                        onCloseRequested: root.closeIndexSetupRequested()
                        onRequestSystemScopePicker: root.requestSystemScopePicker()
                    }
                }
            }

            // Quit-confirm modal. Pushed by Main.qml when the user presses
            // cancel on Hub. Default focus is "No" so an accidental press
            // can't quit; "Yes" routes through `quitConfirmAccepted` and the
            // router calls Qt.quit().
            Loader {
                id: quitConfirmModalLoader
                anchors.fill: parent
                z: 300
                active: root.quitConfirmModalRequested
                sourceComponent: Component {
                    Modal {
                        open: root.quitConfirmModalVisible
                        kind: "confirm"
                        title: qsTr("Quit Zaparoo Frontend?")
                        body: qsTr("Are you sure you want to exit?")
                        onConfirmed: root.quitConfirmAccepted()
                        onCancelRequested: root.closeQuitConfirmRequested()
                    }
                }
            }

            // List-picker modal. Settings opens this for picker rows
            // (Language, Browsing layout, Button style, and others). The
            // fieldId round-trip lets the router dispatch the chosen id
            // back to the matching Browse.Settings.set_X without parsing
            // the title.
            Loader {
                id: listPickerModalLoader
                anchors.fill: parent
                z: 300
                active: root.listPickerModalRequested
                sourceComponent: Component {
                    ListPickerModal {
                        anchors.fill: parent
                        open: root.listPickerModalVisible
                        title: root.listPickerTitle
                        entries: root.listPickerEntries
                        initialId: root.listPickerInitialId
                        // A launcher save in flight locks the picker on
                        // its "Saving…" row -- see ListPickerModal's own
                        // `locked` doc comment.
                        locked: root.listPickerFieldId === "system_launcher_pending" || root.listPickerFieldId === "game_launcher_pending"
                        onAccepted: id => root.listPickerAccepted(root.listPickerFieldId, id)
                        onCloseRequested: root.listPickerCloseRequested(root.listPickerFieldId)
                    }
                }
            }

            // Jump-to-letter grid (West button → "Jump to letter"). Entries are
            // bound live so the grid populates when the facet lands.
            Loader {
                id: letterJumpModalLoader
                anchors.fill: parent
                z: 300
                active: root.letterJumpModalRequested
                sourceComponent: Component {
                    LetterJumpModal {
                        anchors.fill: parent
                        open: root.letterJumpModalVisible
                        entries: root.letterJumpEntries
                        loading: root.letterJumpLoading
                        onAccepted: offset => root.letterJumpAccepted(offset)
                        onCloseRequested: root.letterJumpCloseRequested()
                    }
                }
            }

            // Modal scrim backstop for the CRT overscan band. Modal
            // scrims fill `scene`, which is inset by the safe area, so
            // the full-bleed background would otherwise glow undimmed
            // around every open modal. Four edge strips extend the same
            // Theme.scrim into the band - strips rather than one big
            // rectangle because a full overlay would double-dim the
            // scene area on top of the modal's own scrim. Strip
            // thickness follows the framebuffer axis each scene edge
            // maps to (the swap mirrors the rotation).
            Item {
                id: crtScrimBackstop

                readonly property int bandX: (root._sceneRotated ? root._crtInsetH : root._crtInsetW) / 2
                readonly property int bandY: (root._sceneRotated ? root._crtInsetW : root._crtInsetH) / 2

                anchors.fill: parent
                visible: root.crtNativePath && ScreenManager.hasModal
                z: 350

                Rectangle {
                    x: -crtScrimBackstop.bandX
                    y: -crtScrimBackstop.bandY
                    width: crtScrimBackstop.width + 2 * crtScrimBackstop.bandX
                    height: crtScrimBackstop.bandY
                    color: Theme.scrim
                }
                Rectangle {
                    x: -crtScrimBackstop.bandX
                    y: crtScrimBackstop.height
                    width: crtScrimBackstop.width + 2 * crtScrimBackstop.bandX
                    height: crtScrimBackstop.bandY
                    color: Theme.scrim
                }
                Rectangle {
                    x: -crtScrimBackstop.bandX
                    y: 0
                    width: crtScrimBackstop.bandX
                    height: crtScrimBackstop.height
                    color: Theme.scrim
                }
                Rectangle {
                    x: crtScrimBackstop.width
                    y: 0
                    width: crtScrimBackstop.bandX
                    height: crtScrimBackstop.height
                    color: Theme.scrim
                }
            }

            // ── Instructions bar ──────────────────────────────────────────────────────

            Rectangle {
                id: instructionsBar

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                height: Sizing.pctH(6)
                // Sits above every modal scrim (modals max out at z: 310 — see
                // CommercialNoticeModal) so the help cue stays readable while a
                // dialog is open. The bar's content is already modal-aware
                // (helpEntries above branches per topModal), so the cue under
                // the modal is the right one.
                z: 400
                // No fill or border of its own — a bordered box reads as a
                // floating bar. The two children below paint a background and
                // a single top keyline instead, both full-bleed past the CRT
                // safe-area inset (the same trick the scene background uses
                // at L582-588) so on the CRT path the fill reaches the true
                // framebuffer edge and covers the whole bottom overscan band,
                // with the keyline landing exactly on the safe-area line.
                color: "transparent"

                Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: -Math.max(root._crtInsetW, root._crtInsetH)
                    anchors.rightMargin: -Math.max(root._crtInsetW, root._crtInsetH)
                    anchors.bottomMargin: -Math.max(root._crtInsetW, root._crtInsetH)
                    color: Theme.bgBar
                }

                Rectangle {
                    anchors.left: parent.left
                    anchors.leftMargin: -Math.max(root._crtInsetW, root._crtInsetH)
                    anchors.right: parent.right
                    anchors.rightMargin: -Math.max(root._crtInsetW, root._crtInsetH)
                    anchors.top: parent.top
                    height: Sizing.stroke(1)
                    color: Theme.borderSubtle
                }

                // (activeScreen, screenState, modal?)-keyed lookup. The modal
                // row wins outright; otherwise per-screen entries vary with
                // the screen's data-state (Loading / Error / Empty / Ready).
                // Error and Empty share the retry-or-back row on Systems and
                // Games (both wire `accept` to re-fire `set_category` /
                // `set_system` in non-Ready state). Hub has no retry handler
                // — CategoriesModel binds eagerly via bind_to_endpoint! and
                // recovers automatically — so its non-Ready row drops the
                // Retry entry rather than promising behavior the screen
                // doesn't implement.
                //
                // During a forward transition the router's input gate still
                // swallows presses, but the source help row stays frozen until
                // the destination cut. Removing it early made the otherwise
                // static source screen look as though it was dismantling while
                // work continued. Modals still win outright.
                //
                // Each entry resolves to a button glyph (Dpad / ButtonA /
                // ButtonB / ButtonX) plus a label. The button names are routed
                // through Resources.iconUrl(), which owns the qrc path rules.
                //
                // Label vocabulary is deliberately minimal: D-pad is always
                // "Move"; A is "Open" for both drill-downs and launches (the
                // tile and screen title carry the specific identity, so the
                // verb doesn't need to repeat that); B is "Back" on every
                // screen with one to go back to. The Hub root has none — B
                // is unbound there, and Quit lives in the View menu instead
                // (see HubScreen.qml's routing comment). Sentence case
                // throughout.
                readonly property var helpEntries: {
                    if (root.contextMenuVisible)
                        return [
                            {
                                button: "Dpad",
                                label: qsTr("Move")
                            },
                            {
                                button: "ButtonA",
                                label: qsTr("Select")
                            },
                            {
                                buttons: ["ButtonB", "ButtonX"],
                                label: qsTr("Close")
                            }
                        ];
                    if (root.cardWriteModalVisible)
                        return [
                            {
                                button: "ButtonB",
                                label: qsTr("Cancel")
                            }
                        ];
                    if (root.qrCodeModalVisible || root.gameInfoModalVisible)
                        return [
                            {
                                button: "ButtonB",
                                label: qsTr("Close")
                            }
                        ];
                    if (root.logUploadModalVisible) {
                        const phase = root.logUploadModal ? root.logUploadModal.phase : "";
                        const success = root.logUploadModal ? root.logUploadModal._stateSuccess : "__none__";
                        const error = root.logUploadModal ? root.logUploadModal._stateError : "__none__";
                        if (phase === success)
                            return [
                                {
                                    button: "ButtonA",
                                    label: qsTr("Done")
                                },
                                {
                                    button: "ButtonB",
                                    label: qsTr("Close")
                                }
                            ];
                        if (phase === error)
                            return [
                                {
                                    button: "ButtonA",
                                    label: qsTr("Retry")
                                },
                                {
                                    button: "ButtonB",
                                    label: qsTr("Close")
                                }
                            ];
                        // Idle / uploading: only Cancel.
                        return [
                            {
                                button: "ButtonB",
                                label: qsTr("Cancel")
                            }
                        ];
                    }
                    if (root.commercialNoticeModalVisible)
                        return [
                            {
                                button: "ButtonA",
                                label: qsTr("I understand")
                            }
                        ];
                    if (root.actionErrorModalVisible)
                        return [
                            {
                                button: "ButtonA",
                                label: root.actionErrorButtonLabel
                            },
                            {
                                button: "ButtonB",
                                label: qsTr("Close")
                            }
                        ];
                    if (root.coreVersionModalVisible || root.randomFailedModalVisible)
                        return [
                            {
                                button: "ButtonA",
                                label: qsTr("OK")
                            }
                        ];
                    // A launcher save in flight locks the list picker on its
                    // "Saving…" row (Main.qml's modalListPicker routing
                    // branch swallows every action while this is true) --
                    // Move/Select/Cancel would be false advertising, so swap
                    // them for the same status text the row itself shows.
                    if (root.listPickerModalVisible && (root.listPickerFieldId === "system_launcher_pending" || root.listPickerFieldId === "game_launcher_pending"))
                        return [
                            {
                                label: qsTr("Saving…")
                            }
                        ];
                    if (root.quitConfirmModalVisible || root.settingNeedsRestartModalVisible || root.listPickerModalVisible || root.letterJumpModalVisible)
                        return [
                            {
                                button: "Dpad",
                                label: qsTr("Move")
                            },
                            {
                                button: "ButtonA",
                                label: qsTr("Select")
                            },
                            {
                                button: "ButtonB",
                                label: qsTr("Cancel")
                            }
                        ];
                    // Below the list-picker branch above deliberately: both
                    // setup modals stay mounted while their nested picker
                    // is open, so the picker's own row has to win first.
                    // Without this branch the bar fell through to the
                    // Settings screen underneath and advertised its rows
                    // while a modal owned input.
                    if (root.indexSetupModalVisible || root.scrapeSetupModalVisible) {
                        const setupModal = root.indexSetupModalVisible ? root.indexSetupModal : root.scrapeSetupModal;
                        return [
                            {
                                button: "Dpad",
                                label: qsTr("Move")
                            },
                            {
                                button: "ButtonA",
                                label: setupModal !== null ? setupModal.focusedActionLabel : qsTr("Select")
                            },
                            {
                                button: "ButtonB",
                                label: qsTr("Back")
                            }
                        ];
                    }
                    if (root.crtCalibrationModalVisible)
                        return [
                            {
                                button: "Dpad",
                                label: qsTr("Adjust")
                            },
                            {
                                button: "ButtonA",
                                label: qsTr("Save")
                            }
                        ];
                    // The Hub's optimistic pre-connect paint (see
                    // MainLayout's `optimisticHubVisible`) renders real
                    // tiles with a fully-computed help bar below — Options/
                    // View availability doesn't depend on Core being
                    // connected, so falling through to the Hub branch below
                    // one frame earlier than `bootComplete` is exposing an
                    // existing computation, not inventing new logic. Without
                    // this carve-out the optimistic Hub painted with zero
                    // button hints, which was the biggest "looks unfinished"
                    // cue on cold start.
                    if ((!root.bootComplete && !root.coreIndependentStartupVisible && !root.optimisticHubVisible) || root.startupRestoreCurtainVisible)
                        return [];
                    if (root.activeScreen === root.screenHub) {
                        // Mid-reorder (Options -> Move armed): D-pad
                        // repositions the held tile, Accept places it,
                        // Cancel reverts — nothing else is live (see
                        // HubScreen.qml's `_handleMoveAction`).
                        if (root.hubScreen !== null && root.hubScreen.moveArmed)
                            return [
                                {
                                    button: "Dpad",
                                    label: qsTr("Reposition")
                                },
                                {
                                    button: "ButtonA",
                                    label: qsTr("Place")
                                },
                                {
                                    button: "ButtonB",
                                    label: qsTr("Cancel")
                                }
                            ];
                        // Hub always has the actions row (Recently Played /
                        // Settings), so Move/Open applies even when the
                        // categories row is empty (0 systems indexed) — the
                        // help bar must reflect that the actions row is
                        // navigable, otherwise the user reads "nothing to
                        // move to" and misses the Settings tile entirely.
                        // Options
                        // (Move/Hide-or-Delete, plus category- or
                        // Favorites-specific entries) is available on any
                        // tile with a real `Browse.HubLayout` backing —
                        // placeholders and any `kind === "empty"` entry
                        // (a real persisted blank, or the trailing empty
                        // slot — see HubScreen.qml's `_blankEntry`) do not.
                        // A blank is an implementation detail, not
                        // something the user picks up and moves on its own.
                        const hubEntry = root.hubScreen !== null ? root.hubScreen.items[root.hubScreen.currentIndex] : null;
                        const onCategoryTile = hubEntry != null && hubEntry.kind === "category";
                        const categoryErrorFocused = onCategoryTile && (Browse.CategoriesModel.error_message ?? "") !== "";
                        const hubOptionsAvailable = hubEntry != null && hubEntry.kind !== "empty" && !categoryErrorFocused && hubEntry.hubIndex >= 0;
                        // D-pad moves; L/R shoulders page-jump, shown only
                        // when there's a second page to jump to — same
                        // "Move" fold Systems already uses.
                        const hubPages = root.hubScreen !== null ? root.hubScreen.pageCount : 1;
                        let row = [
                            {
                                buttons: hubPages > 1 ? ["ButtonL", "ButtonR", "Dpad"] : ["Dpad"],
                                label: qsTr("Move")
                            },
                            {
                                button: "ButtonA",
                                label: categoryErrorFocused ? qsTr("Retry") : qsTr("Open")
                            }
                        ];
                        if (hubOptionsAvailable)
                            row.push({
                                button: "ButtonX",
                                label: qsTr("Options")
                            });
                        row.push({
                            button: "ButtonY",
                            label: qsTr("View")
                        });
                        return row;
                    }
                    if (root.activeScreen === root.screenSystems) {
                        if (root.systemsScreenState === "loading")
                            return [
                                {
                                    button: "ButtonB",
                                    label: qsTr("Back")
                                }
                            ];
                        if (root.systemsScreenState === "ready") {
                            if (root.systemsScreen === null)
                                return [];
                            // D-pad moves; L/R shoulders page-jump. Folded into
                            // one "Move" cue, with the shoulder glyphs shown only
                            // when there's a second page to jump to so we don't
                            // promise a press that no-ops on a single page.
                            const pages = root.systemsScreen.systemsGrid.pageCount;
                            let row = [
                                {
                                    buttons: pages > 1 ? ["ButtonL", "ButtonR", "Dpad"] : ["Dpad"],
                                    label: qsTr("Move")
                                }
                            ];
                            row.push({
                                button: "ButtonA",
                                label: qsTr("Open")
                            }, {
                                button: "ButtonX",
                                label: qsTr("Options")
                            }, {
                                button: "ButtonB",
                                label: qsTr("Back")
                            });
                            return row;
                        }
                        return [
                            {
                                button: "ButtonA",
                                label: qsTr("Retry")
                            },
                            {
                                button: "ButtonB",
                                label: qsTr("Back")
                            }
                        ];
                    }
                    if (root.activeScreen === root.screenFavorites || root.activeScreen === root.screenFavoriteSystems || root.activeScreen === root.screenRecents) {
                        const isFavorites = root.activeScreen === root.screenFavorites;
                        const isFavoriteSystems = root.activeScreen === root.screenFavoriteSystems;
                        const state = isFavorites ? root.favoritesScreenState : (isFavoriteSystems ? root.favoriteSystemsScreenState : root.recentsScreenState);
                        const screen = isFavorites ? root.favoritesScreen : (isFavoriteSystems ? root.favoriteSystemsScreen : root.recentsScreen);
                        if (screen === null)
                            return [];
                        const grid = isFavorites ? screen.favoritesGrid : (isFavoriteSystems ? screen.favoriteSystemsGrid : screen.recentsGrid);
                        if (state === "loading")
                            return [
                                {
                                    button: "ButtonB",
                                    label: qsTr("Back")
                                }
                            ];
                        if (state === "ready") {
                            const pages = grid.pageCount;
                            // D-pad moves; L/R shoulders page-jump. Folded into
                            // one "Move" cue; shoulder glyphs appear only with a
                            // second page.
                            let row = [
                                {
                                    buttons: pages > 1 ? ["ButtonL", "ButtonR", "Dpad"] : ["Dpad"],
                                    label: qsTr("Move")
                                }
                            ];
                            row.push({
                                button: "ButtonA",
                                label: qsTr("Open")
                            });
                            if (isFavorites || isFavoriteSystems)
                                row.push({
                                    button: "ButtonX",
                                    label: qsTr("Options")
                                });
                            if (isFavorites || isFavoriteSystems)
                                row.push({
                                    button: "ButtonY",
                                    label: qsTr("View")
                                });
                            row.push({
                                button: "ButtonB",
                                label: qsTr("Back")
                            });
                            return row;
                        }
                        // Empty/error.
                        const fallback = [
                            {
                                button: "ButtonA",
                                label: qsTr("Retry")
                            }
                        ];
                        if (isFavorites || isFavoriteSystems)
                            fallback.push({
                                button: "ButtonY",
                                label: qsTr("View")
                            });
                        fallback.push({
                            button: "ButtonB",
                            label: qsTr("Back")
                        });
                        return fallback;
                    }
                    if (root.activeScreen === root.screenSettings) {
                        if (root.settingsScreen === null)
                            return [];
                        if (root.settingsScreen.showingRootGrid) {
                            if (root.settingsScreen.optimisticLoading)
                                return [
                                    {
                                        button: "ButtonB",
                                        label: qsTr("Back")
                                    }
                                ];
                            let gridRow = [];
                            if (root.settingsScreen.fieldCount > 1)
                                gridRow.push({
                                    button: "Dpad",
                                    label: qsTr("Move")
                                });
                            if (root.settingsScreen.fieldCount > 0)
                                gridRow.push({
                                    button: "ButtonA",
                                    label: qsTr("Open")
                                });
                            gridRow.push({
                                button: "ButtonB",
                                label: qsTr("Back")
                            });
                            return gridRow;
                        }
                        let row = [];
                        // Up/Down moves between fields; only useful when there
                        // are 2+ fields.
                        if (root.settingsScreen.fieldCount > 1) {
                            row.push({
                                buttons: ["DpadUp", "DpadDown"],
                                label: qsTr("Move")
                            });
                        }
                        // Left/Right cycles the focused field's value. Skip
                        // the cue when the focused field is an action row
                        // (no left/right binding) or there are no fields.
                        if (root.settingsScreen.fieldCount > 0 && !root.settingsScreen.focusedFieldIsAction) {
                            row.push({
                                buttons: ["DpadLeft", "DpadRight"],
                                label: qsTr("Change")
                            });
                        }
                        if (root.settingsScreen.focusedFieldIsToggle)
                            row.push({
                                button: "ButtonA",
                                label: qsTr("Toggle")
                            });
                        else if (root.settingsScreen.focusedFieldIsAction && !root.settingsScreen.focusedActionDisabled)
                            row.push({
                                button: "ButtonA",
                                label: root.settingsScreen.focusedActionLabel
                            });
                        row.push({
                            button: "ButtonB",
                            label: qsTr("Back")
                        });
                        return row;
                    }
                    if (root.activeScreen === root.screenUpdate) {
                        return root.updateScreen !== null ? root.updateScreen.helpEntries : [];
                    }
                    if (root.activeScreen === root.screenAbout) {
                        if (root.aboutScreen === null)
                            return [];
                        let row = [];
                        // Up/Down only meaningful when the body actually
                        // overflows the viewport (per the minimal help-bar
                        // policy — never advertise a press that no-ops).
                        if (root.aboutScreen.contentOverflows)
                            row.push({
                                buttons: ["DpadUp", "DpadDown"],
                                label: qsTr("Scroll")
                            });
                        row.push({
                            button: "ButtonB",
                            label: qsTr("Back")
                        });
                        return row;
                    }
                    // games
                    if (root.gamesScreenState === "loading")
                        return [
                            {
                                button: "ButtonB",
                                label: qsTr("Back")
                            }
                        ];
                    if (root.gamesScreenState === "ready") {
                        if (root.gamesScreen === null)
                            return [];
                        const pages = root.gamesScreen.gamesGrid.pageCount;
                        // Mirror the real context-menu gate used by
                        // GamesScreen's `contextMenuEnabledAt`. Singleton
                        // folders with media identity can launch and be
                        // favorited; a plain directory and a filesystem `root`
                        // both get the folder shortcut action. All of them
                        // open a menu, so all of them must advertise Options
                        // -- this used to test media capability alone, which
                        // left every folder row with a working Options button
                        // and no cue saying so. Keep the two in step.
                        const idx = root.gamesScreen.gamesGrid.currentIndex;
                        const hasContextMenu = Browse.GamesModel.is_media_capable_at(idx) || Browse.GamesModel.entry_type_at(idx) === "directory" || Browse.GamesModel.is_filesystem_root_at(idx);
                        // D-pad moves; L/R shoulders page-jump. Folded into one
                        // "Move" cue; shoulder glyphs appear only with a second
                        // page.
                        let row = [
                            {
                                buttons: pages > 1 ? ["ButtonL", "ButtonR", "Dpad"] : ["Dpad"],
                                label: qsTr("Move")
                            }
                        ];
                        row.push({
                            button: "ButtonA",
                            label: qsTr("Open")
                        });
                        if (hasContextMenu)
                            row.push({
                                button: "ButtonX",
                                label: qsTr("Options")
                            });
                        // West (Y) opens the list-scoped "View" menu (go to
                        // letter, and later sort/filter/layout). Mirrors North's
                        // item-scoped Options; the menu stays page/list-scoped.
                        row.push({
                            button: "ButtonY",
                            label: qsTr("View")
                        });
                        row.push({
                            button: "ButtonB",
                            label: qsTr("Back")
                        });
                        return row;
                    }
                    const fallback = [
                        {
                            button: "ButtonA",
                            label: qsTr("Retry")
                        }
                    ];
                    // Empty favorites-only scope must keep View reachable so
                    // user can clear filter. Errors retain Retry/Back only.
                    if (root.gamesScreenState === "empty")
                        fallback.push({
                            button: "ButtonY",
                            label: qsTr("View")
                        });
                    fallback.push({
                        button: "ButtonB",
                        label: qsTr("Back")
                    });
                    return fallback;
                }

                Row {
                    x: Sizing.center(parent.width, width)
                    // A small downward bias off dead-center — arithmetic
                    // centering here has no dependency on the bar's own
                    // border (it never had a bottom border of its own; see
                    // the two full-bleed fill/keyline Rectangles above),
                    // this is purely a feel adjustment. Trimmed 0.4 -> 0.3
                    // (round 6, item 5), then 0.3 -> 0.2 (round 6 follow-up)
                    // — still read a pixel too far down at 1080p/720p.
                    y: Sizing.center(parent.height, height) + Sizing.pctH(0.2)
                    spacing: Sizing.pctW(2)

                    Repeater {
                        model: instructionsBar.helpEntries

                        // Each entry is either a single-glyph cue
                        // (`{ button: "ButtonA", label: "Open" }`) or a
                        // multi-glyph cue rendered as N icons in a row before
                        // the label (`{ buttons: ["DpadLeft", "DpadRight"],
                        // label: "Change" }`). The Settings screen uses the
                        // multi-glyph form to disambiguate "left/right cycles
                        // the value" from "up/down moves between fields".
                        delegate: Row {
                            id: helpEntry
                            required property var modelData
                            spacing: Sizing.pctW(0.6)

                            // A status-only entry (e.g. "Saving…" while a
                            // pending save locks the list picker, below)
                            // supplies neither `button` nor `buttons` --
                            // renders as a bare label with no icon.
                            readonly property var buttonList: helpEntry.modelData.buttons !== undefined ? helpEntry.modelData.buttons : (helpEntry.modelData.button !== undefined ? [helpEntry.modelData.button] : [])

                            Repeater {
                                model: helpEntry.buttonList
                                delegate: Image {
                                    required property string modelData
                                    anchors.verticalCenter: helpEntry.verticalCenter
                                    height: Sizing.pctH(4)
                                    width: height
                                    fillMode: Image.PreserveAspectFit
                                    sourceSize.height: Sizing.px(height)
                                    sourceSize.width: Sizing.px(width)
                                    source: Resources.iconUrl(modelData, Theme.textPrimary)
                                    smooth: true
                                }
                            }

                            Text {
                                anchors.verticalCenter: helpEntry.verticalCenter
                                text: helpEntry.modelData.label
                                font.family: Theme.fontUi
                                font.pixelSize: Sizing.fontBody
                                color: Theme.textPrimary
                                renderType: Text.NativeRendering
                            }
                        }
                    }
                }
            }

            MouseArea {
                anchors.fill: parent
                z: 10000
                visible: !Browse.Settings.current_mouse_enabled
                enabled: visible
                hoverEnabled: true
                acceptedButtons: Qt.AllButtons
                cursorShape: Qt.BlankCursor
            }

            // Screen-burn protection. Sits inside `scene` so the bake-time
            // grab captures the same logical dimensions Sizing reads from
            // (CRT preview included). Z is above modals (300) and the help
            // bar (400) so the screensaver covers every chrome layer; the
            // mouse-blanking MouseArea above (z: 10000) still wins when
            // mouse input is disabled, which keeps the cursor hidden.
            ScreensaverOverlay {
                id: screensaverOverlay

                anchors.fill: parent
                // Cover the full framebuffer, not just the safe-area-
                // inset scene - the overscan band's background must not
                // sit static on a CRT while the screensaver runs.
                anchors.margins: -Math.max(root._crtInsetW, root._crtInsetH)
                z: 500
            }
        }

        Item {
            id: debugCrtSafeAreaGuide

            objectName: "debugCrtSafeAreaGuide"
            visible: root._debugCrtSafeAreaGuideVisible
            anchors.fill: parent
            z: 20000

            readonly property int insetX: Sizing.px(parent.width * 0.05)
            readonly property int insetY: Sizing.px(parent.height * 0.05)
            readonly property int deepInsetX: Sizing.px(parent.width * 0.10)
            readonly property int deepInsetY: Sizing.px(parent.height * 0.10)
            readonly property int line: Sizing.stroke(1)
            readonly property color guideColor: "#ff4fd8"
            readonly property color deepGuideColor: "#31d7ff"

            Rectangle {
                objectName: "debugCrtActionSafeRect"
                x: debugCrtSafeAreaGuide.insetX
                y: debugCrtSafeAreaGuide.insetY
                width: Math.max(1, Sizing.px(parent.width - 2 * debugCrtSafeAreaGuide.insetX))
                height: Math.max(1, Sizing.px(parent.height - 2 * debugCrtSafeAreaGuide.insetY))
                color: "transparent"
                border.color: debugCrtSafeAreaGuide.guideColor
                border.width: debugCrtSafeAreaGuide.line
            }

            Rectangle {
                objectName: "debugCrtTitleSafeRect"
                x: debugCrtSafeAreaGuide.deepInsetX
                y: debugCrtSafeAreaGuide.deepInsetY
                width: Math.max(1, Sizing.px(parent.width - 2 * debugCrtSafeAreaGuide.deepInsetX))
                height: Math.max(1, Sizing.px(parent.height - 2 * debugCrtSafeAreaGuide.deepInsetY))
                color: "transparent"
                border.color: debugCrtSafeAreaGuide.deepGuideColor
                border.width: debugCrtSafeAreaGuide.line
            }
        }

        // CRT screen-position calibration. Mounted as a sibling of
        // `scene` (painted after it, so on top of everything inside)
        // because the test pattern must address the TRUE framebuffer
        // edges - it opts out of both the safe-area inset and the
        // orientation rotation, which a border/grid pattern doesn't
        // need. Input still routes through Main.qml's modal chain via
        // ScreenManager.
        Loader {
            id: crtCalibrationModalLoader
            anchors.fill: parent
            active: root.crtCalibrationModalRequested
            sourceComponent: Component {
                CrtCalibrationModal {
                    anchors.fill: parent
                    open: root.crtCalibrationModalVisible
                    onCloseRequested: root.closeCrtCalibrationRequested()
                }
            }
        }
    }
}
