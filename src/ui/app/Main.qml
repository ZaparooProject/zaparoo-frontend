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
    readonly property string modalCoreVersion: "core_version_warning"
    readonly property string modalActionError: "action_error"
    readonly property string modalRandomFailed: "random_failed"
    // Sentinel id for the favorites "default order" row; maps to an empty
    // sort mode. Must never be "" — see openFavoritesSortMenu.
    readonly property string _favoritesSortDefault: "default"
    readonly property string modalLogUpload: "log_upload"
    readonly property string modalQuitConfirm: "quit_confirm"
    readonly property string modalListPicker: "list_picker"
    readonly property string modalLetterJump: "letter_jump"
    readonly property string modalSettingNeedsRestart: "restart_confirm"
    readonly property string modalCrtCalibration: "crt_calibration"
    readonly property string modalScrapeSetup: "scrape_setup"
    readonly property string modalIndexSetup: "index_setup"
    // Sentinel id for the "All systems" entry on the media job system-
    // scope picker (Round 11) -- see `_buildSystemScopeEntries`. Must
    // never collide with a real category or system id; category entries
    // are prefixed "cat:" for the same reason.
    readonly property string _systemScopeAll: "*"

    // One-shot session flag: an authoritative empty catalog starts one
    // background index at most once per frontend process. Browsing remains
    // available while newly discovered systems arrive through catalog polls.
    property bool _firstRunIndexStarted: false
    // One-shot guard for the Core-version warning, same process lifetime:
    // show it at most once even if the
    // link drops and reconnects to the same old Core.
    property bool _coreVersionWarningShown: false
    property string _pendingLanguageSelection: ""
    property string _pendingResolutionSelection: ""
    property bool _resolutionRestartPending: false
    property string _pendingCrtStandardSelection: ""
    // Staged CRT-mode toggle awaiting the restart-confirm modal:
    // "" (none), "on", or "off". Confirming writes the 1-byte enable
    // file and exits with code 42 so Main_MiSTer respawns the frontend
    // with the new mode (see Browse.CrtVideo).
    property string _pendingCrtToggle: ""
    // Staged debug-logging toggle awaiting the restart-confirm modal:
    // "" (none), "on", or "off". Unlike the CRT toggle this needs no
    // Main_MiSTer respawn -- the tracing subscriber is only built once at
    // startup (see settings.rs), so confirming just persists the value and
    // takes the normal in-process restart, the same exit as `language`.
    property string _pendingDebugLoggingToggle: ""
    property bool _discoverMenuPending: false
    property bool _pendingResumeLaunch: false
    property bool _startupRestorePending: false
    property bool _startupRestoreStarted: false
    property string _startupRestoreScreen: ""
    property var _screenReadyCallbacks: ({})
    property var _discoverParentEntries: []
    property string _pendingLauncherSystemId: ""
    property string _pendingLauncherSelectionId: ""
    property string _pendingGameLauncherSystemId: ""
    property string _pendingGameLauncherPath: ""
    property string _pendingGameLauncherSelectionId: ""
    // Set when "Change launcher" is accepted for a game while
    // Browse.GameLauncherOverride.prepare_game (fired at context-menu open)
    // hasn't resolved yet -- see handleContextMenuAccepted's games branch
    // and the deferred-open Connections block below.
    property bool _gameLauncherPickerPending: false
    property string _gameLauncherPickerSystemId: ""
    property string _gameLauncherPickerPath: ""
    property string cardWriteOwner: ""
    property int _cardWriteIndex: -1
    property string _gameInfoOwner: ""
    property int _gameInfoIndex: -1
    property var _actionErrorAcceptedCallback: null
    property var _actionErrorQueue: []
    property int _lastActionErrorSequence: 0
    property string contextMenuMode: "main"
    property string contextMenuOwner: ""
    property int contextMenuIndex: -1
    // The Hub item's own `Browse.HubLayout` position, captured whenever a
    // Hub-owned context menu opens ("categories", "hub_favorites",
    // "hub_action", "hub_item"). Separate from `contextMenuIndex`, which
    // for "categories" already carries a *different* index (the
    // CategoriesModel one its kind-specific entries need) — this is what
    // the menu-agnostic `hub_move`/`hub_remove` entries dispatch against.
    property int _hubItemIndex: -1

    // One in-flight screen-transition sample. Input dispatch starts it, the
    // router records when destination becomes active, and frameSwapped closes
    // it after Qt presents destination's first frame.
    property double _transitionInputStartedAt: 0
    property double _transitionRouteAt: 0
    property string _transitionAction: ""
    property string _transitionFromScreen: ""
    property string _transitionToScreen: ""
    readonly property bool activeCardWritePending: root.cardWriteOwner === "systems" ? Browse.SystemsModel.card_write_pending : root.cardWriteOwner === "games" ? Browse.GamesModel.card_write_pending : root.cardWriteOwner === "favorites" ? Browse.FavoritesModel.card_write_pending : root.cardWriteOwner === "recents" ? Browse.RecentsModel.card_write_pending : false
    readonly property string activeCardWriteError: root.cardWriteOwner === "systems" ? Browse.SystemsModel.card_write_error : root.cardWriteOwner === "games" ? Browse.GamesModel.card_write_error : root.cardWriteOwner === "favorites" ? Browse.FavoritesModel.card_write_error : root.cardWriteOwner === "recents" ? Browse.RecentsModel.card_write_error : ""

    // Color schemes apply live. Settings normalizes missing or unknown IDs to
    // Zaparoo Black before exposing this value.
    Binding {
        target: Theme
        property: "colorSchemeId"
        value: Browse.Settings.current_color_scheme
    }

    // Feed the Motion singleton's master switch from persisted preference and
    // MiSTer's effective render budget. Native 1080p remains available as an
    // explicit quality override, but animation would repeatedly dirty its much
    // larger raster surface. This runtime limit does not overwrite the user's
    // Reduce motion setting, so motion returns when a lower resolution boots.
    Binding {
        target: Motion
        property: "enabled"
        value: !Browse.Settings.current_reduce_motion && !(Browse.Settings.is_mister && !root.crtNativePath && root.videoHeight >= 1080)
    }

    // Mirror the "Show original filenames" setting onto every model that
    // surfaces a game name. Bound centrally (not per-screen) so browse,
    // favorites, recents, the resume banner, and launch/now-playing titles
    // all flip together regardless of which screen is mounted. Each model's
    // setter re-emits dataChanged so already-built delegates refresh in place.
    Binding {
        target: (root.gamesScreenRequested || root.activeScreen === root.screenGames) ? Browse.GamesModel : null
        property: "show_original_filenames"
        value: Browse.Settings.current_show_original_filenames
    }
    Binding {
        target: (root.favoritesScreenRequested || root.activeScreen === root.screenFavorites) ? Browse.FavoritesModel : null
        property: "show_original_filenames"
        value: Browse.Settings.current_show_original_filenames
    }
    Binding {
        target: (root._firstFrameSeen || root.recentsScreenRequested || root.activeScreen === root.screenRecents) ? Browse.RecentsModel : null
        property: "show_original_filenames"
        value: Browse.Settings.current_show_original_filenames
    }

    // Bound here (not in GamesScreen.qml) because `set_system` can fire
    // from the accept handler before the games screen mounts; binding
    // inside the screen fires only on `Component.onCompleted`, after the
    // first fetch has already gone out with the model's default
    // page_size. That made the first cursor page misaligned with the
    // visual grid pageSize and produced half-loaded pages on every
    // subsequent cursor advance.
    readonly property int _gamesListFetchSize: 30
    readonly property var _gamesGridProfile: BrowseLayouts.themeProfile(BrowseLayouts.currentThemeId, "gamesGrid")
    readonly property var _gamesGridStatusProfile: root._gamesGridProfile && root._gamesGridProfile.status ? root._gamesGridProfile.status : null
    readonly property var _gamesGridFooterProfile: root._gamesGridProfile && root._gamesGridProfile.footer ? root._gamesGridProfile.footer : null
    readonly property int _gamesGridTopStripBottom: Sizing.headerBottom + (root._gamesGridStatusProfile ? root._gamesGridStatusProfile.topMargin : Sizing.pctH(1)) + (root._gamesGridStatusProfile ? root._gamesGridStatusProfile.stripHeight : Sizing.pctH(7))
    readonly property int _gamesGridBottomMargin: root._gamesGridFooterProfile ? root._gamesGridFooterProfile.gridBottomMargin : (Sizing.pctH(8) + Sizing.pctH(7))
    readonly property int _gamesGridViewportWidth: Math.max(1, Sizing.screenWidth)
    readonly property int _gamesGridViewportHeight: Math.max(1, Sizing.screenHeight - root._gamesGridTopStripBottom - root._gamesGridBottomMargin)
    readonly property var _gamesGridShape: Sizing.gamesGridShape(root._gamesGridViewportWidth, root._gamesGridViewportHeight)
    readonly property int _gamesGridColumns: root._gamesGridShape.columns
    readonly property int _gamesGridRows: root._gamesGridShape.rows
    readonly property int _gamesPageSize: Browse.Settings.current_games_browse_layout === "list" ? root._gamesListFetchSize : root._gamesGridColumns * root._gamesGridRows
    on_GamesPageSizeChanged: {
        if (root.gamesScreenRequested || root.activeScreen === root.screenGames)
            root._syncGamesModelLayout();
    }
    // Cover size requested from Core for grid tiles. Same per-view, snapped tier
    // (128/256/512/768) the grid tile decodes at (Tile.qml drives sourceSize off
    // the identical helper), so the request matches what is shown and Core caches
    // one resized WebP per tier, keeping the constantly-browsed grid small in RAM.
    readonly property int _gamesCoverMaxSize: Sizing.gamesGridCoverSourceSize(root._gamesGridViewportWidth, root._gamesGridViewportHeight)
    // The detail pane shows the cover larger than a grid tile, so it requests its
    // own snapped tier (a step above the grid). Baked into the detail keys, this
    // gives the pane an independent, crisper cache entry instead of sharing the
    // grid's smaller image.
    readonly property int _gamesDetailCoverMaxSize: Sizing.detailCoverSourceSize(root._gamesGridViewportWidth, root._gamesGridViewportHeight)
    // _gamesDetailCoverMaxSize derives from _gamesCoverMaxSize, so this one
    // handler re-syncs both sizes whenever the grid shape changes.
    on_GamesCoverMaxSizeChanged: {
        if (root.gamesScreenRequested || root.favoritesScreenRequested || root.recentsScreenRequested)
            root._syncCoverSizing();
    }

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
    // Keep the detail-cover tier's inputs identical to the fetch-size
    // inputs (`_gamesDetailCoverMaxSize` below), so request tier and
    // decode tier can never diverge.
    Binding {
        target: Sizing
        property: "detailCoverViewportWidth"
        value: root._gamesGridViewportWidth
    }
    Binding {
        target: Sizing
        property: "detailCoverViewportHeight"
        value: root._gamesGridViewportHeight
    }

    function _requestScreen(screen: string): void {
        if (screen === root.screenSystems)
            root.systemsScreenRequested = true;
        else if (screen === root.screenGames) {
            root.gamesScreenRequested = true;
            root._syncGamesModelLayout();
        } else if (screen === root.screenFavorites) {
            root.favoritesScreenRequested = true;
            root._syncCoverSizing();
        } else if (screen === root.screenFavoriteSystems)
            root.favoriteSystemsScreenRequested = true;
        else if (screen === root.screenRecents) {
            root.recentsScreenRequested = true;
            root._syncCoverSizing();
        } else if (screen === root.screenSettings)
            root.settingsScreenRequested = true;
        else if (screen === root.screenAbout)
            root.aboutScreenRequested = true;
    }

    function _syncGamesModelLayout(): void {
        Browse.GamesModel.page_size = root._gamesPageSize;
        root._syncCoverSizing();
    }

    function _syncCoverSizing(): void {
        Browse.GamesModel.set_cover_max_size(root._gamesCoverMaxSize);
        Browse.GamesModel.set_detail_cover_max_size(root._gamesDetailCoverMaxSize);
    }

    function _screenItem(screen: string): var {
        if (screen === root.screenSystems)
            return root.systemsScreen;
        if (screen === root.screenGames)
            return root.gamesScreen;
        if (screen === root.screenFavorites)
            return root.favoritesScreen;
        if (screen === root.screenFavoriteSystems)
            return root.favoriteSystemsScreen;
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
        else if (modal === root.modalCoreVersion)
            root.coreVersionModalRequested = true;
        else if (modal === root.modalActionError)
            root.actionErrorModalRequested = true;
        else if (modal === root.modalRandomFailed)
            root.randomFailedModalRequested = true;
        else if (modal === root.modalLogUpload)
            root.logUploadModalRequested = true;
        else if (modal === root.modalQuitConfirm)
            root.quitConfirmModalRequested = true;
        else if (modal === root.modalListPicker)
            root.listPickerModalRequested = true;
        else if (modal === root.modalLetterJump)
            root.letterJumpModalRequested = true;
        else if (modal === root.modalSettingNeedsRestart)
            root.settingNeedsRestartModalRequested = true;
        else if (modal === root.modalCrtCalibration)
            root.crtCalibrationModalRequested = true;
        else if (modal === root.modalScrapeSetup)
            root.scrapeSetupModalRequested = true;
        else if (modal === root.modalIndexSetup)
            root.indexSetupModalRequested = true;
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
        const savedScreen = root._validStartupScreen(Browse.AppState.active_screen);
        root.activeScreen = root.screenHub;
        if (savedScreen !== "" && savedScreen !== root.screenHub) {
            root._startupRestorePending = true;
            root._startupRestoreScreen = savedScreen;
            // MainLayout seeds this before Component.onCompleted so no first-frame
            // ghost Hub can leak during non-Hub restores; assign here to break the
            // initial binding and keep the router-owned curtain explicit.
            root.startupRestoreCurtainVisible = true;
        } else {
            root.startupRestoreCurtainVisible = false;
        }
        root._startupTrace("startup/qml Component.onCompleted", "savedScreen=" + savedScreen, "initialActiveScreen=" + root.activeScreen, "startupRestorePending=" + root._startupRestorePending, "connectionState=" + Browse.AppStatus.connection_state);
        // Start the `custom/hub/` override scan before first paint. It is a
        // spawn_blocking directory walk that never touches the GUI thread
        // (rust/frontend/src/models/image_overrides.rs), so starting it here
        // costs the first frame nothing and usually lands before it — which is
        // what makes HubScreen's bundled-key default swap invisibly. The model
        // has its own idempotency guard, so calling it once here is enough.
        Browse.ImageOverrides.load_hub_overrides();
        // Same early-arrival reasoning as restoreFromCategoriesReset just
        // below: if the catalog was already seeded synchronously before
        // this Item finished constructing, the Connections block's
        // onModelReset (below) can't have fired yet to catch it (its
        // target already existed and already reset before this
        // Connections object itself came alive) — reconcile here too so a
        // fresh install's very first Hub paint already reflects the real
        // layout instead of a placeholder frame.
        root._reconcileHubLayout();
        // Fire the focus restore here so Hub focus is seated and marked ready
        // before first paint. Do not cascade into SystemsModel yet: first
        // paint stays Hub-only, and the post-frame handler below runs the
        // cascade needed by saved-screen restore and later drill-downs.
        root.hubScreen.restoreFromCategoriesReset(false);
        root._maybeArmHubResumeFocus();
        // Open the commercial-use notice on first paint of an unacked install.
        // Indexing is independent of modal routing and can start behind it.
        root._maybeOpenCommercialNotice();
        // Kick the background first-run check in case READY, media status, and
        // an empty catalog landed before our Connections wired up.
        root._maybeCompleteBoot();
        root._maybeStartFirstRunIndex();
        root._maybeStartStartupRestore();
    }

    on_FirstFrameSeenChanged: {
        if (root._firstFrameSeen) {
            if (Browse.CategoriesModel.count > 0)
                root.hubScreen.restoreFromCategoriesReset(true);
            // SystemsScreen's QML tree costs about a second to instantiate on
            // MiSTer. Mount it just after Hub's first frame, while the user is
            // orienting, instead of charging that one-time cost to first
            // category Accept. Its cover requests remain disabled while
            // inactive, so this warms structure without decoding SVG logos.
            systemsScreenWarmMountTimer.restart();
            root._maybeStartStartupRestore();
        }
    }

    Connections {
        target: Browse.ImageOverrides
        function onHub_loadedChanged(): void {
            if (Browse.ImageOverrides.hub_loaded)
                Browse.ImageOverrides.load_system_overrides();
        }
        function onSystems_loadedChanged(): void {
            if (Browse.ImageOverrides.systems_loaded && Browse.CategoriesModel.count > 0)
                Browse.SystemsModel.reproject();
        }
    }

    function _isStableNavigationScreen(screen: string): bool {
        if (screen === root.screenHub || screen === root.screenSystems || screen === root.screenGames || screen === root.screenFavorites || screen === root.screenFavoriteSystems || screen === root.screenRecents || screen === root.screenSettings || screen === root.screenAbout)
            return true;
        return false;
    }

    function _validStartupScreen(screen: string): string {
        if (root._isStableNavigationScreen(screen))
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
            root._reconcileHubLayout();
            root.hubScreen.restoreFromCategoriesReset(root._firstFrameSeen);
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
        target: root._systemsModelConnectionsEnabled ? Browse.SystemsModel : null
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
        target: root._gamesModelConnectionsEnabled ? Browse.GamesModel : null
        function onModelReset(): void {
            if (root.gamesScreen === null) {
                root._whenScreenReady(root.screenGames, function () {
                    root._restoreGamesScreenSelection();
                });
                return;
            }
            root._restoreGamesScreenSelection();
        }
        // Same-sized folder pages update existing delegates rather than
        // emitting modelReset. Restore persisted selection from this explicit
        // revision edge so optimized Back navigation keeps identical behavior.
        function onRows_revisionChanged(): void {
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
        //
        // A restore's own bulk fetch (`fetch_more_restore`) trickles in
        // over several frame-gapped sub-batches (`chunk_for_subbatching`
        // in `apply_append_page`) rather than landing as one atomic
        // insert, so `count` now bumps once per sub-batch while
        // `loading_more` stays true for the whole fetch. Acting on an
        // intermediate sub-batch's partial count — rather than waiting for
        // the fetch to fully settle — both wastes several redundant
        // `fetch_more_restore()` calls (silently no-op'd by
        // `fetch_more_with_limit`'s `loading_more` guard, since a second
        // bulk fetch can't start while the first is still trickling) and,
        // worse, can give up the restore early on a still-partial row set.
        //
        // `loading` (the *initial*-browse flag, distinct from
        // `loading_more`) needs the same treatment: `set_system`/`set_path`
        // deliberately re-run `start_initial_browse` even for an
        // already-current scope ("SystemsScreen accept always wants a
        // round trip" — see that function's own comment in games.rs) and
        // `start_initial_browse` resets `has_next_page`/`loading_more` to
        // false *synchronously*, before the (often cache-served, but still
        // separately-signalled) corrected values land a moment later.
        // Widening the restore's own window from near-instant to several
        // hundred ms made it much likelier for one of those redundant
        // re-browses to land mid-restore and be observed at exactly that
        // stale, mid-reset instant — reading `has_next_page: false` as
        // "genuinely exhausted" and abandoning the walk early, sometimes
        // onto a page that hasn't been repopulated yet. Wait for both
        // fetch kinds to fully settle before evaluating.
        function onCountChanged(): void {
            if (Browse.GamesModel.loading_more || Browse.GamesModel.loading)
                return;
            root._continueGamesRestore();
        }
        // Fires once a bulk restore fetch (`fetch_more_restore`) fully
        // settles — see `onCountChanged`'s comment above for why this,
        // not every intermediate sub-batch's count change, is the right
        // edge to resume the restore walk on, and why `loading` is
        // checked too.
        function onLoading_moreChanged(): void {
            if (Browse.GamesModel.loading_more || Browse.GamesModel.loading)
                return;
            root._continueGamesRestore();
        }
        // A redundant `start_initial_browse` (same-scope re-browse, or a
        // genuine scope change) settling is the other edge that can leave
        // a restore walk stalled — see `onCountChanged`'s comment above.
        function onLoadingChanged(): void {
            if (Browse.GamesModel.loading_more || Browse.GamesModel.loading)
                return;
            root._continueGamesRestore();
        }
        // `apply_append_page` intentionally publishes terminal
        // has_next_page=false after countChanged so pending grid jumps see the
        // fresh row count. A restore target that no longer exists therefore
        // cannot finish from onCountChanged: it still sees the prior true
        // value and its guarded follow-up fetch is rejected because the final
        // cursor is already empty. Recheck on the terminal edge to clear the
        // loading gate instead of leaving "Loading games…" stuck forever.
        // Also gated on `!loading` now — see `onCountChanged`'s comment —
        // since `start_initial_browse` can produce this exact same
        // false-edge as a side effect of a redundant re-browse, not just a
        // genuinely exhausted folder.
        function onHas_next_pageChanged(): void {
            if (root._pendingGameRestorePath === "" || Browse.GamesModel.has_next_page || Browse.GamesModel.loading_more || Browse.GamesModel.loading)
                return;
            root._restoreGamesScreenSelection();
        }
    }

    // Cross-screen transitions: each screen signals its intent and this
    // router flips ScreenManager, then applies route policy such as
    // launch-resume persistence. Keeps the screens themselves ignorant
    // of AppState so they can be reused in test harnesses that don't
    // wire the full persistence layer.
    function _beginTransitionTiming(action: string): void {
        root._transitionInputStartedAt = Date.now();
        root._transitionRouteAt = 0;
        root._transitionAction = action;
        root._transitionFromScreen = root.activeScreen;
        root._transitionToScreen = "";
    }

    function _markTransitionRouted(screen: string): void {
        if (root._transitionInputStartedAt <= 0 || screen === root._transitionFromScreen)
            return;
        root._transitionRouteAt = Date.now();
        root._transitionToScreen = screen;
        console.info("responsiveness transition routed" + " action=" + root._transitionAction + " from=" + root._transitionFromScreen + " to=" + screen + " route_ms=" + Math.max(0, root._transitionRouteAt - root._transitionInputStartedAt));
    }

    function _finishTransitionTiming(): void {
        if (root._transitionToScreen === "" || root._transitionRouteAt <= 0)
            return;
        const presentedAt = Date.now();
        console.info("responsiveness transition presented" + " action=" + root._transitionAction + " from=" + root._transitionFromScreen + " to=" + root._transitionToScreen + " route_ms=" + Math.max(0, root._transitionRouteAt - root._transitionInputStartedAt) + " present_ms=" + Math.max(0, presentedAt - root._transitionRouteAt) + " total_ms=" + Math.max(0, presentedAt - root._transitionInputStartedAt));
        root._transitionInputStartedAt = 0;
        root._transitionRouteAt = 0;
        root._transitionAction = "";
        root._transitionFromScreen = "";
        root._transitionToScreen = "";
    }

    onFramePresented: {
        root._finishTransitionTiming();
        if (!root.gamesCoverRevealReady && root.activeScreen === root.screenGames && !Browse.GamesModel.loading && root.pendingTransition === "") {
            const presentedAt = Date.now();
            root.gamesCoverRevealReady = true;
            console.debug("responsiveness game covers enabled after model frame");
            if (root.gamesNavigationInputAt > 0) {
                const modelReadyAt = root.gamesNavigationModelReadyAt > 0 ? root.gamesNavigationModelReadyAt : presentedAt;
                console.info("responsiveness folder navigation presented" + " action=" + root.gamesNavigationAction + " model_ms=" + Math.max(0, modelReadyAt - root.gamesNavigationInputAt) + " present_ms=" + Math.max(0, presentedAt - modelReadyAt) + " total_ms=" + Math.max(0, presentedAt - root.gamesNavigationInputAt));
                root.gamesNavigationInputAt = 0;
                root.gamesNavigationModelReadyAt = 0;
                root.gamesNavigationAction = "";
            }
        }
    }

    function _goto(screen: string): void {
        root._requestScreen(screen);
        root._startupTrace("startup/qml goto", "from=" + root.activeScreen, "to=" + screen, "pendingTransition=" + root.pendingTransition);
        ScreenManager.activeScreen = screen;
        root._markTransitionRouted(screen);
        if (root._isLaunchResumeScreen(screen))
            Browse.AppState.active_screen = screen;
    }

    // Long-running operational screens should not be restored after the
    // process is killed. Resume only stable navigation destinations.
    function _isLaunchResumeScreen(screen: string): bool {
        return root._isStableNavigationScreen(screen);
    }

    function _allowsScreensaver(screen: string): bool {
        if (screen === root.screenUpdate)
            return root.updateScreen !== null && root.updateScreen.allowsScreensaver;
        return true;
    }

    function _backTargetReady(screen: string): bool {
        const item = root._screenItem(screen);
        if (item === null || item === undefined)
            return false;
        if (screen === root.screenHub)
            return true;
        if (screen === root.screenSystems)
            return !Browse.SystemsModel.loading;
        if (screen === root.screenGames)
            return !Browse.GamesModel.loading;
        if (screen === root.screenFavorites)
            return !Browse.FavoritesModel.loading;
        if (screen === root.screenFavoriteSystems)
            return !Browse.FavoriteSystemsModel.loading;
        if (screen === root.screenRecents)
            return !Browse.RecentsModel.loading;
        return true;
    }

    function _maybeCompleteBackTransition(): void {
        if (root.pendingTransition !== "back")
            return;
        const target = root._backTransitionTarget;
        if (target === "" || !root._backTargetReady(target))
            return;
        root._backTransitionTarget = "";
        backTransitionTimer.stop();
        root._completeTransition(target);
    }

    // Back navigation is usually a return to an already-mounted, already-filled
    // screen. Cut immediately in that case. Keep the delayed Loading cue only
    // when real work is pending: lazy screen mount, catalog boot, or a model
    // fill still in flight.
    function _navigateBackToScreen(screen: string): void {
        if (screen === root.activeScreen)
            return;
        root._backTransitionTarget = "";
        backTransitionTimer.stop();
        if (root._backTargetReady(screen)) {
            root._goto(screen);
            root._resetIdle();
            return;
        }
        root._backTransitionTarget = screen;
        root.pendingTransition = "back";
        root._whenScreenReady(screen, function () {
            if (root.pendingTransition !== "back" || root._backTransitionTarget !== screen)
                return;
            root._maybeCompleteBackTransition();
            if (root.pendingTransition === "back")
                backTransitionTimer.restart();
        });
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
    property var _favoriteSystemsReadyCallback: null
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
    property bool _deferredSystemPending: false
    readonly property bool _systemsModelConnectionsEnabled: root.systemsScreenRequested || (root._firstFrameSeen && root._startupRestorePending) || root._categoryReadyCallback !== null || root._deferredCategoryPending || root._catalogWaitCategory !== ""
    readonly property bool _gamesModelConnectionsEnabled: root.gamesScreenRequested || root._systemReadyCallback !== null || root._deferredSystemPending
    readonly property bool _favoritesModelConnectionsEnabled: root.favoritesScreenRequested || root._favoritesReadyCallback !== null
    readonly property bool _favoriteSystemsModelConnectionsEnabled: root.favoriteSystemsScreenRequested || root._favoriteSystemsReadyCallback !== null
    readonly property bool _recentsModelConnectionsEnabled: root.recentsScreenRequested || root._recentsReadyCallback !== null || root._pendingResumeLaunch
    // Saved games-screen entry path that wasn't on the freshly seeded
    // page 1 of MediaBrowse. The GamesModel.onCountChanged watcher
    // below paginates forward via fetch_more until the path is found
    // or `has_next_page` goes false. Cleared on resolution and on
    // any navigation that starts a new browse target so a stale
    // restore can't keep paginating after the user moves on.
    property string _pendingGameRestorePath: ""
    gamesSelectionRestorePending: root._pendingGameRestorePath !== ""
    property string _backTransitionTarget: ""
    property string _pendingFolderBackTargetPath: ""
    property string _pendingFolderBackSystemId: ""
    property var _folderBackReadyCallback: null
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

    function _completeDeferredSystemIfReady(targetSystemId: string): bool {
        if (root._systemReadyCallback === null)
            return false;
        if (Browse.GamesModel.loading)
            return false;
        if (Browse.GamesModel.current_system_id !== targetSystemId)
            return false;
        root._startupTrace("startup/qml deferred system ready", "systemId=" + targetSystemId + " count=" + Browse.GamesModel.count);
        const cb = root._systemReadyCallback;
        root._systemReadyCallback = null;
        cb();
        return true;
    }

    function _completeFolderBackRebrowseIfReady(): bool {
        if (root._folderBackReadyCallback === null)
            return false;
        if (Browse.GamesModel.loading)
            return false;
        const cb = root._folderBackReadyCallback;
        root._folderBackReadyCallback = null;
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
        else if (root.pendingTransition === "favorite_systems")
            favoriteSystemsTransitionTimer.restart();
        else if (root.pendingTransition === "recents")
            recentsTransitionTimer.restart();
        else if (root.pendingTransition === "settings")
            root._whenScreenReady(root.screenSettings, function () {
                if (root.pendingTransition === "settings")
                    root._completeTransition(root.screenSettings);
            });
        root._maybeCompleteBackTransition();
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
        target: root._systemsModelConnectionsEnabled ? Browse.SystemsModel : null
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
            if (cb === null) {
                root._maybeCompleteBackTransition();
                return;
            }
            root._categoryReadyCallback = null;
            cb();
            root._maybeCompleteBackTransition();
        }
    }
    Connections {
        target: root._gamesModelConnectionsEnabled ? Browse.GamesModel : null
        function onLoadingChanged(): void {
            if (Browse.GamesModel.loading)
                return;
            if (root.gamesNavigationInputAt > 0 && root.gamesNavigationModelReadyAt <= 0)
                root.gamesNavigationModelReadyAt = Date.now();
            if (root._deferredSystemPending) {
                root._startupTrace("startup/qml system loading edge ignored", "reason=deferred-pending systemId=" + Browse.GamesModel.current_system_id + " count=" + Browse.GamesModel.count);
                return;
            }
            const cb = root._systemReadyCallback;
            if (cb !== null) {
                root._systemReadyCallback = null;
                cb();
            }
            root._completeFolderBackRebrowseIfReady();
            root._maybeCompleteBackTransition();
        }
    }
    Connections {
        target: root._recentsModelConnectionsEnabled ? Browse.RecentsModel : null
        function onLoadingChanged(): void {
            if (Browse.RecentsModel.loading)
                return;
            root._maybeCompletePendingResumeLaunch();
            const cb = root._recentsReadyCallback;
            if (cb === null) {
                root._maybeCompleteBackTransition();
                return;
            }
            root._recentsReadyCallback = null;
            cb();
            root._maybeCompleteBackTransition();
        }

        function onResume_availableChanged(): void {
            root._maybeCompletePendingResumeLaunch();
        }
    }
    Connections {
        target: root._favoritesModelConnectionsEnabled ? Browse.FavoritesModel : null
        function onLoadingChanged(): void {
            if (Browse.FavoritesModel.loading)
                return;
            const cb = root._favoritesReadyCallback;
            if (cb === null) {
                root._maybeCompleteBackTransition();
                return;
            }
            root._favoritesReadyCallback = null;
            cb();
            root._maybeCompleteBackTransition();
        }
    }
    Connections {
        target: root._favoriteSystemsModelConnectionsEnabled ? Browse.FavoriteSystemsModel : null
        function onModelReset(): void {
            if (root.favoriteSystemsScreen !== null)
                root.favoriteSystemsScreen.restoreSelection();
        }
        function onLoadingChanged(): void {
            if (Browse.FavoriteSystemsModel.loading)
                return;
            const cb = root._favoriteSystemsReadyCallback;
            if (cb === null) {
                root._maybeCompleteBackTransition();
                return;
            }
            root._favoriteSystemsReadyCallback = null;
            cb();
            root._maybeCompleteBackTransition();
        }
    }

    // Ensure SystemsModel is filled with `category`, then call cb().
    // Synchronous on the no-op return path (same category already
    // populated — a re-Accept after Esc-back); the set_category call
    // is still made for parity with the prior behaviour even though
    // Rust early-returns when the category already matches. Async
    // path waits for loadingChanged. The timer keeps the loadingChanged
    // bookkeeping on the same asynchronous path without deliberately
    // holding the transition long enough to show the global loading cue.
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
        deferredCategorySetTimer.interval = 1;
        deferredCategorySetTimer.restart();
    }

    // Ensure GamesModel is filled with `systemId`, then call cb().
    // When the system is already current and populated (re-Accept
    // after Esc-back), set_system still re-issues start_initial_browse,
    // but the cached result applies inline through the watcher's seed
    // — loading flips back to false before set_system returns — so we
    // can call cb() synchronously on this path. Cold-load goes through
    // the systemReadyCallback waiter below; when replacing populated
    // games rows during a transition, defer to the delayed loading cue
    // for the same pre-feedback-freeze reason as _ensureCategory.
    function _ensureSystem(systemId: string, cb): void {
        if (Browse.GamesModel.current_system_id === systemId && Browse.GamesModel.count > 0) {
            // Skip the redundant re-browse while a deep-position restore
            // walk (`_pendingGameRestorePath`) is actively growing this
            // exact model via chained `fetch_more_restore()` calls.
            // `set_system`'s re-browse is correct and wanted for a live
            // re-Accept after Esc-back, but here it's an internal
            // re-entry into an already-populated, already-correct model —
            // re-running it would truncate the model back down to just
            // page 1 (`initial_row_replacement`'s `TruncateInPlace`,
            // since the restore has grown `count` past a fresh page's
            // worth) and reset `has_next_page`/`loading_more`/`append_seq`,
            // discarding the restore's progress and dropping its
            // still-pending sub-batches. The model is already correct
            // for `systemId`; the in-flight restore chain owns finishing
            // it from here.
            if (root._pendingGameRestorePath === "")
                Browse.GamesModel.set_system(systemId);
            cb();
            return;
        }
        root._systemReadyCallback = cb;
        root._deferredSystemPending = true;
        deferredSystemSetTimer.targetSystemId = systemId;
        deferredSystemSetTimer.interval = root.pendingTransition !== "" && Browse.GamesModel.count > 0 ? root.loadingIndicatorDelayMs + 50 : 1;
        deferredSystemSetTimer.restart();
    }

    // Reconcile the Hub's persisted layout against whatever categories
    // Core just reported — see Browse.HubLayout's header comment and
    // zaparoo_core::hub_layout::HubLayout::reconcile. Additive-only and
    // idempotent (a no-op when nothing's new, or when Core hasn't answered
    // at all yet — an empty list never seeds), so it's safe to call on
    // every CategoriesModel reset rather than needing a guard here.
    function _currentCategoryIds(): var {
        const ids = [];
        for (let i = 0; i < Browse.CategoriesModel.count; i++)
            ids.push(Browse.CategoriesModel.category_at(i));
        return ids;
    }

    function _reconcileHubLayout(): void {
        Browse.HubLayout.reconcile(root._currentCategoryIds());
    }

    // View -> "Add item…". Delegates to HubScreen's own resolvers so labels
    // match what the tile actually shows once added (see
    // HubScreen.qml's `buildAddEntries`).
    function openHubAddMenu(): void {
        if (root.hubScreen === null)
            return;
        const entries = root.hubScreen.buildAddEntries();
        if (entries.length === 0) {
            // `buildAddEntries()` only ever offers detected categories and
            // built-in actions (see its own doc comment) -- once every one
            // of those is already placed, this is a legitimate "nothing
            // left to add" state, not a bug. Returning silently here used
            // to read as the button doing nothing at all.
            //
            // Tried and rejected: opening the same picker with one
            // informational, non-actionable row. NN/G's empty-state
            // guidance is to teach, not just announce -- "use the empty
            // state to provide help cues; tell the user what could be
            // displayed, and how to populate the area with that content" --
            // and a bare "everything's already on the Hub" row doesn't say
            // where to go next. Route through the existing action_error
            // modal instead, which already carries a body for exactly this:
            // point at the real add path, "Add to Hub" on a Systems/Games/
            // Favorites/Recents row's Options menu, not this menu.
            root.presentActionError("hub_add_empty", qsTr("Nothing left to add"), qsTr("All categories and actions are already on the Hub. To add a game or system, open its Options menu and choose \"Add to Hub\"."), qsTr("OK"), null);
            return;
        }
        root.openListPickerModal(qsTr("Add item"), entries, entries[0].id, "hub_add_pick");
    }

    // View -> "Reset layout": wipe and reseed from Core's currently-detected
    // categories plus the built-in actions — see
    // zaparoo_core::hub_layout::HubLayout::reset's doc comment.
    function resetHubLayout(): void {
        Browse.HubLayout.reset_layout(root._currentCategoryIds());
    }

    // West/Y on the Hub — the page-scoped "View" menu. Settings and Quit
    // live here (second-last / last) rather than on a dedicated
    // button: Quit used to be bound to Cancel/B, which risked a stray
    // Escape/B press quitting the app from the Hub root; both are one-off
    // navigational shortcuts, not layout-editing actions like the two
    // entries above them, but View is the Hub's only page-scoped menu.
    function openHubPageMenu(): void {
        const entries = [
            {
                id: "hub_add",
                label: qsTr("Add item…")
            },
            {
                id: "hub_reset",
                label: qsTr("Reset layout")
            },
            {
                id: "hub_settings",
                label: qsTr("Settings")
            },
            {
                id: "hub_quit",
                label: qsTr("Quit")
            }
        ];
        root.openListPickerModal(qsTr("View"), entries, "hub_add", "page_menu_hub");
    }

    // Hub Accept routing. Empty-row passthrough preserves the committed
    // "Enter on empty hub goes to Systems" behaviour and
    // keeps the navigation test synchronous. The Resume action is a
    // hub payload rather than a category and launches the latest
    // resumable history row. Otherwise: tentatively pin the
    // destination to Systems, fill the chosen category, then either
    // bypass to Games (MiSTer Arcade singleton) or fall through to
    // Systems immediately. Systems paints one stable frame with cover
    // requests gated, then enables SVG decoding after that frame swaps.
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
                root._completeTransition(root.screenSystems);
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

    // Fire the actual resume launch and arm the desktop safety-clear. The
    // "Loading game…" cue (pendingTransition === "resume") stays up through
    // the launch: on MiSTer the process is replaced by the game before the
    // timer fires, so the cue covers the core swap; on desktop nothing
    // replaces us, so resumeLaunchCueTimer clears the cue and restores input.
    // Started only here, at dispatch — never while still waiting on the
    // connection — so a slow coalesce keeps the cue for as long as it needs.
    function _dispatchResumeLaunch(): void {
        Browse.RecentsModel.launch_resume();
        resumeLaunchCueTimer.restart();
    }

    function _maybeCompletePendingResumeLaunch(): void {
        if (!root._pendingResumeLaunch || root.pendingTransition !== "resume")
            return;
        if (Browse.RecentsModel.resume_loading)
            return;
        if (Browse.RecentsModel.resume_available) {
            root._pendingResumeLaunch = false;
            root._dispatchResumeLaunch();
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
        // Optimistic loader, same contract as the other Hub actions: paint
        // the "Loading game…" cue (and hide the ghost-Hub tiles / gate input)
        // immediately, before we know whether the launch can proceed.
        // _cancelResumeLaunch clears it on the no-resumable-game branch.
        root.pendingTransition = "resume";
        if (!Browse.RecentsModel.resume_loading && Browse.RecentsModel.resume_available) {
            root._dispatchResumeLaunch();
            return;
        }
        if (Browse.RecentsModel.resume_loading || Browse.AppStatus.connection_state !== 2) {
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

    function _setFavoritesSystem(systemId): void {
        const normalized = systemId === undefined || systemId === null ? "" : String(systemId);
        root.favoritesSystemId = normalized;
        Browse.FavoritesModel.set_system(normalized);
    }

    function _navigateToFavorites(systemId): void {
        root._setFavoritesSystem(systemId);
        root.pendingTransition = "favorites";
        favoritesTransitionTimer.restart();
    }

    function _completeFavoriteSystemsTransition(): void {
        if (root.pendingTransition !== "favorite_systems")
            return;
        root.favoriteSystemsScreen.restoreSelection();
        root._completeTransition(root.screenFavoriteSystems);
    }

    function _startFavoriteSystemsTransitionLoad(): void {
        if (root.pendingTransition !== "favorite_systems")
            return;
        root._whenScreenReady(root.screenFavoriteSystems, function () {
            if (root.pendingTransition !== "favorite_systems")
                return;
            if (root._catalogStillBooting())
                return;
            if (!Browse.FavoriteSystemsModel.loading) {
                root._completeFavoriteSystemsTransition();
                return;
            }
            root._favoriteSystemsReadyCallback = root._completeFavoriteSystemsTransition;
        });
    }

    function _navigateToFavoriteSystems(): void {
        root.pendingTransition = "favorite_systems";
        favoriteSystemsTransitionTimer.restart();
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

    // Hub → Update transition. Placeholder screen with no async data,
    // so the flip is instant.
    function _navigateToUpdate(): void {
        if (!root.updateEnabled)
            return;
        root._goto(root.screenUpdate);
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
        // Focus is now finalized from persisted state; let the grid render
        // focus (snapped, since the screen's _focusArmed is still false until
        // the first user input).
        root.systemsScreen._restoreDone = true;
        // Browse the saved system whenever we actually have one, regardless
        // of `idx`. `SystemsModel` is populated for `HubState.category` —
        // the Hub's own last-viewed category cursor, which has nothing to
        // do with which category the saved system belongs to — so
        // `index_for_system_id` legitimately misses whenever those two
        // categories differ (e.g. Hub last sat on "Computer" while the
        // saved game is on a Console system). `idx < 0` only means "not in
        // this possibly-wrong category's list", never "invalid system id";
        // `set_system` doesn't need a `SystemsModel` index at all, just the
        // id string, which Core resolves independently. The old `idx >= 0`
        // gate fell back to `SystemsModel.system_id_at(0)` — silently
        // browsing whatever system happened to be first in the WRONG
        // category instead of the one actually saved.
        if (savedSystem !== "") {
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
        // Selection finalized from persisted state; let the grid render focus
        // (snapped, since _focusArmed is still false until the first input).
        root.gamesScreen._restoreDone = true;
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
            Browse.GamesModel.fetch_more_restore();
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

    // Resume an in-progress deep-page restore walk once a bulk fetch has
    // fully settled (`onCountChanged` / `onLoading_moreChanged` above,
    // both gated on `!Browse.GamesModel.loading_more` before calling this).
    // Looks for the saved path in what's now loaded; fetches another bulk
    // chunk if not found and more pages exist; gives up in place otherwise.
    function _continueGamesRestore(): void {
        if (root.gamesScreen === null) {
            root._whenScreenReady(root.screenGames, function () {
                if (root._pendingGameRestorePath !== "")
                    root._continueGamesRestore();
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
            // Restoration is not user-visible page navigation: bulk-load
            // up to Core's 300-row limit so a saved page deep in a large
            // parent does not require dozens of sequential 10-row RPCs.
            // The loading gate remains up and bulk insertion skips the
            // frame-gapped visible-page trickle.
            Browse.GamesModel.fetch_more_restore();
            return;
        }
        root._pendingGameRestorePath = "";
        root._maybeFinishStartupGamesRestore();
    }

    // Systems Accept routing. Pin destination to Games, fill the
    // chosen system, then flip. The Games→back routing decision is
    // re-evaluated live from current state at B-press time (see
    // gamesScreen.onRequestSystemsScreen below) so this path needs
    // no per-transition flag.
    // `fromHub` distinguishes this function's two callers: HubScreen's
    // `system` shortcut (skips Systems entirely — pass `true`) vs.
    // SystemsScreen's own accept handler (normal drill-down — pass
    // `false`). It's persisted on `GamesState` so `onRequestSystemsScreen`
    // below can route Back to Hub instead of Systems on the shortcut
    // path — a screen the user never visited. See the routing contract in
    // CLAUDE.md and `games_state.rs`'s own module doc.
    function _navigateFromSystems(systemId: string, fromHub: bool): void {
        root.gamesNavigationInputAt = 0;
        root.gamesNavigationModelReadyAt = 0;
        root.gamesNavigationAction = "";
        root._requestScreen(root.screenGames);
        Browse.SystemsState.system_id = systemId;
        // Setting system_id on GamesState resets path_stack/selected_at_level
        // (and entered_from_hub) to root level — the new system's browse
        // always starts at the initial games-screen view, regardless of
        // where the user was in a prior system's folder tree.
        Browse.GamesState.system_id = systemId;
        Browse.GamesState.set_entered_from_hub(fromHub);
        root.gamesCoverRevealReady = false;
        root.pendingTransition = "games";
        root._ensureSystem(systemId, function () {
            root._completeTransition(root.screenGames);
        });
    }

    // A Hub `folder` shortcut's Accept path. Same shape as
    // `_navigateFromSystems` (Hub/Systems → Games, first-time screen
    // transition) plus pushing the shortcut's own folder level once the
    // system is ready — unlike `_navigateIntoFolder`, which assumes the
    // user is already ON screenGames and drilling down in-screen (its
    // flash-cut/folder-navigation-timing logic doesn't apply to a fresh
    // transition). `systemId` empty is a malformed/pre-this-feature layout
    // entry; no-op rather than asking GamesModel to load an empty system.
    // Hub-only entry point (no SystemsScreen equivalent calls this), so
    // `entered_from_hub` is always `true` — see `_navigateFromSystems`'s
    // own doc comment above for why Back needs this breadcrumb.
    function _navigateFromHubFolder(systemId: string, path: string): void {
        if (systemId === "" || path === "")
            return;
        root.gamesNavigationInputAt = 0;
        root.gamesNavigationModelReadyAt = 0;
        root.gamesNavigationAction = "";
        root._requestScreen(root.screenGames);
        Browse.SystemsState.system_id = systemId;
        Browse.GamesState.system_id = systemId;
        Browse.GamesState.set_entered_from_hub(true);
        root.gamesCoverRevealReady = false;
        root.pendingTransition = "games";
        root._ensureSystem(systemId, function () {
            Browse.GamesState.push_level(path, "");
            Browse.GamesModel.set_path(path);
            root._completeTransition(root.screenGames);
        });
    }

    function _beginFolderNavigationTiming(action: string): void {
        const screenInputAt = root.gamesScreen !== null ? root.gamesScreen.lastNavigationInputAt : 0;
        root.gamesNavigationInputAt = screenInputAt > 0 ? screenInputAt : Date.now();
        root.gamesNavigationModelReadyAt = 0;
        root.gamesNavigationAction = action;
        if (root.gamesScreen !== null)
            root.gamesScreen.lastNavigationInputAt = 0;
    }

    // Folder drill-down inside the games screen. Stays on screenGames
    // — no pendingTransition flip — so the in-screen ScreenStateOverlay
    // handles the loading/empty/error cue while the new browse settles.
    // Pushes the new level onto GamesState before issuing the browse so
    // a kill mid-load still resumes inside the folder.
    function _navigateIntoFolder(path: string): void {
        if (path === "")
            return;
        root._beginFolderNavigationTiming("forward");
        Browse.GamesState.push_level(path, "");
        root.gamesCoverRevealReady = false;
        // Cut the accept flash before the model swaps content. This stays on
        // screenGames — active never toggles false, so screenSettling cannot
        // catch it — and a same-index selection (row 0 in both the old and
        // new folder) means the row's `active` binding never changes either,
        // so nothing else clears an in-flight flash. Without this, a fast
        // (cached) folder load can land while the flash's PauseAnimation is
        // still mid-flight, painting the new folder's row inverted.
        if (root.gamesScreen !== null)
            root.gamesScreen.releaseActivate();
        Browse.GamesModel.set_path(path);
    }

    function _rebrowseGamesFolderTarget(path: string, systemId: string): void {
        root.gamesCoverRevealReady = false;
        if (path === "") {
            if (systemId !== "")
                Browse.GamesModel.set_system(systemId);
        } else {
            Browse.GamesModel.set_path(path);
        }
    }

    // Folder pop-up inside the games screen. Cached parents take the same direct
    // path as folder drill-down, so Back does not show fake global Loading for a
    // browse result we can seed synchronously. Cold parents keep the delayed cue
    // before rebrowse so the UI does not look dead if the reset/RPC stalls.
    function _navigateOutOfFolder(): void {
        const stack = Browse.GamesState.path_stack;
        if (stack.length <= 1)
            return;
        root._beginFolderNavigationTiming("back");
        Browse.GamesState.pop_level();
        const newStack = Browse.GamesState.path_stack;
        const target = newStack[newStack.length - 1];
        const systemId = target === "" ? Browse.GamesState.system_id : "";
        root._pendingFolderBackTargetPath = "";
        root._pendingFolderBackSystemId = "";
        folderBackTransitionTimer.stop();
        if (Browse.GamesModel.browse_cached_for_path(target)) {
            root._rebrowseGamesFolderTarget(target, systemId);
            root._resetIdle();
            return;
        }
        root.pendingTransition = "folder_back";
        root._pendingFolderBackTargetPath = target;
        root._pendingFolderBackSystemId = systemId;
        folderBackTransitionTimer.restart();
    }

    function _completeFolderBackTransition(): void {
        const target = root._pendingFolderBackTargetPath;
        const systemId = root._pendingFolderBackSystemId;
        root._pendingFolderBackTargetPath = "";
        root._pendingFolderBackSystemId = "";
        if (root.pendingTransition !== "folder_back")
            return;
        root._folderBackReadyCallback = root._finishFolderBackTransition;
        root._rebrowseGamesFolderTarget(target, systemId);
        root._completeFolderBackRebrowseIfReady();
    }

    function _finishFolderBackTransition(): void {
        if (root.pendingTransition !== "folder_back")
            return;
        root.pendingTransition = "";
        root._resetIdle();
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
        const restoredScreen = root._startupRestoreScreen;
        root._startupTrace("startup/qml finishStartupRestore", "target=" + restoredScreen, "activeScreen=" + root.activeScreen);
        startupRestoreKickTimer.stop();
        root._startupRestorePending = false;
        root._startupRestoreStarted = false;
        root._startupRestoreScreen = "";
        root.startupRestoreCurtainVisible = false;
        if (restoredScreen === "" || restoredScreen === root.screenHub)
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
        if (!root._firstFrameSeen)
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
            const restoredSystemId = Browse.Settings.current_favorites_grouping === "system" ? Browse.FavoriteSystemsState.selected_path : "";
            root._setFavoritesSystem(restoredSystemId);
            root._whenScreenReady(root.screenFavorites, function () {
                root._resumeFavoritesCovers();
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
        if (targetScreen === root.screenFavoriteSystems) {
            root._whenScreenReady(root.screenFavoriteSystems, function () {
                if (Browse.FavoriteSystemsModel.loading) {
                    root._favoriteSystemsReadyCallback = function () {
                        root.favoriteSystemsScreen.restoreSelection();
                        root._finishStartupRestore();
                        root._goto(root.screenFavoriteSystems);
                    };
                } else {
                    root.favoriteSystemsScreen.restoreSelection();
                    root._finishStartupRestore();
                    root._goto(root.screenFavoriteSystems);
                }
            });
            return;
        }
        if (targetScreen === root.screenRecents) {
            root._whenScreenReady(root.screenRecents, function () {
                Browse.RecentsModel.ensure_loaded();
                root._resumeRecentsCovers();
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
            root.hubScreen.restoreFromCategoriesReset(true);
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
    onFavoriteSystemsScreenChanged: root._flushScreenReady(root.screenFavoriteSystems)
    onRecentsScreenChanged: root._flushScreenReady(root.screenRecents)
    onSettingsScreenChanged: root._flushScreenReady(root.screenSettings)
    onAboutScreenChanged: root._flushScreenReady(root.screenAbout)

    Connections {
        target: root.hubScreen
        // Round 6 collapses Hub's five destination signals
        // (requestAccept(category) + one requestXScreen per action) into
        // one requestAccept(kind, id, system) — see CLAUDE.md -> "Screens
        // and routing": "forward = signal + payload, router decides
        // destination". Hub's forward target is heterogeneous (a category
        // or one of a handful of actions), so kind + id is the payload
        // rather than a bare string.
        function onRequestAccept(kind: string, id: string, system: string): void {
            if (kind === "category") {
                root._navigateFromHub(id);
                return;
            }
            if (kind === "action") {
                if (id === "resume") {
                    root._navigateFromHub("resume");
                } else if (id === "favorites") {
                    if (Browse.Settings.current_favorites_grouping === "system")
                        root._navigateToFavoriteSystems();
                    else
                        root._navigateToFavorites("");
                } else if (id === "recents") {
                    root._navigateToRecents();
                } else if (id === "update") {
                    root._navigateToUpdate();
                } else if (id === "settings") {
                    root._navigateToSettings();
                }
                return;
            }
            if (kind === "zapscript") {
                // No screen change -- Core handles the launch/core-swap
                // externally, same as every other launch invokable
                // (SystemsModel.launch_at, GamesModel.launch_at, ...);
                // the Hub just stays put and lets it happen.
                Browse.HubLayout.run_script(id);
                // Settle the push-in cue back to rest -- see
                // HubScreen.releaseActivate()'s doc comment. Every other
                // accept kind here navigates away and gets this for free
                // via the screen's own settling reset.
                if (root.hubScreen !== null)
                    root.hubScreen.releaseActivate();
                return;
            }
            // `system` and `folder` shortcuts land the user on Games having
            // skipped Systems entirely, the same shape as the
            // MiSTer-Arcade-singleton bypass just above
            // (onRequestSystemsScreen). Both now persist
            // `GamesState.entered_from_hub` (set `true` here, `false` from
            // SystemsScreen's own accept handler below) so B/Cancel from a
            // shortcut-entered Games screen returns to Hub instead of
            // Systems — a screen the user never visited on this path. This
            // used to be left as Systems unconditionally, with "Back to
            // Hub" only reachable via the unconditional View-menu entry
            // (openPageMenu / openFavoritesPageMenu); that entry is
            // unaffected by this change, just no longer the only way back.
            if (kind === "system") {
                // A `system` shortcut can point at a launch-only (virtual)
                // system -- "Add to Hub" doesn't exclude those, since the
                // shortcut is still useful as a one-press launcher. Apply
                // the same guard SystemsScreen's own accept handler uses
                // below: launch it directly and stay put, never drill into
                // an empty games browse.
                if (Browse.SystemsModel.is_launchable_system(id)) {
                    Browse.SystemsModel.launch_system_id(id);
                    // Settle the push-in cue back to rest -- same "stays
                    // put" case as the zapscript branch above.
                    if (root.hubScreen !== null)
                        root.hubScreen.releaseActivate();
                    return;
                }
                root._navigateFromSystems(id, true);
                return;
            }
            if (kind === "folder") {
                root._navigateFromHubFolder(system, id);
                return;
            }
        }
        function onRequestRetry(): void {
            Browse.CategoriesModel.refresh();
        }
        // HubScreen emits the category id, not an index -- once the
        // layout can freely interleave categories with everything else, a
        // flat position no longer has any fixed relationship to a
        // CategoriesModel index the way it did when categories always
        // occupied a contiguous prefix. Resolve here so
        // openContextMenu/buildContextMenuEntries/handleContextMenuAccepted
        // (shared by every owner) stay untouched, still index-based.
        function onRequestContextMenu(hubIndex: int, categoryId: string, anchorRect, anchorRadius: int): void {
            const index = Browse.CategoriesModel.index_for_category(categoryId);
            if (index < 0)
                return;
            root._hubItemIndex = hubIndex;
            root.openContextMenu("categories", index, anchorRect, anchorRadius);
        }
        function onRequestActionContextMenu(hubIndex: int, actionId: string, anchorRect): void {
            root.openHubActionContextMenu(hubIndex, actionId, anchorRect);
        }
        function onRequestItemContextMenu(hubIndex: int, kind: string, anchorRect, anchorRadius: int): void {
            root.openHubItemContextMenu(hubIndex, kind, anchorRect, anchorRadius);
        }
        function onRequestPageMenu(): void {
            root.openHubPageMenu();
        }
    }
    Connections {
        target: root.favoritesScreen
        function onRequestHubScreen(): void {
            if (root.favoritesSystemId !== "" && Browse.Settings.current_favorites_grouping === "system")
                root._navigateBackToScreen(root.screenFavoriteSystems);
            else
                root._navigateBackToScreen(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect, anchorRadius: int): void {
            root.openContextMenu("favorites", index, anchorRect, anchorRadius);
        }
        function onRequestPageMenu(): void {
            root.openFavoritesPageMenu();
        }
    }
    Connections {
        target: root.favoriteSystemsScreen
        function onRequestAccept(systemId: string): void {
            if (systemId !== "")
                root._navigateToFavorites(systemId);
        }
        function onRequestHubScreen(): void {
            root._navigateBackToScreen(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect, anchorRadius: int): void {
            root.openContextMenu("favorite_systems", index, anchorRect, anchorRadius);
        }
        function onRequestPageMenu(): void {
            root.openFavoriteSystemsPageMenu();
        }
    }
    Connections {
        target: root.recentsScreen
        function onRequestHubScreen(): void {
            root._navigateBackToScreen(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect, anchorRadius: int): void {
            root.openContextMenu("recents", index, anchorRect, anchorRadius);
        }
    }
    Connections {
        target: root.updateScreen
        function onRequestHubScreen(): void {
            root._goto(root.screenHub);
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
            else if (actionId === "runScraper")
                root.openScrapeSetupModal(root._systemScopeAll);
            else if (actionId === "updateMediaDb")
                root.openIndexSetupModal();
            else if (actionId === "aboutLicense")
                root._navigateToAbout();
            else if (actionId === "documentation")
                root.openDocumentationQrModal();
            else if (actionId === "crtEnable" || actionId === "crtDisable")
                root.stageCrtToggle(actionId === "crtEnable");
            else if (actionId === "debugLoggingEnable" || actionId === "debugLoggingDisable")
                root.stageDebugLoggingToggle(actionId === "debugLoggingEnable");
            else if (actionId === "crtCalibration")
                root.openCrtCalibrationModal();
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
                Browse.SystemsModel.retry();
                return;
            }
            // Launch-only (virtual) systems carry a zapScript and have no
            // browsable media. Run them directly and stay on the systems
            // grid; never route into an empty games browse.
            if (Browse.SystemsModel.is_launchable_system(systemId)) {
                Browse.SystemsModel.launch_system_id(systemId);
                return;
            }
            // Normal Systems-originated drill-down — Back returns to
            // Systems, not Hub. See `_navigateFromSystems`'s own doc
            // comment.
            root._navigateFromSystems(systemId, false);
        }
        function onRequestHubScreen(): void {
            root._navigateBackToScreen(root.screenHub);
        }
        function onRequestContextMenu(index: int, anchorRect, anchorRadius: int): void {
            root.openContextMenu("systems", index, anchorRect, anchorRadius);
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
                root._navigateBackToScreen(root.screenHub);
                return;
            }
            // Separate, additive check — NOT part of the Arcade live eval
            // above, and does not change it. Covers the Hub `system`/
            // `folder` shortcut case (see `_navigateFromSystems`'s and
            // `_navigateFromHubFolder`'s own doc comments): unlike the
            // Arcade case, there's no live-derivable signal for "was this
            // Games screen entered from Hub" — `system_id` looks
            // identical either way — so it needs the persisted
            // `GamesState.entered_from_hub` breadcrumb instead. Cleared
            // here once consumed, so a later Systems-originated entry
            // into a different system starts clean.
            if (Browse.GamesState.entered_from_hub) {
                Browse.GamesState.set_entered_from_hub(false);
                root._navigateBackToScreen(root.screenHub);
                return;
            }
            root._navigateBackToScreen(root.screenSystems);
        }
        function onRequestNavigateIntoFolder(path: string): void {
            root._navigateIntoFolder(path);
        }
        function onRequestNavigateOutOfFolder(): void {
            root._navigateOutOfFolder();
        }
        function onRequestContextMenu(index: int, anchorRect, anchorRadius: int): void {
            root.openContextMenu("games", index, anchorRect, anchorRadius);
        }
        function onRequestPageMenu(): void {
            root.openPageMenu();
        }
    }

    onActiveCardWritePendingChanged: root.handleCardWriteStatus()
    onActiveCardWriteErrorChanged: root.handleCardWriteStatus()
    onCancelCardWriteRequested: root.cancelCardWrite()
    onCloseGameInfoRequested: root.closeGameInfoModal()
    onCloseQrCodeRequested: root.closeQrCodeModal()
    onActionErrorAccepted: root.closeActionErrorModal(true)
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
                root._replaceContextMenuEntryLabel("discover_loading", qsTr("No alternates found"), "discover_unavailable");
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
    // Relabels the still-open games context menu's "Change launcher" entry
    // once the one-row fetch `openContextMenu` kicked off resolves --
    // Browse.GameLauncherOverride's own sequence ticket already discards a
    // late response for a superseded game, so this only needs to confirm
    // the menu is still showing a game's entries before relabeling.
    Connections {
        target: Browse.GameLauncherOverride
        function onLoadingChanged(): void {
            if (Browse.GameLauncherOverride.loading)
                return;
            if (root._gameLauncherPickerPending) {
                root._gameLauncherPickerPending = false;
                root._openGameLauncherPicker(root._gameLauncherPickerSystemId, root._gameLauncherPickerPath);
            }
            if (!root.contextMenuVisible || root.contextMenuMode !== "main" || root.contextMenuOwner !== "games")
                return;
            const currentGameLauncher = Browse.GameLauncherOverride.current_override;
            root._replaceContextMenuEntryLabel("change_launcher", currentGameLauncher !== "" ? qsTr("Change launcher: %1").arg(currentGameLauncher) : qsTr("Change launcher"), "change_launcher");
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
    // Entry order follows docs/content-style.md's menu-ordering rule:
    // primary action first, then frequent actions, then organizational
    // actions (Move/Add to Hub/Hide), then long-running maintenance
    // (index/scrape) last.
    function buildContextMenuEntries(owner: string, entryType: string, mediaCapable: bool, hasNfc: bool, isFavorite: bool, systemId: string, isHidden: bool, category: string) {
        if (owner === "systems") {
            const entries = [
                {
                    id: "launch_system",
                    label: qsTr("Launch system")
                }
            ];
            // Launch-only (virtual) systems have no media and no launcher
            // choice, so omit launcher/index/scrape actions for them.
            const launchable = Browse.SystemsModel.is_launchable_system(systemId);
            if (!launchable)
                entries.push({
                    id: "launch_random_system",
                    label: qsTr("Random game")
                });
            if (!launchable && !Browse.SystemLaunchers.loading && Browse.SystemLaunchers.error_message === "" && Browse.SystemLaunchers.launcher_count_for_system(systemId) > 0) {
                const currentSystemLauncher = Browse.SystemLaunchers.current_launcher_for_system(systemId);
                entries.push({
                    id: "change_launcher",
                    label: currentSystemLauncher !== "" ? qsTr("Change launcher: %1").arg(currentSystemLauncher) : qsTr("Change launcher")
                });
            }
            entries.push({
                id: "add_to_hub",
                label: qsTr("Add to Hub")
            });
            entries.push({
                id: "toggle_hide_system",
                label: isHidden ? qsTr("Unhide") : qsTr("Hide")
            });
            const mediaBusy = Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping;
            if (!launchable && !mediaBusy) {
                entries.push({
                    id: "index_system",
                    label: qsTr("Update media database")
                }, {
                    id: "scrape_system",
                    label: qsTr("Get metadata")
                });
            }
            return entries;
        }
        if (owner === "categories") {
            // Hide/unhide retired for Hub categories -- the Hub is a
            // persisted layout now (Browse.HubLayout); removing a category
            // tile is a layout edit, not a hide flag. See
            // docs/plans/ui-geometry-refresh.md's Hub roadmap. The systems
            // grid's own hide/unhide (owner === "systems", above) is
            // unrelated and unchanged.
            //
            // Move/Hide (the organizational actions) are spliced in by
            // openContextMenu, between this list's primary entry and its
            // maintenance entries -- see the comment there.
            const mediaBusy = Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping;
            const entries = [];
            // Index/scrape act on the category's indexable systems, which
            // excludes launch-only ones. Show the actions only when the
            // category has at least one indexable system; a category whose
            // members are all launch-only yields none and the actions would
            // no-op, so omit them rather than show dead entries.
            const hasIndexable = category !== "" && Browse.SystemsModel.system_ids_for_category(category).length > 0;
            if (hasIndexable)
                entries.push({
                    id: "launch_random_category",
                    label: qsTr("Random game")
                });
            if (!mediaBusy && hasIndexable) {
                entries.push({
                    id: "index_category",
                    label: qsTr("Update media database")
                }, {
                    id: "scrape_category",
                    label: qsTr("Get metadata")
                });
            }
            return entries;
        }
        if (owner === "favorite_systems") {
            return [
                {
                    id: "launch_random_favorite_system",
                    label: qsTr("Random game")
                }
            ];
        }
        if (owner === "hub_favorites") {
            return [
                {
                    id: "launch_random_favorite",
                    label: qsTr("Random game")
                }
            ];
        }
        if (owner === "hub_action" || owner === "hub_item") {
            // No kind-specific entries of their own -- callers append the
            // universal Move/Remove (see `_hubMoveRemoveEntries`).
            return [];
        }
        if (owner === "recents") {
            // No favorite-toggle here -- see the module doc comment on
            // rust/frontend/src/models/recents.rs for why (Core's
            // media.history carries no tags).
            const entries = [
                {
                    id: "launch_game",
                    label: qsTr("Launch game")
                },
                {
                    id: "more_info",
                    label: qsTr("Details")
                }
            ];
            if (hasNfc)
                entries.push({
                    id: "write_card",
                    label: qsTr("Write to NFC token")
                });
            entries.push({
                id: "qr_code",
                label: qsTr("Write with App")
            });
            // Discover alt. versions only ever works for the literal
            // MiSTer Arcade/MRA system -- alternate_versions.rs matches
            // `system_id == "Arcade"` exactly, not the 32-system "Arcade"
            // category Core reports. See docs/style.md or the round-10
            // plan for the full reasoning.
            if (systemId === CategoryIds.arcadeId)
                entries.push({
                    id: "discover",
                    label: qsTr("Discover alt. versions")
                });
            entries.push({
                id: "add_to_hub",
                label: qsTr("Add to Hub")
            });
            // Maintenance last, per docs/content-style.md's ordering rule.
            // A coverless game is the moment a user goes looking for
            // artwork, and this is the menu they open to look -- see the
            // scrape_game dispatch for why it scopes to the system.
            if (!(Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping))
                entries.push({
                    id: "scrape_game",
                    label: qsTr("Get metadata")
                });
            return entries;
        }
        if (owner === "games" || owner === "favorites") {
            if (entryType === "root" && !mediaCapable)
                return [];
            if (entryType === "directory" && !mediaCapable) {
                // A plain browsable folder has no media-scoped actions of
                // its own, and Favorites never contains a directory row (it
                // is a flat list of games) -- only Games can reach this
                // branch, so the empty-array side never actually executes.
                // Left as an explicit owner check rather than an
                // unconditional entry so a directory row on a future
                // Favorites-like owner doesn't silently inherit a folder
                // shortcut action.
                return owner === "games" ? [
                    {
                        id: "add_to_hub",
                        label: qsTr("Add to Hub")
                    }
                ] : [];
            }
            const entries = [
                {
                    id: "launch_game",
                    label: qsTr("Launch game")
                },
                {
                    id: "more_info",
                    label: qsTr("Details")
                },
                {
                    id: "toggle_favorite",
                    label: isFavorite ? qsTr("Remove from favorites") : qsTr("Add to favorites")
                }
            ];
            // Per-media launcher override -- Games only for now (see
            // docs/content-style.md's "library" scope for this feature).
            // Gated the same way as the system-level entry: launchers must
            // be loaded and the game's system must have more than one to
            // choose between. The label itself (with the current override
            // name, if any) is filled in asynchronously once the context
            // menu opens -- see openContextMenu's `prepare_game` call and
            // the Browse.GameLauncherOverride Connections block below.
            if (owner === "games" && !Browse.SystemLaunchers.loading && Browse.SystemLaunchers.error_message === "" && Browse.SystemLaunchers.launcher_count_for_system(systemId) > 0)
                entries.push({
                    id: "change_launcher",
                    label: qsTr("Change launcher")
                });
            if (hasNfc)
                entries.push({
                    id: "write_card",
                    label: qsTr("Write to NFC token")
                });
            entries.push({
                id: "qr_code",
                label: qsTr("Write with App")
            });
            // See the "recents" branch above for why this is gated on the
            // literal Arcade system rather than shown unconditionally.
            if (systemId === CategoryIds.arcadeId)
                entries.push({
                    id: "discover",
                    label: qsTr("Discover alt. versions")
                });
            entries.push({
                id: "add_to_hub",
                label: qsTr("Add to Hub")
            });
            // See the "recents" branch above.
            if (!(Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping))
                entries.push({
                    id: "scrape_game",
                    label: qsTr("Get metadata")
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

    // Shared by the system- and game-scoped "Change launcher" pickers --
    // both Browse.SystemLaunchers and Browse.GameLauncherOverride publish
    // parallel picker_ids/picker_labels lists built by the same Rust
    // `picker_entries_for_system` helper: "__default__" first (labelled
    // "Default"), then each launcher id, then an optional synthetic
    // "Current: <id>" row when the current value isn't in the launcher
    // list. This just localizes those two special-cased labels.
    function _launcherPickerEntries(ids: var, labels: var): var {
        const entries = [];
        for (let i = 0; i < ids.length; i++) {
            const launcherId = ids[i];
            const label = labels[i];
            entries.push({
                id: launcherId,
                label: launcherId === "__default__" ? qsTr("Default") : (label.indexOf("Current: ") === 0 ? qsTr("Current: %1").arg(launcherId) : label)
            });
        }
        return entries;
    }

    // Opens the per-game launcher picker from Browse.GameLauncherOverride's
    // current picker_ids/picker_labels/current_override. Callers must have
    // already confirmed `!Browse.GameLauncherOverride.loading` -- see
    // handleContextMenuAccepted's games "change_launcher" branch (opens
    // immediately) and the deferred-open Connections block below (opens
    // once a still-in-flight prepare_game resolves).
    function _openGameLauncherPicker(gameSystemId: string, gamePath: string): void {
        const entries = root._launcherPickerEntries(Browse.GameLauncherOverride.picker_ids, Browse.GameLauncherOverride.picker_labels);
        if (entries.length > 0)
            root.openListPickerModal(qsTr("Change launcher"), entries, Browse.GameLauncherOverride.current_override === "" ? "__default__" : Browse.GameLauncherOverride.current_override, "game_launcher:" + gameSystemId + "\n" + gamePath);
    }

    // Relabels one row of a ListPickerModal entries array in place, id
    // unchanged. Used to show "Saving…" on just the row the user picked
    // while a launcher save is in flight -- every other row stays exactly
    // as it was, matching the picker's normal appearance rather than
    // collapsing to a single placeholder row. The lock in handleAction's
    // modalListPicker branch (grep "system_launcher_pending") is what
    // actually stops Up/Down from moving focus off this row, or Accept/
    // Cancel from doing anything, while pending.
    function _relabelPickerEntry(entries: var, targetId: string, nextLabel: string): var {
        return entries.map(entry => entry.id === targetId ? {
            id: entry.id,
            label: nextLabel
        } : entry);
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
                    label: qsTr("Discover alt. versions")
                });
            } else {
                entries.push(entry);
            }
        }
        return entries;
    }

    // Universal Move/Hide-or-Delete, appended to every Hub-owned menu below
    // — empty (no menu) for an entry with no real `Browse.HubLayout`
    // backing yet (the bootstrap placeholder window; see HubScreen.qml's
    // `_blankEntry` doc comment). `kind` is never `"empty"` here — a blank
    // tile never reaches this function; see `openHubItemContextMenu`'s
    // guard. The remove label depends on `kind`: category/action stay
    // tracked in `known` (see hub_layout.rs) and come straight back via
    // View -> Add item…, so "Hide" is accurate — nothing is lost.
    // system/folder/zapscript aren't tracked at all, so removing one
    // really is permanent; "Delete" says so rather than implying a
    // reversibility that isn't there.
    function _hubMoveRemoveEntries(hubIndex: int, kind: string): var {
        if (hubIndex < 0)
            return [];
        const removeLabel = kind === "category" || kind === "action" ? qsTr("Hide") : qsTr("Delete");
        return [
            {
                id: "hub_move",
                label: qsTr("Move")
            },
            {
                id: "hub_remove",
                label: removeLabel
            }
        ];
    }

    function openHubActionContextMenu(hubIndex: int, actionId: string, anchorRect): void {
        const owner = actionId === "favorites" ? "hub_favorites" : "hub_action";
        const entries = root.buildContextMenuEntries(owner, "", false, false, false, "", false, "").concat(root._hubMoveRemoveEntries(hubIndex, "action"));
        if (entries.length === 0)
            return;
        root._hubItemIndex = hubIndex;
        root.contextMenuEntries = entries;
        root.contextMenuOwner = owner;
        root.contextMenuIndex = 0;
        root.contextMenuMode = "main";
        root.contextMenuAnchor = anchorRect;
        root.contextMenuAnchorRadius = 0;
        root._requestModal(root.modalContextMenu);
        root.contextMenuVisible = true;
        if (ScreenManager.topModal !== root.modalContextMenu)
            ScreenManager.pushModal(root.modalContextMenu);
    }

    // system / folder / zapscript — none of these have a kind-specific
    // menu, only the universal Move/Hide-or-Delete. `HubScreen.qml`'s
    // dispatch already never emits the signal that reaches here for a
    // blank tile (`kind === "empty"`) — a gap is an implementation
    // detail, not something to open Options on — but guard it here too
    // rather than trust a single call site.
    function openHubItemContextMenu(hubIndex: int, kind: string, anchorRect, anchorRadius: int): void {
        if (kind === "empty")
            return;
        const entries = root._hubMoveRemoveEntries(hubIndex, kind);
        if (entries.length === 0)
            return;
        root._hubItemIndex = hubIndex;
        root.contextMenuEntries = entries;
        root.contextMenuOwner = "hub_item";
        root.contextMenuIndex = 0;
        root.contextMenuMode = "main";
        root.contextMenuAnchor = anchorRect;
        root.contextMenuAnchorRadius = anchorRadius;
        root._requestModal(root.modalContextMenu);
        root.contextMenuVisible = true;
        if (ScreenManager.topModal !== root.modalContextMenu)
            ScreenManager.pushModal(root.modalContextMenu);
    }

    function openContextMenu(owner: string, index: int, anchorRect, anchorRadius: int): void {
        if (index < 0)
            return;
        let entryType = "";
        let isFavorite = false;
        let systemId = "";
        let mediaCapable = false;
        let isHidden = false;
        let category = "";
        if (owner === "systems") {
            if (index >= Browse.SystemsModel.count)
                return;
            systemId = Browse.SystemsModel.system_id_at(index);
            isHidden = Browse.SystemsModel.is_hidden_at(index);
        } else if (owner === "categories") {
            if (index >= Browse.CategoriesModel.count)
                return;
            category = Browse.CategoriesModel.category_at(index);
            isHidden = Browse.CategoriesModel.is_hidden_at(index);
        } else if (owner === "games") {
            if (index >= Browse.GamesModel.count)
                return;
            entryType = Browse.GamesModel.entry_type_at(index);
            mediaCapable = Browse.GamesModel.is_media_capable_at(index);
            isFavorite = Browse.GamesModel.is_favorite_at(index);
            systemId = Browse.GamesModel.system_id_at(index);
        } else if (owner === "favorites") {
            if (index >= Browse.FavoritesModel.count)
                return;
            mediaCapable = true;
            isFavorite = Browse.FavoritesModel.is_favorite_at(index);
            systemId = Browse.FavoritesModel.system_id_at(index);
        } else if (owner === "favorite_systems") {
            if (index >= Browse.FavoriteSystemsModel.count)
                return;
            systemId = Browse.FavoriteSystemsModel.system_id_at(index);
        } else if (owner === "recents") {
            if (index >= Browse.RecentsModel.count)
                return;
            systemId = Browse.RecentsModel.system_id_at(index);
        }
        let entries = root.buildContextMenuEntries(owner, entryType, mediaCapable, Browse.SystemStatus.has_nfc, isFavorite, systemId, isHidden, category);
        // "categories" is exclusively Hub-owned (only HubScreen ever opens
        // it) — splice the universal Move/Hide in here, keyed off the Hub
        // flat index the caller stashed in `_hubItemIndex` (NOT `index`,
        // which is the CategoriesModel index the entries above just used).
        // Per docs/content-style.md's ordering rule these organizational
        // actions land right after the primary "Random game" entry (if
        // present) and before the maintenance entries, not tacked onto the
        // end of the whole list.
        if (owner === "categories") {
            const insertAt = entries.length > 0 && entries[0].id === "launch_random_category" ? 1 : 0;
            const moveRemove = root._hubMoveRemoveEntries(root._hubItemIndex, "category");
            entries = entries.slice(0, insertAt).concat(moveRemove, entries.slice(insertAt));
        }
        if (entries.length === 0)
            return;
        root.contextMenuEntries = entries;
        root.contextMenuOwner = owner;
        root.contextMenuIndex = index;
        root.contextMenuMode = "main";
        root._discoverParentEntries = [];
        root._discoverMenuPending = false;
        root.contextMenuAnchor = anchorRect;
        root.contextMenuAnchorRadius = anchorRadius;
        root._requestModal(root.modalContextMenu);
        root.contextMenuVisible = true;
        if (ScreenManager.topModal !== root.modalContextMenu)
            ScreenManager.pushModal(root.modalContextMenu);
        // Games' "Change launcher" entry starts with a plain label; prime
        // the one-row fetch here so Browse.GameLauncherOverride.current_override
        // is ready to relabel it in place (see the Connections block below)
        // by the time the menu is likely to be looked at. Checking the
        // built `entries` (rather than re-deriving the gating condition)
        // keeps this in lockstep with buildContextMenuEntries' own gate.
        // Must run after contextMenuVisible flips true: a warm cache
        // resolves synchronously inside prepare_game, and the relabel
        // Connections below only acts while contextMenuVisible is true --
        // firing this earlier silently dropped the relabel on every
        // reopen for a game whose override was already cached (e.g. right
        // after saving one).
        if (owner === "games" && entries.some(entry => entry.id === "change_launcher")) {
            const gamePath = Browse.GamesModel.path_at(index);
            if (gamePath !== "") {
                // A pending deferred-open from a previous game's still-in-flight
                // prepare_game (see the Browse.GameLauncherOverride Connections
                // block) is now stale -- this prepare_game call supersedes it,
                // same as the model's own sequence ticket does internally.
                root._gameLauncherPickerPending = false;
                Browse.GameLauncherOverride.prepare_game(systemId, gamePath);
            }
        }
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
        root._hubItemIndex = -1;
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
        const hubItemIndex = root._hubItemIndex;
        if (targetIndex < 0)
            return;
        if (id === "hub_move") {
            root.closeContextMenu();
            if (hubItemIndex >= 0 && root.hubScreen !== null)
                root.hubScreen.beginMove(hubItemIndex);
            return;
        }
        if (id === "hub_remove") {
            root.closeContextMenu();
            if (hubItemIndex >= 0)
                Browse.HubLayout.remove_item(hubItemIndex);
            return;
        }
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
            root._replaceContextMenuEntryLabel("discover", qsTr("Searching…"), "discover_loading");
            Browse.AlternateVersions.discover_for(systemId, name, path);
            return;
        }
        root.closeContextMenu();
        if (id === "change_launcher") {
            if (owner === "games") {
                const gameSystemId = Browse.GamesModel.system_id_at(targetIndex);
                const gamePath = Browse.GamesModel.path_at(targetIndex);
                if (gameSystemId === "" || gamePath === "")
                    return;
                if (Browse.GameLauncherOverride.loading) {
                    // openContextMenu's prepare_game call hasn't resolved
                    // yet (a fast press, or Core busy elsewhere) --
                    // picker_ids/picker_labels would still be empty. Wait
                    // for it instead of opening nothing; the
                    // Browse.GameLauncherOverride Connections below opens
                    // the picker once loading clears.
                    root._gameLauncherPickerPending = true;
                    root._gameLauncherPickerSystemId = gameSystemId;
                    root._gameLauncherPickerPath = gamePath;
                    return;
                }
                root._openGameLauncherPicker(gameSystemId, gamePath);
                return;
            }
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId === "")
                return;
            Browse.SystemLaunchers.prepare_system(systemId);
            const entries = root._launcherPickerEntries(Browse.SystemLaunchers.picker_ids, Browse.SystemLaunchers.picker_labels);
            if (entries.length > 0)
                root.openListPickerModal(qsTr("Change launcher"), entries, Browse.SystemLaunchers.current_launcher, "system_launcher:" + systemId);
        } else if (id.startsWith("alternate_version:")) {
            const altIndex = Number(id.slice("alternate_version:".length));
            if (!Number.isNaN(altIndex))
                Browse.AlternateVersions.launch_at(altIndex);
        } else if (id === "launch_system") {
            Browse.SystemsModel.launch_at(targetIndex);
        } else if (id === "launch_random_system") {
            Browse.SystemsModel.launch_random_at(targetIndex);
        } else if (id === "launch_random_category") {
            const categoryName = Browse.CategoriesModel.category_at(targetIndex);
            if (categoryName !== "") {
                const systemIds = Browse.SystemsModel.system_ids_for_category(categoryName);
                Browse.SystemsModel.launch_random_systems(systemIds);
            }
        } else if (id === "launch_random_favorite") {
            Browse.FavoritesModel.launch_random();
        } else if (id === "launch_random_favorite_system") {
            const systemId = Browse.FavoriteSystemsModel.system_id_at(targetIndex);
            if (systemId !== "")
                Browse.FavoritesModel.launch_random_for_system(systemId);
        } else if (id === "index_system") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId !== "")
                Browse.MediaStatus.start_index_for_system(systemId);
        } else if (id === "scrape_system") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId !== "")
                root.openScrapeSetupModal(systemId);
        } else if (id === "toggle_hide_system") {
            const systemId = Browse.SystemsModel.system_id_at(targetIndex);
            if (systemId !== "") {
                if (Browse.SystemsState.is_system_hidden(systemId))
                    Browse.SystemsState.unhide_system(systemId);
                else
                    Browse.SystemsState.hide_system(systemId);
                Browse.SystemsModel.reproject();
            }
        } else if (id === "index_category") {
            const categoryName = Browse.CategoriesModel.category_at(targetIndex);
            if (categoryName !== "") {
                const systemIds = Browse.SystemsModel.system_ids_for_category(categoryName);
                if (systemIds.length > 0)
                    Browse.MediaStatus.start_index_for_systems(systemIds);
            }
        } else if (id === "scrape_category") {
            const categoryName = Browse.CategoriesModel.category_at(targetIndex);
            if (categoryName !== "")
                root.openScrapeSetupModal("cat:" + categoryName);
        } else if (id === "scrape_game") {
            // Core scrapes by system, not by game (MediaScrapeParams is
            // `{ scraperId, systems, force }`), so the best we can scope
            // to today is the game's own system. Routing through the
            // setup modal means that wider scope is visible in the
            // Systems row instead of happening silently. Narrows to the
            // game once Core grows a media/path filter.
            const gameSystemId = owner === "favorites" ? Browse.FavoritesModel.system_id_at(targetIndex) : (owner === "recents" ? Browse.RecentsModel.system_id_at(targetIndex) : Browse.GamesModel.system_id_at(targetIndex));
            root.openScrapeSetupModal(gameSystemId !== "" ? gameSystemId : root._systemScopeAll);
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
                root.beginCardWrite("systems", targetIndex);
                Browse.SystemsModel.write_card_at(targetIndex);
            } else if (owner === "games") {
                root.beginCardWrite("games", targetIndex);
                Browse.GamesModel.write_card_at(targetIndex);
            } else if (owner === "favorites") {
                root.beginCardWrite("favorites", targetIndex);
                Browse.FavoritesModel.write_card_at(targetIndex);
            } else if (owner === "recents") {
                root.beginCardWrite("recents", targetIndex);
                Browse.RecentsModel.write_card_at(targetIndex);
            }
        } else if (id === "qr_code") {
            const text = owner === "systems" ? Browse.SystemsModel.launch_text_at(targetIndex) : owner === "games" ? Browse.GamesModel.launch_text_at(targetIndex) : owner === "favorites" ? Browse.FavoritesModel.launch_text_at(targetIndex) : owner === "recents" ? Browse.RecentsModel.launch_text_at(targetIndex) : "";
            if (text !== "") {
                Browse.QrCode.generate(root._buildQrPayload(text));
                if (Browse.QrCode.size > 0)
                    root.openQrCodeModal(qsTr("Write with App"), qsTr("Scan this code with the Zaparoo App to write this game to a Zaparoo token."), "");
            }
        } else if (id === "discover_unavailable" || id === "discover_loading") {
            return;
        } else if (id === "add_to_hub") {
            root._addToHub(owner, targetIndex);
        }
    }

    // "Add to Hub" — creates a `system`/`folder`/`zapscript` shortcut from a
    // Systems/Games/Favorites/Recents row via `Browse.HubLayout.add_target_item`.
    // `owner === "games"` covers both a plain directory (folder shortcut)
    // and a media row (game shortcut); Favorites/Recents are always a flat
    // list of games, so they only ever create a zapscript shortcut, the
    // same as a Games media row. A game shortcut's `name` is the one place
    // this layout caches a value resolved from Core — a deliberate,
    // user-approved exception to the no-Core-metadata rule (see
    // `zaparoo_core::hub_layout`'s doc comment on `add_target_item`), so the
    // tile has a real title without needing Core reachable to render, and
    // doubles as a rename hook (edit `name` in frontend.toml).
    function _addToHub(owner: string, index: int): void {
        if (owner === "systems") {
            const systemId = Browse.SystemsModel.system_id_at(index);
            if (systemId !== "")
                Browse.HubLayout.add_target_item("system", systemId, "", "", "", "", "");
            return;
        }
        if (owner === "games") {
            const entryType = Browse.GamesModel.entry_type_at(index);
            const systemId = Browse.GamesModel.current_system_id;
            if (entryType === "directory") {
                const path = Browse.GamesModel.path_at(index);
                if (path !== "")
                    Browse.HubLayout.add_target_item("folder", "", path, "", "", "", systemId);
                return;
            }
            const path = Browse.GamesModel.path_at(index);
            const script = Browse.GamesModel.launch_text_at(index);
            if (script === "")
                return;
            const name = Browse.GamesModel.name_at(index);
            Browse.HubLayout.add_target_item("zapscript", "", path, script, name, "", systemId);
            return;
        }
        // Favorites/Recents rows: same accessor shape openGameInfo already
        // uses for these two owners a few lines below, since neither model
        // has a single "current system" the way a Games browse does --
        // each row carries its own system_id_at(index).
        if (owner !== "favorites" && owner !== "recents")
            return;
        const model = owner === "favorites" ? Browse.FavoritesModel : Browse.RecentsModel;
        const systemId = model.system_id_at(index);
        const path = model.path_at(index);
        const script = model.launch_text_at(index);
        if (script === "")
            return;
        const name = model.name_at(index);
        Browse.HubLayout.add_target_item("zapscript", "", path, script, name, "", systemId);
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
        root._gameInfoOwner = owner;
        root._gameInfoIndex = index;
        Browse.GameInfo.load(systemId, path, title);
        root._requestModal(root.modalGameInfo);
        root.gameInfoModalVisible = true;
        if (ScreenManager.topModal !== root.modalGameInfo)
            ScreenManager.pushModal(root.modalGameInfo);
    }

    function closeGameInfoModal(): void {
        root.gameInfoModalVisible = false;
        Browse.GameInfo.clear();
        root._gameInfoOwner = "";
        root._gameInfoIndex = -1;
        if (ScreenManager.topModal === root.modalGameInfo)
            ScreenManager.popModal();
    }

    function handleGameInfoError(): void {
        if (!root.gameInfoModalVisible || (Browse.GameInfo.error_message ?? "") === "")
            return;
        const owner = root._gameInfoOwner;
        const index = root._gameInfoIndex;
        root.closeGameInfoModal();
        root.presentActionError("game_info:" + owner, qsTr("Details unavailable"), qsTr("Could not load details for this item. Check Zaparoo Core and try again."), qsTr("Retry"), function () {
            root.openGameInfo(owner, index);
        });
    }

    Connections {
        target: Browse.GameInfo
        function onError_messageChanged(): void {
            root.handleGameInfoError();
        }
    }

    // Callers own the payload: generate into the shared Browse.QrCode slot
    // and check `size > 0` first, then pass the copy that describes what
    // was generated. `urlText` is the readable fallback for a web
    // destination — a QR is not scannable at 240p over composite, so
    // anywhere the code points at a page the URL has to be legible too.
    function openQrCodeModal(title: string, instruction: string, urlText: string): void {
        root.qrCodeModalTitle = title;
        root.qrCodeModalInstruction = instruction;
        root.qrCodeModalUrlText = urlText;
        root._requestModal(root.modalQrCode);
        root.qrCodeModalVisible = true;
        if (ScreenManager.topModal !== root.modalQrCode)
            ScreenManager.pushModal(root.modalQrCode);
    }

    // Docs pointer from Settings > About. Deep-links the Frontend guide
    // rather than the site root, per the "link where it promises to go"
    // rule; the scrape modal carries its own narrower link to the
    // artwork page.
    readonly property string _docsUrl: "https://zaparoo.org/docs/frontend/"

    function openDocumentationQrModal(): void {
        Browse.QrCode.generate(root._docsUrl);
        if (Browse.QrCode.size > 0)
            root.openQrCodeModal(qsTr("Documentation"), qsTr("Scan this code to open the Zaparoo Frontend guide on your phone."), "zaparoo.org/docs/frontend");
    }

    function closeQrCodeModal(): void {
        root.qrCodeModalVisible = false;
        if (ScreenManager.topModal === root.modalQrCode)
            ScreenManager.popModal();
    }

    // First-run indexing is background work, not a navigation gate. Start once
    // after both media status and the catalog are authoritative and the catalog
    // reports no indexed systems. `indexed_count` deliberately ignores
    // launch-only virtual systems, which can already make Hub non-empty.
    function _shouldStartFirstRunIndex(connectionState: int, mediaStatusSeeded: bool, catalogLoaded: bool, indexedCount: int): bool {
        return !root._firstRunIndexStarted && connectionState === 2 && mediaStatusSeeded && catalogLoaded && indexedCount === 0;
    }

    function _maybeStartFirstRunIndex(): void {
        if (!root._shouldStartFirstRunIndex(Browse.AppStatus.connection_state, Browse.MediaStatus.seeded, Browse.CategoriesModel.loaded, Browse.CategoriesModel.indexed_count))
            return;
        root._firstRunIndexStarted = true;
        if (!Browse.MediaStatus.indexing && !Browse.MediaStatus.optimizing)
            Browse.MediaStatus.start_index();
    }

    function _catalogRefreshScreenActive(): bool {
        return root.activeScreen === root.screenHub || root.activeScreen === root.screenSystems || root.activeScreen === root.screenFavoriteSystems;
    }

    function _refreshCatalogDuringIndex(): void {
        if (root.activeScreen === root.screenFavoriteSystems)
            Browse.FavoriteSystemsModel.retry();
        else
            Browse.CategoriesModel.refresh();
    }

    // Poll only while an index is actively discovering content and only on
    // screens that display category/system membership. Catalog consumers skip
    // no-op model resets, so unchanged polls stay invisible while newly indexed
    // systems still appear progressively. Completion also gets the Store's
    // MEDIA_DB invalidation refetch for correctness.
    Timer {
        id: catalogIndexRefreshTimer

        interval: 5000
        repeat: true
        running: Browse.MediaStatus.indexing && root._catalogRefreshScreenActive()
        onTriggered: root._refreshCatalogDuringIndex()
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
        // Now that the notice is dismissed, advance to the Core-version warning.
        root._maybeOpenCoreVersionWarning();
    }

    // Core-version warning lifecycle. Shown once per session, behind the
    // commercial notice, when Core reports a version older than the
    // frontend's minimum. Warn-only: the OK button dismisses it and the
    // chain advances to the media-DB first-run check. `core_version_checked`
    // gates the open so we don't evaluate before the async `version` fetch
    // has answered; `core_version_supported` defaults true so we never
    // flash the warning pre-check.
    function _maybeOpenCoreVersionWarning(): void {
        if (root._coreVersionWarningShown)
            return;
        if (!Browse.Notice.commercial_ack)
            return;
        if (!Browse.AppStatus.core_version_checked)
            return;
        if (Browse.AppStatus.core_version_supported) {
            root._coreVersionWarningShown = true;
            return;
        }
        root._coreVersionWarningShown = true;
        root._requestModal(root.modalCoreVersion);
        root.coreVersionModalVisible = true;
        if (ScreenManager.topModal !== root.modalCoreVersion)
            ScreenManager.pushModal(root.modalCoreVersion);
    }

    function closeCoreVersionModal(): void {
        root.coreVersionModalVisible = false;
        if (ScreenManager.topModal === root.modalCoreVersion)
            ScreenManager.popModal();
    }

    function _showActionError(entry): void {
        root._requestModal(root.modalActionError);
        root.actionErrorKey = entry.key;
        root.actionErrorTitle = entry.title;
        root.actionErrorBody = entry.body;
        root.actionErrorButtonLabel = entry.buttonLabel;
        root._actionErrorAcceptedCallback = entry.accepted;
        root.randomFailedModalVisible = entry.key === "random";
        root.actionErrorModalVisible = true;
        if (ScreenManager.topModal !== root.modalActionError)
            ScreenManager.pushModal(root.modalActionError);
    }

    function presentActionError(key: string, title: string, body: string, buttonLabel: string, accepted): void {
        if (key === "")
            return;
        if (root.actionErrorModalVisible && root.actionErrorKey === key)
            return;
        for (let i = 0; i < root._actionErrorQueue.length; i++) {
            if (root._actionErrorQueue[i].key === key)
                return;
        }
        const entry = {
            key: key,
            title: title,
            body: body,
            buttonLabel: buttonLabel !== "" ? buttonLabel : qsTr("OK"),
            accepted: accepted
        };
        if (root.actionErrorModalVisible || root._actionErrorQueue.length > 0) {
            root._actionErrorQueue = root._actionErrorQueue.concat([entry]);
            if (!root.actionErrorModalVisible)
                actionErrorQueueTimer.restart();
            return;
        }
        root._showActionError(entry);
    }

    function closeActionErrorModal(runAccepted: bool): void {
        if (!root.actionErrorModalVisible)
            return;
        const accepted = root._actionErrorAcceptedCallback;
        const wasRandom = root.actionErrorKey === "random";
        root.actionErrorModalVisible = false;
        root.randomFailedModalVisible = false;
        root.actionErrorKey = "";
        root.actionErrorTitle = "";
        root.actionErrorBody = "";
        root.actionErrorButtonLabel = qsTr("OK");
        root._actionErrorAcceptedCallback = null;
        if (ScreenManager.topModal === root.modalActionError)
            ScreenManager.popModal();
        if (wasRandom) {
            Browse.GamesModel.clear_random_error();
            Browse.FavoritesModel.clear_random_error();
            Browse.SystemsModel.clear_random_error();
        }
        if (runAccepted && typeof accepted === "function")
            accepted();
        if (root._actionErrorQueue.length > 0)
            actionErrorQueueTimer.restart();
    }

    function _showNextActionError(): void {
        if (root.actionErrorModalVisible || root._actionErrorQueue.length === 0)
            return;
        const entry = root._actionErrorQueue[0];
        root._actionErrorQueue = root._actionErrorQueue.slice(1);
        root._showActionError(entry);
    }

    function openRandomFailedModal(): void {
        root.presentActionError("random", qsTr("Random game"), qsTr("No matching games found."), qsTr("OK"), null);
    }

    function closeRandomFailedModal(): void {
        if (root.actionErrorModalVisible && root.actionErrorKey === "random")
            root.closeActionErrorModal(false);
    }

    function _presentReportedActionError(kind: string, context: string): void {
        let title = qsTr("Action failed");
        let body = qsTr("The action could not be completed. Check Zaparoo Core and try again.");
        if (kind === "launch") {
            title = qsTr("Launch failed");
            body = context !== "" ? qsTr("Could not start %1. Check Zaparoo Core and try again.").arg(context) : qsTr("Could not start this item. Check Zaparoo Core and try again.");
        } else if (kind === "favorite") {
            title = qsTr("Favorite update failed");
            body = qsTr("Could not update this favorite. Check Zaparoo Core and try again.");
        } else if (kind === "media_index") {
            title = qsTr("Media update failed");
            body = qsTr("Could not start the media database update. Check Zaparoo Core and try again.");
        } else if (kind === "media_scrape") {
            title = qsTr("Get metadata failed");
            body = qsTr("Could not start getting metadata. Check Zaparoo Core and try again.");
        } else if (kind === "media_scrapers") {
            title = qsTr("Source list unavailable");
            body = qsTr("Could not load the list of metadata sources. Check Zaparoo Core and try again.");
        } else if (kind === "media_cancel") {
            title = qsTr("Cancel failed");
            body = qsTr("Could not cancel the media operation. Check Zaparoo Core and try again.");
        } else if (kind === "launcher") {
            title = qsTr("Launcher update failed");
            body = qsTr("Could not change the launcher. Check Zaparoo Core and try again.");
        } else if (kind === "alternate_discovery") {
            title = qsTr("Alternate versions unavailable");
            body = qsTr("Could not find alternate versions. Check Zaparoo Core and try again.");
        } else if (kind === "qr_code") {
            title = qsTr("QR code failed");
            body = qsTr("Could not create the QR code for this item.");
        } else if (kind === "setting") {
            title = qsTr("Setting not saved");
            body = qsTr("Could not save this setting. Try again.");
        }
        root.presentActionError(kind + ":" + context, title, body, qsTr("OK"), null);
    }

    Connections {
        target: Browse.ActionError
        function onSequenceChanged(): void {
            const sequences = Browse.ActionError.event_sequences;
            const kinds = Browse.ActionError.event_kinds;
            const contexts = Browse.ActionError.event_contexts;
            for (let i = 0; i < sequences.length; i++) {
                const sequence = Number(sequences[i]);
                if (sequence <= 0 || sequence === root._lastActionErrorSequence)
                    continue;
                root._lastActionErrorSequence = sequence;
                root._presentReportedActionError(kinds[i] ?? "", contexts[i] ?? "");
            }
        }
    }

    Connections {
        target: Browse.GamesModel
        function onRandom_errorChanged(): void {
            if ((Browse.GamesModel.random_error ?? "") !== "")
                root.openRandomFailedModal();
        }
    }

    Connections {
        target: Browse.FavoritesModel
        function onRandom_errorChanged(): void {
            if ((Browse.FavoritesModel.random_error ?? "") !== "")
                root.openRandomFailedModal();
        }
    }

    Connections {
        target: Browse.SystemsModel
        function onRandom_errorChanged(): void {
            if ((Browse.SystemsModel.random_error ?? "") !== "")
                root.openRandomFailedModal();
        }
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

    // "Get metadata" setup modal lifecycle. Every entry point routes here
    // — the Settings > Library row and the system/category/game
    // context-menu entries — so the chosen source, scope and replace flag
    // are always the user's rather than hardcoded. `scope` uses the same
    // "*"/"cat:<Category>"/<system id> sentinel as
    // `_buildSystemScopeEntries`; a context-menu caller passes the thing
    // it was invoked on so the modal opens pre-scoped and the Systems row
    // shows exactly what is about to run. The modal fetches the source
    // list on open via `Browse.MediaStatus.refresh_scrapers()`.
    function openScrapeSetupModal(scope: string): void {
        Browse.MediaStatus.refresh_scrapers();
        // `_requestModal` activates the Loader, and a sourceComponent
        // Loader instantiates synchronously, so the item exists by the
        // next line even on the very first open. Seed the scope before
        // flipping `visible`, since `onOpenChanged` is what reads it.
        root._requestModal(root.modalScrapeSetup);
        if (root.scrapeSetupModal !== null)
            root.scrapeSetupModal.initialSystemScope = scope !== "" ? scope : root._systemScopeAll;
        root.scrapeSetupModalVisible = true;
        if (ScreenManager.topModal !== root.modalScrapeSetup)
            ScreenManager.pushModal(root.modalScrapeSetup);
    }

    function closeScrapeSetupModal(): void {
        root.scrapeSetupModalVisible = false;
        if (ScreenManager.topModal === root.modalScrapeSetup)
            ScreenManager.popModal();
    }

    onCloseScrapeSetupRequested: root.closeScrapeSetupModal()

    // Index setup modal lifecycle (round 11). Triggered from the Settings
    // "Update media database" action when idle; mirrors the scrape setup
    // modal above minus the scraper choice and re-scrape toggle, since a
    // full index has neither.
    function openIndexSetupModal(): void {
        root._requestModal(root.modalIndexSetup);
        root.indexSetupModalVisible = true;
        if (ScreenManager.topModal !== root.modalIndexSetup)
            ScreenManager.pushModal(root.modalIndexSetup);
    }

    function closeIndexSetupModal(): void {
        root.indexSetupModalVisible = false;
        if (ScreenManager.topModal === root.modalIndexSetup)
            ScreenManager.popModal();
    }

    onCloseIndexSetupRequested: root.closeIndexSetupModal()

    // Nested picker for the scraper-choice row inside ScrapeSetupModal —
    // stacks a second modal on top (ScreenManager.pushModal appends, so
    // the picker becomes topModal and correctly receives input while the
    // setup modal stays mounted underneath it). The picker's own accept
    // writes back to `scrapeSetupModal.selectedScraperId` directly (see
    // the `fieldId === "scraperChoice"` branch in `onListPickerAccepted`)
    // rather than a Browse.Settings setter, since the choice isn't
    // persisted until Start is pressed.
    function openScraperChoicePicker(): void {
        if (root.scrapeSetupModal === null)
            return;
        const ids = Browse.MediaStatus.scraper_ids;
        const names = Browse.MediaStatus.scraper_names;
        const entries = [];
        for (let i = 0; i < ids.length; i++)
            entries.push({
                id: ids[i],
                label: names[i] !== undefined && names[i] !== "" ? names[i] : ids[i]
            });
        root.openListPickerModal(qsTr("Source"), entries, root.scrapeSetupModal.selectedScraperId, "scraperChoice");
    }

    onRequestScraperPicker: root.openScraperChoicePicker()

    // Flat "All systems / All <Category> systems / one system" entry list
    // shared by both media job modals' Systems row. "cat:" ids resolve to
    // `system_ids_for_category` at Start; the individual-system entries
    // resolve straight to their own id. Categories with zero indexable
    // systems are skipped, same gate the systems/categories context menu
    // already uses to decide whether to offer "index_category"/
    // "scrape_category" at all.
    function _buildSystemScopeEntries(): var {
        const entries = [
            {
                id: root._systemScopeAll,
                label: qsTr("All systems")
            }
        ];
        for (let i = 0; i < Browse.CategoriesModel.count; i++) {
            const category = Browse.CategoriesModel.category_at(i);
            if (category === "" || Browse.SystemsModel.system_ids_for_category(category).length === 0)
                continue;
            entries.push({
                id: "cat:" + category,
                label: qsTr("All %1 systems").arg(category)
            });
        }
        const ids = Browse.SystemsModel.all_indexable_system_ids();
        for (let i = 0; i < ids.length; i++)
            entries.push({
                id: ids[i],
                label: Browse.SystemsModel.system_name_for_id(ids[i])
            });
        return entries;
    }

    // Nested picker for the Systems row, shared by ScrapeSetupModal and
    // IndexSetupModal (see `requestSystemScopePicker`'s doc comment in
    // MainLayout.qml for why one signal covers both). The two modals are
    // mutually exclusive, so whichever one is currently visible is the
    // request's source and the accept target.
    function _activeSystemScopeModal(): var {
        if (root.scrapeSetupModalVisible)
            return root.scrapeSetupModal;
        if (root.indexSetupModalVisible)
            return root.indexSetupModal;
        return null;
    }

    function openSystemScopePicker(): void {
        const target = root._activeSystemScopeModal();
        if (target === null)
            return;
        root.openListPickerModal(qsTr("Systems"), root._buildSystemScopeEntries(), target.selectedSystemScope, "systemScope");
    }

    onRequestSystemScopePicker: root.openSystemScopePicker()

    function handleLogUploadError(): void {
        // LogUpload state 3 is terminal failure; full detail is already logged
        // by the Rust model. Replace the phase view with standard error chrome.
        if (!root.logUploadModalVisible || Browse.LogUpload.state !== 3)
            return;
        root.closeLogUploadModal();
        root.presentActionError("log_upload", qsTr("Log upload failed"), qsTr("Could not upload the logs. Check the network connection and try again."), qsTr("Retry"), function () {
            root.openLogUploadModal();
        });
    }

    Connections {
        target: Browse.LogUpload
        function onStateChanged(): void {
            root.handleLogUploadError();
        }
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

    function _isViewListPicker(fieldId: string): bool {
        return fieldId === "page_menu" || fieldId === "page_menu_favorites" || fieldId === "page_menu_favorite_systems" || fieldId === "page_menu_hub";
    }

    // Open the page/list-scoped operations menu (West button), the "View"
    // counterpart to North's item-scoped "Options". Go to… stays
    // pre-focused so common path is fixed West-then-Accept chord. Letter
    // index fetch starts here so buckets are likely ready when user opens rail.
    function openPageMenu(): void {
        Browse.GamesModel.load_letter_index();
        const entries = [
            {
                "id": "jump_letter",
                "label": qsTr("Go to…")
            }
        ];
        // Core path random is recursive, so folders containing only nested
        // media remain valid scopes. Omit only when current browse is empty.
        if (Browse.GamesModel.total_files > 0 || Browse.GamesModel.total_dirs > 0)
            entries.push({
                "id": "launch_random",
                "label": qsTr("Random game")
            });
        // Level-local favorites projection: files of this folder filter
        // to the favorite ones; directories stay for navigation.
        entries.push({
            "id": "games_filter",
            "label": qsTr("Show: %1").arg(Browse.GamesModel.favorites_only ? qsTr("Favorites") : qsTr("All"))
        });
        // Games is always at least two steps from the Hub (Hub -> Systems
        // -> Games), or one via the MiSTer Arcade-singleton bypass -- an
        // unconditional escape hatch regardless of how the user actually
        // arrived, rather than special-casing shortcut-entered navigation.
        // See docs/style.md or the plan for the full reasoning.
        entries.push({
            "id": "back_to_hub",
            "label": qsTr("Back to Hub")
        });
        root.openListPickerModal(qsTr("View"), entries, "jump_letter", "page_menu");
    }

    // Games filter picker: the page-menu row announces the active state
    // ("Show: All" / "Show: Favorites"); this picker presents the actual
    // choice, preselected on what is active, mirroring the favorites
    // screen's filter menu.
    function openGamesFilterMenu(): void {
        const entries = [
            {
                "id": "all",
                "label": qsTr("All")
            },
            {
                "id": "favorites",
                "label": qsTr("Favorites")
            }
        ];
        const active = Browse.GamesModel.favorites_only ? "favorites" : "all";
        root.openListPickerModal(qsTr("Show"), entries, active, "games_filter_pick");
    }

    // Favorites View controls Core-backed ordering, random launch, and the
    // grouping preference shared with Favorite Systems.
    function openFavoritesPageMenu(): void {
        const entries = [
            {
                "id": "favorites_sort",
                "label": qsTr("Sort: %1").arg(root._favoritesSortLabel())
            },
            {
                "id": "favorites_mode",
                "label": qsTr("Group by: %1").arg(root._favoritesGroupingLabel())
            },
            {
                "id": "launch_random_favorite",
                "label": qsTr("Random favorite")
            },
            // Favorites is one step from the Hub when grouping is "none",
            // two when "system" (Hub -> Favorite Systems -> Favorites) --
            // unconditional regardless of grouping, same reasoning as
            // Games' own entry above.
            {
                "id": "back_to_hub",
                "label": qsTr("Back to Hub")
            }
        ];
        root.openListPickerModal(qsTr("View"), entries, "favorites_sort", "page_menu_favorites");
    }

    function openFavoriteSystemsPageMenu(): void {
        const entries = [
            {
                "id": "favorites_mode",
                "label": qsTr("Group by: %1").arg(root._favoritesGroupingLabel())
            }
        ];
        root.openListPickerModal(qsTr("View"), entries, "favorites_mode", "page_menu_favorite_systems");
    }

    function _favoritesGroupingLabel(): string {
        return Browse.Settings.current_favorites_grouping === "system" ? qsTr("System") : qsTr("None");
    }

    function openFavoritesModeMenu(): void {
        const entries = [
            {
                "id": "none",
                "label": qsTr("None")
            },
            {
                "id": "system",
                "label": qsTr("System")
            }
        ];
        root.openListPickerModal(qsTr("Group by"), entries, Browse.Settings.current_favorites_grouping, "favorites_mode_pick");
    }

    function _favoritesSortLabel(): string {
        return Browse.FavoritesModel.sort_mode === "name" ? qsTr("A-Z") : qsTr("Default");
    }

    function openFavoritesSortMenu(): void {
        // Same trap as the filter picker: ListPickerModal treats an empty id
        // as "nothing pending" and never emits an accept for it, so the
        // default row needs a real id mapped back on accept. Without this,
        // choosing A-Z once left no way back to Core's order.
        const entries = [
            {
                "id": root._favoritesSortDefault,
                "label": qsTr("Default")
            },
            {
                "id": "name",
                "label": qsTr("A-Z")
            }
        ];
        const active = Browse.FavoritesModel.sort_mode === "" ? root._favoritesSortDefault : Browse.FavoritesModel.sort_mode;
        root.openListPickerModal(qsTr("Sort"), entries, active, "favorites_sort_pick");
    }

    // Re-parse the model's facet JSON into the live grid entries. Bound through
    // a Connections below so the grid populates the instant the fetch lands.
    function _refreshLetterJumpEntries(): void {
        const scheme = Browse.GamesModel.letter_index_scheme;
        let parsed = [];
        try {
            parsed = JSON.parse(Browse.GamesModel.letter_index_json);
        } catch (e) {
            parsed = [];
        }
        root.letterJumpEntries = Array.isArray(parsed) ? parsed : [];
        // Empty scheme = facet still resolving; anything else is final.
        root.letterJumpLoading = scheme === "";
    }

    function openLetterJumpModal(): void {
        root._refreshLetterJumpEntries();
        root._requestModal(root.modalLetterJump);
        root.letterJumpModalVisible = true;
        if (ScreenManager.topModal !== root.modalLetterJump)
            ScreenManager.pushModal(root.modalLetterJump);
    }

    function closeLetterJumpModal(): void {
        root.letterJumpModalVisible = false;
        root.letterJumpEntries = [];
        root.letterJumpLoading = false;
        if (ScreenManager.topModal === root.modalLetterJump)
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
        else if (fieldId === "resolution") {
            root._pendingResolutionSelection = selectedId;
            root._resolutionRestartPending = true;
        } else if (fieldId === "crtVideoStandard")
            root._pendingCrtStandardSelection = selectedId;
        root.openSettingNeedsRestartModal();
    }

    function stageCrtToggle(enable: bool): void {
        root._pendingCrtToggle = enable ? "on" : "off";
        root.openSettingNeedsRestartModal();
    }

    function stageDebugLoggingToggle(enable: bool): void {
        root._pendingDebugLoggingToggle = enable ? "on" : "off";
        root.openSettingNeedsRestartModal();
    }

    function cancelPendingRestart(): void {
        root._pendingLanguageSelection = "";
        root._pendingResolutionSelection = "";
        root._resolutionRestartPending = false;
        root._pendingCrtStandardSelection = "";
        root._pendingCrtToggle = "";
        root._pendingDebugLoggingToggle = "";
        root.closeSettingNeedsRestartModal();
    }

    function confirmPendingRestart(): void {
        // CRT-mode toggle takes a different exit: Main_MiSTer owns the
        // respawn (the new mode needs a different fb setup and --crt
        // flag), so persist the flag byte and exit with the reserved
        // reload code instead of the in-process execvp restart. If the
        // flag write fails, stay up rather than respawn into the old
        // mode and let the user retry.
        if (root._pendingCrtToggle !== "") {
            const enable = root._pendingCrtToggle === "on";
            root._pendingCrtToggle = "";
            root.closeSettingNeedsRestartModal();
            if (Browse.CrtVideo.write_crt_enable_file(enable))
                Qt.exit(42);
            return;
        }
        const language = root._pendingLanguageSelection;
        const resolution = root._pendingResolutionSelection;
        const resolutionPending = root._resolutionRestartPending;
        const crtStandard = root._pendingCrtStandardSelection;
        const debugLogging = root._pendingDebugLoggingToggle;
        if (resolutionPending && !Browse.Settings.set_resolution(resolution)) {
            root._pendingLanguageSelection = "";
            root._pendingResolutionSelection = "";
            root._resolutionRestartPending = false;
            root._pendingCrtStandardSelection = "";
            root._pendingDebugLoggingToggle = "";
            root.closeSettingNeedsRestartModal();
            return;
        }
        root._pendingLanguageSelection = "";
        root._pendingResolutionSelection = "";
        root._resolutionRestartPending = false;
        root._pendingCrtStandardSelection = "";
        root._pendingDebugLoggingToggle = "";
        root.closeSettingNeedsRestartModal();
        if (language !== "")
            Browse.Settings.set_language(language);
        if (debugLogging !== "")
            Browse.Settings.set_debug_logging(debugLogging === "on");
        if (crtStandard !== "") {
            Browse.CrtVideo.set_video_standard(crtStandard);
            // A standard change must respawn through Main_MiSTer (exit
            // 42), not the in-process execvp restart: Main owns the fb
            // geometry (programs it pre-spawn and re-asserts ~1 s in,
            // reading the mode byte from the CRT state file), so it
            // has to re-read the new mode before the next frontend
            // boots. The desktop preview has no Main; the execvp
            // restart below re-reads frontend.toml and resizes the
            // preview canvas.
            if (Browse.Settings.is_mister) {
                if (Browse.CrtVideo.write_crt_enable_file(true))
                    Qt.exit(42);
                return;
            }
        }
        root.restartApp();
    }

    function restartApp() {
        Qt.exit(1000);
    }

    // The frontend correcting its own wrong boot-time resolution guess
    // (e.g. the TV was off at launch and came on later — see
    // mister_runtime::watch_for_output_change), not a setting the user
    // chose, so it restarts silently through the same path a CRT toggle
    // uses rather than routing through confirmPendingRestart's
    // user-facing modal. `output_resolution_stale` flips exactly once
    // per process and never resets, so this fires at most once per
    // session.
    Connections {
        target: Browse.Settings
        function onOutput_resolution_staleChanged(): void {
            if (Browse.Settings.output_resolution_stale)
                root.restartApp();
        }
    }

    // CRT calibration lifecycle. Full-bleed test pattern mounted
    // outside the safe-area inset (see MainLayout); arrows nudge the
    // centering trims live through Browse.CrtVideo.
    function openCrtCalibrationModal(): void {
        root._requestModal(root.modalCrtCalibration);
        root.crtCalibrationModalVisible = true;
        if (ScreenManager.topModal !== root.modalCrtCalibration)
            ScreenManager.pushModal(root.modalCrtCalibration);
    }

    function closeCrtCalibrationModal(): void {
        root.crtCalibrationModalVisible = false;
        if (ScreenManager.topModal === root.modalCrtCalibration)
            ScreenManager.popModal();
    }

    onCloseCrtCalibrationRequested: root.closeCrtCalibrationModal()

    // Shared by the system- and game-scoped launcher saves below: only one
    // of either can be in flight at a time (one ListPickerModal instance),
    // so a single delay timer + relabel target covers both.
    //
    // The row keeps its normal label through `launcherSavingDelay`'s
    // window and only switches to "Saving…" if the write is still pending
    // once it elapses -- 300ms, the same threshold already used
    // everywhere else a loading cue can show in this app
    // (MainLayout.loadingIndicatorDelayMs, ScreenStateOverlay.loadingDelayMs).
    // A save that completes inside that window never shows "Saving…" at
    // all, so there's nothing to flash. No animation on the swap itself.
    property string _pendingLauncherRelabelId: ""

    Timer {
        id: launcherSavingDelay
        interval: 300
        repeat: false
        onTriggered: {
            if (root._pendingLauncherRelabelId !== "")
                root.listPickerEntries = root._relabelPickerEntry(root.listPickerEntries, root._pendingLauncherRelabelId, qsTr("Saving…"));
        }
    }

    function beginSystemLauncherUpdate(systemId: string, selectedId: string): void {
        root._pendingLauncherSystemId = systemId;
        root._pendingLauncherSelectionId = selectedId;
        // Entries/title are untouched here -- the full list stays exactly
        // as it was when the user pressed Accept. initialId still needs to
        // move to the picked row now, ahead of the delayed relabel: that
        // reassigns listPickerEntries, which re-derives currentIndex from
        // initialId (see ListPickerModal's onEntriesChanged), and it would
        // otherwise still be the old current launcher's id from when the
        // picker first opened.
        root.listPickerInitialId = selectedId;
        root.listPickerFieldId = "system_launcher_pending";
        root._pendingLauncherRelabelId = selectedId;
        launcherSavingDelay.restart();
        Browse.SystemLaunchers.set_system_launcher(systemId, selectedId);
    }

    function clearPendingLauncherUpdate(): void {
        root._pendingLauncherSystemId = "";
        root._pendingLauncherSelectionId = "";
        launcherSavingDelay.stop();
        root._pendingLauncherRelabelId = "";
    }

    function retrySystemLauncherUpdate(systemId: string, selectedId: string): void {
        // The picker was closed for the error modal, so the full list is
        // gone -- rebuild it the same way the original "Change launcher"
        // open did, from the already-loaded picker_ids/picker_labels.
        const entries = root._launcherPickerEntries(Browse.SystemLaunchers.picker_ids, Browse.SystemLaunchers.picker_labels);
        root.openListPickerModal(qsTr("Change launcher"), entries, selectedId, "system_launcher_pending");
        root._pendingLauncherSystemId = systemId;
        root._pendingLauncherSelectionId = selectedId;
        root._pendingLauncherRelabelId = selectedId;
        launcherSavingDelay.restart();
        Browse.SystemLaunchers.set_system_launcher(systemId, selectedId);
    }

    function showSystemLauncherUpdateError(): void {
        const systemId = root._pendingLauncherSystemId;
        const selectedId = root._pendingLauncherSelectionId;
        root.closeListPickerModal();
        root.clearPendingLauncherUpdate();
        root.presentActionError("launcher:" + systemId, qsTr("Launcher update failed"), qsTr("Could not change the launcher. Check Zaparoo Core and try again."), qsTr("Retry"), function () {
            root.retrySystemLauncherUpdate(systemId, selectedId);
        });
    }

    // Per-game counterparts of the four functions above -- same "keep the
    // full list, delay-then-relabel the picked row" shape, writing through
    // Browse.GameLauncherOverride (media.meta.update) instead of
    // Browse.SystemLaunchers (settings.update).
    function beginGameLauncherUpdate(systemId: string, path: string, selectedId: string): void {
        root._pendingGameLauncherSystemId = systemId;
        root._pendingGameLauncherPath = path;
        root._pendingGameLauncherSelectionId = selectedId;
        root.listPickerInitialId = selectedId;
        root.listPickerFieldId = "game_launcher_pending";
        root._pendingLauncherRelabelId = selectedId;
        launcherSavingDelay.restart();
        Browse.GameLauncherOverride.set_game_launcher(systemId, path, selectedId);
    }

    function clearPendingGameLauncherUpdate(): void {
        root._pendingGameLauncherSystemId = "";
        root._pendingGameLauncherPath = "";
        root._pendingGameLauncherSelectionId = "";
        launcherSavingDelay.stop();
        root._pendingLauncherRelabelId = "";
    }

    function retryGameLauncherUpdate(systemId: string, path: string, selectedId: string): void {
        const entries = root._launcherPickerEntries(Browse.GameLauncherOverride.picker_ids, Browse.GameLauncherOverride.picker_labels);
        root.openListPickerModal(qsTr("Change launcher"), entries, selectedId, "game_launcher_pending");
        root._pendingGameLauncherSystemId = systemId;
        root._pendingGameLauncherPath = path;
        root._pendingGameLauncherSelectionId = selectedId;
        root._pendingLauncherRelabelId = selectedId;
        launcherSavingDelay.restart();
        Browse.GameLauncherOverride.set_game_launcher(systemId, path, selectedId);
    }

    function showGameLauncherUpdateError(): void {
        const systemId = root._pendingGameLauncherSystemId;
        const path = root._pendingGameLauncherPath;
        const selectedId = root._pendingGameLauncherSelectionId;
        root.closeListPickerModal();
        root.clearPendingGameLauncherUpdate();
        root.presentActionError("game_launcher:" + systemId + "\n" + path, qsTr("Launcher update failed"), qsTr("Could not change the launcher. Check Zaparoo Core and try again."), qsTr("Retry"), function () {
            root.retryGameLauncherUpdate(systemId, path, selectedId);
        });
    }

    function handleListPickerCloseRequested(): void {
        if (root.listPickerFieldId === "system_launcher_pending" || root.listPickerFieldId === "game_launcher_pending")
            return;
        root.closeListPickerModal();
    }

    onListPickerAccepted: (fieldId, selectedId) => {
        if (fieldId === "page_menu") {
            root.closeListPickerModal();
            if (selectedId === "jump_letter")
                root.openLetterJumpModal();
            else if (selectedId === "launch_random")
                Browse.GamesModel.launch_random();
            else if (selectedId === "games_filter")
                root.openGamesFilterMenu();
            else if (selectedId === "back_to_hub")
                root._navigateBackToScreen(root.screenHub);
            return;
        }
        if (fieldId === "games_filter_pick") {
            root.closeListPickerModal();
            const enabled = selectedId === "favorites";
            Browse.GamesModel.apply_favorites_filter(enabled);
            Browse.GamesState.favorites_filter = enabled;
            return;
        }
        if (fieldId === "page_menu_favorites") {
            root.closeListPickerModal();
            if (selectedId === "favorites_sort")
                root.openFavoritesSortMenu();
            else if (selectedId === "launch_random_favorite")
                Browse.FavoritesModel.launch_random();
            else if (selectedId === "favorites_mode")
                root.openFavoritesModeMenu();
            else if (selectedId === "back_to_hub")
                root._navigateBackToScreen(root.screenHub);
            return;
        }
        if (fieldId === "page_menu_favorite_systems") {
            root.closeListPickerModal();
            if (selectedId === "favorites_mode")
                root.openFavoritesModeMenu();
            return;
        }
        if (fieldId === "page_menu_hub") {
            root.closeListPickerModal();
            if (selectedId === "hub_add")
                root.openHubAddMenu();
            else if (selectedId === "hub_reset")
                root.resetHubLayout();
            else if (selectedId === "hub_settings")
                root._navigateToSettings();
            else if (selectedId === "hub_quit")
                root.openQuitConfirmModal();
            return;
        }
        if (fieldId === "hub_add_pick") {
            root.closeListPickerModal();
            // Board-model placement: lands on the Hub's current cell when
            // it's a blank, otherwise appended after the last tile — see
            // zaparoo_core::hub_layout::HubLayout::add_item. With
            // `skipEmptyCells` on, the cursor can no longer normally be
            // resting on a blank when this menu is opened, so `target`
            // now almost always falls through to append; kept anyway
            // since Rust already guards it (a non-blank target is a
            // no-op there) and it's still correct on the rare path where
            // a Move session left the cursor somewhere unusual.
            const cursorEntry = root.hubScreen !== null ? root.hubScreen.items[root.hubScreen.currentIndex] : null;
            const target = cursorEntry ? cursorEntry.hubIndex : -1;
            const beforeCount = root.hubScreen !== null ? Browse.HubLayout.item_count() : 0;
            let added = false;
            if (selectedId === "blank")
                added = Browse.HubLayout.add_item("blank", "", target);
            else {
                const sep = selectedId.indexOf(":");
                if (sep > 0)
                    added = Browse.HubLayout.add_item(selectedId.slice(0, sep), selectedId.slice(sep + 1), target);
            }
            // Hand the newly placed item straight to Move so it can be
            // positioned immediately instead of just sitting wherever
            // append/target left it — see HubScreen.qml's
            // `armMoveForHubIndex`. `add_item` only grows `item_count()`
            // when it actually appended (a filled `target` blank leaves
            // the count unchanged), so that comparison is what tells us
            // whether the new item landed at `target` or at the old
            // count's own position.
            if (added && root.hubScreen !== null) {
                const afterCount = Browse.HubLayout.item_count();
                const newHubIndex = afterCount > beforeCount ? beforeCount : target;
                root.hubScreen.armMoveForHubIndex(newHubIndex);
            }
            return;
        }
        if (fieldId === "favorites_mode_pick") {
            root.closeListPickerModal();
            if (Browse.Settings.current_favorites_grouping === selectedId)
                return;
            Browse.Settings.set_favorites_grouping(selectedId);
            if (selectedId === "system")
                root._navigateToFavoriteSystems();
            else
                root._navigateToFavorites("");
            return;
        }
        if (fieldId === "favorites_sort_pick") {
            root.closeListPickerModal();
            const mode = selectedId === root._favoritesSortDefault ? "" : selectedId;
            Browse.FavoritesModel.set_sort_mode(mode);
            if (!Browse.FavoritesModel.loading && root.favoritesScreen !== null)
                root.favoritesScreen.restoreSelection();
            return;
        }
        if (fieldId === "system_launcher_pending" || fieldId === "game_launcher_pending")
            return;
        if (fieldId.startsWith("system_launcher:")) {
            root.beginSystemLauncherUpdate(fieldId.slice("system_launcher:".length), selectedId);
            return;
        }
        if (fieldId.startsWith("game_launcher:")) {
            // "game_launcher:<systemId>\n<path>" -- see openContextMenu's
            // gameSystemId/gamePath pairing for why a newline separator is
            // safe (system IDs never contain one; paths never contain a
            // literal newline either).
            const rest = fieldId.slice("game_launcher:".length);
            const separatorIndex = rest.indexOf("\n");
            if (separatorIndex < 0)
                return;
            root.beginGameLauncherUpdate(rest.slice(0, separatorIndex), rest.slice(separatorIndex + 1), selectedId);
            return;
        }
        // Round 10: nested picker opened by ScrapeSetupModal itself
        // (see its own `requestScraperPicker` signal below) -- writes
        // straight back to the modal's own local `selectedScraperId`
        // rather than a Browse.Settings setter, since the choice isn't
        // persisted until "Start" is pressed. Only pops the picker; the
        // scrape setup modal underneath stays open.
        if (fieldId === "scraperChoice") {
            root.closeListPickerModal();
            if (root.scrapeSetupModal !== null)
                root.scrapeSetupModal.selectedScraperId = selectedId;
            return;
        }
        // Round 11: nested Systems-row picker shared by ScrapeSetupModal
        // and IndexSetupModal (see `openSystemScopePicker`/
        // `_activeSystemScopeModal` above) -- same "write straight back to
        // the still-open modal underneath" pattern as `scraperChoice`.
        if (fieldId === "systemScope") {
            root.closeListPickerModal();
            const target = root._activeSystemScopeModal();
            if (target !== null)
                target.selectedSystemScope = selectedId;
            return;
        }
        if (fieldId === "resolution") {
            root.closeListPickerModal();
            if (selectedId !== Browse.Settings.current_resolution)
                root.stageSettingRestart(fieldId, selectedId);
            return;
        } else if (fieldId === "language") {
            root.closeListPickerModal();
            if (selectedId !== Browse.Settings.current_language)
                root.stageSettingRestart(fieldId, selectedId);
            return;
        } else if (fieldId === "orientation") {
            Browse.Settings.set_orientation(selectedId);
        } else if (fieldId === "clockFormat")
            Browse.Settings.set_clock_format(selectedId);
        else if (fieldId === "region") {
            Browse.Settings.set_region(selectedId);
            Browse.SystemsModel.reproject();
            Browse.CategoriesModel.reproject();
        } else if (fieldId === "systemsLayout")
            Browse.Settings.set_systems_browse_layout(selectedId);
        else if (fieldId === "gamesLayout")
            Browse.Settings.set_games_browse_layout(selectedId);
        else if (fieldId === "systemLogoStyle")
            Browse.Settings.set_system_logo_style(selectedId);
        else if (fieldId === "colorScheme")
            Browse.Settings.set_color_scheme(selectedId);
        else if (fieldId === "buttonLayout")
            Browse.Settings.set_button_layout(selectedId);
        else if (fieldId === "screensaverTimeout")
            Browse.Settings.set_screensaver_timeout(selectedId);
        else if (fieldId === "mediaImageType")
            Browse.Settings.set_media_image_type(selectedId);
        else if (fieldId === "crtVideoStandard") {
            root.closeListPickerModal();
            if (selectedId !== Browse.CrtVideo.current_video_standard)
                root.stageSettingRestart(fieldId, selectedId);
            return;
        }
        root.closeListPickerModal();
    }
    onListPickerCloseRequested: root.handleListPickerCloseRequested()

    onLetterJumpAccepted: offset => {
        root.closeLetterJumpModal();
        if (root.gamesScreen !== null)
            root.gamesScreen.jumpToItem(offset);
    }
    onLetterJumpCloseRequested: root.closeLetterJumpModal()

    // Keep the open grid in sync with the facet as it lands. The model clears
    // its facet to the loading state on `load_letter_index`, then fills it; this
    // re-parses into the live grid entries each time either changes.
    Connections {
        target: root.letterJumpModalRequested ? Browse.GamesModel : null
        enabled: root.letterJumpModalRequested
        function onLetter_index_jsonChanged(): void {
            if (root.letterJumpModalVisible)
                root._refreshLetterJumpEntries();
        }
        function onLetter_index_schemeChanged(): void {
            if (root.letterJumpModalVisible)
                root._refreshLetterJumpEntries();
        }
    }

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
        target: Browse.GameLauncherOverride
        function onUpdate_pendingChanged(): void {
            if (root._pendingGameLauncherSystemId === "" || Browse.GameLauncherOverride.update_pending)
                return;
            if (Browse.GameLauncherOverride.update_error === "") {
                root.clearPendingGameLauncherUpdate();
                root.closeListPickerModal();
            } else {
                root.showGameLauncherUpdateError();
            }
        }
    }

    Connections {
        target: Browse.AppStatus
        function onConnection_stateChanged(): void {
            root._maybeStartFirstRunIndex();
            root._maybeCompleteBoot();
            root._maybeStartStartupRestore();
            root._maybeCompletePendingResumeLaunch();
        }
        // The version fetch resolves asynchronously after connect; this is
        // the edge that lets the chain advance past the version-warning gate.
        function onCore_version_checkedChanged(): void {
            root._maybeOpenCoreVersionWarning();
        }
    }

    Connections {
        target: Browse.MediaStatus
        function onSeededChanged(): void {
            root._maybeStartFirstRunIndex();
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
            root._maybeStartFirstRunIndex();
            root._maybeStartStartupRestore();
            root._maybeContinueOptimisticTransitions();
        }
        function onCountChanged(): void {
            root._maybeStartFirstRunIndex();
            root._maybeStartStartupRestore();
            root._maybeContinueOptimisticTransitions();
        }
        function onIndexed_countChanged(): void {
            root._maybeStartFirstRunIndex();
        }
    }

    onCloseCommercialNoticeRequested: root.closeCommercialNoticeModal()
    onCloseCoreVersionRequested: root.closeCoreVersionModal()
    onCloseRandomFailedRequested: root.closeRandomFailedModal()

    function beginCardWrite(owner: string, index: int): void {
        if (owner === "systems")
            Browse.SystemsModel.cancel_card_write();
        else if (owner === "games")
            Browse.GamesModel.cancel_card_write();
        else if (owner === "favorites")
            Browse.FavoritesModel.cancel_card_write();
        else if (owner === "recents")
            Browse.RecentsModel.cancel_card_write();
        root.cardWriteOwner = owner;
        root._cardWriteIndex = index;
        root.cardWriteFailed = false;
        root._requestModal(root.modalCardWrite);
        root.cardWriteModalVisible = true;
        if (ScreenManager.topModal !== root.modalCardWrite)
            ScreenManager.pushModal(root.modalCardWrite);
    }

    function retryCardWrite(owner: string, index: int): void {
        if (index < 0)
            return;
        root.beginCardWrite(owner, index);
        if (owner === "systems")
            Browse.SystemsModel.write_card_at(index);
        else if (owner === "games")
            Browse.GamesModel.write_card_at(index);
        else if (owner === "favorites")
            Browse.FavoritesModel.write_card_at(index);
        else if (owner === "recents")
            Browse.RecentsModel.write_card_at(index);
    }

    function handleCardWriteStatus(): void {
        if (!root.cardWriteModalVisible || root.cardWriteOwner === "")
            return;
        if (root.activeCardWritePending)
            return;
        if (root.activeCardWriteError !== "") {
            const owner = root.cardWriteOwner;
            const index = root._cardWriteIndex;
            root.hideCardWriteModal();
            root.presentActionError("card_write:" + owner, qsTr("Token write failed"), qsTr("Could not write to this token. Check that it is writable and try again."), qsTr("Retry"), function () {
                root.retryCardWrite(owner, index);
            });
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
        else if (root.cardWriteOwner === "recents")
            Browse.RecentsModel.cancel_card_write();
        root.hideCardWriteModal();
    }

    function hideCardWriteModal(): void {
        root.cardWriteModalVisible = false;
        root.cardWriteFailed = false;
        root.cardWriteOwner = "";
        root._cardWriteIndex = -1;
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
        // Swallow input while the warm-resume curtain is up. The target
        // screen restores behind a hidden Hub (activeScreen stays Hub), so
        // any press here would drive the ghost Hub the user can't see.
        if (root._startupRestorePending && root.startupRestoreCurtainVisible && !ScreenManager.hasModal)
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
        if ((root.pendingTransition !== "" || root.transitionCueVisible) && !ScreenManager.hasModal) {
            root._startupTrace("input/qml drop", "reason=pending-transition", "action=" + action, "pendingTransition=" + root.pendingTransition, "transitionCueVisible=" + root.transitionCueVisible);
            return;
        }
        if (!ScreenManager.hasModal)
            root._beginTransitionTiming(action);
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
            if (ScreenManager.topModal === root.modalActionError) {
                if (action === "cancel")
                    root.closeActionErrorModal(false);
                else if (root.actionErrorModal !== null)
                    root.actionErrorModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalCardWrite && action === "cancel") {
                root.cancelCardWrite();
            } else if (ScreenManager.topModal === root.modalQrCode && action === "cancel") {
                root.closeQrCodeModal();
            } else if (ScreenManager.topModal === root.modalGameInfo) {
                if (root.gameInfoModal !== null)
                    root.gameInfoModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalContextMenu) {
                if (root.contextMenu !== null)
                    root.contextMenu.handleAction(action);
            } else if (ScreenManager.topModal === root.modalCommercialNotice) {
                if (root.commercialNoticeModal !== null)
                    root.commercialNoticeModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalCoreVersion) {
                if (root.coreVersionModal !== null)
                    root.coreVersionModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalRandomFailed) {
                // Legacy stack id retained for sessions restored across a hot
                // reload; new failures use the shared action-error modal.
                if (action === "accept" || action === "cancel")
                    root.closeRandomFailedModal();
            } else if (ScreenManager.topModal === root.modalLogUpload) {
                if (root.logUploadModal !== null)
                    root.logUploadModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalScrapeSetup) {
                if (root.scrapeSetupModal !== null)
                    root.scrapeSetupModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalIndexSetup) {
                if (root.indexSetupModal !== null)
                    root.indexSetupModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalQuitConfirm) {
                if (root.quitConfirmModal !== null)
                    root.quitConfirmModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalSettingNeedsRestart) {
                if (root.settingNeedsRestartModal !== null)
                    root.settingNeedsRestartModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalListPicker) {
                // Locked while a launcher save is in flight -- no focus
                // movement, no cancel, no resubmit. Accept/Cancel are
                // already separately no-ops downstream once they reach
                // onListPickerAccepted/handleListPickerCloseRequested (that
                // still matters for a direct mouse click on a row, which
                // bypasses this action routing entirely), but gating here
                // too is what actually stops Up/Down from moving focus off
                // the "Saving…" row.
                if (root.listPickerFieldId === "system_launcher_pending" || root.listPickerFieldId === "game_launcher_pending")
                    return;
                if (action === "page_menu" && root._isViewListPicker(root.listPickerFieldId))
                    root.closeListPickerModal();
                else if (root.listPickerModal !== null)
                    root.listPickerModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalLetterJump) {
                if (root.letterJumpModal !== null)
                    root.letterJumpModal.handleAction(action);
            } else if (ScreenManager.topModal === root.modalCrtCalibration) {
                if (root.crtCalibrationModal !== null)
                    root.crtCalibrationModal.handleAction(action);
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
        } else if (root.activeScreen === root.screenFavoriteSystems) {
            if (root.favoriteSystemsScreen !== null)
                root.favoriteSystemsScreen.handleAction(action);
        } else if (root.activeScreen === root.screenRecents) {
            if (root.recentsScreen !== null)
                root.recentsScreen.handleAction(action);
        } else if (root.activeScreen === root.screenUpdate) {
            if (root.updateScreen !== null)
                root.updateScreen.handleAction(action);
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
    // Tap-only entry is deliberately difficult: normal alternating navigation
    // must not suspend covers or show the letter overlay. A held direction
    // bypasses this threshold on its first controlled repeat tick.
    readonly property int _rapidNavigationTapThreshold: 4
    // Window for collapsing a second delivery of the same key into one
    // press — hardware contact bounce or input-stack double send. Far
    // below _repeatInitialMs and the repeat tick so it never touches
    // hold-repeat, and below the floor for a deliberate human re-tap.
    readonly property int _duplicateInputWindowMs: 40
    property int _lastPressedKey: 0
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
        target: root.favoriteSystemsScreen
        property: "detailRapidScrollActive"
        value: root.activeScreen === root.screenFavoriteSystems && root.rapidNavigationActive
    }

    Binding {
        target: root.favoriteSystemsScreen
        property: "detailRapidIndicatorActive"
        value: root.activeScreen === root.screenFavoriteSystems && root.rapidNavigationIndicatorActive
    }

    Binding {
        target: root.favoriteSystemsScreen
        property: "detailRapidScrollAction"
        value: root.activeScreen === root.screenFavoriteSystems ? root.rapidNavigationAction : ""
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
        // Direction changes cancel tap-driven rapid mode immediately. Only a
        // sustained hold (forceActive from repeat timer) or four consecutive
        // same-direction taps inside the quiet window can enter it.
        if (!sameBurst && !forceActive) {
            root.rapidNavigationActive = false;
            root.rapidNavigationIndicatorActive = false;
        }
        if (forceActive || root._rapidNavigationTapCount >= root._rapidNavigationTapThreshold) {
            root.rapidNavigationActive = true;
            root.rapidNavigationIndicatorActive = true;
        }
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

    // Drop a second delivery of the same key while the guard window is
    // open — hardware contact bounce or input-stack double send. A
    // different key, or the same key after the window closes, is not a
    // duplicate. Pure so tst_navigation can assert it without a clock.
    function _isDuplicateInput(key: int, lastKey: int, withinWindow: bool): bool {
        return withinWindow && key === lastKey;
    }

    // State-machine half of handleKey: records the held key/action and
    // arms the initial-delay timer. Pulled out of handleKey so unit
    // tests can drive the repeat state machine without also routing
    // through handleAction → real screens. No-op for non-dpad actions.
    function _prepareRapidNavigationSnapshot(action: string): void {
        // Keep the pre-activation capture stable once rapid mode is showing;
        // recapturing then would grab the intentionally hidden live cell layer.
        if (root.rapidNavigationActive || !root._isRapidNavigationAction(action))
            return;
        let screen = null;
        if (root.activeScreen === root.screenGames)
            screen = root.gamesScreen;
        else if (root.activeScreen === root.screenFavorites)
            screen = root.favoritesScreen;
        else if (root.activeScreen === root.screenFavoriteSystems)
            screen = root.favoriteSystemsScreen;
        else if (root.activeScreen === root.screenRecents)
            screen = root.recentsScreen;
        if (screen !== null)
            screen.prepareRapidSnapshot();
    }

    function _armRepeat(action: string, key: int): void {
        if (!root._isRepeatableAction(action))
            return;
        root._startupTrace("input/qml repeat arm", "action=" + action, "key=" + key, "previousAction=" + root._heldAction, "previousKey=" + root._heldKey);
        root._heldAction = action;
        root._heldKey = key;
        repeatTick.stop();
        root._prepareRapidNavigationSnapshot(action);
        repeatInitial.restart();
    }

    // "Swap controller confirm/cancel" (Settings > Controls & Input) flips
    // which physical button accepts vs cancels, independent of Main_MiSTer's
    // own OSD OK/Cancel swap -- a way to fix a mucked-up controller mapping
    // without leaving the frontend. Applied at this single seam so every
    // screen and modal sees the swap uniformly. Never applies while the
    // keyboard is the active input source (`_keyboardActive`): Enter/Escape
    // are fixed keys, not something a controller mapping mistake affects.
    // The help-bar glyphs flip in lockstep via the same guard -- see
    // MainLayout.qml's `_swapConfirmCancel` binding comment.
    function _swapConfirmCancelAction(action: string): string {
        if (!Browse.Settings.current_swap_confirm_cancel || root._keyboardActive)
            return action;
        if (action === "accept")
            return "cancel";
        if (action === "cancel")
            return "accept";
        return action;
    }

    // "Swap controller options/view" -- same idea as
    // `_swapConfirmCancelAction` above, for ButtonX/ButtonY. A controller
    // whose X/Y (or equivalent) mapping is backwards is common enough on
    // its own to warrant an independent toggle, separate from the
    // confirm/cancel swap. Same keyboard exemption (Tab/Space are fixed
    // keys); the help-bar glyphs flip in lockstep via
    // MainLayout.qml's `_swapOptionsView` binding comment.
    function _swapOptionsViewAction(action: string): string {
        if (!Browse.Settings.current_swap_options_view || root._keyboardActive)
            return action;
        if (action === "context_menu")
            return "page_menu";
        if (action === "page_menu")
            return "context_menu";
        return action;
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
        const action = root._swapOptionsViewAction(root._swapConfirmCancelAction(Browse.Input.action_for_key(key)));
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
        if (root._idleScreensaverMs <= 0 || !root._allowsScreensaver(root.activeScreen)) {
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
        if (!root.bootComplete || root.pendingTransition !== "" || root.transitionCueVisible || !root._allowsScreensaver(root.activeScreen))
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
        screensaverOverlay.activate(Resources.screensaverLogoUrl(w), Qt.rect(pt.x, pt.y, w, h));
    }

    Timer {
        id: idleTimer
        interval: root._idleScreensaverMs > 0 ? root._idleScreensaverMs : 60000
        repeat: false
        running: root._idleScreensaverMs > 0 && root._allowsScreensaver(root.activeScreen)
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
    // on bare cursor moves without a button being pressed. Disable
    // this root-level hover area when mouse support is off so the
    // scene-level blank-cursor blocker owns both cursor and clicks.
    MouseArea {
        anchors.fill: parent
        z: 9001
        visible: Browse.Settings.current_mouse_enabled
        enabled: visible
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
        // Round 10: only arm rapid-nav tracking when the active root
        // screen actually owns the held direction. `handleAction`'s
        // single-press path already skips this while a modal is open (it
        // returns before ever reaching its own call to
        // `_noteRapidNavigationAction`) -- this repeat-tick path used to
        // call it unconditionally, which meant holding up/down inside a
        // modal (e.g. GameInfoModal's own scroll) still armed
        // `rapidNavigationActive` every tick. That property drives the
        // Games/Favorites/FavoriteSystems/Recents screen's rapid-scroll
        // ghost-snapshot regardless of whether a modal currently covers
        // it (see MediaListScreen.qml's `_gateHide`, which has no
        // `ScreenManager.hasModal` term), so the still-visible-behind-
        // the-scrim grid popped its freeze-frame in and out on every
        // repeat tick -- the reported "background visibly re-lays-out"
        // while scrolling the details dialog.
        if (!ScreenManager.hasModal)
            root._noteRapidNavigationAction(root._heldAction, true);
        root.handleAction(root._heldAction);
    }

    Timer {
        id: actionErrorQueueTimer
        interval: 0
        repeat: false
        onTriggered: root._showNextActionError()
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

    // Open while a duplicate-input guard window is active; see the
    // Keys.onPressed handler below.
    Timer {
        id: duplicateInputGuard
        interval: root._duplicateInputWindowMs
        repeat: false
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
        //
        // Then drop a second delivery of the same key inside the
        // duplicate-input window: some controllers / input stacks
        // double-send a single press, which would otherwise act
        // twice. Returning before restart() keeps the window anchored
        // to the first accepted press so a bounce can't extend it.
        Keys.onPressed: event => {
            if (event.isAutoRepeat)
                return;
            if (root._isDuplicateInput(event.key, root._lastPressedKey, duplicateInputGuard.running))
                return;
            root._lastPressedKey = event.key;
            duplicateInputGuard.restart();
            root.handleKey(event.key);
        }
        Keys.onReleased: event => {
            if (event.isAutoRepeat)
                return;
            root.handleKeyRelease(event.key);
        }
    }

    // Transition cue. Item, not Rectangle — the source screen's existing
    // page background stays visible underneath; never
    // paint a full-screen fill. The delayed indicator suppresses flashes
    // for quick loads; once it appears, screen `transitioning` bindings hide
    // primary content so the centered "Loading…" reads alone in the cleared
    // band. Do not apply a minimum-visible tail here: when the work completes
    // near the delay threshold, the destination must not paint underneath a
    // stale loading label. Parented into `scene` (not `root`) and sized to
    // it, not the window, so the centered position matches the per-screen
    // ScreenStateOverlay cue it hands off to — both now center within the
    // same safe-area-inset rect instead of the cue centering on the raw
    // window and the screen cue centering on the smaller inset content,
    // which used to jump the row a few pixels at handoff. Living outside
    // `scene` also meant this cue ignored the CRT safe-area inset and
    // `scene.rotation`, so it painted unrotated over a rotated TATE scene;
    // reparenting fixes that too. z: 250 sits above chrome (headerBar: 200,
    // BootOverlay: 50, screen content: 0) but below real modals (commercial
    // notice and up start at 310) so a modal that legitimately coexists with
    // a pending transition still wins visually.
    Item {
        parent: root.scene
        anchors.fill: parent
        visible: transitionCueActive || transitionCue.showing
        z: 250

        readonly property bool startupRestoreCueActive: root.bootComplete && root.startupRestoreCurtainVisible && root._startupRestoreScreen !== ""
        readonly property bool transitionCueActive: (root.pendingTransition !== "" && !root.startupRestoreCurtainVisible) || startupRestoreCueActive
        readonly property string cueScreen: root.pendingTransition !== "" ? root.pendingTransition : root._startupRestoreScreen

        DelayedLoadingIndicator {
            id: transitionCue
            active: parent.transitionCueActive
            delayMs: parent.startupRestoreCueActive ? 0 : root.loadingIndicatorDelayMs
            minimumVisibleMs: 0
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            onShowingChanged: root.transitionCueVisible = showing
            Component.onCompleted: root.transitionCueVisible = showing
            text: {
                switch (parent.cueScreen) {
                case "systems":
                    return qsTr("Loading systems…");
                case "games":
                    return qsTr("Loading games…");
                case "resume":
                    return qsTr("Loading game…");
                case "favorites":
                case "favorite_systems":
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

    Timer {
        id: systemsScreenWarmMountTimer
        interval: 250
        repeat: false
        onTriggered: {
            if (!root.systemsScreenRequested) {
                console.debug("responsiveness systems screen warm mount start");
                root.systemsScreenRequested = true;
            }
        }
    }

    Timer {
        id: resumeLaunchTimer
        interval: 50
        repeat: false
        onTriggered: root._startResumeLaunch()
    }

    // Desktop safety-clear for the resume "Loading game…" cue. On MiSTer the
    // launch replaces this process before this fires, so it never triggers and
    // the cue covers the core swap. On desktop nothing replaces us, so clear
    // the cue (and ungate input) once the launch has had time to take.
    Timer {
        id: resumeLaunchCueTimer
        interval: 8000
        repeat: false
        onTriggered: {
            if (root.pendingTransition === "resume")
                root.pendingTransition = "";
        }
    }

    Timer {
        id: favoritesTransitionTimer
        interval: 50
        repeat: false
        onTriggered: root._startFavoritesTransitionLoad()
    }

    Timer {
        id: favoriteSystemsTransitionTimer
        interval: 50
        repeat: false
        onTriggered: root._startFavoriteSystemsTransitionLoad()
    }

    Timer {
        id: recentsTransitionTimer
        interval: 50
        repeat: false
        onTriggered: root._startRecentsTransitionLoad()
    }

    Timer {
        id: backTransitionTimer
        interval: root.loadingIndicatorDelayMs + 50
        repeat: false
        onTriggered: root._maybeCompleteBackTransition()
    }

    Timer {
        id: folderBackTransitionTimer
        interval: root.loadingIndicatorDelayMs + 50
        repeat: false
        onTriggered: root._completeFolderBackTransition()
    }

    // Deferred set_category trigger. When the existing model has rows,
    // the caller stretches the interval to the delayed cue threshold plus
    // one frame so the transition indicator is visible before synchronous
    // delegate teardown can freeze the GUI thread. Qt.callLater / interval
    // 0 fire inside the same event loop iteration before the next render.
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

    Timer {
        id: deferredSystemSetTimer
        interval: 1
        repeat: false
        property string targetSystemId: ""
        onTriggered: {
            const systemId = deferredSystemSetTimer.targetSystemId;
            root._startupTrace("startup/qml deferred system trigger", "systemId=" + systemId);
            Browse.GamesModel.set_system(systemId);
            root._deferredSystemPending = false;
            root._completeDeferredSystemIfReady(systemId);
        }
    }
}
