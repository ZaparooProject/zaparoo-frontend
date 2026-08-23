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
// call on a Zaparoo.Browse singleton (set_category, index_for_category,
// etc.) still trips qmllint's "Member can be shadowed" check. Until
// the schema grows method-level finality, suppress the compiler
// category file-wide.
// qmllint disable compiler

// Hub screen — one uniform paged grid the user navigates with a single flat
// cursor, rendering `Browse.HubLayout`'s persisted `[[hub.items]]` layout in
// the user's own order (a "go all in" replacement for round 6's hide/order
// storage — see `docs/plans/ui-geometry-refresh.md`'s Hub roadmap). A tile
// can be a category, a built-in action, a specific system, a folder, or an
// arbitrary ZapScript — see `_resolveLayoutEntry` below, one resolver per
// kind. The layout records INTENT (this tile exists, here); each resolver
// folds in whatever LIVE visibility rule that kind needs (Resume needs
// Recents to answer, Update needs the build flag + internet, a category
// needs Core to currently confirm it) — the persisted order never changes
// just because something is temporarily unavailable.
//
// Bootstrap exception: a genuinely fresh install has no persisted layout
// yet (`Browse.HubLayout.is_unseeded()`) AND Core hasn't answered this
// launch either. For that narrow window only, categories fall back to
// hardcoded placeholders (`_placeholderCategories`) while actions still
// resolve live in their fixed built-in order — see `items` below. Keying
// this off `is_unseeded()` rather than `item_count() === 0` matters once a
// SEEDED layout can be emptied out entirely (every tile removed, trailing
// blanks trimmed) — that's a real, reachable zero-item state that must
// render as an empty Hub, not fall back to fake placeholder tiles. The
// moment Core answers, `Main.qml` calls `Browse.HubLayout.reconcile(...)`,
// which seeds the real layout from what Core reported; every later launch
// (the common case) renders the real layout immediately, no placeholder
// window at all.
//
// `PagedGrid` -- the same component Systems/Media already use -- owns cell
// layout, paging, and directional navigation (`moveSelection`). The grid's
// shape is a FIXED per-tier table (`Sizing.hubGridColumns/Rows`), never
// fitted to the viewport the way Systems/Games are -- see `Sizing.qml`'s
// `hubGridShape` comment. Tiles are square (`PagedGrid.squareCells`) and
// paging is the normal way to reach anything past the first page, not an
// edge case -- `handleAction` below wires L/R shoulder page turns.
//
// A `blank`-kind layout entry (a deliberate spacer the user placed, e.g. to
// push the next item onto a fresh page) renders through `emptyDelegate` --
// see the `PagedGrid` block below -- as a genuinely blank, focusable-but-
// inert slot, the SAME component `_padToPageSize`'s trailing tail padding
// uses for the last page's leftover cells.
//
// No edit mode. Move/Hide/Add live in the same two menus every other
// screen already uses -- Options (North/X, item-scoped) and View (West/Y,
// page-scoped) -- not built in this pass; see the plan's Phase D.
//
// Pure input dispatcher: emits one of `requestAccept(kind, id, system)`
// (forward; the router decides destination -- see CLAUDE.md -> "Screens and
// routing") or `requestRetry`. Cancel is a deliberate no-op on the Hub root
// (there is no "back" from the top of the screen stack) except while a Move
// is armed, where it's intercepted by `_handleMoveAction`/`_cancelMove`;
// Quit lives in the View menu instead (Main.qml's `openHubPageMenu`).
//
// All cross-screen orchestration (model fills, deferred set_category,
// cover prefetch, transition overlay, screen flip) lives in Main.qml.
// `transitioning` is written by the router so the grid hides during the
// loading wait.
Item {
    id: hub

    Component.onCompleted: {
        console.debug("startup/qml component HubScreen completed");
        // `onItemsChanged` isn't guaranteed to fire for `items`' very
        // first evaluation (there's no prior value to have "changed" from)
        // -- populate the grid model explicitly so `hubGridModel` isn't
        // empty for first paint. See `_syncGridModel`'s doc comment.
        hub._syncGridModel();
        // Raw construction-time default, before Main.qml's first
        // restoreFromCategoriesReset() call lands (and for test harnesses
        // that never call it at all) — PagedGrid's own currentIndex
        // defaults to 0, which now points at the first CATEGORY whenever
        // any exist. Round <=5's static currentRow/currentIndex defaults
        // intentionally landed on Resume instead (the highest-value
        // action, and harmless to navigate before any real focus has been
        // armed) — reproduce that here now that "first action" is no
        // longer index 0.
        hub.currentIndex = hub._actionIndexForId("resume");
    }

    // Prefer a user override cover key over the bundled default. Pure: takes
    // the override-lookup result (empty string when none) and the fallback,
    // so it is unit-testable without the Browse.ImageOverrides singleton.
    // Hub overrides live under the `hub/` customization subfolder, keyed by
    // category id (Arcade/Computer/Console/Handheld) or action id
    // (resume/favorites/recents/update/settings); see docs/customization.md.
    function _preferOverride(overrideKey: string, fallbackKey: string): string {
        return (overrideKey && overrideKey.length > 0) ? overrideKey : fallbackKey;
    }

    // Resolve the cover key for a Hub item: a user override from the `hub/`
    // namespace if present, else the bundled key.
    //
    // Never returns empty. Withholding the bundled key until the override scan
    // lands used to guarantee at least one blank frame on every Hub tile —
    // a filesystem round-trip's worth of pop-in charged to every user, to spare
    // the few with a `custom/hub/` folder a one-frame swap. Main.qml now starts
    // the scan at construction rather than after the first frame, so it usually
    // completes before first paint and the swap is invisible anyway.
    function _hubCoverKey(id: string, fallbackKey: string): string {
        if (!Browse.ImageOverrides.hub_loaded)
            return fallbackKey;
        return hub._preferOverride(Browse.ImageOverrides.override_cover_key("hub", id), fallbackKey);
    }

    readonly property var _placeholderCategories: [
        {
            id: CategoryIds.arcadeId,
            name: CategoryIds.displayName(CategoryIds.arcadeId),
            coverKey: hub._hubCoverKey(CategoryIds.arcadeId, CategoryIds.coverKey(CategoryIds.arcadeId))
        },
        {
            id: CategoryIds.computerId,
            name: CategoryIds.displayName(CategoryIds.computerId),
            coverKey: hub._hubCoverKey(CategoryIds.computerId, CategoryIds.coverKey(CategoryIds.computerId))
        },
        {
            id: CategoryIds.consoleId,
            name: CategoryIds.displayName(CategoryIds.consoleId),
            coverKey: hub._hubCoverKey(CategoryIds.consoleId, CategoryIds.coverKey(CategoryIds.consoleId))
        },
        {
            id: CategoryIds.handheldId,
            name: CategoryIds.displayName(CategoryIds.handheldId),
            coverKey: hub._hubCoverKey(CategoryIds.handheldId, CategoryIds.coverKey(CategoryIds.handheldId))
        },
        {
            id: CategoryIds.otherId,
            name: CategoryIds.displayName(CategoryIds.otherId),
            coverKey: hub._hubCoverKey(CategoryIds.otherId, CategoryIds.coverKey(CategoryIds.otherId))
        }
    ]
    property bool transitioning: false
    // Flat cursor over `items` (categories, then actions, back to back).
    // Alias to the grid's own currentIndex so mouse hover/click
    // (handled entirely inside PagedGrid) and keyboard navigation
    // (handleAction below, via pagedGrid.moveSelection) stay a single
    // source of truth — see PagedGrid.qml.
    property alias currentIndex: pagedGrid.currentIndex
    // Exposes the grid's page count so MainLayout's help bar can show the
    // L/R shoulder glyphs only when there's a second page to jump to (same
    // pattern Systems already uses) — `pagedGrid` is a bare `id`, lexically
    // scoped to this file, so a caller in another file can't reach
    // `pagedGrid.pageCount` directly.
    readonly property alias pageCount: pagedGrid.pageCount
    // Test-only geometry access — `pagedGrid` is a bare `id`, lexically
    // scoped to this file, so a test can't reach `pagedGrid.height`
    // directly to verify the equal-gap vertical layout below.
    readonly property alias _gridHeight: pagedGrid.height
    // Test-only: cross-checks Sizing.hubTileSize (which duplicates this
    // grid's squareCells fit so Settings can read it without a HubScreen
    // instance) against the real resolved value, so the two can't silently
    // drift apart.
    readonly property alias _gridCellWidth: pagedGrid.cellWidth
    // False on the first-paint path so Hub can draw a static Resume tile
    // without touching RecentsModel. MainLayout flips this after the
    // first frame, then Resume can hide/update from Core history.
    property bool resumeModelEnabled: false
    // Incremented on each Accept so the focused tile plays its push-in
    // animation. Forwarded to the grid, which forwards it to every tile;
    // only the focused+selected Tile fires its animation.
    property int activatePulse: 0
    // False until the user takes control of focus (first input). Combined
    // with `_restoreDone` into `_focusReady`, which gates whether the tiles
    // render focus at all.
    property bool _focusArmed: false
    // Set true once the load-time category restore has run. Combined with
    // `_focusArmed` into `_focusReady`, which gates whether the tiles render
    // focus at all — so the default first-tile selection never paints a
    // ring before `restoreFromCategoriesReset` corrects focus to the saved
    // tile on a cold start.
    property bool _restoreDone: false
    readonly property bool _focusReady: hub._focusArmed || hub._restoreDone

    // `system` is only ever populated for `kind === "folder"` -- a folder
    // shortcut's re-entry needs its owning system id to establish
    // GamesState's path stack before pushing the folder's own path (see
    // Main.qml's router). Every other kind passes "".
    signal requestAccept(kind: string, id: string, system: string)
    signal requestRetry
    // Emitted when the user opens the options menu on a category tile.
    // `anchorRect` is the tile's bounding rect mapped to hub coordinates,
    // used by the context menu to position itself. `anchorRadius` is the
    // tile's corner radius so the menu's scrim can cut a rounded hole
    // around it instead of a square one. `categoryId` (not a flat `items`
    // or CategoriesModel index) — once the layout can freely interleave
    // categories with everything else, a flat index no longer has any
    // fixed relationship to a CategoriesModel index (round <=6 relied on
    // categories always occupying a contiguous prefix; that's no longer
    // true). Main.qml resolves the id to a CategoriesModel index itself
    // before opening the menu. `hubIndex` is the entry's real
    // `Browse.HubLayout` position (-1 during the bootstrap placeholder
    // window) — separate from `categoryId`, since Main.qml needs it purely
    // to dispatch the universal Move/Remove entries this menu now also
    // carries, unrelated to the CategoriesModel index the category-specific
    // entries use.
    signal requestContextMenu(hubIndex: int, categoryId: string, anchorRect: rect, anchorRadius: int)
    // `hubIndex`: see `requestContextMenu`'s doc comment. Fired for every
    // action tile now (not just Favorites) so Move/Remove reach them too;
    // Favorites alone still carries its extra "Random game" entry.
    signal requestActionContextMenu(hubIndex: int, actionId: string, anchorRect: rect)
    // Fired for `system`/`folder`/`zapscript` only — never a `blank` tile
    // (an implementation detail, not an interactable object; see
    // `handleAction`'s "context_menu" branch) and never tail padding (see
    // `_blankEntry`'s `hubIndex` doc comment). These kinds have no menu of
    // their own, only the universal Move/Hide-or-Delete.
    signal requestItemContextMenu(hubIndex: int, kind: string, anchorRect: rect, anchorRadius: int)
    // West/Y — the page-scoped "View" menu (Add item… / Reset layout).
    signal requestPageMenu

    // Header→grid, grid→activeLabel, and activeLabel→help-bar all match —
    // round 6 follow-up: the previous layout centered the (grid + fixed
    // pctH(3) gap + activeLabel) block in the band, which only made the two
    // OUTER gaps equal to each other (a side effect of centering), never to
    // the fixed inner one — visibly uneven. Solving for one gap size and
    // using it in all three places replaces that.
    readonly property int _activeLabelHeight: Sizing.pctH(7)
    // The band between the HeaderBar bottom and the help bar top,
    // independent of pagedGrid.height (used both as the cell-fit ceiling
    // below and, once the grid's real height is known, to solve for the
    // gap).
    readonly property int _verticalBand: Math.max(0, hub.height - Sizing.headerBottom - Sizing.pctH(6))
    // Ceiling fed to pagedGrid's `heightBudget` (see that property's doc
    // comment for why the grid needs a ceiling distinct from its own
    // `height`) — reserves activeLabel's height plus a nominal minimum gap
    // allowance on each side so the grid can't grow to consume the whole
    // band. Computed independent of `pagedGrid.height` (which is itself
    // derived FROM the fitted cell size below — reading it here would be
    // circular); the real, equal gap is solved afterward from the grid's
    // actual resolved height.
    readonly property int _gridHeightBudget: Math.max(0, hub._verticalBand - hub._activeLabelHeight - 3 * Sizing.pctH(2))
    // The actual equal gap, now that pagedGrid.height is resolved: whatever
    // room the band has left after the grid's real height and the label's
    // fixed height, split three ways.
    readonly property int _verticalGap: Math.max(0, Math.round((hub._verticalBand - pagedGrid.height - hub._activeLabelHeight) / 3))
    readonly property int _blockY: Sizing.headerBottom + hub._verticalGap

    readonly property bool resumeKnownUnavailable: hub.resumeModelEnabled && !Browse.RecentsModel.resume_loading && !Browse.RecentsModel.resume_available && Browse.AppStatus.connection_state === 2
    readonly property bool resumeActionVisible: !hub.resumeKnownUnavailable
    readonly property bool _internetAvailable: Browse.SystemStatus.has_wifi_internet || Browse.SystemStatus.has_lan_internet
    readonly property string _emptyCatalogFallbackAction: Browse.BuildInfo.update_enabled && hub._internetAvailable ? "update" : "settings"

    // ── Layout entry resolvers ───────────────────────────────────────────
    // One resolver per kind `Browse.HubLayout` can hand back (see that
    // singleton's header comment — it already filters out the reserved
    // `collection` kind and anything this build doesn't recognise, so every
    // kind reaching here is one of the five below). Each returns a
    // PagedGrid-ready row (`name`/`coverKey`/`favorite`/`hidden`/`disabled`/
    // `stateReason`/`disambiguatingTags`/`isEmpty`, Hub's own `kind`+`id`),
    // or `null` to skip the tile entirely for now — still round-trips on
    // save (Rust owns persistence), just isn't rendered this frame. `null`
    // is reserved for STRUCTURAL absence only (a compile-time feature this
    // build lacks, an id this build doesn't recognise) — anything whose
    // availability can change at runtime (still loading, currently
    // unconfirmed, no internet right now) always returns a real entry with
    // `disabled` set instead, so the tile's presence and position in the
    // grid never depend on live data — only its enabled/disabled look does.
    // See this round's plan ("Tile state consolidation") for why: letting
    // tiles pop in and out as Core/Recents/SystemStatus answer was both a
    // layout-shift bug and, because `restoreFromCategoriesReset` looks up
    // the persisted focus by scanning this same array, the "focus goes
    // missing on boot" bug.

    // Resume is always present; `disabled` is the same boolean that used to
    // gate its existence (definitively no resumable game, per confirmed
    // Core history) — see `resumeKnownUnavailable`. Update is present
    // whenever this build has the feature at all (a compile-time constant,
    // genuinely structural); `disabled` there tracks live internet
    // reachability instead of gating existence, so a Wi-Fi toggle mid-session
    // never adds or removes the tile, only its look.
    function _resolveActionEntry(id: string): var {
        if (id === "resume") {
            const resumeName = hub.resumeModelEnabled ? Browse.RecentsModel.resume_name : "";
            return hub._actionEntry("resume", hub._hubCoverKey("resume", "icons/PlayOutline"), resumeName.length > 0 ? resumeName : qsTr("Resume"), hub.resumeKnownUnavailable, qsTr("No recent games"));
        }
        if (id === "favorites")
            return hub._actionEntry("favorites", hub._hubCoverKey("favorites", "icons/HeartOutline"), qsTr("Favorites"));
        if (id === "recents")
            return hub._actionEntry("recents", hub._hubCoverKey("recents", "icons/History"), qsTr("Recently Played"));
        if (id === "update") {
            if (!Browse.BuildInfo.update_enabled)
                return null;
            return hub._actionEntry("update", hub._hubCoverKey("update", "icons/RefreshCw"), qsTr("Update"), !hub._internetAvailable, qsTr("No internet connection"));
        }
        if (id === "settings")
            return hub._actionEntry("settings", hub._hubCoverKey("settings", "icons/Tools"), qsTr("Settings & Utilities"));
        // A future build's action id this one doesn't recognise -- stays in
        // the persisted layout untouched, just not rendered.
        return null;
    }

    function _actionEntry(id: string, coverKey: string, text: string, disabled: bool, stateReason: string): var {
        return {
            kind: "action",
            id: id,
            name: text,
            coverKey: coverKey,
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: disabled ?? false,
            stateReason: stateReason ?? "",
            isEmpty: false
        };
    }

    // A category Core hasn't (yet, or ever) confirmed stays in the
    // persisted layout -- reconciliation is add-only, never remove (see
    // hub_layout.rs) -- and always renders; `disabled` tracks whether Core
    // currently lists it, rather than gating existence. `loaded` gates the
    // check so a category isn't wrongly marked disabled before Core has
    // answered at all this launch.
    function _resolveCategoryEntry(id: string): var {
        const canonicalId = CategoryIds.canonicalize(id);
        const unconfirmed = Browse.CategoriesModel.loaded && Browse.CategoriesModel.index_for_category(canonicalId) < 0;
        return {
            kind: "category",
            id: canonicalId,
            name: CategoryIds.displayName(canonicalId),
            coverKey: hub._hubCoverKey(canonicalId, CategoryIds.coverKey(canonicalId)),
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: unconfirmed,
            stateReason: unconfirmed ? qsTr("Not available") : "",
            isEmpty: false
        };
    }

    // A specific-system tile. Name/cover resolve through
    // Browse.HubLayout's id-only lookups (no live category row needed --
    // the system's own category may not even be the active one) unless the
    // layout entry carries an explicit override.
    function _resolveSystemEntry(id: string, nameOverride: string, iconOverride: string): var {
        if (id === "")
            return null;
        return {
            kind: "system",
            id: id,
            name: nameOverride !== "" ? nameOverride : Browse.HubLayout.resolve_system_name(id),
            coverKey: iconOverride !== "" ? hub._hubCoverKey(iconOverride, "icons/File") : Browse.HubLayout.resolve_system_cover_key(id),
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: false,
            stateReason: "",
            isEmpty: false
        };
    }

    // A folder shortcut, addressed by path (a folder's only stable
    // identity -- see GamesState.path_stack). Falls back to the path's
    // final segment as the display name, same as GamesScreen's own
    // `_folderNameForPath`.
    function _resolveFolderEntry(path: string, nameOverride: string, iconOverride: string, system: string): var {
        if (path === "")
            return null;
        return {
            kind: "folder",
            id: path,
            path: path,
            // The owning system id, carried through to Accept (see
            // `requestAccept`'s doc comment) so the router can re-establish
            // GamesState's system before pushing this folder's path.
            system: system,
            name: nameOverride !== "" ? nameOverride : hub._folderNameForPath(path),
            coverKey: iconOverride !== "" ? hub._hubCoverKey(iconOverride, "icons/Folder") : hub._hubCoverKey(path, "icons/Folder"),
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: false,
            stateReason: "",
            isEmpty: false
        };
    }

    function _folderNameForPath(path: string): string {
        const trimmed = path.replace(/\/+$/, "");
        const idx = trimmed.lastIndexOf("/");
        return idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
    }

    // An arbitrary-ZapScript tile. Neither `system` nor `path` on the
    // layout entry is a launch target -- the script text itself is what
    // runs -- but together they identify a LINKED GAME the same way a
    // Games-grid row or a Favorites/Recents row is addressed, and cover
    // art resolves the same way: a real Core lookup through
    // Browse.HubLayout.resolve_media_cover_key, not just a generic icon.
    // `system` alone (no `path`) is a softer hint for a script with no
    // single game to fetch art for (e.g. "launch a random game in this
    // system") and falls back to that system's logo instead. Falls back to
    // the script text as the label when no name override is set.
    function _resolveZapScriptEntry(script: string, nameOverride: string, iconOverride: string, systemHint: string, pathHint: string): var {
        if (script === "")
            return null;
        let coverKey = "icons/File";
        if (iconOverride !== "")
            coverKey = hub._hubCoverKey(iconOverride, "icons/File");
        else if (systemHint !== "" && pathHint !== "")
            coverKey = Browse.HubLayout.resolve_media_cover_key(systemHint, pathHint);
        else if (systemHint !== "")
            coverKey = Browse.HubLayout.resolve_system_cover_key(systemHint);
        return {
            kind: "zapscript",
            id: script,
            script: script,
            name: nameOverride !== "" ? nameOverride : script,
            coverKey: coverKey,
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: false,
            stateReason: "",
            isEmpty: false
        };
    }

    // `hubIndex` (all entry kinds, including `_blankEntry`) is the entry's
    // real position in `Browse.HubLayout` -- the same index `move_held_to`/
    // `remove_item`/`add_item`'s `target` take (a real persisted `blank`
    // tile can be the TARGET of a move/add, same as any gap). -1 means
    // "not really backed by the persisted layout": the bootstrap
    // placeholder window (`items`' `else` branch, below) and
    // `_padToPageSize`'s synthetic tail padding both use the same
    // `kind: "empty"` shape a real persisted `blank` tile does. Options
    // never opens on ANY `kind === "empty"` entry regardless of `hubIndex`
    // -- a blank is an implementation detail, not something you pick up
    // and move on its own; see `handleAction`'s "context_menu" branch.
    function _blankEntry(hubIndex: int): var {
        return {
            kind: "empty",
            id: "",
            name: "",
            coverKey: "",
            favorite: 0,
            hidden: false,
            disambiguatingTags: "",
            disabled: false,
            stateReason: "",
            isEmpty: true,
            hubIndex: hubIndex ?? -1
        };
    }

    // One row of `Browse.HubLayout.items_snapshot()`'s batched fields (see
    // that qinvokable's doc comment for the delimited format) -- `i` is
    // both this row's real Browse.HubLayout position (the snapshot is
    // visible-order, same as the old per-field `item_*_at` accessors) and
    // this entry's `hubIndex`.
    function _resolveSnapshotRow(i: int, fields: var): var {
        const kind = fields[0];
        if (kind === "blank")
            return hub._blankEntry(i);
        const id = fields[1];
        const path = fields[2];
        const script = fields[3];
        const name = fields[4];
        const icon = fields[5];
        const system = fields[6];
        let entry = null;
        if (kind === "category")
            entry = hub._resolveCategoryEntry(id);
        else if (kind === "action")
            entry = hub._resolveActionEntry(id);
        else if (kind === "system")
            entry = hub._resolveSystemEntry(id, name, icon);
        else if (kind === "folder")
            entry = hub._resolveFolderEntry(path, name, icon, system);
        else if (kind === "zapscript")
            entry = hub._resolveZapScriptEntry(script, name, icon, system, path);
        if (entry)
            entry.hubIndex = i;
        return entry;
    }

    readonly property var _builtInActionIds: ["resume", "favorites", "recents", "update", "settings"]
    readonly property int _pageSize: Sizing.hubGridColumns * Sizing.hubGridRows

    // Pad the end of the list up to a full page, so the *last* page's
    // trailing remainder renders as deliberate empty slots (via
    // `emptyDelegate`, see the PagedGrid block below) instead of just blank
    // background -- every earlier page is already exactly full by
    // construction, so this only ever adds cells to the tail. Pads even
    // from an empty list now that a seeded layout can be emptied out
    // entirely (every tile removed, trailing blanks trimmed) -- that's a
    // real, navigable empty Hub, not a broken zero-height grid.
    //
    // `minPages` (default 0) is a FLOOR on the total page count, not an
    // increment on top of the natural page count -- used while `moveArmed`
    // so the held tile can always be pushed onto a fresh page; see
    // `beginMove`'s `_moveArmedTotalPages`. A floor (rather than "current
    // natural pages + 1", recomputed every press) means placing a tile
    // into the reserve page raises the natural page count to match the
    // SAME floor, not past it -- an increment would keep receding one page
    // further every time real content grows to reach a new target,
    // producing unbounded page growth. See `beginMove`'s doc comment.
    function _padToPageSize(list: var, pageSize: int, minPages: int): var {
        if (pageSize <= 0)
            return list;
        const floor = Math.max(0, minPages ?? 0);
        const naturalPages = list.length === 0 ? 1 : Math.ceil(list.length / pageSize);
        const targetPages = Math.max(naturalPages, floor);
        const target = targetPages * pageSize;
        if (target === list.length)
            return list;
        const padded = list.slice();
        for (let i = list.length; i < target; i++)
            padded.push(hub._blankEntry(-1));
        return padded;
    }

    // The flat item list PagedGrid renders, in the user's own order once
    // the layout is seeded -- see the header comment's "Bootstrap
    // exception" for the one window where it isn't.
    readonly property var items: {
        // Reads Browse.HubLayout.revision explicitly as a dependency:
        // items_snapshot/is_unseeded are qinvokable METHODS, not
        // qproperties, so calling them below does not itself register a
        // reactive dependency (see Browse.HubLayout's header comment) --
        // revision is what makes this recompute after reconcile() or an
        // edit-UI mutation.
        const _rev = Browse.HubLayout.revision;
        const list = [];
        if (!Browse.HubLayout.is_unseeded()) {
            const snapshot = Browse.HubLayout.items_snapshot();
            // An empty layout's snapshot is "" -- String.split on an empty
            // string yields [""], one phantom empty row, not zero rows.
            if (snapshot.length > 0) {
                const rows = snapshot.split("");
                for (let i = 0; i < rows.length; i++) {
                    const entry = hub._resolveSnapshotRow(i, rows[i].split(""));
                    if (entry)
                        list.push(entry);
                }
            }
        } else {
            // Resume seeds first, ahead of every category, so it lands in
            // the top-left cell by default -- mirrors
            // zaparoo_core::hub_layout::HubLayout::reconcile's unseeded
            // seed order, so the bootstrap window looks the same as the
            // real layout it's about to be replaced by.
            const resumeEntry = hub._resolveActionEntry(hub._builtInActionIds[0]);
            if (resumeEntry) {
                resumeEntry.hubIndex = -1;
                list.push(resumeEntry);
            }
            for (let i = 0; i < hub._placeholderCategories.length; i++) {
                const p = hub._placeholderCategories[i];
                list.push({
                    kind: "category",
                    id: p.id,
                    name: p.name,
                    coverKey: p.coverKey,
                    favorite: 0,
                    hidden: false,
                    disambiguatingTags: "",
                    disabled: false,
                    stateReason: "",
                    isEmpty: false,
                    // Bootstrap-window placeholder, not a real
                    // Browse.HubLayout row yet -- see `_blankEntry`'s
                    // `hubIndex` doc comment.
                    hubIndex: -1
                });
            }
            for (let i = 1; i < hub._builtInActionIds.length; i++) {
                const entry = hub._resolveActionEntry(hub._builtInActionIds[i]);
                if (entry) {
                    entry.hubIndex = -1;
                    list.push(entry);
                }
            }
        }
        return hub._padToPageSize(list, hub._pageSize, hub.moveArmed ? hub._moveArmedTotalPages : 0);
    }

    // `hubGridModel` (the visual-tree `ListModel` below) mirrors `items`
    // for `pagedGrid`'s `model` binding only -- every OTHER read of an
    // entry (`_commitCurrent`, `_emitActivate`, `handleAction`,
    // `_itemIndexForId`, the Move block, etc.) still reads the full-fidelity
    // `items` array directly and is unaffected by any of this.
    //
    // Binding `pagedGrid.model` straight to `hub.items` (a `var` property)
    // meant every recompute handed the Repeater a brand-new JS array;
    // `QQmlDelegateModel` has no diffing for plain-array models, so that's
    // a full `clear()` + `regenerate()` -- every Tile in the grid destroyed
    // and reconstructed, with the current page's incubated SYNCHRONOUSLY
    // (see PagedGrid.qml) -- on every `Browse.HubLayout.revision` bump.
    // During a Move session that fires on every single d-pad press, which
    // is the dominant cost behind Move feeling sluggish (the other being
    // Rust's per-press disk write -- see `with_hub_layout_mut_unsaved`).
    //
    // `ListModel.setProperty` patches one row in place without touching
    // any other row's delegate, so the shared prefix touches exactly the
    // rows whose values actually differ -- normally 2, a swap's pair -- and
    // every other Tile stays alive, mid-incubation state and all. A length
    // change (Add/Remove/Reset, or a Move that crosses onto a fresh page)
    // appends/removes only the tail delta.
    //
    // Deliberately NEVER calls `clear()`. That would drop the model to
    // ZERO rows for an instant before the loop below repopulates it, and
    // PagedGrid.qml's `onItemCountChanged` (`:726-747`) watches for exactly
    // that shape ("model shed rows") to defensively clamp `currentIndex`
    // into range -- at count zero, that means `currentIndex = 0`. Since
    // arming Move itself changes `items`' padding (see `_moveArmedTotalPages`
    // below), a `clear()` here fired on every single `beginMove` call,
    // before the first press even landed, silently resetting the held
    // tracking to slot 0 regardless of what was actually focused. Growing
    // only ever goes existing->existing+k and shrinking only ever goes
    // existing->existing-k, so count never passes through zero unless the
    // real final size genuinely is zero.
    function _syncGridModel(): void {
        const list = hub.items;
        const existing = hubGridModel.count;
        const shared = Math.min(existing, list.length);
        for (let i = 0; i < shared; i++) {
            const entry = list[i];
            const row = hubGridModel.get(i);
            if (row.name !== entry.name)
                hubGridModel.setProperty(i, "name", entry.name);
            if (row.coverKey !== entry.coverKey)
                hubGridModel.setProperty(i, "coverKey", entry.coverKey);
            if (row.favorite !== entry.favorite)
                hubGridModel.setProperty(i, "favorite", entry.favorite);
            if (row.hidden !== entry.hidden)
                hubGridModel.setProperty(i, "hidden", entry.hidden);
            if (row.disabled !== (entry.disabled === true))
                hubGridModel.setProperty(i, "disabled", entry.disabled === true);
            if (row.disambiguatingTags !== entry.disambiguatingTags)
                hubGridModel.setProperty(i, "disambiguatingTags", entry.disambiguatingTags);
            if (row.isEmpty !== entry.isEmpty)
                hubGridModel.setProperty(i, "isEmpty", entry.isEmpty);
        }
        if (list.length > existing) {
            for (let i = existing; i < list.length; i++) {
                const entry = list[i];
                hubGridModel.append({
                    name: entry.name,
                    coverKey: entry.coverKey,
                    favorite: entry.favorite,
                    hidden: entry.hidden,
                    disabled: entry.disabled === true,
                    disambiguatingTags: entry.disambiguatingTags,
                    isEmpty: entry.isEmpty
                });
            }
        } else if (list.length < existing) {
            hubGridModel.remove(list.length, existing - list.length);
        }
    }

    // Flat index of the first entry of `kind`, or -1. Used where the old
    // category/action block split used to give a fixed boundary
    // (`_categoryItems.length`) -- once the layout can freely interleave
    // any kind in the user's own order, "where do actions start" has no
    // fixed answer, so callers search instead.
    function _firstItemIndexOfKind(kind: string): int {
        for (let i = 0; i < hub.items.length; i++) {
            if (hub.items[i].kind === kind)
                return i;
        }
        return -1;
    }

    // Flat index of the first entry matching (kind, id), or -1. `kind` is
    // "category" or "action" — "empty" padding entries carry no id and are
    // never a lookup target.
    function _itemIndexForId(kind: string, id: string): int {
        for (let i = 0; i < hub.items.length; i++) {
            const entry = hub.items[i];
            if (entry.kind === kind && entry.id === id)
                return i;
        }
        return -1;
    }

    // Flat index of the action with `id`, falling back to the first
    // action-kind slot when not found -- built-in actions always include
    // at least Favorites/Recently Played/Settings, so this fallback holds
    // until a future edit UI can remove every action, in which case it
    // falls back once more to index 0.
    function _actionIndexForId(id: string): int {
        const idx = hub._itemIndexForId("action", id);
        if (idx >= 0)
            return idx;
        const anyAction = hub._firstItemIndexOfKind("action");
        return anyAction >= 0 ? anyAction : 0;
    }

    function _remapActionFocus(): void {
        const current = hub.items[hub.currentIndex];
        if (!current || current.kind !== "action")
            return;
        hub.currentIndex = hub._actionIndexForId(Browse.HubState.selected_action);
    }

    function focusResumeIfVisible(): void {
        if (!hub.resumeActionVisible)
            return;
        const idx = hub._itemIndexForId("action", "resume");
        if (idx < 0)
            return;
        hub.currentIndex = idx;
        hub._commitCurrent();
    }

    function _focusFallbackAfterResumeRemoved(): void {
        const anyCategory = hub._firstItemIndexOfKind("category");
        if (anyCategory >= 0) {
            hub.currentIndex = anyCategory;
            hub._commitCurrent();
            return;
        }
        hub.currentIndex = hub._actionIndexForId("settings");
        hub._commitCurrent();
    }

    // `items` itself is the single reactive signal now -- the old
    // category/action block split needed a separate `onActionEntriesChanged`
    // specifically so a category-only change couldn't spuriously trigger
    // this; once the layout can interleave any kind, `items` already
    // recomputes on every relevant change (Browse.HubLayout.revision, live
    // action-visibility state, category confirmation), and the kind check
    // below makes a category-only firing a harmless no-op here.
    onItemsChanged: {
        // Sync the lightweight grid model unconditionally, armed or not --
        // this is the part of `items` changing that must always reach the
        // screen, including every intermediate press of a Move session
        // (each swap's two traded cells have to repaint). See
        // `_syncGridModel`'s doc comment.
        hub._syncGridModel();
        // A Move session drives `currentIndex` itself (`_moveSplice`/
        // `_movePage`/`_acceptMove`/`_cancelMove`) and every intermediate
        // press bumps `revision`, which re-fires this handler mid-session --
        // neither branch below is meaningful while a tile is held (the
        // fallback below reseats off `HubState.selected_action`, which the
        // move hasn't touched and shouldn't consult), so both would fight
        // the move's own cursor tracking. Bail out of the rest while armed.
        if (hub.moveArmed)
            return;
        // Only treat a vanishing Resume tile as a real removal once the user
        // is driving focus (_focusArmed). During the cold-boot settle the
        // resume fetch can briefly read unavailable before it resolves to the
        // just-played game; reacting then would jump focus to the first
        // category and persist it, stranding the user off the
        // (about-to-reappear) Resume tile. While !_focusArmed, fall through
        // to _remapActionFocus, which keeps focus aligned to the saved
        // "resume" intent without overwriting persisted state.
        const current = hub.items[hub.currentIndex];
        const onAction = current !== undefined && current.kind === "action";
        if (hub._focusArmed && onAction && Browse.HubState.selected_action === "resume" && !hub.resumeActionVisible) {
            hub._focusFallbackAfterResumeRemoved();
            return;
        }
        hub._remapActionFocus();
    }

    // Test-harness hook so `tst_navigation.qml` can reset focus between
    // cases without poking pagedGrid's aliased currentIndex directly.
    function resetFocus(): void {
        hub.currentIndex = hub._actionIndexForId("resume");
    }

    // Restore the hub from the persisted `Browse.HubState`. The router
    // decides whether this pass should cascade into
    // `SystemsModel.set_category`; first Hub paint restores focus only,
    // then post-frame restore/transition paths can pay for Systems.
    //
    // Called from two sites in Main.qml — the Component.onCompleted
    // early-arrival path (catalog already seeded synchronously) and the
    // CategoriesModel.onModelReset listener (later refreshes). On a
    // refresh the category list can reorder, so the row index MUST be
    // re-seeded even when SystemsModel is already on the chosen
    // category — otherwise the visible focus drifts off whichever
    // screen the user is on.
    // Flat `items` position of `categoryId`. Distinct from a
    // CategoriesModel index (`idx`/`chosenCategoryIndex` in
    // restoreFromCategoriesReset below) — that had a fixed relationship to
    // a flat position back when categories always occupied a contiguous
    // prefix of `items`; once the layout can freely interleave any kind,
    // it no longer does. Falls back to whichever category tile IS actually
    // on screen (the bootstrap placeholder path, or Core simply hasn't
    // confirmed this particular one yet) rather than -1, so a restore
    // never lands on nothing when at least one category tile exists.
    function _flatIndexForCategory(categoryId: string): int {
        const found = hub._itemIndexForId("category", categoryId);
        if (found >= 0)
            return found;
        return hub._firstItemIndexOfKind("category");
    }

    function restoreFromCategoriesReset(cascadeSystems: bool): void {
        // Focus is now being finalized from persisted state; let the tiles
        // render focus from here on (snapped, since `_focusArmed` is still
        // false until the first user input).
        hub._restoreDone = true;
        const savedCategory = CategoryIds.canonicalize(Browse.HubState.category);
        const idx = savedCategory === "" ? -1 : Browse.CategoriesModel.index_for_category(savedCategory);
        const chosenCategoryIndex = idx >= 0 ? idx : 0;
        const chosenCategory = idx >= 0 ? savedCategory : Browse.CategoriesModel.category_at(chosenCategoryIndex);

        // Restore which item was focused. `selected_item` (round 6) is the
        // authoritative record when present; `selected_row`/`selected_action`
        // stay as the fallback for a state.toml written by an older build
        // (serde defaults them to empty/0 when absent, so this branch is
        // also what a genuinely fresh install takes). When the catalog
        // reports 0 categories the category block has no tiles to focus, so
        // we drop focus onto Update when it exists, otherwise Settings so
        // the user lands on an actionable tile.
        const savedItem = Browse.HubState.selected_item;
        const savedRow = Browse.HubState.selected_row;
        const savedAction = Browse.HubState.selected_action;
        if (savedItem !== "") {
            const sep = savedItem.indexOf(":");
            const savedKind = sep >= 0 ? savedItem.slice(0, sep) : "";
            // A folder path or ZapScript body can itself contain ":"
            // (a Windows-style drive path, a "**launch.system:x" directive)
            // — splitting on only the FIRST ":" (already what `indexOf` +
            // `slice` do above) keeps the rest of the id intact regardless,
            // so this restore path works for every persisted kind, not
            // just category/action.
            const savedId = sep >= 0 ? savedItem.slice(sep + 1) : "";
            const restoredIdx = savedKind !== "" ? hub._itemIndexForId(savedKind, savedId) : -1;
            if (restoredIdx >= 0) {
                hub.currentIndex = restoredIdx;
            } else if (idx >= 0) {
                hub.currentIndex = hub._flatIndexForCategory(chosenCategory);
            } else if (hub.resumeActionVisible) {
                hub.currentIndex = hub._actionIndexForId("resume");
            } else if (Browse.CategoriesModel.count === 0) {
                hub.currentIndex = hub._actionIndexForId(hub._emptyCatalogFallbackAction);
            } else {
                hub.currentIndex = hub._flatIndexForCategory(chosenCategory);
            }
        } else if (savedRow === 1 && savedAction !== "") {
            hub.currentIndex = hub._actionIndexForId(savedAction);
        } else if (idx >= 0) {
            hub.currentIndex = hub._flatIndexForCategory(chosenCategory);
        } else if (hub.resumeActionVisible) {
            // Non-persisting seat: this is a programmatic restore, so it must
            // not commit like focusResumeIfVisible() would. Committing here
            // clobbers a saved category-row intent whenever the catalog is
            // momentarily empty (cold-boot restore, in-session catalog
            // refresh), stranding the user on Resume when they back out.
            hub.currentIndex = hub._actionIndexForId("resume");
        } else if (Browse.CategoriesModel.count === 0) {
            hub.currentIndex = hub._actionIndexForId(hub._emptyCatalogFallbackAction);
        } else {
            hub.currentIndex = hub._flatIndexForCategory(chosenCategory);
        }

        if (!cascadeSystems)
            return;
        // Cold boot before Core delivers the catalog: focus was seated above
        // (and `_restoreDone` set, so the focus ring paints immediately
        // instead of waiting for the connection), but the set_category
        // cascade needs the real catalog. Defer it — this function re-runs
        // on CategoriesModel.onModelReset once the catalog lands, and the
        // cascade fires then.
        if (Browse.CategoriesModel.count <= 0)
            return;
        if (Browse.SystemsModel.current_category === chosenCategory && Browse.SystemsModel.count > 0)
            return;
        Browse.SystemsModel.set_category(chosenCategory);
    }

    // Side-effect of every focus move: persist HubState. We do NOT call
    // SystemsModel.set_category here — that one's reserved for Accept
    // (and the router orchestrates it). Calling it on every left/right
    // press fires two model resets per press, each destroying-and-
    // recreating SystemsScreen's bound delegates on the UI thread —
    // choppy on MiSTer even though SystemsScreen is `visible: false`.
    function _commitCurrent(): void {
        const entry = hub.items[hub.currentIndex];
        if (!entry || entry.kind === "empty")
            return;
        Browse.HubState.selected_item = entry.kind + ":" + entry.id;
        if (entry.kind === "category") {
            Browse.HubState.selected_row = 0;
            if (entry.id !== "")
                Browse.HubState.category = entry.id;
        } else {
            // `selected_row`/`selected_action` are the fallback restore
            // path for a state.toml written before `selected_item` existed
            // (round 6) -- pre-dates every kind but category/action, so
            // "not category" is a fine generic bucket here; `selected_item`
            // above is what actually restores a system/folder/zapscript
            // tile precisely.
            Browse.HubState.selected_row = 1;
            Browse.HubState.selected_action = entry.id;
        }
    }

    // Emit the navigation signal for the currently selected entry.
    // Separated from _activateCurrent so DeferredAction can call it after
    // the push-in cue has had time to play. `requestAccept`'s payload
    // meaning varies by kind -- category/action id, system id, folder
    // path, or the raw ZapScript text -- Main.qml's router dispatches on
    // `kind` to know which.
    function _emitActivate(): void {
        const entry = hub.items[hub.currentIndex];
        if (!entry || entry.kind === "empty")
            return;
        if (entry.kind === "category") {
            // Unconfirmed/absent is a quiet no-op, same as a disabled
            // action below -- distinct from a genuine Core error, which
            // still offers Retry.
            if (entry.disabled === true)
                return;
            if ((Browse.CategoriesModel.error_message ?? "") !== "") {
                hub.requestRetry();
                return;
            }
            // During optimistic boot the visible category row is backed
            // by localized placeholder labels. Accept the stable category
            // id, not the display name, so persisted HubState and router
            // comparisons remain locale-independent.
            hub.requestAccept("category", entry.id, "");
            return;
        }
        if (entry.kind === "action") {
            if (entry.disabled === true)
                return;
            hub.requestAccept("action", entry.id, "");
            return;
        }
        if (entry.kind === "system") {
            hub.requestAccept("system", entry.id, "");
            return;
        }
        if (entry.kind === "folder") {
            hub.requestAccept("folder", entry.path, entry.system);
            return;
        }
        if (entry.kind === "zapscript") {
            hub.requestAccept("zapscript", entry.script, "");
        }
    }

    function _activateCurrent(): void {
        const entry = hub.items[hub.currentIndex];
        if (!entry || entry.kind === "empty")
            return;
        hub.activatePulse++;
        hub._commitCurrent();
        pressCommit.arm();
    }

    // ── Move (Options -> Move) ──────────────────────────────────────────
    // Armed by Main.qml's `hub_move` dispatch (see handleContextMenuAccepted
    // there). Board model: every direction/page press SWAPS the held tile
    // with whatever occupies the destination cell (another tile, or a
    // blank) — always exactly two cells change, never a splice, so a
    // deliberate gap the user left elsewhere is never disturbed. Rust owns
    // the session (`begin_move`/`move_held_to`/`commit_move`/`cancel_move`,
    // `rust/frontend/src/models/hub_layout.rs`): every intermediate press
    // mutates in memory only, so a fast run of presses costs zero disk I/O;
    // Accept performs the session's one real save, Cancel restores the
    // pre-session snapshot exactly (nothing was ever written to disk to
    // undo). `_moveStartFlat` is the FLAT `items` index (not a
    // Browse.HubLayout position) the held tile started at, so Cancel can
    // reseat the cursor directly with no translation needed — the
    // restored snapshot renders byte-identical to how it looked at
    // `beginMove` time.
    property bool moveArmed: false
    property int _moveStartFlat: -1
    // Total page budget for the whole session, frozen at arm time (current
    // real content's pages, plus exactly one reserve) — see
    // `_padToPageSize`'s doc comment for why this must be a floor computed
    // ONCE rather than recomputed from "current real content" on every
    // press: the latter keeps receding a page further every time a press
    // makes Rust materialize new real blanks to reach a target, producing
    // unbounded page growth. `0` outside a session (a no-op floor).
    property int _moveArmedTotalPages: 0

    function beginMove(hubIndex: int): void {
        if (hubIndex < 0)
            return;
        // The tile Options was opened on is always still under the cursor
        // (menus don't let focus move while open) — reject rather than
        // guess if that's somehow not true.
        const current = hub.items[hub.currentIndex];
        if (!current || current.hubIndex !== hubIndex)
            return;
        if (!Browse.HubLayout.begin_move(hubIndex))
            return;
        hub._moveStartFlat = hub.currentIndex;
        hub._moveArmedTotalPages = Math.max(1, Math.ceil(Browse.HubLayout.item_count() / hub._pageSize)) + 1;
        hub.moveArmed = true;
    }

    // View -> Add item… arms Move on whatever it just placed, instead of
    // relying on the cursor already resting on the target cell the way
    // `beginMove` above assumes (Options always opens on the tile it
    // moves). With `skipEmptyCells` on outside a Move session, the cursor
    // can no longer normally be parked on a blank -- entering empty space
    // only ever happens while carrying something, so Add now hands the
    // user something to carry rather than silently placing it wherever
    // Rust's own append/target rule happened to land it. See
    // Main.qml's `hub_add_pick` handler for the caller.
    //
    // Seating the cursor here is a raw jump, not a `moveSelection` step,
    // so it skips the commit-on-navigate pairing every directional press
    // gets in `handleAction` -- `_commitCurrent()` here is what keeps
    // `HubState.selected_item` pointing at the newly added tile rather
    // than wherever the cursor was before the View menu opened. That
    // matters because a Move session itself is in-memory only (see
    // `beginMove`'s doc comment): if MiSTer kills the process mid-Move,
    // only this committed selection survives, not the tile's held
    // position.
    function armMoveForHubIndex(hubIndex: int): void {
        if (hubIndex < 0)
            return;
        const flat = hub.items.findIndex(entry => entry && entry.hubIndex === hubIndex);
        if (flat < 0)
            return;
        hub.currentIndex = flat;
        hub._commitCurrent();
        hub.beginMove(hubIndex);
    }

    function _clearMoveState(): void {
        hub.moveArmed = false;
        hub._moveStartFlat = -1;
        hub._moveArmedTotalPages = 0;
    }

    function _acceptMove(): void {
        Browse.HubLayout.commit_move();
        hub._clearMoveState();
    }

    function _cancelMove(): void {
        Browse.HubLayout.cancel_move();
        hub.currentIndex = hub._moveStartFlat;
        hub._clearMoveState();
        hub._commitCurrent();
    }

    // Translate a flat `items` index into the Browse.HubLayout position
    // placing a tile there means: the entry's own `hubIndex` when it's
    // already backed by the layout (a real tile, or a real persisted
    // blank), or — for `_padToPageSize`'s synthetic tail padding
    // (`hubIndex === -1`) — the position placing a tile there would CREATE,
    // offset from `item_count()` by how far past the first padding cell the
    // target sits. Padding is always contiguous at the very end of `items`
    // and mirrors the same order Rust's real positions would extend into,
    // so this is well-defined for any flat index a cursor can reach.
    function _realIndexForFlat(flatIndex: int): int {
        const entry = hub.items[flatIndex];
        if (!entry)
            return -1;
        if (entry.hubIndex >= 0)
            return entry.hubIndex;
        const firstPad = hub._firstPadFlatIndex();
        if (firstPad < 0)
            return -1;
        return Browse.HubLayout.item_count() + (flatIndex - firstPad);
    }

    // Flat index of the first synthetic tail-padding entry, or `items`'
    // length if there is none. Valid only once the layout is seeded (every
    // resolved entry then carries a real `hubIndex`, so padding is the
    // only source of `hubIndex === -1`) — exactly the state Move requires
    // to arm in the first place.
    function _firstPadFlatIndex(): int {
        for (let i = 0; i < hub.items.length; i++) {
            if (hub.items[i].hubIndex < 0)
                return i;
        }
        return hub.items.length;
    }

    // Board model: a held tile must never wrap around the grid's edges.
    // Normal navigation's closed loop (Up on row 0 wraps to the last page —
    // see PagedGrid.moveSelection, deliberately left alone for ordinary
    // browsing) makes sense when you're just moving focus around; it does
    // NOT make sense while carrying a tile, where it would silently
    // teleport the held tile to the opposite end of the grid. Checked here,
    // not in PagedGrid, so normal navigation everywhere else is untouched.
    // Pure geometry -- reads only pagedGrid's current row/column/page and
    // shape, never the held tile or Rust -- so this is exercised directly
    // in tests without needing a seeded Browse.HubLayout.
    function _wouldWrapVertically(dRow: int): bool {
        if (dRow > 0)
            return pagedGrid.currentRow === pagedGrid.rows - 1 && pagedGrid.currentPage === pagedGrid.totalPageCount - 1;
        if (dRow < 0)
            return pagedGrid.currentRow === 0 && pagedGrid.currentPage === 0;
        return false;
    }

    // Same shape as `_wouldWrapVertically`, for L/R same-row wrap. Ordinary
    // browsing's row-wrap clamps to a partial last row's actual filled
    // span (see PagedGrid.moveSelection's `maxColOnRow`) so it never walks
    // through a hole -- but Move's padding (`_moveArmedTotalPages`) always
    // fills whole pages, so every row a held tile can reach is exactly
    // `columns` wide and there's never a partial row to special-case. Left
    // at column 0 (or Right at the last column) would otherwise wrap
    // within the same row onto a synthetic tail-padding cell, which
    // `move_held_to` then materializes into a brand-new real position far
    // from where the tile actually was -- the held tile silently jumps to
    // the opposite edge of its row while the tile it was previously
    // swapped with is left stranded, never actually touched by the press
    // that seemed to move past it. Same class of bug `_wouldWrapVertically`
    // guards against, just on the horizontal axis.
    function _wouldWrapColumn(dCol: int): bool {
        if (dCol > 0)
            return pagedGrid.currentColumn === pagedGrid.columns - 1;
        if (dCol < 0)
            return pagedGrid.currentColumn === 0;
        return false;
    }

    // Same shape as `_wouldWrapVertically`, for L/R shoulder page jumps.
    function _wouldWrapPage(delta: int): bool {
        if (delta < 0)
            return pagedGrid.currentPage === 0;
        if (delta > 0)
            return pagedGrid.currentPage === pagedGrid.totalPageCount - 1;
        return false;
    }

    function _moveSplice(dCol: int, dRow: int): void {
        const fromFlat = hub.currentIndex;
        const heldEntry = hub.items[fromFlat];
        if (!heldEntry || heldEntry.hubIndex < 0)
            return;
        if (hub._wouldWrapColumn(dCol))
            return;
        if (hub._wouldWrapVertically(dRow))
            return;
        if (!pagedGrid.moveSelection(dCol, dRow))
            return;
        const toReal = hub._realIndexForFlat(hub.currentIndex);
        if (toReal < 0 || !Browse.HubLayout.move_held_to(heldEntry.hubIndex, toReal)) {
            hub.currentIndex = fromFlat;
            return;
        }
        hub._commitCurrent();
    }

    function _movePage(delta: int): void {
        const fromFlat = hub.currentIndex;
        const heldEntry = hub.items[fromFlat];
        if (!heldEntry || heldEntry.hubIndex < 0)
            return;
        if (hub._wouldWrapPage(delta))
            return;
        if (!pagedGrid.pageBy(delta))
            return;
        const toReal = hub._realIndexForFlat(hub.currentIndex);
        if (toReal < 0 || !Browse.HubLayout.move_held_to(heldEntry.hubIndex, toReal)) {
            hub.currentIndex = fromFlat;
            return;
        }
        hub._commitCurrent();
    }

    // context_menu / page_menu are deliberately unhandled here: opening
    // another menu (or the View rail) mid-move would leave the item picked
    // up behind it. Accept/Cancel are the only way out.
    function _handleMoveAction(action: string): void {
        if (action === "left")
            hub._moveSplice(-1, 0);
        else if (action === "right")
            hub._moveSplice(1, 0);
        else if (action === "down")
            hub._moveSplice(0, 1);
        else if (action === "up")
            hub._moveSplice(0, -1);
        else if (action === "page_prev")
            hub._movePage(-1);
        else if (action === "page_next")
            hub._movePage(1);
        else if (action === "accept")
            hub._acceptMove();
        else if (action === "cancel")
            hub._cancelMove();
    }

    // Entries for View -> "Add item…": every known category/action key not
    // currently in the layout (Browse.HubLayout.available_*), resolved
    // through the same per-kind resolvers the rest of this file uses so
    // labels match what the tile will actually show once added. No "Blank
    // space" entry -- a blank tile is an implementation detail (a
    // deliberate gap), not something the user creates as a first-class
    // menu choice, same reasoning as why Options never opens on one. Gaps
    // still form naturally through Hide/Delete and through Move leaving
    // one behind. Called by Main.qml's View handling.
    function buildAddEntries(): var {
        const entries = [];
        const count = Browse.HubLayout.available_count();
        for (let i = 0; i < count; i++) {
            const kind = Browse.HubLayout.available_kind_at(i);
            const id = Browse.HubLayout.available_id_at(i);
            // Resume is a special case among the resolvers this loop
            // otherwise defers to: _resolveActionEntry deliberately returns
            // the last-played GAME's name (so the live tile matches what it
            // will show once added), which reads as unrecognizable as "the
            // Resume slot" in a bare list of menu rows -- and returns null
            // entirely while no history is currently resumable, which would
            // silently drop Resume from this picker even though it's still
            // a valid, re-addable slot. Bypass the resolver here and always
            // offer the literal "Resume" label (same source string
            // _resolveActionEntry itself falls back to, so no new
            // translatable text).
            if (kind === "action" && id === "resume") {
                entries.push({
                    id: "action:resume",
                    label: qsTr("Resume")
                });
                continue;
            }
            const resolved = kind === "category" ? hub._resolveCategoryEntry(id) : kind === "action" ? hub._resolveActionEntry(id) : null;
            if (!resolved)
                continue;
            entries.push({
                id: kind + ":" + id,
                label: resolved.name
            });
        }
        return entries;
    }

    function handleAction(action: string): void {
        hub._focusArmed = true;
        if (hub.moveArmed) {
            hub._handleMoveAction(action);
            return;
        }
        if (action === "left") {
            if (pagedGrid.moveSelection(-1, 0))
                hub._commitCurrent();
        } else if (action === "right") {
            if (pagedGrid.moveSelection(1, 0))
                hub._commitCurrent();
        } else if (action === "down") {
            if (pagedGrid.moveSelection(0, 1))
                hub._commitCurrent();
        } else if (action === "up") {
            if (pagedGrid.moveSelection(0, -1))
                hub._commitCurrent();
        } else if (action === "page_prev") {
            // L shoulder — every other paged screen supports this; the Hub
            // didn't because paging used to be a rare edge case here. Now
            // that grouping padding is gone (items flow across pages with
            // no gap), it's the normal case.
            if (pagedGrid.pageBy(-1))
                hub._commitCurrent();
        } else if (action === "page_next") {
            // R shoulder.
            if (pagedGrid.pageBy(1))
                hub._commitCurrent();
        } else if (action === "accept") {
            hub._activateCurrent();
        } else if (action === "context_menu") {
            const entry = hub.items[hub.currentIndex];
            if (!entry)
                return;
            // A category id / action id that Core hasn't confirmed yet (the
            // bootstrap placeholder window, `hubIndex === -1`) is safe to
            // emit for "category"/"action" kinds -- Main.qml's id-to-index
            // resolution (categories) or the `hubIndex < 0` guard (actions,
            // Move/Remove) just no-ops rather than mutating anything.
            if (entry.kind === "category") {
                hub.requestContextMenu(entry.hubIndex, entry.id, pagedGrid.currentCellRectIn(hub), pagedGrid.currentCellRadius);
            } else if (entry.kind === "action") {
                hub.requestActionContextMenu(entry.hubIndex, entry.id, pagedGrid.currentCellRectIn(hub));
            } else if (entry.kind !== "empty" && entry.hubIndex >= 0) {
                // system / folder / zapscript only -- `kind === "empty"`
                // covers BOTH a real persisted blank tile and synthetic
                // tail padding. Neither opens Options: a blank is an
                // implementation detail (a deliberate gap), not something
                // you pick up and move on its own -- only a real tile
                // lands ON a gap, absorbing it; the gap itself is inert.
                hub.requestItemContextMenu(entry.hubIndex, entry.kind, pagedGrid.currentCellRectIn(hub), pagedGrid.currentCellRadius);
            }
        } else if (action === "page_menu") {
            hub.requestPageMenu();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    DeferredAction {
        id: pressCommit
        onDeferred: hub._emitActivate()
    }

    Component {
        id: tileDelegate
        Tile {}
    }

    // Genuinely blank placeholder for the `isEmpty` rows `_padToPageSize`
    // appends to the last page — see EmptySlot.qml and docs/style.md ->
    // "Empty slots". PagedGrid resolves this in place of `tileDelegate` for
    // any row whose `isEmpty` role is true; every other PagedGrid caller
    // leaves `emptyDelegate` at its default `null` and is unaffected.
    Component {
        id: emptySlotDelegate
        EmptySlot {}
    }

    // Lightweight mirror of `items`, patched in place by `_syncGridModel` --
    // see that function's doc comment. This is what `pagedGrid.model` binds
    // to; every other read of an entry goes through `hub.items` directly.
    ListModel {
        id: hubGridModel
    }

    // One uniform grid replaces round <=5's two hand-positioned Repeater
    // rows. `columnsOverride`/`rowsOverride` pin the shape to the fixed
    // per-tier table (see Sizing.qml's `hubGridShape`) instead of the
    // viewport-fitted default every other PagedGrid caller uses.
    PagedGrid {
        id: pagedGrid

        // Square cells (docs/style.md -> "Tile aspect and grid blocks":
        // "Hub rows remain square"), fit against BOTH axes of the Hub's own
        // reserved band — `squareCells` clamps cellWidth/cellHeight to the
        // smaller of the two independent fits. `heightBudget` (rather than
        // this item's own `height`) is what the height half of that fit
        // reads, because `height` below is itself derived FROM the fitted
        // cell size; fitting against `height` directly would be circular.
        squareCells: true
        heightBudget: hub._gridHeightBudget

        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width
        height: pagedGrid.topInset + pagedGrid.bottomInset + pagedGrid.rows * pagedGrid.cellHeight + (pagedGrid.rows - 1) * pagedGrid.cellSpacingY
        y: hub._blockY

        model: hubGridModel
        delegate: tileDelegate
        emptyDelegate: emptySlotDelegate
        columnsOverride: Sizing.hubGridColumns
        rowsOverride: Sizing.hubGridRows
        heldIndex: hub.moveArmed ? hub.currentIndex : -1
        // Normal browsing steps over blanks as if they weren't there; a
        // Move session must still be able to target one (or the reserve
        // page `beginMove` pads in) -- that's the whole mechanic. See
        // PagedGrid.qml's `skipEmptyCells` doc comment.
        skipEmptyCells: !hub.moveArmed

        activatePulse: hub.activatePulse
        screenSettling: !hub.visible
        focusReady: hub._focusReady

        // Hide the whole grid while the router holds us here on a forward
        // transition or while its load error is shown — the error text
        // then paints alone against the screen background instead of over
        // stale/broken category tiles. Unlike round <=5, this now also
        // hides the actions block during a categories error (previously
        // only categories hid); a full-grid error state is simpler and the
        // user can still Cancel/quit or retry.
        visible: !hub.transitioning && (Browse.CategoriesModel.error_message ?? "") === ""

        onItemHovered: index => {
            hub._focusArmed = true;
            hub._commitCurrent();
        }
        onItemClicked: index => {
            hub._focusArmed = true;
            hub._commitCurrent();
            hub._activateCurrent();
        }
        onItemRightClicked: index => {
            hub._focusArmed = true;
            hub._commitCurrent();
            const entry = hub.items[index];
            if (!entry)
                return;
            // Mirror `handleAction`'s "context_menu" dispatch exactly --
            // right-click and Options (X) open the same menu. Previously
            // called these signals with the wrong arity (`entry.id` first,
            // no `hubIndex` at all), so a right-click's Move/Hide/Delete
            // silently targeted whatever `_hubItemIndex` a coerced string
            // happened to become in Main.qml, not this tile.
            if (entry.kind === "category") {
                hub.requestContextMenu(entry.hubIndex, entry.id, pagedGrid.currentCellRectIn(hub), pagedGrid.currentCellRadius);
            } else if (entry.kind === "action") {
                hub.requestActionContextMenu(entry.hubIndex, entry.id, pagedGrid.currentCellRectIn(hub));
            } else if (entry.kind !== "empty" && entry.hubIndex >= 0) {
                // See handleAction's "context_menu" branch: a blank tile
                // (kind === "empty") never opens Options.
                hub.requestItemContextMenu(entry.hubIndex, entry.kind, pagedGrid.currentCellRectIn(hub), pagedGrid.currentCellRadius);
            }
        }
        // Mirrors handleAction's page_prev/page_next dispatch (including
        // the moveArmed branch) for the mouse-wheel path -- previously
        // unwired, so scrolling the wheel over the Hub grid silently did
        // nothing even though PagedGrid already emitted this signal.
        onPageWheelRequested: delta => {
            if (hub.moveArmed) {
                hub._movePage(delta);
                return;
            }
            if (pagedGrid.pageBy(delta))
                hub._commitCurrent();
        }
    }

    // Footer row — single big line under the grid, swaps text on every
    // move, plus a page cue in the reserved right slot (same one-third
    // discipline TopStatusStrip uses; see PageIndicator.qml). Hidden
    // during a forward transition like the grid above (`hub.transitioning`)
    // — the label used to stay up under the destination's "Loading…" cue
    // (e.g. "Recently Played" lingering while Games loads), reading as
    // stale source context rather than the settled label the Favorites
    // path already showed correctly.
    ActiveLabel {
        id: activeLabel

        anchors.top: pagedGrid.bottom
        anchors.topMargin: hub._verticalGap
        anchors.left: parent.left
        anchors.right: parent.right
        height: hub._activeLabelHeight
        // Reserves the same width as the PageIndicator's corner slot
        // below so a long tile name elides before it reaches the
        // chevrons/page count, rather than running underneath them.
        sideInset: Sizing.px(hub.width / 3)
        text: {
            // currentIndex can briefly outrun items.length during cold
            // launch, before HubState is clamped to it. Guard the lookup so
            // an undefined access doesn't surface as a TypeError in the log.
            const entry = hub.items[hub.currentIndex];
            return entry && entry.kind !== "empty" ? entry.name : "";
        }
        // Worded reason for a disabled tile -- the detail cue, next to the
        // muted front edge's glanceable one (Tile.qml's `edgeColor`). Hub
        // tiles carry no per-tile caption to fold this into the way
        // Games/Favorites/Recents do (see Tile.qml's caption `tags`), so it
        // surfaces here instead, only while that tile is focused.
        tags: {
            const entry = hub.items[hub.currentIndex];
            return entry && entry.disabled === true ? (entry.stateReason ?? "") : "";
        }
        // Hidden while a categories error is showing AND the current tile
        // is a category, so the error text can show alone instead of a
        // stale label -- was an index-range check (`currentIndex >=
        // _categoryItems.length`, "we're on an action") back when
        // categories always occupied a fixed prefix of `items`; a kind
        // check is what that meant and still means now that any kind can
        // sit at any position.
        visible: {
            if (hub.transitioning)
                return false;
            const entry = hub.items[hub.currentIndex];
            return (!entry || entry.kind !== "category") || (Browse.CategoriesModel.error_message ?? "") === "";
        }
    }

    // Page cue — right corner of the footer row, alongside activeLabel.
    // No left-corner counterpart: Hub has no natural "N items" count the
    // way Systems/Games do, so that slot simply stays empty (still the
    // same reserved one-third width, for consistency with every other
    // footer -- there just isn't anything to put there).
    PageIndicator {
        id: hubPageIndicator

        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(3)
        anchors.verticalCenter: activeLabel.verticalCenter
        currentPage: pagedGrid.currentPage
        totalPages: pagedGrid.totalPageCount
        hasPagesAbove: pagedGrid.hasPagesAbove
        hasPagesBelow: pagedGrid.hasPagesBelow
        visible: activeLabel.visible

        onPageRequested: delta => {
            if (hub.moveArmed) {
                hub._movePage(delta);
                return;
            }
            if (pagedGrid.pageBy(delta))
                hub._commitCurrent();
        }
    }

    // CategoriesModel has no `loading` qproperty — the catalog is
    // fetched eagerly via bind_to_endpoint!. The brief cold-launch
    // window where count===0 surfaces as "No categories" is acceptable
    // per the "Loading is brief" locked decision in MVP_PLAN.md.
    ScreenStateOverlay {
        x: pagedGrid.x + Sizing.center(pagedGrid.width, width)
        y: pagedGrid.y + Sizing.center(pagedGrid.height, height)
        width: pagedGrid.width
        height: pagedGrid.height
        enabled: Browse.CategoriesModel.loaded || (Browse.CategoriesModel.error_message ?? "") !== ""
        loading: false
        errorMessage: Browse.CategoriesModel.error_message ?? ""
        count: Browse.CategoriesModel.loaded ? Browse.CategoriesModel.count : 1
        emptyText: qsTr("No systems available. Run Update media database from Settings.")
    }
}
