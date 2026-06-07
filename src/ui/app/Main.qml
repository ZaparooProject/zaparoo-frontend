// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtQuick.Window
import Zaparoo.Theme
import Zaparoo.Screens
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton still trips qmllint's "Member can
// be shadowed" check. Until the schema grows method-level finality,
// suppress the compiler category file-wide.
// qmllint disable compiler

// Runtime wrapper around MainLayout. The visual tree lives in
// MainLayout.qml (editable by designers in Qt Design Studio) and the
// individual screens in Zaparoo.Screens; this file is a thin router
// that translates raw Qt key events into actions, dispatches them to
// the active screen (or topmost modal), and persists user-visible
// navigation state across kills.
MainLayout {
    id: root

    // Fullscreen builds (MiSTer) fill the screen; desktop windowed
    // builds inherit MainLayout's 1280x720 design defaults so the user
    // can resize freely. MainLayout binds width/height to Screen.* when
    // fullScreen is true so the first paint is at the correct dims;
    // doing it imperatively here would land after the first frame and
    // leave a 1280x720 slice on screen for one frame.

    readonly property string modalCardWrite: "card_write"
    readonly property string modalContextMenu: "context_menu"
    readonly property string modalGameInfo: "game_info"
    readonly property string modalQrCode: "qr_code"
    readonly property string modalCommercialNotice: "commercial_notice"
    readonly property string modalFirstRunIndex: "first_run_index"
    readonly property string modalLogUpload: "log_upload"
    readonly property string modalQuitConfirm: "quit_confirm"
    readonly property string modalListPicker: "list_picker"
    readonly property string modalSettingNeedsRestart: "restart_confirm"

    // One-shot session flag: the first-run modal is shown at most
    // once per frontend process, even if the WS link drops and the
    // mediadb-empty condition would otherwise be satisfied again.
    property bool _firstRunIndexShown: false
    property string _pendingLanguageSelection: ""
    property string _pendingResolutionSelection: ""
    property bool _discoverMenuPending: false
    property bool _pendingResumeLaunch: false
    property bool _startupRestorePending: false
    property bool _startupRestoreStarted: false
    property string _startupRestoreScreen: ""
    property var _screenReadyCallbacks: ({})
    property var _discoverParentEntries: []
    property string _pendingLauncherSystemId: ""
    property string _pendingLauncherSelectionId: ""
    property string cardWriteOwner: ""
    property string contextMenuMode: "main"
    property string contextMenuOwner: ""
    property int contextMenuIndex: -1
    readonly property bool activeCardWritePending: root.cardWriteOwner === "systems" ? Browse.SystemsModel.card_write_pending : root.cardWriteOwner === "games" ? Browse.GamesModel.card_write_pending : root.cardWriteOwner === "favorites" ? Browse.FavoritesModel.card_write_pending : false
    readonly property string activeCardWriteError: root.cardWriteOwner === "systems" ? Browse.SystemsModel.card_write_error : root.cardWriteOwner === "games" ? Browse.GamesModel.card_write_error : root.cardWriteOwner === "favorites" ? Browse.FavoritesModel.card_write_error : ""

    // Bound here (not in GamesScreen.qml) because `set_system` can fire
    // from the accept handler before the games screen mounts; binding
    // inside the screen fires only on `Component.onCompleted`, after the
    // first fetch has already gone out with the model's default
    // page_size. That made the first cursor page misaligned with the
    // visual grid pageSize and produced half-loaded pages on every
    // subsequent cursor advance.
    readonly property int _gamesListFetchSize: 30
    readonly property var _gamesGridShape: Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight)
    readonly property int _gamesGridColumns: root._gamesGridShape.columns
    readonly property int _gamesGridRows: root._gamesGridShape.rows
    readonly property int _gamesPageSize: Browse.Settings.current_browse_layout === "list" ? root._gamesListFetchSize : root._gamesGridColumns * root._gamesGridRows
    on_GamesPageSizeChanged: Browse.GamesModel.page_size = root._gamesPageSize

    // Bind Sizing to the scene's logical dimensions, not the
    // ApplicationWindow's. Outside CRT preview the scene fills the
    // window so the values are identical to the prior imperative
    // writes; in preview the scene is fixed at videoWidth x
    // videoHeight and the bindings keep Sizing reading logical
    // pixels for pctW/pctH/px/etc.
    Binding {
        target: Sizing
        property: "screenWidth"
        value: root.scene.width
    }
    Binding {
        target: Sizing
        property: "screenHeight"
        value: root.scene.height
    }

    function _requestScreen(screen: string): void {
        if (screen === root.screenSystems)
            root.systemsScreenRequested = true;
        else if (screen === root.screenGames)
            root.gamesScreenRequested = true;
        else if (screen === root.screenFavorites)
            root.favoritesScreenRequested = true;
        else if (screen === root.screenRecents)
            root.recentsScreenRequested = true;
        else if (screen === root.screenSettings)
            root.settingsScreenRequested = true;
        else if (screen === root.screenAbout)
            root.aboutScreenRequested = true;
    }

    function _primeStartupRestoreScreen(screen: string): void {
        if (screen === root.screenSystems) {
            root._requestScreen(root.screenSystems);
            return;
        }
        if (screen === root.screenGames) {
            root._requestScreen(root.screenSystems);
            root._requestScreen(root.screenGames);
            return;
        }
        root._requestScreen(screen);
    }

    function _screenItem(screen: string): var {
        if (screen === root.screenSystems)
            return root.systemsScreen;
        if (screen === root.screenGames)
            return root.gamesScreen;
        if (screen === root.screenFavorites)
            return root.favoritesScreen;
        if (screen === root.screenRecents)
            return root.recentsScreen;
        if (screen === root.screenSettings)
            return root.settingsScreen;
        if (screen === root.screenAbout)
            return root.aboutScreen;
        return root.hubScreen;
    }

    function _whenScreenReady(screen: string, callback): void {
        root._requestScreen(screen);
        const item = root._screenItem(screen);
        if (item !== null && item !== undefined) {
            callback(item);
            return;
        }
        const pending = root._screenReadyCallbacks[screen] || [];
        pending.push(callback);
        root._screenReadyCallbacks[screen] = pending;
    }

    function _flushScreenReady(screen: string): void {
        const item = root._screenItem(screen);
        if (item === null || item === undefined)
            return;
        const pending = root._screenReadyCallbacks[screen] || [];
        if (pending.length === 0)
            return;
        delete root._screenReadyCallbacks[screen];
        for (let i = 0; i < pending.length; i++)
            pending[i](item);
    }

    function _requestModal(modal: string): void {
        if (modal === root.modalCardWrite)
            root.cardWriteModalRequested = true;
        else if (modal === root.modalContextMenu)
            root.contextMenuRequested = true;
        else if (modal === root.modalGameInfo)
            root.gameInfoModalRequested = true;
        else if (modal === root.modalQrCode)
            root.qrCodeModalRequested = true;
        else if (modal === root.modalCommercialNotice)
            root.commercialNoticeModalRequested = true;
        else if (modal === root.modalFirstRunIndex)
            root.firstRunIndexModalRequested = true;
        else if (modal === root.modalLogUpload)
            root.logUploadModalRequested = true;
        else if (modal === root.modalQuitConfirm)
            root.quitConfirmModalRequested = true;
        else if (modal === root.modalListPicker)
            root.listPickerModalRequested = true;
        else if (modal === root.modalSettingNeedsRestart)
            root.settingNeedsRestartModalRequested = true;
    }

    Component.onCompleted: {
        // Desktop CRT preview applies one initial integer scale here,
        // then MainLayout snaps later user resizes to the supported
        // 3x..5x steps. Fullscreen embedded sizing is handled by
        // MainLayout's width/height bindings so first paint matches
        // the FB layout.
        if (!root.fullScreen && root._crtPreviewActive) {
            root.applyCrtPreviewScale(root._crtPreviewInitialScale);
        }
        Browse.GamesModel.page_size = root._gamesPageSize;
        const savedScreen = root._validStartupScreen(Browse.AppState.active_screen);
        root.startupRestoreCurtainVisible = savedScreen !== "" && savedScreen !== root.screenHub;
        if (root.startupRestoreCurtainVisible) {
            root._startupRestorePending = true;
            root._startupRestoreScreen = savedScreen;
            root._primeStartupRestoreScreen(savedScreen);
            root.activeScreen = savedScreen;
            startupRestoreKickTimer.restart();
        } else {
            root.activeScreen = root.screenHub;
        }
        root._startupTrace("startup/qml Component.onCompleted", "savedScreen=" + savedScreen, "initialActiveScreen=" + root.activeScreen, "startupRestorePending=" + root._startupRestorePending, "connectionState=" + Browse.AppStatus.connection_state);
        Browse.FavoritesModel.cover_requests_paused = root.activeScreen !== root.screenFavorites;
        Browse.RecentsModel.cover_requests_paused = root.activeScreen !== root.screenRecents;
        // If the catalog is already ready, fire the restore here so
        // the cascade (set_category → SystemsModel reset → seed
        // currentIndex → set_system → GamesModel reset) lands before
        // first paint. Otherwise the CategoriesModel.onModelReset
        // Connection below fires it on first delivery.
        if (Browse.CategoriesModel.count > 0)
            root.hubScreen.restoreFromCategoriesReset();
        root._maybeArmHubResumeFocus();
        // Open the commercial-use notice on first paint of an unacked
        // install. Sits in front of the media-DB first-run modal in the
        // routing order — `_maybeOpenFirstRunIndex` early-returns until
        // `Browse.Notice.commercial_ack` flips true, at which point the
        // notice's close handler retriggers the media-DB check.
        root._maybeOpenCommercialNotice();
        // Kick the first-run check in case both READY and a seeded
        // empty-mediadb snapshot landed before our Connections wired up
        // (e.g. an unusually fast warm-cache reconnect).
        root._maybeCompleteBoot();
        root._maybeOpenFirstRunIndex();
        root._maybeStartStartupRestore();
    }

    function _validStartupScreen(screen: string): string {
        if (screen === root.screenHub || screen === root.screenSystems || screen === root.screenGames || screen === root.screenFavorites || screen === root.screenRecents || screen === root.screenSettings || screen === root.screenAbout)
            return screen;
        return "";
    }

    // Seed row/grid indices from persisted state when models deliver new
    // data. A miss (category renamed, ROM deleted) falls back to index 0
    // and leaves the saved identifier untouched on disk — so the user's
    // intent survives a transient catalog gap. State writes only happen
    // in each screen's handleAction (user navigation); these programmatic
    // seeds are inert.
    //
    // Always cascade into set_category (even on a miss or first-launch empty
    // HubState.category): SystemsModel is the only way to drive the next
    // onModelReset handler, and a games-screen restore depends on that chain
    // firing so GamesModel.set_system runs.
    Connections {
        target: Browse.CategoriesModel
        function onModelReset(): void {
            root.hubScreen.restoreFromCategoriesReset();
            root._maybeStartStartupRestore();
            root._maybeContinueOptimisticTransitions();
        }
        function onLoadedChanged(): void {
            root._maybeContinueOptimisticTransitions();
        }
        function onError_messageChanged(): void {
            root._maybeContinueOptimisticTransitions();
        }
    }
    Connections {
        target: Browse.SystemsModel
        // On a games-screen restore, GamesState.system_id is authoritative;
        // fall back to SystemsState.system_id only if it's empty (edge case:
        // user pressed Enter on an empty systems grid and we flipped the
        // screen without ever committing a system). On a hub or systems
        // restore, SystemsState.system_id is authoritative — don't peek at
        // GamesState, or we'd override the user's position with a stale
        // games target from a prior escape-back-up-the-stack.
        function onModelReset(): void {
            if (root.systemsScreen === null) {
                root._whenScreenReady(root.screenSystems, function () {
                    root._restoreSystemsScreenSelection();
                });
                return;
            }
            root._restoreSystemsScreenSelection();
        }
    }
    Connections {
        target: Browse.GamesModel
        function onModelReset(): void {
            if (root.gamesScreen === null) {
                root._whenScreenReady(root.screenGames, function () {
                    root._restoreGamesScreenSelection();
                });
                return;
            }
            root._restoreGamesScreenSelection();
        }
        // Pages 2+ append rows via begin_insert_rows / end_insert_rows
        // (no model reset), so we can't piggy-back on onModelReset to
        // retry the lookup. `count` bumps on every append, giving us a
        // stable per-page edge to resume the deep-page restore on.
        function onCountChanged(): void {
            if (root.gamesScreen === null) {
                root._whenScreenReady(root.screenGames, function () {
                    if (root._pendingGameRestorePath !== "")
                        root._restoreGamesScreenSelection();
                });
                return;
            }
            const path = root._pendingGameRestorePath;
            if (path === "")
                return;
            // User backed out to Hub/Systems before pagination caught
            // up — selected_at_level isn't touched by a peer-screen
            // exit, so without this gate the loop would keep hammering
            // fetch_more in the background until the folder exhausts.
            if (root.activeScreen !== root.screenGames && !(root._startupRestorePending && root._startupRestoreScreen === root.screenGames)) {
                root._pendingGameRestorePath = "";
                return;
            }
            // User input updates `selected_at_level` on every move,
            // so a divergence between the pending path and the top
            // of stack means the user navigated during the restore
            // — drop the auto-restore and let them stay where they
            // landed.
            const sels = Browse.GamesState.selected_at_level;
            const currentTop = sels.length > 0 ? sels[sels.length - 1] : "";
            if (currentTop !== path) {
                root._pendingGameRestorePath = "";
                root._maybeFinishStartupGamesRestore();
                return;
            }
            const idx = Browse.GamesModel.index_for_game_path(path);
            if (idx >= 0) {
                root._setGamesRestoreIndex(idx);
                root._pendingGameRestorePath = "";
                root._maybeFinishStartupGamesRestore();
                return;
            }
            if (Browse.GamesModel.has_next_page) {
                // fetch_more is itself debounced by `loading_more` and
                // `has_next_page`, so a redundant call here is a cheap
                // no-op rather than a duplicate request.
                Browse.GamesModel.fetch_more();
                return;
            }
            root._pendingGameRestorePath = "";
            root._maybeFinishStartupGamesRestore();
        }
    }

    // Cross-screen transitions: each screen signals its intent and this
    // router writes persistence + flips ScreenManager. Keeps the screens
    // themselves ignorant of AppState so they can be reused in test
    // harnesses that don't wire the full persistence layer.
    //
    // The runtime + persistence writes always go together — naming the
    // pair as a single helper makes the invariant explicit and keeps
    // the four request handlers below a single line each.
    function _goto(screen: string): void {
        root._requestScreen(screen);
        root._startupTrace("startup/qml goto", "from=" + root.activeScreen, "to=" + screen, "pendingTransition=" + root.pendingTransition);
        ScreenManager.activeScreen = screen;
        Browse.AppState.active_screen = screen;
    }

    // Single-shot callback slots fired by the loadingChanged
    // listeners below. Only one transition is in flight at a time
    // (input gate guarantees this), so two scalars are enough.
    // `pendingTransition` itself lives in MainLayout — the source
    // screen's content-hiding bindings (row/grid `visible`) resolve
    // there, so the property has to be declared at that level for
    // the QML lint pass to be happy.
    property var _categoryReadyCallback: null
    property var _systemReadyCallback: null
    property var _favoritesReadyCallback: null
    property var _recentsReadyCallback: null
    property string _catalogWaitCategory: ""
    // Set when `_ensureCategory` arms `deferredCategorySetTimer` and
    // cleared inside the timer's `onTriggered` after `set_category`
    // actually fires. Gates `_categoryReadyCallback` consumption so a
    // stale `SystemsModel.loading` false-edge from an unrelated in-flight
    // fill (e.g. `restoreFromCategoriesReset` already running) can't
    // complete the transition before our own `set_category` has been
    // issued.
    property bool _deferredCategoryPending: false
    // Saved games-screen entry path that wasn't on the freshly seeded
    // page 1 of MediaBrowse. The GamesModel.onCountChanged watcher
    // below paginates forward via fetch_more until the path is found
    // or `has_next_page` goes false. Cleared on resolution and on
    // any navigation that starts a new browse target so a stale
    // restore can't keep paginating after the user moves on.
    property string _pendingGameRestorePath: ""

    function _catalogStillBooting(): bool {
        return !Browse.CategoriesModel.loaded && (Browse.CategoriesModel.error_message ?? "") === "";
    }

    function _completeDeferredCategoryIfReady(targetCategory: string): bool {
        if (root._categoryReadyCallback === null)
            return false;
        if (Browse.SystemsModel.loading)
            return false;
        if (Browse.SystemsModel.current_category !== targetCategory)
            return false;
        if (root._catalogStillBooting())
            return false;
        root._startupTrace("startup/qml deferred category ready", "category=" + targetCategory + " count=" + Browse.SystemsModel.count);
        const cb = root._categoryReadyCallback;
        root._categoryReadyCallback = null;
        cb();
        return true;
    }

    function _maybeContinueOptimisticTransitions(): void {
        if (root._catalogStillBooting())
            return;
        if (root._catalogWaitCategory !== "" && root._categoryReadyCallback !== null) {
            const category = root._catalogWaitCategory;
            const cb = root._categoryReadyCallback;
            root._catalogWaitCategory = "";
            root._startupTrace("startup/qml catalog wait continue", "category=" + category);
            root._ensureCategory(category, cb, false);
        }
        if (root.pendingTransition === "favorites")
            favoritesTransitionTimer.restart();
        else if (root.pendingTransition === "recents")
            recentsTransitionTimer.restart();
        else if (root.pendingTransition === "settings")
            root._whenScreenReady(root.screenSettings, function () {
                if (root.pendingTransition === "settings")
                    root._completeTransition(root.screenSettings);
            });
    }

    // Listen for SystemsModel fills owned by an in-flight transition.
    // `loading` flips true at the start of set_category and false when
    // the async tokio worker posts the filled rows back. Listening to
    // the false edge gives us a single, unambiguous "fill complete"
    // signal — onModelReset would also fire on the synchronous clear
    // (count=0) at the start of set_category, indistinguishable from a
    // category that legitimately fills with 0 rows. The callback slot
    // is consumed at most once per transition; a stray fire when no
    // transition is pending is a no-op.
    Connections {
        target: Browse.SystemsModel
        function onLoadingChanged(): void {
            if (Browse.SystemsModel.loading)
                return;
            // Deferred set_category hasn't fired yet — this false-edge
            // belongs to a prior in-flight fill, not our transition.
            if (root._deferredCategoryPending) {
                root._startupTrace("startup/qml category loading edge ignored", "reason=deferred-pending category=" + Browse.SystemsModel.current_category + " count=" + Browse.SystemsModel.count);
                return;
            }
            // Optimistic Hub can issue set_category before the catalog
            // exists. That worker legitimately resolves empty; keep the
            // normal loading cue up until CategoriesModel delivers an
            // authoritative loaded/error edge, then retry the category.
            if (root._catalogWaitCategory !== "" && root._catalogStillBooting())
                return;
            const cb = root._categoryReadyCallback;
            if (cb === null)
                return;
            root._categoryReadyCallback = null;
            cb();
        }
    }
    Connections {
        target: Browse.GamesModel
        function onLoadingChanged(): void {
            if (Browse.GamesModel.loading)
                return;
            const cb = root._systemReadyCallback;
            if (cb === null)
                return;
            root._systemReadyCallback = null;
            cb();
        }
    }
    Connections {
        target: Browse.RecentsModel
        function onLoadingChanged(): void {
            if (Browse.RecentsModel.loading)
                return;
            root._maybeCompletePendingResumeLaunch();
            const cb = root._recentsReadyCallback;
            if (cb === null)
                return;
            root._recentsReadyCallback = null;
            cb();
        }

        function onResume_availableChanged(): void {
            root._maybeCompletePendingResumeLaunch();
        }
    }
    Connections {
        target: Browse.FavoritesModel
        function onLoadingChanged(): void {
            if (Browse.FavoritesModel.loading)
                return;
            const cb = root._favoritesReadyCallback;
            if (cb === null)
                return;
            root._favoritesReadyCallback = null;
            cb();
        }
    }

    // Ensure SystemsModel is filled with `category`, then call cb().
    // Synchronous on the no-op return path (same category already
    // populated — a re-Accept after Esc-back); the set_category call
    // is still made for parity with the prior behaviour even though
    // Rust early-returns when the category already matches. Async
    // path waits for loadingChanged; the 50 ms defer hides
    // set_category's synchronous teardown of SystemsScreen's bound
    // tile delegates behind the transition overlay, so the user sees
    // overlay → frozen-under-overlay → grid instead of freeze →
    // flash → grid. Qt.callLater is not enough; it fires inside the
    // same event loop iteration before the next render polish/sync
    // pass.
    function _ensureCategory(category: string, cb, waitForCatalog): void {
        if (waitForCatalog && root._catalogStillBooting()) {
            root._startupTrace("startup/qml catalog wait arm", "category=" + category);
            root._categoryReadyCallback = cb;
            root._catalogWaitCategory = category;
            return;
        }
        if (Browse.SystemsModel.current_category === category && Browse.SystemsModel.count > 0) {
            root._categoryReadyCallback = null;
            Browse.SystemsModel.set_category(category);
            cb();
            return;
        }
        root._startupTrace("startup/qml defer category set", "category=" + category);
        root._categoryReadyCallback = cb;
        root._deferredCategoryPending = true;
        deferredCategorySetTimer.targetCategory = category;
        deferredCategorySetTimer.restart();
    }

    // Ensure GamesModel is filled with `systemId`, then call cb().
    // When the system is already current and populated (re-Accept
    // after Esc-back), set_system still re-issues start_initial_browse,
    // but the cached result applies inline through the watcher's seed
    // — loading flips back to false before set_system returns — so we
    // can call cb() synchronously on this path. Cold-load goes through
    // the systemReadyCallback waiter below.
    function _ensureSystem(systemId: string, cb): void {
        if (Browse.GamesModel.current_system_id === systemId && Browse.GamesModel.count > 0) {
            Browse.GamesModel.set_system(systemId);
            cb();
            return;
        }
        root._systemReadyCallback = cb;
        Browse.GamesModel.set_system(systemId);
    }

    // Hub Accept routing. Empty-row passthrough preserves the committed
    // "Enter on empty hub goes to Systems" behaviour and
    // keeps the navigation test synchronous. The Resume action is a
    // hub payload rather than a category and launches the latest
    // resumable history row. Otherwise: tentatively pin the
    // destination to Systems, fill the chosen category, then either
    // bypass to Games (MiSTer Arcade singleton) or fall through to
    // Systems with a cover-prefetch warmup so the destination paints
    // with logos already in QPixmapCache.
    function _navigateFromHub(category: string): void {
        if (category === "") {
            root._goto(root.screenSystems);
            return;
        }
        if (category === "resume") {
            root._navigateResumeFromHub();
            return;
        }
        Browse.HubState.category = category;
        root._requestScreen(root.screenSystems);
        root.pendingTransition = "systems";
        root._ensureCategory(category, function () {
            const arcadeBypass = Browse.Platform.is_mister && Browse.Platform.ready && category === CategoryIds.arcadeId && Browse.SystemsModel.count === 1;
            console.log("arcade-bypass eval:", "category=" + JSON.stringify(category), "platform.is_mister=" + Browse.Platform.is_mister, "platform.ready=" + Browse.Platform.ready, "systems.count=" + Browse.SystemsModel.count, "→ bypass=" + arcadeBypass);
            if (arcadeBypass) {
                root._requestScreen(root.screenGames);
                const systemId = Browse.SystemsModel.system_id_at(0);
                Browse.SystemsState.system_id = systemId;
                Browse.GamesState.system_id = systemId;
                root.pendingTransition = "games";
                root._ensureSystem(systemId, function () {
                    root._completeTransition(root.screenGames);
                });
            } else {
                root._prefetchSystemCovers(function () {
                    root._completeTransition(root.screenSystems);
                });
            }
        }, true);
    }

    function _cancelResumeLaunch(): void {
        root._pendingResumeLaunch = false;
        if (root.pendingTransition === "resume")
            root.pendingTransition = "";
        if (root.activeScreen !== root.screenHub)
            root._goto(root.screenHub);
    }

    function _maybeCompletePendingResumeLaunch(): void {
        if (!root._pendingResumeLaunch || root.pendingTransition !== "resume")
            return;
        if (Browse.RecentsModel.resume_loading)
            return;
        if (Browse.RecentsModel.resume_available) {
            root._pendingResumeLaunch = false;
            Browse.RecentsModel.launch_resume();
            return;
        }
        if (Browse.AppStatus.connection_state === 2 || Browse.AppStatus.connection_state === 3)
            root._cancelResumeLaunch();
    }

    function _startResumeLaunch(): void {
        if (root.pendingTransition !== "resume")
            return;
        root._pendingResumeLaunch = true;
        root._maybeCompletePendingResumeLaunch();
    }

    function _navigateResumeFromHub(): void {
        if (!Browse.RecentsModel.resume_loading && Browse.RecentsModel.resume_available) {
            Browse.RecentsModel.launch_resume();
            return;
        }
        if (Browse.RecentsModel.resume_loading || Browse.AppStatus.connection_state !== 2) {
            root.pendingTransition = "resume";
            resumeLaunchTimer.restart();
            return;
        }
        root._cancelResumeLaunch();
    }

    function _completeFavoritesTransition(): void {
        if (root.pendingTransition !== "favorites")
            return;
        root.favoritesScreen.restoreSelection();
        root._completeTransition(root.screenFavorites);
    }

    function _startFavoritesTransitionLoad(): void {
        if (root.pendingTransition !== "favorites")
            return;
        root._whenScreenReady(root.screenFavorites, function () {
            if (root.pendingTransition !== "favorites")
                return;
            root._resumeFavoritesCovers();
            if (root._catalogStillBooting())
                return;
            if (!Browse.FavoritesModel.loading) {
                root._completeFavoritesTransition();
                return;
            }
            root._favoritesReadyCallback = root._completeFavoritesTransition;
        });
    }

    function _navigateToFavorites(): void {
        root.pendingTransition = "favorites";
        favoritesTransitionTimer.restart();
    }

    function _completeRecentsTransition(): void {
        if (root.pendingTransition !== "recents")
            return;
        root.recentsScreen.restoreSelection();
        root._completeTransition(root.screenRecents);
    }

    // Hub → Recents transition. The paginated history load is lazy so
    // Hub Resume does not pay for `media.history` during startup. Start
    // it only once the Recents screen is actually requested, then wait
    // on `loadingChanged` so the user sees the centred "Loading…" cue
    // rather than an empty grid.
    function _startRecentsTransitionLoad(): void {
        if (root.pendingTransition !== "recents")
            return;
        root._whenScreenReady(root.screenRecents, function () {
            if (root.pendingTransition !== "recents")
                return;
            Browse.RecentsModel.ensure_loaded();
            root._resumeRecentsCovers();
            if (root._catalogStillBooting())
                return;
            if (!Browse.RecentsModel.loading) {
                root._completeRecentsTransition();
                return;
            }
            root._recentsReadyCallback = root._completeRecentsTransition;
        });
    }

    function _navigateToRecents(): void {
        root.pendingTransition = "recents";
        recentsTransitionTimer.restart();
    }

    function _resumeFavoritesCovers(): void {
        Browse.FavoritesModel.cover_requests_paused = false;
        if (root.favoritesScreen === null)
            return;
        const first = root.favoritesScreen.mediaGrid.currentPage * root.favoritesScreen.mediaGrid.pageSize;
        Browse.FavoritesModel.refresh_cover_keys(first, root.favoritesScreen.mediaGrid.pageSize * 2);
    }

    function _resumeRecentsCovers(): void {
        Browse.RecentsModel.cover_requests_paused = false;
        if (root.recentsScreen === null)
            return;
        const first = root.recentsScreen.mediaGrid.currentPage * root.recentsScreen.mediaGrid.pageSize;
        Browse.RecentsModel.refresh_cover_keys(first, root.recentsScreen.mediaGrid.pageSize * 2);
    }

    // Hub → Settings transition. During optimistic boot, keep the same
    // centered Loading cue as other Hub actions until the catalog has
    // reached an authoritative state; after that Settings can flip
    // instantly because its singleton seeds from persisted state.
    function _navigateToSettings(): void {
        root._requestScreen(root.screenSettings);
        if (root._catalogStillBooting()) {
            root.pendingTransition = "settings";
            return;
        }
        root._whenScreenReady(root.screenSettings, function () {
            root._goto(root.screenSettings);
        });
    }

    // Settings → About transition. Static info screen, no async data,
    // so the flip is instant — same shape as _navigateToSettings above.
    function _navigateToAbout(): void {
        root._whenScreenReady(root.screenAbout, function () {
            root._goto(root.screenAbout);
        });
    }

    function _restoreSystemsScreenSelection(): void {
        const savedSystem = root.activeScreen === root.screenGames ? (Browse.GamesState.system_id !== "" ? Browse.GamesState.system_id : Browse.SystemsState.system_id) : Browse.SystemsState.system_id;
        const idx = savedSystem === "" ? -1 : Browse.SystemsModel.index_for_system_id(savedSystem);
        root.systemsScreen.systemsGrid.setCurrentIndexImmediate(idx >= 0 ? idx : 0);
        if (idx >= 0) {
            Browse.GamesModel.set_system(savedSystem);
            const stack = Browse.GamesState.path_stack;
            const top = stack.length > 0 ? stack[stack.length - 1] : "";
            if (top !== "")
                Browse.GamesModel.set_path(top);
        } else if (root.activeScreen === root.screenGames && Browse.SystemsModel.count > 0) {
            Browse.GamesModel.set_system(Browse.SystemsModel.system_id_at(0));
        }
    }

    function _setGamesRestoreIndex(index: int): void {
        if (root.gamesScreen === null)
            return;
        root.gamesScreen.suppressSelectionPersist = true;
        root.gamesScreen.gamesGrid.setCurrentIndexImmediate(index);
        root.gamesScreen.suppressSelectionPersist = false;
    }

    function _restoreGamesScreenSelection(): bool {
        const sels = Browse.GamesState.selected_at_level;
        const savedPath = sels.length > 0 ? sels[sels.length - 1] : "";
        const idx = savedPath === "" ? -1 : Browse.GamesModel.index_for_game_path(savedPath);
        if (idx >= 0) {
            root._setGamesRestoreIndex(idx);
            root._pendingGameRestorePath = "";
            return true;
        }
        if (savedPath !== "" && Browse.GamesModel.has_next_page) {
            root._pendingGameRestorePath = savedPath;
            root._setGamesRestoreIndex(0);
            Browse.GamesModel.fetch_more();
            return false;
        }
        root._pendingGameRestorePath = "";
        root._setGamesRestoreIndex(0);
        return true;
    }

    function _maybeFinishStartupGamesRestore(): void {
        if (!root._startupRestorePending || root._startupRestoreScreen !== root.screenGames)
            return;
        if (root._pendingGameRestorePath !== "")
            return;
        root._finishStartupRestore();
        root._goto(root.screenGames);
    }

    // Systems Accept routing. Pin destination to Games, fill the
    // chosen system, then flip. The Games→back routing decision is
    // re-evaluated live from current state at B-press time (see
    // gamesScreen.onRequestSystemsScreen below) so this path needs
    // no per-transition flag.
    function _navigateFromSystems(systemId: string): void {
        root._requestScreen(root.screenGames);
        Browse.SystemsState.system_id = systemId;
        // Setting system_id on GamesState resets path_stack/selected_at_level
        // to root level — the new system's browse always starts at the
        // initial games-screen view, regardless of where the user was in
        // a prior system's folder tree.
        Browse.GamesState.system_id = systemId;
        root.pendingTransition = "games";
        root._ensureSystem(systemId, function () {
            root._completeTransition(root.screenGames);
        });
    }

    // Folder drill-down inside the games screen. Stays on screenGames
    // — no pendingTransition flip — so the in-screen ScreenStateOverlay
    // handles the loading/empty/error cue while the new browse settles.
    // Pushes the new level onto GamesState before issuing the browse so
    // a kill mid-load still resumes inside the folder.
    function _navigateIntoFolder(path: string): void {
        if (path === "")
            return;
        Browse.GamesState.push_level(path, "");
        Browse.GamesModel.set_path(path);
    }

    // Folder pop-up inside the games screen. Pops the deepest level off
    // the stack, then drives the model back to the parent path. If we
    // pop to the root level (path_stack[0] is always "") the call goes
    // through `set_system` so the model re-runs the
    // single-root-auto-nav decision rather than browsing the literal
    // empty path with no system filter.
    function _navigateOutOfFolder(): void {
        const stack = Browse.GamesState.path_stack;
        if (stack.length <= 1)
            return;
        Browse.GamesState.pop_level();
        const newStack = Browse.GamesState.path_stack;
        const target = newStack[newStack.length - 1];
        if (target === "") {
            const sid = Browse.GamesState.system_id;
            if (sid !== "")
                Browse.GamesModel.set_system(sid);
        } else {
            Browse.GamesModel.set_path(target);
        }
    }

    // Clear the pending flag, then flip. Order matters: clearing
    // first lets the destination screen paint without the overlay
    // still drawing over it, and lets bindings dependent on
    // pendingTransition (source screen visibility) settle to the
    // post-transition state in the same frame as the screen swap.
    function _completeTransition(screen: string): void {
        root._startupTrace("startup/qml completeTransition", "to=" + screen, "from=" + root.activeScreen);
        root.pendingTransition = "";
        root._goto(screen);
        // Restart the idle countdown so the screensaver gate (which
        // skips activation while a transition is in flight) does not
        // leave the timer dead after the gate opens. No-op when the
        // screensaver setting is "off".
        root._resetIdle();
    }

    function _finishStartupRestore(): void {
        root._startupTrace("startup/qml finishStartupRestore", "target=" + root._startupRestoreScreen, "activeScreen=" + root.activeScreen);
        startupRestoreKickTimer.stop();
        root._startupRestorePending = false;
        root._startupRestoreStarted = false;
        root._startupRestoreScreen = "";
        root.startupRestoreCurtainVisible = false;
        root._maybeArmHubResumeFocus();
    }

    function _maybeArmHubResumeFocus(): void {
        if (root.activeScreen !== root.screenHub || root._startupRestorePending)
            return;
        root.hubScreen.focusResumeIfVisible();
    }

    function _maybeStartStartupRestore(): void {
        if (!root._startupRestorePending || root._startupRestoreStarted)
            return;
        if (startupRestoreKickTimer.running)
            return;
        const targetScreen = root._startupRestoreScreen;
        if (targetScreen !== root.screenSettings && targetScreen !== root.screenAbout && Browse.AppStatus.connection_state !== 2)
            return;
        root._startupTrace("startup/qml maybeStartStartupRestore", "target=" + targetScreen, "categories=" + Browse.CategoriesModel.count, "systems=" + Browse.SystemsModel.count, "recentsLoading=" + Browse.RecentsModel.loading, "favoritesLoading=" + Browse.FavoritesModel.loading);
        if (targetScreen === "") {
            root._finishStartupRestore();
            return;
        }
        root._startupRestoreStarted = true;
        if (targetScreen === root.screenSettings || targetScreen === root.screenAbout) {
            root._whenScreenReady(targetScreen, function () {
                root._finishStartupRestore();
                root._goto(targetScreen);
            });
            return;
        }
        if (targetScreen === root.screenFavorites) {
            root._whenScreenReady(root.screenFavorites, function () {
                if (Browse.FavoritesModel.loading) {
                    root._favoritesReadyCallback = function () {
                        root.favoritesScreen.restoreSelection();
                        root._finishStartupRestore();
                        root._goto(root.screenFavorites);
                    };
                } else {
                    root.favoritesScreen.restoreSelection();
                    root._finishStartupRestore();
                    root._goto(root.screenFavorites);
                }
            });
            return;
        }
        if (targetScreen === root.screenRecents) {
            root._whenScreenReady(root.screenRecents, function () {
                if (Browse.RecentsModel.loading) {
                    root._recentsReadyCallback = function () {
                        root.recentsScreen.restoreSelection();
                        root._finishStartupRestore();
                        root._goto(root.screenRecents);
                    };
                } else {
                    root.recentsScreen.restoreSelection();
                    root._finishStartupRestore();
                    root._goto(root.screenRecents);
                }
            });
            return;
        }
        if (Browse.CategoriesModel.count <= 0) {
            const catalogError = Browse.CategoriesModel.error_message ?? "";
            if (!Browse.CategoriesModel.loaded && catalogError === "") {
                root._startupRestoreStarted = false;
                root._startupTrace("startup/qml startupRestore waitingForCatalog");
                return;
            }
            root._startupTrace("startup/qml startupRestore emptyCatalog", "loaded=" + Browse.CategoriesModel.loaded, "error=" + catalogError);
            root._finishStartupRestore();
            root._goto(targetScreen);
            return;
        }
        if (targetScreen === root.screenHub) {
            root.hubScreen.restoreFromCategoriesReset();
            root._finishStartupRestore();
            root._goto(root.screenHub);
            return;
        }
        const category = Browse.HubState.category;
        if (category === "") {
            root._startupTrace("startup/qml startupRestore missingCategory");
            root._finishStartupRestore();
            return;
        }
        if (targetScreen === root.screenSystems || targetScreen === root.screenGames)
            root._requestScreen(root.screenSystems);
        if (targetScreen === root.screenGames)
            root._requestScreen(root.screenGames);
        root._ensureCategory(category, function () {
            const arcadeBypass = Browse.Platform.is_mister && Browse.Platform.ready && category === CategoryIds.arcadeId && Browse.SystemsModel.count === 1;
            const arcadeSystemId = arcadeBypass ? Browse.SystemsModel.system_id_at(0) : "";
            root._startupTrace("startup/qml startupRestore categoryReady", "category=" + category, "target=" + targetScreen, "arcadeBypass=" + arcadeBypass, "systemsCount=" + Browse.SystemsModel.count);
            if (targetScreen === root.screenSystems) {
                if (arcadeBypass) {
                    Browse.SystemsState.system_id = arcadeSystemId;
                    Browse.GamesState.system_id = arcadeSystemId;
                    root._startupRestoreScreen = root.screenGames;
                    root.activeScreen = root.screenGames;
                    root._ensureSystem(arcadeSystemId, function () {
                        root._whenScreenReady(root.screenGames, function () {
                            if (root._restoreGamesScreenSelection())
                                root._maybeFinishStartupGamesRestore();
                        });
                    });
                    return;
                }
                root._whenScreenReady(root.screenSystems, function () {
                    root._restoreSystemsScreenSelection();
                    root._finishStartupRestore();
                    root._goto(root.screenSystems);
                });
                return;
            }
            const systemId = Browse.GamesState.system_id !== "" ? Browse.GamesState.system_id : (Browse.SystemsState.system_id !== "" ? Browse.SystemsState.system_id : arcadeSystemId);
            if (systemId === "") {
                root._startupTrace("startup/qml startupRestore missingSystemId", "category=" + category, "target=" + targetScreen);
                root._finishStartupRestore();
                return;
            }
            root._whenScreenReady(root.screenSystems, function () {
                root._restoreSystemsScreenSelection();
                root._systemReadyCallback = function () {
                    root._startupTrace("startup/qml startupRestore systemReady", "systemId=" + Browse.GamesModel.current_system_id, "target=" + targetScreen);
                    root._whenScreenReady(root.screenGames, function () {
                        if (root._restoreGamesScreenSelection())
                            root._maybeFinishStartupGamesRestore();
                    });
                };
                if (!Browse.GamesModel.loading) {
                    const cb = root._systemReadyCallback;
                    root._systemReadyCallback = null;
                    cb();
                }
            });
        });
    }

    Timer {
        id: startupRestoreKickTimer
        interval: 120
        repeat: false
        onTriggered: root._maybeStartStartupRestore()
    }

    onSystemsScreenChanged: root._flushScreenReady(root.screenSystems)
    onGamesScreenChanged: root._flushScreenReady(root.screenGames)
    onFavoritesScreenChanged: root._flushScreenReady(root.screenFavorites)
    onRecentsScreenChanged: root._flushScreenReady(root.screenRecents)
    onSettingsScreenChanged: root._flushScreenReady(root.screenSettings)
    onAboutScreenChanged: root._flushScreenReady(root.screenAbout)

    Connections {
        target: root.hubScreen
        function onRequestAccept(category: string): void {
            root._navigateFromHub(category);
        }
        function onRequestQuit(): void {
            root.openQuitConfirmModal();
        }
        function onRequestFavoritesScreen(): void {
            root._navigateToFavorites();
        }
        function onRequestRecentsScreen(): void {
            root._navigateToRecents();
        }
        function onRequestSettingsScreen(): void {
            root._navigateToSettings();
        }
    }
    Connections {
        target: root.favoritesScreen
        function onRequestHubScreen(): void {
            root._goto(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect): void {
            root.openContextMenu("favorites", index, anchorRect);
        }
    }
    Connections {
        target: root.recentsScreen
        function onRequestHubScreen(): void {
            root._goto(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect): void {
            root.openContextMenu("recents", index, anchorRect);
        }
    }
    Connections {
        target: root.settingsScreen
        function onRequestHubScreen(): void {
            root._goto(root.screenHub);
        }
        function onRequestAccept(actionId: string): void {
            if (actionId === "uploadLog")
                root.openLogUploadModal();
            else if (actionId === "aboutLicense")
                root._navigateToAbout();
        }
        function onRequestListPicker(title: string, entries: var, initialId: string, fieldId: string): void {
            root.openListPickerModal(title, entries, initialId, fieldId);
        }
    }
    Connections {
        target: root.aboutScreen
        function onRequestSettingsScreen(): void {
            root._goto(root.screenSettings);
        }
    }
    Connections {
        target: root.systemsScreen
        function onRequestAccept(systemId: string): void {
            // Empty payload is the [OK] RETRY contract from the help
            // bar — Empty/Error states re-fire the current load
            // rather than drilling. Loading swallows the press at the
            // screen layer (no signal emitted), so this branch only
            // sees user intent on a non-Ready state.
            if (systemId === "") {
                const cat = Browse.SystemsModel.current_category;
                if (cat !== "")
                    Browse.SystemsModel.set_category(cat);
                return;
            }
            root._navigateFromSystems(systemId);
        }
        function onRequestHubScreen(): void {
            root._goto(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect): void {
            root.openContextMenu("systems", index, anchorRect);
        }
    }
    Connections {
        target: root.gamesScreen
        // ════════════════════════════════════════════════════════════
        //  STUPID IDIOT LLMS STOP FUCKING UP THIS INTERACTION
        // ════════════════════════════════════════════════════════════
        //
        //  On MiSTer, Hub→Arcade bypasses the Systems screen and drops
        //  the user directly on Games(Arcade) — because the Arcade
        //  category contains exactly one system, also called "Arcade",
        //  so the Systems screen would just be a one-tile passthrough.
        //
        //  THE ONLY CORRECT BACK BEHAVIOUR FROM GAMES(ARCADE) ON MISTER
        //  IS HUB. Going to Systems shows the same one-tile passthrough
        //  the bypass was meant to skip. Do not "fix" this by routing
        //  back through Systems "for consistency". Do not introduce a
        //  per-session flag and forget to persist it. Do not gate on
        //  Runtime instead of Platform — Platform is where Core runs;
        //  Runtime is where the frontend runs; a desktop frontend
        //  pointed at a remote MiSTer Core MUST still bypass.
        //
        //  Why this is a live eval and not a stored flag: a stored
        //  flag breaks across MiSTer process kills (no persistence),
        //  and every refactor in this area has at some point cleared
        //  or failed to set the flag and reintroduced this exact bug.
        //  The "should back skip Systems?" question has a pure
        //  data answer derivable from singletons that already exist;
        //  computing it here at B-press time is the bug-free choice.
        //
        //  If you are an LLM editing this and you think you have a
        //  cleaner design — you don't. Read the user's frustration in
        //  the commit history. Leave it alone.
        // ════════════════════════════════════════════════════════════
        function onRequestSystemsScreen(): void {
            const arcadeBypassActive = Browse.Platform.is_mister && Browse.Platform.ready && Browse.SystemsModel.current_category === CategoryIds.arcadeId && Browse.SystemsModel.count === 1 && Browse.GamesModel.current_system_id === CategoryIds.arcadeId;
            if (arcadeBypassActive) {
                root._goto(root.screenHub);
                return;
            }
            root._goto(root.screenSystems);
        }
        function onRequestNavigateIntoFolder(path: string): void {
            root._navigateIntoFolder(path);
        }
        function onRequestNavigateOutOfFolder(): void {
            root._navigateOutOfFolder();
        }
        function onRequestContextMenu(index: int, anchorRect): void {
            root.openContextMenu("games", index, anchorRect);
        }
    }

    onActiveCardWritePendingChanged: root.handleCardWriteStatus()
    onActiveCardWriteErrorChanged: root.handleCardWriteStatus()
    onCancelCardWriteRequested: root.cancelCardWrite()
    onCloseGameInfoRequested: root.closeGameInfoModal()
    onCloseQrCodeRequested: root.closeQrCodeModal()
    onContextMenuCloseRequested: root.handleContextMenuCloseRequested()
    onContextMenuAccepted: id => root.handleContextMenuAccepted(id)
    Connections {
        target: Browse.AlternateVersions
        function onLoadingChanged(): void {
            if (Browse.AlternateVersions.loading || !root._discoverMenuPending)
                return;
            root._discoverMenuPending = false;
            if (!root.contextMenuVisible || root.contextMenuMode !== "main")
                return;
            if (Browse.AlternateVersions.count <= 0)
                root._replaceContextMenuEntryLabel("discover_loading", "No alternates found", "discover_unavailable");
            if (Browse.AlternateVersions.count <= 0)
                return;
            const entries = [];
            for (let i = 0; i < Browse.AlternateVersions.count; i++) {
                entries.push({
                    id: "alternate_version:" + i,
                    label: Browse.AlternateVersions.name_at(i)
                });
            }
            if (entries.length === 0)
                return;
            root._discoverParentEntries = root.contextMenuEntries;
            root.contextMenuEntries = entries;
            root.contextMenuMode = "alternate_versions";
            if (root.contextMenu !== null)
                root.contextMenu.currentIndex = 0;
        }
    }

    // Pure helper — owner/entryType/mediaCapable/hasNfc/isFavorite → list of `{id,label}` entries.
    // Empty list = no menu (caller bails out of openContextMenu).
    //
    // Annotated as `: var` (not `list<var>`): MiSTer's AOT-compiled
    // static QML build coerces the JS array through `list<var>` and the
    // caller saw `entries.length === 0` despite the function pushing 3
    // items in. Plain `var` round-trips cleanly and silences the
    // "insufficiently annotated" coercion warning at the call site.
    function buildContextMenuEntries(owner: string, entryType: string, mediaCapable: bool, hasNfc: bool, isFavorite: bool, systemId: string) {
        if (owner === "systems") {
            const entries = [
                {
                    id: "launch_system",
                    label: qsTr("Launch core")
                }
            ];
            if (!Browse.SystemLaunchers.loading && Browse.SystemLaunchers.error_message === "" && Browse.SystemLaunchers.launcher_count_for_system(systemId) > 0) {
                entries.push({
                    id: "change_launcher",
                    label: qsTr("Change launcher")
                });
            }
            const mediaBusy = Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping;
            if (!mediaBusy) {
                entries.push({
                    id: "index_system",
                    label: qsTr("Update media database")
                }, {
                    id: "scrape_system",
                    label: qsTr("Scrape metadata")
                });
            }
            return entries;
        }
        if (owner === "recents") {
            const entries = [
                {
                    id: "launch_game",
                    label: qsTr("Launch game")
                }
            ];
            if (Browse.Settings.current_discover_arcade_alternate_versions)
                entries.unshift({
                    id: "discover",
                    label: "Discover alt. versions"
                });
            return entries;
        }
        if (owner === "games" || owner === "favorites") {
            if ((entryType === "directory" || entryType === "root") && !mediaCapable)
                return [];
            const entries = [];
            entries.push({
                id: "toggle_favorite",
                label: isFavorite ? qsTr("Remove from favorites") : qsTr("Add to favorites")
            });
            if (hasNfc)
                entries.push({
                    id: "write_card",
                    label: qsTr("Write to NFC token")
                });
            entries.push({
                id: "qr_code",
                label: qsTr("QR code")
            });
            if (Browse.Settings.current_discover_arcade_alternate_versions) {
                entries.push({
                    id: "discover",
                    label: "Discover alt. versions"
                });
            }
            entries.push({
                id: "launch_game",
                label: qsTr("Launch game")
            });
            return entries;
        }
        return [];
    }

    // Pure helper — wrap a zapscript in the zaparoo.app deep-link template.
    // The QR code points the scanning device at this URL; the web app
    // hands the scanned zapscript back to a Core/frontend pairing.
    function _buildQrPayload(zapscript: string): string {
        return "https://zaparoo.app/write?v=" + encodeURIComponent(zapscript);
    }

    function _replaceContextMenuEntryLabel(targetId: string, nextLabel: string, nextId: string): void {
        const entries = [];
        for (let i = 0; i < root.contextMenuEntries.length; i++) {
            const entry = root.contextMenuEntries[i];
            if (entry.id === targetId) {
                entries.push({
                    id: nextId === undefined ? entry.id : nextId,
                    label: nextLabel
                });
            } else {
                entries.push(entry);
            }
        }
        root.contextMenuEntries = entries;
    }

    function _restoreDiscoverContextMenuEntry(entriesIn: var): var {
        const entries = [];
        for (let i = 0; i < entriesIn.length; i++) {
            const entry = entriesIn[i];
            if (entry.id === "discover_loading" || entry.id === "discover_unavailable") {
                entries.push({
                    id: "discover",
                    label: "Discover alt. versions"
                });
            } else {
                entries.push(entry);
            }
        }
        return entries;
    }

    function openContextMenu(owner: string, index: int, anchorRect): void {
        if (index < 0)
            return;
        let entryType = "";
        let isFavorite = false;
        let systemId = "";
        let mediaCapable = false;
        if (owner === "systems") {
            if (index >= Browse.SystemsModel.count)
                return;
            systemId = Browse.SystemsModel.system_id_at(index);
        } else if (owner === "games") {
            if (index >= Browse.GamesModel.count)
                return;
            entryType = Browse.GamesModel.entry_type_at(index);
            mediaCapable = Browse.GamesModel.is_media_capable_at(index);
            isFavorite = Browse.GamesModel.is_favorite_at(index);
        } else if (owner === "favorites") {
            if (index >= Browse.FavoritesModel.count)
                return;
            mediaCapable = true;
            isFavorite = Browse.FavoritesModel.is_favorite_at(index);
        } else if (owner === "recents") {
            if (index >= Browse.RecentsModel.count)
                return;
        }
        const entries = root.buildContextMenuEntries(owner, entryType, mediaCapable, Browse.SystemStatus.has_nfc, isFavorite, systemId);
        if (entries.length === 0)
            return;
        root.contextMenuEntries = entries;
        root.contextMenuOwner = owner;
        root.contextMenuIndex = index;
        root.contextMenuMode = "main";
        root._discoverParentEntries = [];
        root._discoverMenuPending = false;
        root.contextMenuAnchor = anchorRect;
        root._requestModal(root.modalContextMenu);
        root.contextMenuVisible = true;
        if (ScreenManager.topModal !== root.modalContextMenu)
            ScreenManager.pushModal(root.modalContextMenu);
    }

    function handleContextMenuCloseRequested(): void {
        if (root.contextMenuMode === "alternate_versions") {
            root.contextMenuEntries = root._restoreDiscoverContextMenuEntry(root._discoverParentEntries);
            root._discoverParentEntries = [];
            root.contextMenuMode = "main";
            root._discoverMenuPending = false;
            return;
        }
        root.closeContextMenu();
    }

    function closeContextMenu(): void {
        root.contextMenuVisible = false;
        root.contextMenuOwner = "";
        root.contextMenuIndex = -1;
        root.contextMenuMode = "main";
        root._discoverParentEntries = [];
        root._discoverMenuPending = false;
        root.contextMenuEntries = [];
        if (ScreenManager.topModal === root.modalContextMenu)
            ScreenManager.popModal();
    }

    function handleContextMenuAccepted(id: string): void {
        const owner = root.contextMenuOwner;
        const targetIndex = root.contextMenuIndex;
        if (targetIndex < 0)
            return;
        if (id === "discover") {
            let systemId = "";
            let name = "";
            let path = "";
            if (owner === "games") {
                systemId = Browse.GamesModel.system_id_at(targetIndex);
                name = Browse.GamesModel.name_at(targetIndex);
                path = Browse.GamesModel.path_at(targetIndex);
            } else if (owner === "favorites") {
                systemId = Browse.FavoritesModel.system_id_at(targetIndex);
                name = Browse.FavoritesModel.name_at(targetIndex);
                path = Browse.FavoritesModel.path_at(targetIndex);
            } else if (owner === "recents") {
                systemId = Browse.RecentsModel.system_id_at(targetIndex);
                name = Browse.RecentsModel.name_at(targetIndex);
                path = Browse.RecentsModel.path_at(targetIndex);
            }
            root._discoverMenuPending = true;
            root._replaceContextMenuEntryLabel("discover", "Searching....", "discover_loading");
            Browse.AlternateVersions.discover_for(systemId, name, path);
            return;
        }
        root.closeContextMenu();
        if (id === "change_launcher") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId === "")
                return;
            Browse.SystemLaunchers.prepare_system(systemId);
            const entries = [];
            for (let i = 0; i < Browse.SystemLaunchers.picker_ids.length; i++) {
                const launcherId = Browse.SystemLaunchers.picker_ids[i];
                const label = Browse.SystemLaunchers.picker_labels[i];
                entries.push({
                    id: launcherId,
                    label: launcherId === "__default__" ? qsTr("Default") : (label.indexOf("Current: ") === 0 ? qsTr("Current: %1").arg(launcherId) : label)
                });
            }
            if (entries.length > 0)
                root.openListPickerModal(qsTr("Change launcher"), entries, Browse.SystemLaunchers.current_launcher, "system_launcher:" + systemId);
        } else if (id.startsWith("alternate_version:")) {
            const altIndex = Number(id.slice("alternate_version:".length));
            if (!Number.isNaN(altIndex))
                Browse.AlternateVersions.launch_at(altIndex);
        } else if (id === "launch_system") {
            Browse.SystemsModel.launch_at(targetIndex);
        } else if (id === "index_system") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId !== "")
                Browse.MediaStatus.start_index_for_system(systemId);
        } else if (id === "scrape_system") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId !== "")
                Browse.MediaStatus.start_scrape_for_system(systemId);
        } else if (id === "launch_game") {
            if (owner === "favorites")
                Browse.FavoritesModel.launch_at(targetIndex);
            else if (owner === "recents")
                Browse.RecentsModel.launch_at(targetIndex);
            else
                Browse.GamesModel.launch_at(targetIndex);
        } else if (id === "toggle_favorite") {
            if (owner === "games")
                Browse.GamesModel.toggle_favorite_at(targetIndex);
            else if (owner === "favorites")
                Browse.FavoritesModel.toggle_favorite_at(targetIndex);
        } else if (id === "more_info") {
            root.openGameInfo(owner, targetIndex);
        } else if (id === "write_card") {
            if (owner === "systems") {
                root.beginCardWrite("systems");
                Browse.SystemsModel.write_card_at(targetIndex);
            } else if (owner === "games") {
                root.beginCardWrite("games");
                Browse.GamesModel.write_card_at(targetIndex);
            } else if (owner === "favorites") {
                root.beginCardWrite("favorites");
                Browse.FavoritesModel.write_card_at(targetIndex);
            }
        } else if (id === "qr_code") {
            const text = owner === "systems" ? Browse.SystemsModel.launch_text_at(targetIndex) : owner === "games" ? Browse.GamesModel.launch_text_at(targetIndex) : owner === "favorites" ? Browse.FavoritesModel.launch_text_at(targetIndex) : "";
            if (text !== "") {
                Browse.QrCode.generate(root._buildQrPayload(text));
                root.openQrCodeModal();
            }
        } else if (id === "discover_unavailable" || id === "discover_loading") {
            return;
        }
    }

    function openGameInfo(owner: string, index: int): void {
        let systemId = "";
        let path = "";
        let title = "";
        if (owner === "games") {
            systemId = Browse.GamesModel.system_id_at(index);
            path = Browse.GamesModel.path_at(index);
            title = Browse.GamesModel.name_at(index);
        } else if (owner === "favorites") {
            systemId = Browse.FavoritesModel.system_id_at(index);
            path = Browse.FavoritesModel.path_at(index);
            title = Browse.FavoritesModel.name_at(index);
        } else if (owner === "recents") {
            systemId = Browse.RecentsModel.system_id_at(index);
            path = Browse.RecentsModel.path_at(index);
            title = Browse.RecentsModel.name_at(index);
        }
        if (systemId === "" || path === "")
            return;
        Browse.GameInfo.load(systemId, path, title);
        root._requestModal(root.modalGameInfo);
        root.gameInfoModalVisible = true;
        if (ScreenManager.topModal !== root.modalGameInfo)
            ScreenManager.pushModal(root.modalGameInfo);
    }

    function closeGameInfoModal(): void {
        root.gameInfoModalVisible = false;
        Browse.GameInfo.clear();
        if (ScreenManager.topModal === root.modalGameInfo)
            ScreenManager.popModal();
    }

    function openQrCodeModal(): void {
        root._requestModal(root.modalQrCode);
        root.qrCodeModalVisible = true;
        if (ScreenManager.topModal !== root.modalQrCode)
            ScreenManager.pushModal(root.modalQrCode);
    }

    function closeQrCodeModal(): void {
        root.qrCodeModalVisible = false;
        if (ScreenManager.topModal === root.modalQrCode)
            ScreenManager.popModal();
    }

    // First-run modal lifecycle. Push exactly once per session, the
    // moment the catalog resolves Ready and reports zero systems
    // (`CategoriesModel.loaded === true && count === 0`). 0 visible
    // categories implies a 0-system response from `media.systems` — a
    // mediadb that's missing or never indexed — and the frontend has
    // no UI to render past the hub. The `loaded` gate is critical:
    // the singleton's Default state has `count: 0` before the catalog
    // fetch lands, so without it we'd fire the modal on cold launch
    // before Core has answered. Gating on the catalog instead of
    // MediaStatus.exists/seeded avoids the case where Core reports
    // `database.exists: true` for an empty file — there the catalog
    // is the authoritative "are there games to show?" signal.
    function _maybeOpenFirstRunIndex(): void {
        if (root._firstRunIndexShown)
            return;
        // Defer to the commercial-use notice. The notice's close handler
        // calls back into here once acked, so chaining is automatic and
        // we avoid stacking two modals at the same time.
        if (!Browse.Notice.commercial_ack)
            return;
        if (Browse.AppStatus.connection_state !== 2)
            return;
        if (!Browse.CategoriesModel.loaded)
            return;
        if (Browse.CategoriesModel.count > 0)
            return;
        root._firstRunIndexShown = true;
        root._requestModal(root.modalFirstRunIndex);
        root.firstRunIndexModalVisible = true;
        if (ScreenManager.topModal !== root.modalFirstRunIndex)
            ScreenManager.pushModal(root.modalFirstRunIndex);
    }

    function closeFirstRunIndexModal(): void {
        root.firstRunIndexModalVisible = false;
        if (ScreenManager.topModal === root.modalFirstRunIndex)
            ScreenManager.popModal();
    }

    // Commercial-use first-run notice. Persisted ack lives in
    // `frontend.toml` (not state.toml — MiSTer's tmpfs would re-show
    // the notice on every reboot). The router opens the modal on first
    // paint when the flag is false, and the modal's close handler is
    // what advances to the next first-run gate (mediadb index).
    function _maybeOpenCommercialNotice(): void {
        if (Browse.Notice.commercial_ack)
            return;
        if (root.commercialNoticeModalVisible)
            return;
        // Defer until the cold-launch curtain has lifted. Otherwise
        // the modal paints over the BootOverlay's "Connecting…" cue,
        // and the user perceives the frontend as stuck — they can't
        // tell whether dismissing the notice will reveal a working
        // app or an actual connection failure. Waiting for boot means
        // every "I understand" press lands on a hub that's already
        // ready to use.
        if (!root.bootComplete)
            return;
        root._requestModal(root.modalCommercialNotice);
        root.commercialNoticeModalVisible = true;
        if (ScreenManager.topModal !== root.modalCommercialNotice)
            ScreenManager.pushModal(root.modalCommercialNotice);
    }

    function closeCommercialNoticeModal(): void {
        root.commercialNoticeModalVisible = false;
        if (ScreenManager.topModal === root.modalCommercialNotice)
            ScreenManager.popModal();
        // Now that the notice is dismissed, re-check the media-DB gate
        // — if the catalog had already settled empty behind the notice,
        // this opens that modal as the next step in the chain.
        root._maybeOpenFirstRunIndex();
    }

    // Log-upload modal lifecycle. Triggered from the Settings "Upload
    // log" action; the modal kicks off `Browse.LogUpload.upload()` on
    // its own when `open` flips true. The modal owns its three-phase
    // view; the router only owns push/pop and stack bookkeeping.
    function openLogUploadModal(): void {
        // Reset before showing so a previous success/error from earlier
        // in the session doesn't paint stale state behind the new
        // upload's "Uploading…" copy.
        Browse.LogUpload.reset();
        root._requestModal(root.modalLogUpload);
        root.logUploadModalVisible = true;
        if (ScreenManager.topModal !== root.modalLogUpload)
            ScreenManager.pushModal(root.modalLogUpload);
    }

    function closeLogUploadModal(): void {
        root.logUploadModalVisible = false;
        if (ScreenManager.topModal === root.modalLogUpload)
            ScreenManager.popModal();
    }

    onCloseLogUploadRequested: root.closeLogUploadModal()

    // Quit-confirm lifecycle. Hub's cancel signal lands on
    // `openQuitConfirmModal` instead of `Qt.quit()` so a stray B / Esc
    // can't kill the frontend; the modal owns the actual decision.
    function openQuitConfirmModal(): void {
        root._requestModal(root.modalQuitConfirm);
        root.quitConfirmModalVisible = true;
        if (ScreenManager.topModal !== root.modalQuitConfirm)
            ScreenManager.pushModal(root.modalQuitConfirm);
    }

    function closeQuitConfirmModal(): void {
        root.quitConfirmModalVisible = false;
        if (ScreenManager.topModal === root.modalQuitConfirm)
            ScreenManager.popModal();
    }

    onCloseQuitConfirmRequested: root.closeQuitConfirmModal()
    onQuitConfirmAccepted: Qt.quit()

    onAcceptRestart: root.confirmPendingRestart()
    onCancelRestart: root.cancelPendingRestart()

    // List-picker lifecycle. Settings screens emit requestListPicker
    // with a fieldId that round-trips through the modal so the accept
    // handler can dispatch the chosen id back to the matching
    // Browse.Settings.set_X without re-parsing the title.
    function openListPickerModal(title: string, entries: var, initialId: string, fieldId: string): void {
        root.listPickerTitle = title;
        root.listPickerEntries = entries;
        root.listPickerInitialId = initialId;
        root.listPickerFieldId = fieldId;
        root._requestModal(root.modalListPicker);
        root.listPickerModalVisible = true;
        if (ScreenManager.topModal !== root.modalListPicker)
            ScreenManager.pushModal(root.modalListPicker);
    }

    function closeListPickerModal(): void {
        root.listPickerModalVisible = false;
        root.listPickerTitle = "";
        root.listPickerEntries = [];
        root.listPickerInitialId = "";
        root.listPickerFieldId = "";
        if (ScreenManager.topModal === root.modalListPicker)
            ScreenManager.popModal();
    }

    function openSettingNeedsRestartModal(): void {
        root._requestModal(root.modalSettingNeedsRestart);
        root.settingNeedsRestartModalVisible = true;
        if (ScreenManager.topModal !== root.modalSettingNeedsRestart)
            ScreenManager.pushModal(root.modalSettingNeedsRestart);
    }

    function closeSettingNeedsRestartModal(): void {
        root.settingNeedsRestartModalVisible = false;
        if (ScreenManager.topModal === root.modalSettingNeedsRestart)
            ScreenManager.popModal();
    }

    function stageSettingRestart(fieldId: string, selectedId: string): void {
        if (fieldId === "language")
            root._pendingLanguageSelection = selectedId;
        else if (fieldId === "resolution")
            root._pendingResolutionSelection = selectedId;
        root.openSettingNeedsRestartModal();
    }

    function cancelPendingRestart(): void {
        root._pendingLanguageSelection = "";
        root._pendingResolutionSelection = "";
        root.closeSettingNeedsRestartModal();
    }

    function confirmPendingRestart(): void {
        const language = root._pendingLanguageSelection;
        const resolution = root._pendingResolutionSelection;
        root._pendingLanguageSelection = "";
        root._pendingResolutionSelection = "";
        root.closeSettingNeedsRestartModal();
        if (language !== "")
            Browse.Settings.set_language(language);
        if (resolution !== "")
            Browse.Settings.set_resolution(resolution);
        root.restartApp();
    }

    function restartApp() {
        Qt.exit(1000);
    }

    function beginSystemLauncherUpdate(systemId: string, selectedId: string): void {
        root._pendingLauncherSystemId = systemId;
        root._pendingLauncherSelectionId = selectedId;
        root.listPickerTitle = qsTr("Saving launcher");
        root.listPickerEntries = [
            {
                id: "saving",
                label: qsTr("Saving…")
            }
        ];
        root.listPickerInitialId = "saving";
        root.listPickerFieldId = "system_launcher_pending";
        Browse.SystemLaunchers.set_system_launcher(systemId, selectedId);
    }

    function clearPendingLauncherUpdate(): void {
        root._pendingLauncherSystemId = "";
        root._pendingLauncherSelectionId = "";
    }

    function showSystemLauncherUpdateError(): void {
        root.listPickerTitle = qsTr("Launcher update failed");
        root.listPickerEntries = [
            {
                id: "error",
                label: qsTr("Error: %1").arg(Browse.SystemLaunchers.update_error)
            },
            {
                id: "retry",
                label: qsTr("Retry")
            },
            {
                id: "cancel",
                label: qsTr("Cancel")
            }
        ];
        root.listPickerInitialId = "retry";
        root.listPickerFieldId = "system_launcher_error";
    }

    function handleListPickerCloseRequested(): void {
        if (root.listPickerFieldId === "system_launcher_pending")
            return;
        if (root.listPickerFieldId === "system_launcher_error")
            root.clearPendingLauncherUpdate();
        root.closeListPickerModal();
    }

    onListPickerAccepted: (fieldId, selectedId) => {
        if (fieldId === "system_launcher_pending")
            return;
        if (fieldId === "system_launcher_error") {
            if (selectedId === "error")
                return;
            if (selectedId === "retry" && root._pendingLauncherSystemId !== "")
                root.beginSystemLauncherUpdate(root._pendingLauncherSystemId, root._pendingLauncherSelectionId);
            else {
                root.clearPendingLauncherUpdate();
                root.closeListPickerModal();
            }
            return;
        }
        if (fieldId.startsWith("system_launcher:")) {
            root.beginSystemLauncherUpdate(fieldId.slice("system_launcher:".length), selectedId);
            return;
        }
        if (fieldId === "language") {
            root.closeListPickerModal();
            if (selectedId !== Browse.Settings.current_language)
                root.stageSettingRestart(fieldId, selectedId);
            return;
        } else if (fieldId === "orientation") {
            Browse.Settings.set_orientation(selectedId);
        } else if (fieldId === "browseLayout")
            Browse.Settings.set_browse_layout(selectedId);
        else if (fieldId === "buttonLayout")
            Browse.Settings.set_button_layout(selectedId);
        else if (fieldId === "resolution") {
            root.closeListPickerModal();
            if (selectedId !== Browse.Settings.current_resolution)
                root.stageSettingRestart(fieldId, selectedId);
            return;
        } else if (fieldId === "screensaverTimeout")
            Browse.Settings.set_screensaver_timeout(selectedId);
        else if (fieldId === "mediaImageType")
            Browse.Settings.set_media_image_type(selectedId);
        root.closeListPickerModal();
    }
    onListPickerCloseRequested: root.handleListPickerCloseRequested()

    Connections {
        target: Browse.SystemLaunchers
        function onUpdate_pendingChanged(): void {
            if (root._pendingLauncherSystemId === "" || Browse.SystemLaunchers.update_pending)
                return;
            if (Browse.SystemLaunchers.update_error === "") {
                root.clearPendingLauncherUpdate();
                root.closeListPickerModal();
            } else {
                root.showSystemLauncherUpdateError();
            }
        }
    }

    Connections {
        target: Browse.AppStatus
        function onConnection_stateChanged(): void {
            root._maybeOpenFirstRunIndex();
            root._maybeCompleteBoot();
            root._maybeStartStartupRestore();
            root._maybeCompletePendingResumeLaunch();
        }
    }

    // One-shot dismiss for the cold-launch curtain. The first time the
    // catalog reports READY we flip `bootComplete` and never reset it
    // — a later disconnect surfaces only via the status pill so the
    // user keeps their cached catalog.
    function _maybeCompleteBoot(): void {
        if (root.bootComplete)
            return;
        if (Browse.AppStatus.connection_state === 2) {
            root.bootComplete = true;
            // Curtain just lifted — fire the notice gate now that the
            // hub is paintable. _maybeOpenCommercialNotice early-returns
            // until bootComplete is true, so this is the natural edge.
            root._maybeOpenCommercialNotice();
            // The screensaver gate also early-returns until bootComplete
            // — restart the idle countdown so the timer fires again on
            // the post-boot quiet period. No-op when the setting is
            // "off".
            root._resetIdle();
        }
    }

    Connections {
        target: Browse.CategoriesModel
        function onLoadedChanged(): void {
            root._maybeOpenFirstRunIndex();
            root._maybeStartStartupRestore();
            root._maybeContinueOptimisticTransitions();
        }
        function onCountChanged(): void {
            root._maybeOpenFirstRunIndex();
            root._maybeStartStartupRestore();
            root._maybeContinueOptimisticTransitions();
        }
    }

    onCloseFirstRunIndexRequested: root.closeFirstRunIndexModal()
    onCloseCommercialNoticeRequested: root.closeCommercialNoticeModal()

    function beginCardWrite(owner: string): void {
        if (owner === "systems")
            Browse.SystemsModel.cancel_card_write();
        else if (owner === "games")
            Browse.GamesModel.cancel_card_write();
        else if (owner === "favorites")
            Browse.FavoritesModel.cancel_card_write();
        root.cardWriteOwner = owner;
        root.cardWriteFailed = false;
        root._requestModal(root.modalCardWrite);
        root.cardWriteModalVisible = true;
        cardWriteFailureTimer.stop();
        if (ScreenManager.topModal !== root.modalCardWrite)
            ScreenManager.pushModal(root.modalCardWrite);
    }

    function handleCardWriteStatus(): void {
        if (!root.cardWriteModalVisible || root.cardWriteOwner === "")
            return;
        if (root.activeCardWritePending)
            return;
        if (root.activeCardWriteError !== "") {
            root.cardWriteFailed = true;
            cardWriteFailureTimer.restart();
        } else {
            root.hideCardWriteModal();
        }
    }

    function cancelCardWrite(): void {
        if (root.cardWriteOwner === "systems")
            Browse.SystemsModel.cancel_card_write();
        else if (root.cardWriteOwner === "games")
            Browse.GamesModel.cancel_card_write();
        else if (root.cardWriteOwner === "favorites")
            Browse.FavoritesModel.cancel_card_write();
        root.hideCardWriteModal();
    }

    function hideCardWriteModal(): void {
        cardWriteFailureTimer.stop();
        root.cardWriteModalVisible = false;
        root.cardWriteFailed = false;
        root.cardWriteOwner = "";
        if (ScreenManager.topModal === root.modalCardWrite)
            ScreenManager.popModal();
    }

    // Action router. Called from handleKey (which translates Qt key
    // codes via Browse.Input.action_for_key) and directly from tests.
    // Dispatches to the top modal if any, otherwise the active screen.
    function handleAction(action: string): void {
        root._startupTrace("input/qml handleAction", "action=" + action, "activeScreen=" + root.activeScreen, "pendingTransition=" + root.pendingTransition, "hasModal=" + ScreenManager.hasModal, "heldAction=" + root._heldAction);
        // Screensaver eats the first input cleanly: dismiss the
        // overlay and DO NOT forward the press anywhere. The next
        // press goes through the normal routing below.
        if (root._maybeDismissScreensaver())
            return;
        if (root._startupRestorePending && root.startupRestoreCurtainVisible && root.activeScreen === root._startupRestoreScreen && !ScreenManager.hasModal)
            return;
        root._resetIdle();
        // Input gate. While a forward transition is in flight, swallow
        // every press so a user mashing buttons during the loading
        // wait can't queue a second transition or kick a half-cancel
        // through cancel handlers — the in-flight model call has to
        // settle on its own. Modal handling below still has to run
        // first so an Accept/Esc on a card-write modal isn't
        // accidentally swallowed if a transition is pending behind
        // it (the modal owns input regardless).
        if (root.pendingTransition !== "" && !ScreenManager.hasModal) {
            root._startupTrace("input/qml drop", "reason=pending-transition", "action=" + action, "pendingTransition=" + root.pendingTransition);
            return;
        }
        if (ScreenManager.hasModal) {
            // Single-consumer dispatch. When a second modal lands
            // (action_error variant for game launch / settings reset
            // / etc.), generalise into a per-modal handler table
            // rather than chaining ifs.
            // Only "cancel" aborts an in-flight card write. Treating
            // "accept" the same way would let a fat-fingered OK during
            // pending kill the write the user actually wanted; on
            // success/error the modal auto-dismisses via
            // handleCardWriteStatus, so accept has nothing to do here.
            if (ScreenManager.topModal === root.modalCardWrite && action === "cancel") {
                root.cancelCardWrite();
            } else if (ScreenManager.topModal === root.modalQrCode && action === "cancel") {
                root.closeQrCodeModal();
            } else if (ScreenManager.topModal === root.modalGameInfo) {
                if (root.gameInfoModal !== null)
                    root.gameInfoModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalContextMenu) {
                if (root.contextMenu !== null)
                    root.contextMenu.handleAction(action);
            } else if (ScreenManager.topModal === root.modalFirstRunIndex) {
                if (root.firstRunIndexModal !== null)
                    root.firstRunIndexModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalCommercialNotice) {
                if (root.commercialNoticeModal !== null)
                    root.commercialNoticeModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalLogUpload) {
                if (root.logUploadModal !== null)
                    root.logUploadModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalQuitConfirm) {
                if (root.quitConfirmModal !== null)
                    root.quitConfirmModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalSettingNeedsRestart) {
                if (root.settingNeedsRestartModal !== null)
                    root.settingNeedsRestartModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalListPicker) {
                if (root.listPickerModal !== null)
                    root.listPickerModal.handleAction(action);
            }
            // While a modal owns input, swallow everything not handled
            // above rather than leak it to the root screen.
            return;
        }
        root._noteRapidNavigationAction(action, false);
        if (root.activeScreen === root.screenGames) {
            if (root.gamesScreen !== null)
                root.gamesScreen.handleAction(action);
        } else if (root.activeScreen === root.screenSystems) {
            if (root.systemsScreen !== null)
                root.systemsScreen.handleAction(action);
        } else if (root.activeScreen === root.screenFavorites) {
            if (root.favoritesScreen !== null)
                root.favoritesScreen.handleAction(action);
        } else if (root.activeScreen === root.screenRecents) {
            if (root.recentsScreen !== null)
                root.recentsScreen.handleAction(action);
        } else if (root.activeScreen === root.screenSettings) {
            if (root.settingsScreen !== null)
                root.settingsScreen.handleAction(action);
        } else if (root.activeScreen === root.screenAbout) {
            if (root.aboutScreen !== null)
                root.aboutScreen.handleAction(action);
        } else {
            root.hubScreen.handleAction(action);
        }
    }

    // Hold-to-repeat for navigation actions. Qt's OS-level auto-repeat is
    // dropped (see Keys.onPressed below) because it bursts unpredictably
    // on heavy UI loads and isn't tunable on MiSTer's framebuffer build.
    // Instead, on a real press of a repeatable action we start an
    // initial-delay timer; on its first fire we hand over to a steady
    // tick. Both fire `handleAction(heldAction)`, which means the existing
    // transition gate, modal routing, and screen dispatch all apply
    // unchanged — repeats land on whichever screen / modal is currently
    // active, just like fresh presses.
    readonly property int _repeatInitialMs: 350
    readonly property int _repeatTickMs: 90
    readonly property int _rapidNavigationQuietMs: 260
    property string _heldAction: ""
    property int _heldKey: 0
    property bool rapidNavigationActive: false
    property bool rapidNavigationIndicatorActive: false
    property string rapidNavigationAction: ""
    property int _rapidNavigationTapCount: 0
    // Aliased so tst_navigation.qml can observe the repeat state machine
    // — child Timer ids are file-scoped and aren't reachable otherwise.
    property alias _repeatPending: repeatInitial.running
    property alias _repeatTicking: repeatTick.running

    function _stopRepeat(): void {
        if (root._heldAction !== "" || repeatInitial.running || repeatTick.running)
            root._startupTrace("input/qml repeat stop", "heldAction=" + root._heldAction, "heldKey=" + root._heldKey, "initial=" + repeatInitial.running, "ticking=" + repeatTick.running);
        repeatInitial.stop();
        repeatTick.stop();
        root._heldAction = "";
        root._heldKey = 0;
        // Hold-release commits whatever cell the user landed on. Games
        // screen debounces its `set_selected_at_top` writes (one atomic
        // disk write per move would batter MiSTer's SD card on a Down-
        // hold through 20+ pages); the flush here lands the final
        // selection so a kill during launch resumes on the right entry.
        // No-op when no persist is pending or when another screen is
        // active.
        if (root.gamesScreen !== null)
            root.gamesScreen.flushSelectedPersist();
    }

    Binding {
        target: root.gamesScreen
        property: "detailRapidScrollActive"
        value: root.activeScreen === root.screenGames && root.rapidNavigationActive
    }

    Binding {
        target: root.gamesScreen
        property: "detailRapidIndicatorActive"
        value: root.activeScreen === root.screenGames && root.rapidNavigationIndicatorActive
    }

    Binding {
        target: root.gamesScreen
        property: "detailRapidScrollAction"
        value: root.activeScreen === root.screenGames ? root.rapidNavigationAction : ""
    }

    Binding {
        target: root.favoritesScreen
        property: "detailRapidScrollActive"
        value: root.activeScreen === root.screenFavorites && root.rapidNavigationActive
    }

    Binding {
        target: root.favoritesScreen
        property: "detailRapidIndicatorActive"
        value: root.activeScreen === root.screenFavorites && root.rapidNavigationIndicatorActive
    }

    Binding {
        target: root.favoritesScreen
        property: "detailRapidScrollAction"
        value: root.activeScreen === root.screenFavorites ? root.rapidNavigationAction : ""
    }

    Binding {
        target: root.recentsScreen
        property: "detailRapidScrollActive"
        value: root.activeScreen === root.screenRecents && root.rapidNavigationActive
    }

    Binding {
        target: root.recentsScreen
        property: "detailRapidIndicatorActive"
        value: root.activeScreen === root.screenRecents && root.rapidNavigationIndicatorActive
    }

    Binding {
        target: root.recentsScreen
        property: "detailRapidScrollAction"
        value: root.activeScreen === root.screenRecents ? root.rapidNavigationAction : ""
    }

    function _isRapidNavigationAction(action: string): bool {
        return action === "up" || action === "down" || action === "page_prev" || action === "page_next";
    }

    function _noteRapidNavigationAction(action: string, forceActive: bool): void {
        if (!root._isRapidNavigationAction(action))
            return;
        const sameBurst = rapidNavigationQuiet.running && root.rapidNavigationAction === action;
        root._rapidNavigationTapCount = sameBurst ? root._rapidNavigationTapCount + 1 : 1;
        root.rapidNavigationAction = action;
        if (forceActive || rapidNavigationQuiet.running)
            root.rapidNavigationActive = true;
        if (forceActive || root._rapidNavigationTapCount >= 3)
            root.rapidNavigationIndicatorActive = true;
        rapidNavigationQuiet.restart();
    }

    function _resetRapidNavigation(): void {
        rapidNavigationQuiet.stop();
        root.rapidNavigationActive = false;
        root.rapidNavigationIndicatorActive = false;
        root.rapidNavigationAction = "";
        root._rapidNavigationTapCount = 0;
    }

    function _isRepeatableAction(action: string): bool {
        return action === "up" || action === "down" || action === "left" || action === "right" || action === "page_prev" || action === "page_next";
    }

    // State-machine half of handleKey: records the held key/action and
    // arms the initial-delay timer. Pulled out of handleKey so unit
    // tests can drive the repeat state machine without also routing
    // through handleAction → real screens. No-op for non-dpad actions.
    function _armRepeat(action: string, key: int): void {
        if (!root._isRepeatableAction(action))
            return;
        root._startupTrace("input/qml repeat arm", "action=" + action, "key=" + key, "previousAction=" + root._heldAction, "previousKey=" + root._heldKey);
        root._heldAction = action;
        root._heldKey = key;
        repeatTick.stop();
        repeatInitial.restart();
    }

    // Press handler. Single entry point for both Keys.onPressed and the
    // existing tst_navigation.qml harness (which can't drive Keys events
    // on offscreen windows reliably). Fires the action immediately, then
    // arms the dpad-repeat state machine.
    function handleKey(key: int): void {
        root._startupTrace("input/qml handleKey", "key=" + key, "activeScreen=" + root.activeScreen, "pendingTransition=" + root.pendingTransition, "hasModal=" + ScreenManager.hasModal, "heldAction=" + root._heldAction);
        // Screensaver swallows raw key events ahead of the action map,
        // so the dismissing key is never armed for repeat.
        if (root._maybeDismissScreensaver())
            return;
        const action = Browse.Input.action_for_key(key);
        root._startupTrace("input/qml key mapped", "key=" + key, "action=" + action);
        if (action === "")
            return;
        root.handleAction(action);
        root._armRepeat(action, key);
    }

    // Screen-burn protection. After `_idleScreensaverMs` of input
    // silence (key, gamepad, mouse motion or click) the frontend
    // captures the live scene with an 80%-black scrim baked in once
    // and bounces a copy of the brand mark across the window. Any
    // further input dismisses the overlay; the dismissing press is
    // eaten so the user does not accidentally navigate. The active
    // flag is in-memory only; the timeout itself is persisted
    // through `Browse.Settings.current_screensaver_timeout` (values
    // are seconds as strings, with "off" disabling the feature).
    readonly property int _idleScreensaverMs: {
        const v = Browse.Settings.current_screensaver_timeout;
        if (!v || v === "off")
            return 0;
        const n = parseInt(v, 10);
        return Number.isFinite(n) && n > 0 ? n * 1000 : 0;
    }

    on_IdleScreensaverMsChanged: {
        idleTimer.stop();
        if (root._idleScreensaverMs <= 0) {
            // Switching to "off" while the screensaver is up should
            // tear it down right away — leaving the user staring at a
            // bouncing logo after they explicitly disabled the feature
            // would be confusing.
            if (screensaverOverlay.armed)
                screensaverOverlay.deactivate();
            return;
        }
        idleTimer.start();
    }

    function _resetIdle(): void {
        if (root._idleScreensaverMs <= 0) {
            idleTimer.stop();
            return;
        }
        idleTimer.restart();
    }

    function _maybeDismissScreensaver(): bool {
        if (!screensaverOverlay.armed)
            return false;
        screensaverOverlay.deactivate();
        // A held key dismissed mid-repeat would otherwise keep ticking
        // against an empty target screen.
        root._stopRepeat();
        idleTimer.restart();
        return true;
    }

    function _activateScreensaver(): void {
        if (screensaverOverlay.armed)
            return;
        // Skip while the cold-launch curtain is up or a forward
        // transition is in flight: the BootOverlay and the transition
        // "Loading…" cue are not screen-burn targets, and a screensaver
        // arm during them would race the user-visible animation.
        // `_maybeCompleteBoot` and `_completeTransition` call
        // `_resetIdle()` so the countdown restarts cleanly the moment
        // the gate clears.
        if (!root.bootComplete || root.pendingTransition !== "")
            return;
        const lg = root.headerBar.logoItem;
        if (!lg)
            return;
        const pt = lg.mapToItem(root.scene, 0, 0);
        // PreserveAspectFit means the painted region is narrower than
        // the Image item; using painted{Width,Height} starts the copy
        // flush with the visible logo rather than the Image's full
        // bounding box.
        const w = lg.paintedWidth > 0 ? lg.paintedWidth : lg.width;
        const h = lg.paintedHeight > 0 ? lg.paintedHeight : lg.height;
        screensaverOverlay.activate("qrc:/qt/qml/Zaparoo/App/resources/images/logo.png", Qt.rect(pt.x, pt.y, w, h));
    }

    Timer {
        id: idleTimer
        interval: root._idleScreensaverMs > 0 ? root._idleScreensaverMs : 60000
        repeat: false
        running: root._idleScreensaverMs > 0
        onTriggered: root._activateScreensaver()
    }

    Connections {
        target: screensaverOverlay
        function onUserDismissed(): void {
            root._maybeDismissScreensaver();
        }
    }

    // Mouse-motion idle reset. `Qt.NoButton` lets click and release
    // events fall through to the screensaver overlay's own MouseArea
    // (when armed) or to whatever clickable sits underneath in normal
    // operation. `hoverEnabled: true` is what gets us positionChanged
    // on bare cursor moves without a button being pressed.
    MouseArea {
        anchors.fill: parent
        z: 9001
        hoverEnabled: true
        acceptedButtons: Qt.NoButton
        onPositionChanged: {
            if (root._maybeDismissScreensaver())
                return;
            root._resetIdle();
        }
    }

    // Release handler. Only the key that started the repeat cancels it;
    // a release of any other key in flight (a chord, an unrelated press
    // mid-hold) is ignored.
    function handleKeyRelease(key: int): void {
        root._startupTrace("input/qml handleKeyRelease", "key=" + key, "heldAction=" + root._heldAction, "heldKey=" + root._heldKey);
        if (root._heldAction !== "" && key === root._heldKey)
            root._stopRepeat();
    }

    function _handleRepeatAction(): void {
        root._noteRapidNavigationAction(root._heldAction, true);
        root.handleAction(root._heldAction);
    }

    Timer {
        id: cardWriteFailureTimer
        interval: 1500
        repeat: false
        onTriggered: root.hideCardWriteModal()
    }

    Timer {
        id: rapidNavigationQuiet
        interval: root._rapidNavigationQuietMs
        repeat: false
        onTriggered: {
            root.rapidNavigationActive = false;
            root.rapidNavigationIndicatorActive = false;
            root.rapidNavigationAction = "";
            root._rapidNavigationTapCount = 0;
        }
    }

    Timer {
        id: repeatInitial
        interval: root._repeatInitialMs
        repeat: false
        onTriggered: {
            if (root._heldAction === "")
                return;
            root._handleRepeatAction();
            repeatTick.start();
        }
    }

    Timer {
        id: repeatTick
        interval: root._repeatTickMs
        repeat: true
        onTriggered: {
            if (root._heldAction === "") {
                repeatTick.stop();
                return;
            }
            root._handleRepeatAction();
        }
    }

    // Cancel a stuck repeat if the window loses focus mid-hold; without
    // this, a missed Keys.onReleased (alt-tab, modal grab, compositor
    // quirk) would leave the timer ticking forever. `root.active` is
    // ApplicationWindow's own active property.
    onActiveChanged: {
        if (!root.active)
            root._stopRepeat();
    }

    Item {
        focus: true
        // Drop auto-repeated key events. A held Escape — or a brief
        // stuck press while the main thread is blocked on a model
        // reset — would otherwise queue a burst of `cancel` actions
        // that walk back through games → systems → hub → quit on
        // a single press. Our own controlled repeat (above) takes
        // over for dpad directions only.
        Keys.onPressed: event => {
            if (event.isAutoRepeat)
                return;
            root.handleKey(event.key);
        }
        Keys.onReleased: event => {
            if (event.isAutoRepeat)
                return;
            root.handleKeyRelease(event.key);
        }
    }

    // Forward-transition cue. Item, not Rectangle — the source
    // screen's existing background and circuit-trace texture stay
    // visible underneath; never paint a full-screen fill. The
    // source screen's primary content is hidden by `transitioning`
    // bindings so the centred "Loading…" reads alone in the cleared
    // band. Sized to the full window so anchors.centerIn parks
    // the row in the geometric centre regardless of which screen
    // is the source.
    Item {
        anchors.fill: parent
        visible: (root.pendingTransition !== "" && !root.startupRestoreCurtainVisible) || (root.startupRestoreCurtainVisible && root._startupRestoreScreen !== "")
        z: 100

        readonly property string cueScreen: root.pendingTransition !== "" ? root.pendingTransition : root._startupRestoreScreen

        LoadingIndicator {
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            text: {
                switch (parent.cueScreen) {
                case "systems":
                    return qsTr("Loading systems…");
                case "games":
                    return qsTr("Loading games…");
                case "resume":
                    return qsTr("Loading game…");
                case "favorites":
                    return qsTr("Loading favorites…");
                case "recents":
                    return qsTr("Loading recently played…");
                case "settings":
                    return qsTr("Loading settings…");
                default:
                    return qsTr("Loading…");
                }
            }
        }
    }

    // Hidden cover-decode loop driven by `_prefetchSystemCovers`.
    // While `active`, mounts an Image per SystemsModel row using
    // the same `source` / `sourceSize.width` / `cache` /
    // `asynchronous` settings as Tile.qml's cover Image so the
    // prefetch and the visible Tile share a QPixmapCache slot.
    // As each Image hits Ready or Error, the delegate calls back
    // into `_onCoverDecoded`, which fires the doneCallback and
    // unwinds once every cover is counted. Without this warmup
    // the destination SystemsScreen paints with each Tile showing
    // its procedural text fallback for tens of ms while the PNG
    // decodes — the visible "text → logo pop-in" the deferred
    // flip alone can't fix.
    //
    // Bounded by `systemsCoverPrefetchTimeout` so a missing PNG
    // (silent decode failure that doesn't emit Image.Error) or a
    // genuinely stuck async load never strands the user on the
    // loading overlay.
    Item {
        id: systemsCoverPrefetcher
        visible: false
        property bool active: false
        property var doneCallback: null
        property int total: 0
        property int done: 0

        function _markDone(): void {
            systemsCoverPrefetcher.done++;
            if (systemsCoverPrefetcher.done >= systemsCoverPrefetcher.total) {
                systemsCoverPrefetcher.active = false;
                systemsCoverPrefetchTimeout.stop();
                const cb = systemsCoverPrefetcher.doneCallback;
                systemsCoverPrefetcher.doneCallback = null;
                if (cb !== null)
                    cb();
            }
        }

        Repeater {
            model: systemsCoverPrefetcher.active ? Browse.SystemsModel : null
            delegate: Image {
                required property string coverKey
                source: coverKey === "" ? "" : Resources.coverUrl(coverKey)
                sourceSize.width: 256
                asynchronous: true
                cache: true

                // Each delegate contributes exactly once.
                // Component.onCompleted catches a synchronous Ready
                // (cache hit during construction); onStatusChanged
                // catches the normal async path. `_counted` dedupes
                // so a delegate whose status flips Null → Ready
                // inside construction (and again as the binding
                // settles) tallies once.
                property bool _counted: false
                function _markDone(): void {
                    if (_counted)
                        return;
                    _counted = true;
                    systemsCoverPrefetcher._markDone();
                }

                Component.onCompleted: {
                    if (status === Image.Ready || status === Image.Error || coverKey === "")
                        _markDone();
                }
                onStatusChanged: {
                    if (status === Image.Ready || status === Image.Error)
                        _markDone();
                }
            }
        }
    }

    function _prefetchSystemCovers(cb): void {
        systemsCoverPrefetcher.total = Browse.SystemsModel.count;
        systemsCoverPrefetcher.done = 0;
        if (systemsCoverPrefetcher.total === 0) {
            cb();
            return;
        }
        systemsCoverPrefetcher.doneCallback = cb;
        systemsCoverPrefetcher.active = true;
        systemsCoverPrefetchTimeout.restart();
    }

    Timer {
        id: systemsCoverPrefetchTimeout
        interval: 1500
        repeat: false
        onTriggered: {
            systemsCoverPrefetcher.active = false;
            const cb = systemsCoverPrefetcher.doneCallback;
            systemsCoverPrefetcher.doneCallback = null;
            if (cb !== null)
                cb();
        }
    }

    Timer {
        id: resumeLaunchTimer
        interval: 50
        repeat: false
        onTriggered: root._startResumeLaunch()
    }

    Timer {
        id: favoritesTransitionTimer
        interval: 50
        repeat: false
        onTriggered: root._startFavoritesTransitionLoad()
    }

    Timer {
        id: recentsTransitionTimer
        interval: 50
        repeat: false
        onTriggered: root._startRecentsTransitionLoad()
    }

    // Deferred set_category trigger. Lets the transition overlay
    // paint a frame before set_category's synchronous teardown of
    // SystemsScreen's tile delegates freezes the GUI thread. The
    // 50 ms interval covers a single frame even at MiSTer's ~20 fps
    // software renderer; Qt.callLater / interval 0 fire inside the
    // same event loop iteration before the next render.
    Timer {
        id: deferredCategorySetTimer
        interval: 50
        repeat: false
        property string targetCategory: ""
        onTriggered: {
            const category = deferredCategorySetTimer.targetCategory;
            root._startupTrace("startup/qml deferred category trigger", "category=" + category);
            Browse.SystemsModel.set_category(category);
            // Cleared after set_category so the resulting loading=false
            // edge is the one our callback consumes. If Rust returns
            // early because the same category is already populated, no
            // edge will arrive; complete synchronously in that no-op case.
            root._deferredCategoryPending = false;
            root._completeDeferredCategoryIfReady(category);
        }
    }
}
