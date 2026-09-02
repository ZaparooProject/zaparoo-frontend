// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// layoutProfile and its sub-properties (_gridProfile.leftInset etc.) are
// QVariant-typed JS objects; cannot be statically typed. Structural; suppress compiler.
// qmllint disable compiler

// Bound component behavior is required because the inner Repeater +
// Loader bind to root.* properties (delegate, focused, cellWidth, …)
// across component boundaries. Keep all enclosing-scope reads explicit
// via the file-scope `root` id so qmllint can verify them; do not
// introduce intermediate Items that rely on implicit lookups.
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Paged grid of tiles. Items flow row-major within a page; pages stack
// **vertically** — pressing Down at the bottom row swaps in the next
// page instantly (same column, top row), Up at the top row swaps in
// the previous page (same column, bottom row). Crossing past the last
// page wraps to page 0; mirror for Up at page 0. Left/Right wrap
// **within the current row** and never change pages, so a partial last
// row cycles among its own filled cells.
//
// Selection is `currentIndex` over the source model; (page, row, col)
// are derived. Cells size themselves to the available container minus
// reserved chrome — callers pass a model and delegate, the grid
// handles layout, unconditionally centering the cell block against the
// full inset-to-inset width. There is no in-grid scroll indicator —
// grids are paged, not scrolled; the host screen's footer renders the
// page cue (`PageIndicator.qml`) off `hasPagesAbove`/`hasPagesBelow`/
// `currentPage`/`totalPageCount`, which stay derived from
// `totalItemsOverride` (with `itemCount` fallback) exactly as before, so
// a paginated caller like `GamesScreen` still reflects the dataset's
// true total rather than just the loaded slice.
//
// Page changes are instant cuts (no fade, no slide). On Qt Quick's
// Software adaptation the renderer cannot keep up with a per-frame
// alpha ramp over a busy grid — translucent overlays don't subtract
// from the dirty region, so every cell underneath re-rasterizes per
// frame (cover images, card bodies). See docs/qml-gotchas.md →
// "Software-renderer animation costs" for the full reasoning.
Item {
    id: root

    required property var model
    required property Component delegate
    // Optional. When set, a model row whose `isEmpty` role is true renders
    // through this component instead of `delegate` — a genuinely blank,
    // structural placeholder rather than a real delegate with nothing on
    // it. `null` (every caller but the Hub) is byte-identical to today:
    // `isEmpty` is still a required role every model must supply (see
    // below), but with no `emptyDelegate` every row just renders through
    // `delegate` regardless of its value. See EmptySlot.qml and
    // HubScreen.qml's `_padToPageSize`.
    property Component emptyDelegate: null
    // When true, every cursor path (`moveSelection`, `pageBy`, and the
    // per-cell MouseArea) treats `isEmpty` rows as unreachable rather than
    // an ordinary cell -- the cursor steps past them as if they weren't
    // there instead of landing on them. `false` (default, every caller but
    // the Hub outside a Move session) is byte-identical to before: those
    // models hardcode `isEmpty` to `false` on every row anyway, so this
    // property is a no-op for them regardless of its value. The Hub binds
    // it to `!moveArmed` -- Move must still be able to target a blank (or
    // the reserve page `beginMove` pads in), which is why this is a
    // property the host toggles rather than a permanent behavior. See
    // EmptySlot.qml and docs/style.md -> "Empty slots".
    property bool skipEmptyCells: false

    property int currentIndex: 0
    // List layout keeps this grid as cursor/page authority while rendering a
    // separate row view. Removing the hidden Repeater model avoids one cell
    // object and role-binding set per loaded row; count still follows source
    // model so navigation math remains unchanged.
    property bool suspendDelegates: false
    readonly property int itemCount: root.suspendDelegates ? (root.model && root.model.count !== undefined ? root.model.count : 0) : itemRepeater.count

    // Whether this section currently owns user focus. Tile uses this to
    // gate the selection card so only one section shows the focus cue
    // at a time on screens that host more than one tile section.
    // Defaults to true so call sites that don't care keep working
    // untouched.
    property bool focused: true
    property bool coverLoadingPaused: false
    // False keeps delegate/cursor structure alive while withholding Image
    // sources. Useful during a model replacement: unlike suspending Repeater,
    // it avoids a synchronous teardown/rebuild while still preventing cold
    // cover decodes behind a transition cue.
    property bool coverRequestsEnabled: true
    property bool rapidRenderMode: false
    // Number of pages after current whose covers are decoded speculatively.
    // Media grids keep one page warm; SVG-heavy systems grids can set zero and
    // rely on their router-owned visible-page prefetch gate.
    property int coverLookaheadPages: 1
    // When false, focused tint variants are requested only for selected tile.
    // Normal grids preserve eager variants; systems opt out because every
    // variant requires a separate SVG raster on MiSTer's small ARM CPU.
    property bool eagerFocusedCovers: true
    // Decode bundled artwork inline so a page's logos paint with the page.
    // Narrowed per cell below: only the current page, and never during
    // rapidRenderMode. Hosts that want the old reader-thread behavior for every
    // cell can set this false.
    property bool coverSynchronous: true
    // Sizing.visibleCovers is a tile count, not a page count. Convert it through
    // current column count; treating five covers as five whole pages retained up
    // to 110 live Tile trees and made deep-page Back block Qt's main thread.
    readonly property int _coverRetentionPages: Math.max(1, Math.ceil(Sizing.visibleCovers / Math.max(1, root.columns)))
    // Pulse counter for the one-shot tile press. Callers increment via
    // pulseActivate(); TileLoader forwards the value to Tile where only the
    // focused+selected delegate lowers its face. The same cue serves both
    // forward navigation and game launch.
    property int activatePulse: 0
    function pulseActivate(): void {
        root.activatePulse++;
    }
    // Release counter for the press cue. Callers increment via
    // releaseActivate() to raise the focused tile after a launch that keeps the
    // frontend on the same screen (e.g. an Audio track). Forward navigation
    // never calls this — the screen transition + `settling` reset handles it.
    property int releasePulse: 0
    function releaseActivate(): void {
        root.releasePulse++;
    }
    // When true, Tile delegates return their raised face to rest so a held
    // press from the previous visit does not persist when
    // the screen is shown again. Set by the host screen to `!active`
    // (i.e. true while the screen is off-screen).
    property bool screenSettling: false
    // Forwarded to each Tile's focus-visibility gate. Host screens leave this
    // false until the grid's selection is finalized (restore or first input)
    // so the default tile 0 never paints a ring before restore lands; default
    // true keeps focus rendering on for hosts that do not wire it.
    property bool focusReady: true
    // Hide only cell delegates while retaining scrollbar chrome. Rapid-scroll
    // snapshot mode uses this so the frozen grid replaces cells without making
    // the live gutter disappear.
    property bool cellsVisible: true
    // Optional index -> string callback for a compact label above tile art.
    // Empty/null keeps existing tile geometry unchanged.
    property var tileTopLabelProvider: null
    property var layoutProfile: null
    readonly property var _gridProfile: root.layoutProfile && root.layoutProfile.grid ? root.layoutProfile.grid : null
    readonly property var _surfaceProfile: root.layoutProfile && root.layoutProfile.surface ? root.layoutProfile.surface : null
    readonly property int _cardRadius: root._surfaceProfile ? root._surfaceProfile.cardRadius : Sizing.radiusMd

    // When true, both `cellWidth` and `cellHeight` clamp to the smaller of
    // the two independent per-axis fits, producing square cells regardless
    // of which axis is the binding constraint — see `cellWidth`/`cellHeight`
    // below. Opt-in: every existing caller defaults false and keeps
    // dividing each axis independently, byte-identical to before (see
    // docs/style.md -> "Tile aspect and grid blocks").
    property bool squareCells: false
    // Fixed vertical ceiling used for the height-fit half of `squareCells`,
    // in place of the item's own `height` — needed by a caller (the Hub)
    // that derives ITS `height` FROM the fitted cell size; fitting against
    // `height` in that case would be circular (the fit output would be an
    // input to itself). Every other existing caller already fixes `height`
    // independently (anchors to a sibling), so -1 (the default) simply
    // reproduces the item's own height and changes nothing. Unlike `height`,
    // this is the RAW ceiling — `topInset`/`bottomInset` are subtracted from
    // it internally, same as they are from `height`.
    property int heightBudget: -1
    // Index of a tile the host has picked up for a reorder (the Hub's
    // Options -> Move). -1 (default, every other caller) means nothing is
    // held. The held cell's Tile blinks a solid full-tile overlay on top
    // of itself — see Tile.qml's `delegateHeld` — a single-cell change,
    // not a grid-wide animation, matching CLAUDE.md's dirty-rect rule for
    // the software renderer. The cell's geometry (position, z-order) is
    // otherwise untouched — held does not lift, sink, or resize anything.
    property int heldIndex: -1

    // Emitted when the user is sitting on the last loaded page after a
    // selection move. Models with more data fetch the next page in
    // response; models without ignore it. The grid does not know
    // whether the model has more data — that is a model concern, kept
    // out of this component so it stays generic.
    signal loadMoreRequested(bool urgent)
    // Mouse entry points. Screens own persistence and activation side
    // effects, so the grid only updates its focused index and reports the
    // row the pointer targeted.
    signal itemHovered(int index)
    signal itemClicked(int index)
    signal itemRightClicked(int index)
    signal emptyRightClicked
    signal pageWheelRequested(int delta)

    // Per-instance shape overrides. -1 means "use the shared browse-grid
    // default". Real browse screens now override explicitly, but the
    // fallback stays useful for generic callers and tests.
    property int columnsOverride: -1
    property int rowsOverride: -1
    readonly property int columns: columnsOverride > 0 ? columnsOverride : Sizing.systemsGridColumns
    readonly property int rows: rowsOverride > 0 ? rowsOverride : Sizing.systemsGridRows

    // Pages of buffer to keep ahead of the user's current page before
    // firing `loadMoreRequested`. With `loadAheadPages: 2` the trigger
    // fires when the user enters the second-to-last loaded page,
    // overlapping the RPC + model insert with a full page of selection
    // travel so the new chunk lands before they reach the loaded edge.
    // The model's `loading_more` debounce collapses repeated emissions
    // while a fetch is in flight, so firing earlier doesn't fan out.
    property int loadAheadPages: 2
    readonly property int pageSize: columns * rows
    // Snap to the last fully-loaded page boundary while more chunks are
    // still on the way. Otherwise the user reaches a half-full trailing
    // page, sees "Loading more…", then watches the rest pop in mid-page
    // when the chunk lands. Once `hasMorePages` flips false the partial
    // last page becomes legitimate (it's the real end of the dataset)
    // and we ceil to include it. The `Math.max(1, ...)` floor guard
    // keeps the very first sub-page-sized load from collapsing pageCount
    // to 0 while the initial chunk is still landing.
    readonly property int pageCount: {
        if (itemCount <= 0)
            return 1;
        if (root.hasMorePages)
            return Math.max(1, Math.floor(itemCount / pageSize));
        return Math.ceil(itemCount / pageSize);
    }
    readonly property int currentPage: Math.floor(currentIndex / pageSize)
    readonly property int currentColumn: (currentIndex % pageSize) % columns
    readonly property int currentRow: Math.floor((currentIndex % pageSize) / columns)

    // Page-stack indicators. Used by the right-gutter scroll cue (and
    // by callers that want to drive their own indicator) to gate the
    // up/down arrows. Both are false on a single-page dataset.
    // These intentionally track loaded `pageCount` — they reflect what
    // the user can actually navigate to right now, not a paginated
    // model's reported total.
    readonly property bool hasPagesAbove: currentPage > 0
    // A model that says it has more rows coming has a page below,
    // whether or not the total is known -- the old `!paginationTotalKnown
    // &&` term meant a *known*-total paginated model (Games: fetched
    // exactly `pageSize` rows on first load, `paginationTotalKnown: true`,
    // `hasMorePages: true`) reported `hasPagesBelow: false` on entry:
    // `pageCount` is `floor(itemCount / pageSize) === 1` (only the loaded
    // rows count), so `currentPage(0) < pageCount(1) - 1` is false, and the
    // known-total branch of the old OR term never even looked at
    // `hasMorePages`. The chevrons and "N / M" readout (both gated on this
    // flag -- see PageIndicator.qml's `_hasMultiplePages`) then stayed
    // hidden until the first d-pad move triggered `loadMoreRequested` and
    // grew `itemCount` past a second page. The count itself was always
    // right (it reads the model's own authoritative total, not this flag);
    // only this visibility predicate under-reported it. `totalPageCount`
    // (below) is the more semantically direct comparison here -- and
    // `pageBy()` already wraps against it, so today's chevrons under-report
    // what the shoulder buttons can actually do -- but switching this flag
    // to it changes behavior for every paginated caller in one step; kept
    // to the minimal, already-safe `pageCount` fix for this round.
    readonly property bool hasPagesBelow: currentPage < pageCount - 1 || root.hasMorePages

    // Caller-supplied total item count, used by the scroll thumb so its
    // size and position reflect the full dataset rather than the loaded
    // slice. Default -1 means "fall back to itemCount" — fine for
    // non-paginated models (Systems, Categories, Recents) where the
    // loaded count IS the total. Paginated callers (GamesScreen) bind
    // this to their model's authoritative total so the thumb stays
    // stable while `fetch_more` grows the slice in the background.
    property int totalItemsOverride: -1
    // Unknown-length cursor lists must derive navigation only from loaded rows
    // and `hasMorePages`. Ignore any stale/legacy total hint until a caller
    // explicitly declares its total authoritative.
    readonly property int totalItems: paginationTotalKnown && totalItemsOverride >= 0 ? totalItemsOverride : itemCount
    readonly property int totalPageCount: Math.max(1, Math.ceil(totalItems / pageSize))
    // False when a cursor chain has no authoritative final count. Navigation
    // continues forward by requesting another page instead of wrapping at the
    // current loaded edge; chrome keeps arrows but suppresses the misleading
    // growing scrollbar thumb.
    property bool paginationTotalKnown: true

    // Caller-supplied "more pages exist" flag for paginated models.
    // Drives the pending-target watchdog: if the model says no more pages
    // are coming but the pending target still isn't loaded, we settle on
    // whatever's loaded rather than spinning forever. Non-paginated
    // callers leave this false; their pending targets always resolve
    // immediately because totalPageCount === pageCount.
    property bool hasMorePages: false
    // True while model owns an RPC or frame-gapped append tail. Pending-target
    // navigation waits for it to clear before requesting another cursor page,
    // preventing later-page rows from interleaving with current append.
    property bool loadingMore: false

    // Pending wrap-target state. Set by Up-at-page-0, Down-past-last-
    // loaded, and pageBy when the destination page hasn't been fetched
    // yet. The grid fires `loadMoreRequested` and waits for `itemCount`
    // to grow; once the target page is loaded, it commits the move and
    // clears these. Cleared on any directional move that doesn't match
    // the pending intent (Left/Right, opposite-direction Up/Down) and on
    // model resets (itemCount shrink). -1 means "no pending jump".
    property int _pendingTargetPage: -1
    property int _pendingTargetRow: 0
    property int _pendingTargetCol: 0

    // Pending jump-to-index state. Set by `jumpToIndex` when the absolute
    // target isn't loaded yet; the grid fetches until `itemCount` passes it
    // and `_commitPendingTarget` lands on this exact index. Distinct from the
    // page/row/col wrap-target above: a jump preserves the absolute position
    // (pages stay fixed/global, the cursor lands mid-page on the true target),
    // so it must not be re-derived from page/row/col. The two are mutually
    // exclusive at any moment. -1 means "no pending jump".
    property int _pendingTargetIndex: -1

    // True while a wrap / shoulder-jump / hold-Down-past-edge / jump-to-index
    // move is stashed waiting on a fetch. Screens use this to gate the
    // "Loading more..." indicator so background prefetches stay
    // silent — the indicator only paints when the user is genuinely
    // waiting on input they've already given.
    readonly property bool hasPendingTarget: _pendingTargetPage >= 0 || _pendingTargetIndex >= 0

    // True only while a jump-to-index (letter jump) is waiting on a fetch, as
    // opposed to a page-wrap target. Lets callers bulk-load aggressively for a
    // jump (loading overlay up, target far away) while keeping ordinary
    // page-turn prefetches on the gentle trickle.
    readonly property bool hasPendingJump: _pendingTargetIndex >= 0

    // Absolute row the pending jump will land on, or -1 when no jump is
    // pending. Lets the model size its jump fetch to the gap remaining rather
    // than always pulling the full remainder.
    readonly property int pendingJumpIndex: _pendingTargetIndex

    // Clear every pending-target channel at once. Used by all the
    // "intent changed / resolved / reset" sites so neither the page/row/col
    // wrap-target nor the jump-to-index target can leak across.
    function _clearPendingTarget(): void {
        root._pendingTargetPage = -1;
        root._pendingTargetIndex = -1;
    }

    // Reserved chrome around the cell area. Vertical insets provide equal
    // breathing room around top and bottom rows; `leftInset`/`rightInset`
    // do the same horizontally. There is no scrollbar gutter to reserve —
    // grids are paged, not scrolled; the page cue lives in the host
    // screen's footer (see PageIndicator.qml) and never affects this
    // component's geometry.
    readonly property int leftInset: root._gridProfile ? root._gridProfile.leftInset : (Sizing.tier === "240" ? Sizing.headerSideMargin : Sizing.pctW(3))
    readonly property int rightInset: root._gridProfile ? root._gridProfile.rightInset : (Sizing.tier === "240" ? Sizing.headerSideMargin : Sizing.pctW(3))
    readonly property int topInset: root._gridProfile ? root._gridProfile.topInset : (Sizing.tier === "240" ? 2 : Sizing.pctH(2))
    readonly property int bottomInset: root._gridProfile ? root._gridProfile.bottomInset : (Sizing.tier === "240" ? 4 : Sizing.pctH(2))
    readonly property int cellSpacingX: root._gridProfile ? root._gridProfile.columnGap : (Sizing.tier === "240" ? 4 : Sizing.pctW(2))
    readonly property int cellSpacingY: root._gridProfile ? root._gridProfile.rowGap : (Sizing.tier === "240" ? 4 : Sizing.pctH(4))
    readonly property int _contentWidth: root.columns * root.cellWidth + (root.columns - 1) * root.cellSpacingX
    readonly property int _contentHeight: root.rows * root.cellHeight + (root.rows - 1) * root.cellSpacingY
    // Cell block always centers against the full inset-to-inset width.
    // With no gutter to reserve, `_availableWidth` already is that full
    // width, so this is just ordinary centering — unlike before the
    // gutter's removal, becoming/ceasing to be paginated can no longer
    // shift or resize this block at all.
    readonly property int _cellBlockOffsetX: Math.max(0, Math.floor((root._availableWidth - root._contentWidth) / 2))
    readonly property int _cellBlockOffsetY: Math.max(0, Math.floor((root._availableHeight - root._contentHeight) / 2))

    // Computed cell dimensions — fill the available area, divided by
    // columns × rows.
    readonly property int _availableWidth: Math.max(0, width - leftInset - rightInset)
    readonly property int _availableHeight: Math.max(0, height - topInset - bottomInset)
    // Per-axis fits. `_widthFit` always divides the item's own available
    // width — every caller already sets `width` as a free-standing input
    // (e.g. `width: parent.width`), so there is no circularity to avoid on
    // that axis. `_heightFit` divides `heightBudget` when a caller supplies
    // one (`squareCells`'s doc comment explains why); otherwise it divides
    // the item's own available height, identical to before.
    readonly property int _widthFit: Math.max(0, Math.floor((root._availableWidth - (root.columns - 1) * root.cellSpacingX) / root.columns))
    readonly property int _heightFit: Math.max(0, Math.floor((root._heightFitAvailable - (root.rows - 1) * root.cellSpacingY) / root.rows))
    readonly property int _heightFitAvailable: Math.max(0, (root.heightBudget >= 0 ? root.heightBudget : height) - topInset - bottomInset)
    readonly property int cellWidth: root.squareCells ? Math.min(root._widthFit, root._heightFit) : root._widthFit
    readonly property int cellHeight: root.squareCells ? Math.min(root._widthFit, root._heightFit) : root._heightFit

    function setCurrentIndexImmediate(idx: int): void {
        root.currentIndex = idx;
    }

    // Disarm model-relative navigation before a folder/scope replacement can
    // shrink row count. Resetting from a deep loaded page while Qt still owns a
    // pending delegate index can make DelegateModel cancel an index against the
    // new smaller count. Selection restoration runs after replacement.
    function prepareForModelReplacement(): void {
        root._clearPendingTarget();
        root.currentIndex = 0;
    }

    function _handleWheel(wheel: WheelEvent): void {
        const amount = wheel.angleDelta.y !== 0 ? wheel.angleDelta.y : wheel.pixelDelta.y;
        if (amount === 0)
            return;
        root.pageWheelRequested(amount < 0 ? 1 : -1);
        wheel.accepted = true;
    }

    // Corner radius of the rect `currentCellRectIn()` returns, for
    // ContextMenu's rounded scrim hole (Part 5).
    readonly property int currentCellRadius: root._cardRadius

    function currentCellRectIn(target: Item): rect {
        if (root.itemCount <= 0)
            return Qt.rect(0, 0, 0, 0);
        const local = root.currentIndex % root.pageSize;
        const row = Math.floor(local / root.columns);
        const col = local % root.columns;
        const p = root.mapToItem(target, root.leftInset + root._cellBlockOffsetX + col * (root.cellWidth + root.cellSpacingX), root.topInset + root._cellBlockOffsetY + row * (root.cellHeight + root.cellSpacingY));
        return Qt.rect(p.x, p.y, root.cellWidth, root.cellHeight);
    }

    // Jump the selection by `delta` whole pages. Wraps in both
    // directions over the dataset's `totalPageCount`, not just the
    // loaded slice, so paginated callers (Games) wrap to the true last
    // page rather than the last *loaded* page. If the target page
    // isn't loaded yet, the move is deferred via `_pendingTargetPage`:
    // the grid fires `loadMoreRequested` and commits the jump once
    // `itemCount` grows enough to cover the target. The target lands on
    // (targetPage, currentRow, currentColumn) when that slot exists;
    // on a partial last page it clamps to the last existing item.
    // Returns true if the index changed synchronously, false if a
    // pending-jump was stashed or the dataset is single-page.
    // Reads the `isEmpty` role for an arbitrary flat index. Goes through
    // the Repeater's own delegate (`itemAt`) rather than querying
    // `root.model` directly so this works uniformly whether the model is a
    // QML ListModel (the Hub) or a Rust-backed model (every other caller,
    // which never sets `isEmpty` true) without needing model-type-specific
    // access. The Repeater's `cellItem` delegate is created synchronously
    // (see the `asynchronous` note on the Loader below, which is scoped to
    // the *TileLoader*, not this outer delegate), so `itemAt` never returns
    // null for a valid index.
    function _isEmptyAt(index: int): bool {
        if (index < 0 || index >= root.itemCount)
            return false;
        const cell = itemRepeater.itemAt(index);
        return cell ? cell.isEmpty : false;
    }

    // First non-`isEmpty` cell on `page`, scanning row-major from `fromLocal`
    // (a page-local index) and wrapping within the page's own filled span.
    // -1 if the page has no non-empty cell at all (a fully-blank page, e.g.
    // Move's reserve page or an entirely-emptied Hub). Used by `pageBy`'s
    // skip-empty pass below.
    function _firstNonEmptyOnPage(page: int, fromLocal: int): int {
        const pageStart = page * root.pageSize;
        const itemsOnPage = Math.min(root.pageSize, root.itemCount - pageStart);
        if (itemsOnPage <= 0)
            return -1;
        for (let step = 0; step < itemsOnPage; step++) {
            const local = (fromLocal + step) % itemsOnPage;
            const idx = pageStart + local;
            if (!root._isEmptyAt(idx))
                return idx;
        }
        return -1;
    }

    // Turn the page by `delta`, skipping over any page whose landing slot
    // (and, failing that, whose entire span) is `isEmpty` -- see
    // `skipEmptyCells` above. Bounded by `pageCount` steps: a direction with
    // no reachable non-empty page anywhere settles back on the starting
    // index rather than looping. Any mid-scan failure of the underlying
    // step (`_pageByStep` returning false -- a pending fetch stash, or
    // genuinely nowhere left to go) also settles back to the start; this
    // is deliberately conservative rather than landing partway, since a
    // stash means the destination isn't even loaded yet.
    function pageBy(delta: int): bool {
        if (!root.skipEmptyCells)
            return root._pageByStep(delta);

        const startIndex = root.currentIndex;
        for (let steps = 0; steps < root.pageCount; steps++) {
            if (!root._pageByStep(delta))
                break;
            const found = root._firstNonEmptyOnPage(root.currentPage, root.currentIndex - root.currentPage * root.pageSize);
            if (found < 0)
                continue; // whole page is blank -- keep paging the same direction
            if (found !== root.currentIndex)
                root.currentIndex = found;
            return true;
        }
        if (root.currentIndex !== startIndex)
            root.currentIndex = startIndex;
        return false;
    }

    function _pageByStep(delta: int): bool {
        if (root.itemCount <= 0 || delta === 0)
            return false;
        let targetPage;
        if (!root.paginationTotalKnown && root.hasMorePages) {
            // No final page exists to wrap to yet. Backward paging stops at
            // page zero; forward paging past the loaded edge requests the next
            // cursor page below.
            targetPage = root.currentPage + delta;
            if (targetPage < 0)
                return false;
        } else {
            if (root.totalPageCount <= 1)
                return false;
            const total = root.totalPageCount;
            // JS `%` keeps sign on negatives — normalise into [0, total).
            targetPage = ((root.currentPage + delta) % total + total) % total;
        }
        if (targetPage === root.currentPage)
            return false;
        if (targetPage > root.pageCount - 1) {
            // Target page hasn't been fetched yet. Stash the intent and
            // let the itemCount-change watcher commit it. A fresh page-nav
            // supersedes any pending jump-to-index.
            root._pendingTargetIndex = -1;
            root._pendingTargetPage = targetPage;
            root._pendingTargetRow = root.currentRow;
            root._pendingTargetCol = root.currentColumn;
            root.loadMoreRequested(true);
            return false;
        }
        root._clearPendingTarget();
        const targetSlot = targetPage * root.pageSize + root.currentRow * root.columns + root.currentColumn;
        const lastIdxOnPage = Math.min((targetPage + 1) * root.pageSize, root.itemCount) - 1;
        if (lastIdxOnPage < 0)
            return false;
        const newIndex = Math.min(targetSlot, lastIdxOnPage);
        if (newIndex === root.currentIndex)
            return false;
        root.currentIndex = newIndex;
        // Mirror moveSelection's pre-fetch: when we cross within
        // `loadAheadPages` of the loaded edge, kick a fetch so the next
        // page boundary lands on freshly loaded rows.
        if (root.currentPage >= root.pageCount - root.loadAheadPages - 1)
            root.loadMoreRequested(false);
        return true;
    }

    // Jump the selection to an exact absolute index over the full dataset
    // (`totalItems`), loading the intervening pages if the target hasn't been
    // fetched yet — the basis for "jump to letter". Mirrors `pageBy`'s
    // pending-target machinery but lands on the exact index rather than
    // preserving the current row/col. A target that is already loaded (e.g. a
    // backward jump to an earlier letter) lands immediately; an unrealised
    // target stashes the exact (page,row,col) slot and `_commitPendingTarget`
    // commits it once `fetch_more` has loaded that far. Returns true if the
    // index changed synchronously, false if a pending-jump was stashed.
    function jumpToIndex(targetIndex: int): bool {
        if (root.itemCount <= 0 || root.totalItems <= 0)
            return false;
        const target = Math.max(0, Math.min(targetIndex, root.totalItems - 1));
        if (target < root.itemCount) {
            // Already loaded — land immediately (no walk needed).
            root._clearPendingTarget();
            if (target !== root.currentIndex)
                root.currentIndex = target;
            if (root.currentPage >= root.pageCount - root.loadAheadPages - 1)
                root.loadMoreRequested(false);
            return true;
        }
        // Not fetched yet — stash the absolute target and load until the
        // model has grown past it, then commit on the exact index.
        root._pendingTargetPage = -1;
        root._pendingTargetIndex = target;
        root.loadMoreRequested(true);
        return false;
    }

    // Commit the pending target move once the destination slot is
    // loaded, or settle on the loaded last when the model says no more
    // pages are coming. Wired into `onItemCountChanged` so every
    // fetch-more append re-evaluates it; also fires from the
    // `hasMorePages` watcher so a final empty append still resolves
    // a chain. Waiting on the exact target index (not just `pageCount`
    // catching up) avoids an early commit while the Repeater is mid-
    // materialisation: the target page may report `pageCount` reached
    // while the row/col slot itself isn't realised yet.
    function _commitPendingTarget(): void {
        // Jump-to-index: land on the exact absolute target. Pages stay
        // fixed/global; the cursor lands mid-page on the true target and
        // never a page early. No page-clamp and no settle-back fallback —
        // those only existed to cope with a slow page-at-a-time walk.
        if (root._pendingTargetIndex >= 0) {
            const want = root._pendingTargetIndex;
            if (want < root.itemCount) {
                root._clearPendingTarget();
                if (want !== root.currentIndex)
                    root.currentIndex = want;
                return;
            }
            if (root.hasMorePages) {
                if (!root.loadingMore)
                    root.loadMoreRequested(true);
                return;
            }
            // Dataset genuinely can't reach the target (Core revised the
            // total down, or the listing is shorter than expected). Land on
            // the nearest loaded item to the target — never a full page back.
            root._clearPendingTarget();
            const nearest = Math.min(want, root.itemCount - 1);
            if (nearest >= 0 && nearest !== root.currentIndex)
                root.currentIndex = nearest;
            return;
        }
        if (root._pendingTargetPage < 0)
            return;
        // Known totals may shrink under us, so clamp to their reported final
        // page. Unknown-length cursor lists cannot clamp while another page
        // exists: their loaded total necessarily trails the pending target.
        const totalLast = root.totalPageCount - 1;
        const targetPage = root.paginationTotalKnown || !root.hasMorePages ? Math.min(root._pendingTargetPage, totalLast) : root._pendingTargetPage;
        if (targetPage < 0) {
            root._pendingTargetPage = -1;
            return;
        }
        const targetIdx = targetPage * root.pageSize + root._pendingTargetRow * root.columns + root._pendingTargetCol;
        if (targetIdx >= root.itemCount) {
            // Specific (page, row, col) slot not realised yet.
            if (root.hasMorePages) {
                if (!root.loadingMore)
                    root.loadMoreRequested(true);
                return;
            }
            // Model says no more pages are coming. Settle on the
            // target page's last loaded item if it has any; otherwise
            // fall back to the dataset's overall loaded last so the
            // user's "go to end" intent isn't ignored.
            root._pendingTargetPage = -1;
            const pageStart = targetPage * root.pageSize;
            const lastLoadedOnPage = Math.min((targetPage + 1) * root.pageSize, root.itemCount) - 1;
            if (lastLoadedOnPage >= pageStart) {
                if (lastLoadedOnPage !== root.currentIndex)
                    root.currentIndex = lastLoadedOnPage;
                return;
            }
            const overall = root.itemCount - 1;
            if (overall >= 0 && overall !== root.currentIndex)
                root.currentIndex = overall;
            return;
        }
        root._pendingTargetPage = -1;
        if (targetIdx !== root.currentIndex)
            root.currentIndex = targetIdx;
    }

    // Step the selection by (dCol, dRow). Returns true if the index
    // actually moved. Cardinal moves only — diagonals (dCol and dRow
    // both nonzero) are not produced by any caller and behaviour for
    // them is undefined.
    //
    // Horizontal axis — within-row wrap, never changes page or row:
    // - dCol > 0 at the row's last filled column: wrap to col 0.
    // - dCol < 0 at column 0: wrap to the row's last filled column.
    //   On a partial last row, the "last filled column" is bounded by
    //   the row's actual item count so wraps land on real cells.
    //
    // Vertical axis — page advance/retreat with wrap and partial-page
    // clamp:
    // - dRow > 0 past the last *filled* row on the current page:
    //   advance to (next page, row 0, same col). On the last page, wrap
    //   to (page 0, row 0, same col). On a partial last page this
    //   triggers as soon as Down would step into the empty rows below
    //   the content, not just when stepping off the grid grid-shape.
    // - dRow < 0 above row 0: retreat to (previous page, last row,
    //   same col). On page 0, wrap to (last page, last row, same col).
    // - Landing on a hole on a partial target page (column doesn't
    //   exist there): clamp to the last existing item on the target
    //   page so the user always moves rather than sticking on a hole.
    //
    // When `skipEmptyCells` is set: horizontal presses still step the SAME
    // way `_moveSelectionStep` always has, just repeated until a non-empty
    // cell turns up -- Left/Right staying within the source row is a real,
    // separate invariant (Move's `_wouldWrapColumn` depends on it), not
    // part of what this property changes. Vertical presses do NOT reuse
    // that "same column, repeated step" approach: on a freely-arranged
    // board (the Hub, the only caller) a column can be empty for several
    // rows in a row while a real tile sits one column over, and marching
    // straight down that empty column -- even wrapping through pages --
    // tunnels past it. `_nearestVerticalCandidate` below searches every
    // real tile on the board instead. See its own doc comment.
    function moveSelection(dCol: int, dRow: int): bool {
        if (!root.skipEmptyCells)
            return root._moveSelectionStep(dCol, dRow);

        if (dRow !== 0) {
            const candidate = root._nearestVerticalCandidate(dRow, root.currentIndex);
            if (candidate < 0)
                return false;
            root._clearPendingTarget();
            root.currentIndex = candidate;
            return true;
        }

        // Horizontal: same-row skip, bounded repeat of the ordinary step --
        // see the doc comment above for why this one is unchanged.
        const startIndex = root.currentIndex;
        for (let steps = 0; steps < root.itemCount; steps++) {
            if (!root._moveSelectionStep(dCol, dRow))
                break;
            if (root.currentIndex !== startIndex && !root._isEmptyAt(root.currentIndex))
                return true;
        }
        if (root.currentIndex !== startIndex)
            root.currentIndex = startIndex;
        return false;
    }

    // Finds the best real (non-`isEmpty`) tile in the vertical direction
    // `dRow` from `fromIndex`, searching the WHOLE board rather than a
    // single column -- see `moveSelection`'s doc comment for why the
    // column-only approach tunnels past nearby content on a freely
    // arranged Hub layout. Pages stack vertically (this file's own header
    // comment), so every cell's position collapses to one flat
    // `virtualRow = page * rows + row` axis plus `column`; distance is
    // then scored in plain grid units exactly the way Android's
    // `FocusFinder` -- the algorithm behind d-pad navigation on Android TV
    // since 2007 -- scores pixel rects: `13 * majorAxisDistance² +
    // minorAxisDistance²`. The 13:1 ratio is that same shipped, tuned
    // constant, reused rather than re-derived: it strongly prefers staying
    // column-aligned (a tile 1 row further but perfectly aligned beats one
    // 1 column off), while still being willing to drift a column or two
    // rather than travel several rows past nearer content. Grid units
    // (not pixels) are correct here specifically because the Hub always
    // uses `squareCells: true`, so a row step and a column step already
    // cover equal physical distance.
    //
    // Two passes:
    // 1. Only tiles strictly in the pressed direction. Cheapest, most
    //    common case -- most presses have something below/above.
    // 2. Nothing there at all: wrap. Every remaining real tile is scored
    //    as if the press had continued past the edge and come back
    //    around (mirrors the horizontal skip's own wrap and the
    //    confirmed Move-page wrap), so the winner is whichever tile is
    //    closest to the far edge, still column-weighted the same way.
    // Returns -1 (moveSelection settles back to the start, unmoved) only
    // when `fromIndex` is the sole real tile on the entire board.
    function _nearestVerticalCandidate(dRow: int, fromIndex: int): int {
        const totalRows = root.totalPageCount * root.rows;
        const srcLocal = fromIndex % root.pageSize;
        const srcRow = Math.floor(srcLocal / root.columns);
        const srcCol = srcLocal % root.columns;
        const srcVirtualRow = Math.floor(fromIndex / root.pageSize) * root.rows + srcRow;

        let best = -1;
        let bestScore = Infinity;
        for (let idx = 0; idx < root.itemCount; idx++) {
            if (idx === fromIndex || root._isEmptyAt(idx))
                continue;
            const local = idx % root.pageSize;
            const row = Math.floor(local / root.columns);
            const col = local % root.columns;
            const virtualRow = Math.floor(idx / root.pageSize) * root.rows + row;
            if (dRow > 0 ? virtualRow <= srcVirtualRow : virtualRow >= srcVirtualRow)
                continue;
            const major = dRow > 0 ? virtualRow - srcVirtualRow : srcVirtualRow - virtualRow;
            const minor = Math.abs(col - srcCol);
            const score = 13 * major * major + minor * minor;
            if (score < bestScore) {
                bestScore = score;
                best = idx;
            }
        }
        if (best >= 0)
            return best;

        for (let idx = 0; idx < root.itemCount; idx++) {
            if (idx === fromIndex || root._isEmptyAt(idx))
                continue;
            const local = idx % root.pageSize;
            const row = Math.floor(local / root.columns);
            const col = local % root.columns;
            const virtualRow = Math.floor(idx / root.pageSize) * root.rows + row;
            const major = dRow > 0 ? virtualRow + (totalRows - srcVirtualRow) : srcVirtualRow + (totalRows - virtualRow);
            const minor = Math.abs(col - srcCol);
            const score = 13 * major * major + minor * minor;
            if (score < bestScore) {
                bestScore = score;
                best = idx;
            }
        }
        return best;
    }

    function _moveSelectionStep(dCol: int, dRow: int): bool {
        if (root.itemCount <= 0)
            return false;

        let newPage = root.currentPage;
        let newRow = root.currentRow;
        let newCol = root.currentColumn;

        // Horizontal wrap stays on the source row; clamp the wrap target
        // to the row's actual filled span so a partial last row cycles
        // among its own items rather than walking through a hole.
        if (dCol !== 0) {
            // Sideways step changes the user's intent — drop any
            // pending wrap-target / jump we were waiting on.
            root._clearPendingTarget();
            const rowFirstIndex = root.currentPage * root.pageSize + root.currentRow * root.columns;
            const rowLastIndex = Math.min(root.itemCount - 1, rowFirstIndex + root.columns - 1);
            const maxColOnRow = rowLastIndex - rowFirstIndex;
            const colCandidate = root.currentColumn + dCol;
            if (colCandidate < 0)
                newCol = maxColOnRow;
            else if (colCandidate > maxColOnRow)
                newCol = 0;
            else
                newCol = colCandidate;
        }

        // Vertical wrap crosses page boundaries against the dataset's
        // `totalPageCount`, not the loaded slice. When the destination
        // page hasn't been fetched yet (Up at page 0 with partial load,
        // Down past last-loaded but more pages exist), stash a
        // pending-target jump and ask the model to fetch — the
        // `onItemCountChanged` handler commits the jump once the page
        // lands. The partial-page hole clamp below covers the column-
        // doesn't-exist-on-destination case once the data is present.
        //
        // A Down step that lands in an empty row of a partial last
        // page (rowCandidate fits inside `rows` but is past the page's
        // last filled row) must trigger the same page advance as
        // stepping off the grid — otherwise the hole-clamp at the end
        // of this function lands on the user's current cell and the
        // press appears to do nothing.
        if (dRow !== 0) {
            const rowCandidate = root.currentRow + dRow;
            const itemsOnPage = Math.min(root.pageSize, root.itemCount - root.currentPage * root.pageSize);
            const lastFilledRowOnPage = Math.floor((itemsOnPage - 1) / root.columns);
            if (rowCandidate < 0) {
                if (!root.paginationTotalKnown && root.hasMorePages && root.currentPage === 0)
                    return false;
                const targetPage = root.currentPage === 0 ? root.totalPageCount - 1 : root.currentPage - 1;
                if (targetPage > root.pageCount - 1) {
                    root._pendingTargetIndex = -1;
                    root._pendingTargetPage = targetPage;
                    root._pendingTargetRow = root.rows - 1;
                    root._pendingTargetCol = root.currentColumn;
                    root.loadMoreRequested(true);
                    return false;
                }
                newPage = targetPage;
                newRow = root.rows - 1;
            } else if (rowCandidate >= root.rows || rowCandidate > lastFilledRowOnPage) {
                const lastPage = root.totalPageCount - 1;
                const targetPage = !root.paginationTotalKnown && root.hasMorePages && root.currentPage >= root.pageCount - 1 ? root.currentPage + 1 : (root.currentPage === lastPage ? 0 : root.currentPage + 1);
                if (targetPage > root.pageCount - 1) {
                    root._pendingTargetIndex = -1;
                    root._pendingTargetPage = targetPage;
                    root._pendingTargetRow = 0;
                    root._pendingTargetCol = root.currentColumn;
                    root.loadMoreRequested(true);
                    return false;
                }
                newPage = targetPage;
                newRow = 0;
            } else {
                newRow = rowCandidate;
            }
        }

        let newIndex = newPage * root.pageSize + newRow * root.columns + newCol;
        if (newIndex < 0)
            return false;
        if (newIndex >= root.itemCount) {
            // Target slot is a hole on a partial page. Clamp to the
            // page's last existing item.
            const lastIdxOnPage = Math.min((newPage + 1) * root.pageSize, root.itemCount) - 1;
            if (lastIdxOnPage < 0)
                return false;
            newIndex = lastIdxOnPage;
        }
        if (newIndex === root.currentIndex) {
            // Selection didn't move because the user is at an edge with
            // no data beyond it on this side. If they're within
            // `loadAheadPages` of the loaded edge, ask the model to
            // fetch more so a subsequent press can land on freshly-
            // loaded rows.
            if (root.currentPage >= root.pageCount - root.loadAheadPages - 1)
                root.loadMoreRequested(false);
            return false;
        }
        // Successful directional move clears any pending wrap-target / jump;
        // the user is no longer waiting on it.
        root._clearPendingTarget();
        root.currentIndex = newIndex;
        // Pre-fetch early - when the user enters within `loadAheadPages`
        // of the loaded edge, kick off the next fetch so the network
        // round-trip and model insert overlap with selection travel,
        // and the new chunk lands before they cross the boundary. The
        // model's own debounce (`loading_more` guard) collapses
        // repeated emissions while a fetch is in flight.
        if (root.currentPage >= root.pageCount - root.loadAheadPages - 1)
            root.loadMoreRequested(false);
        return true;
    }

    // Defensive clamp on shrinkage only: if the model shed rows below
    // the saved index, keep us in-bounds. Don't clamp on growth — page
    // appends from cumulative pagination must leave the user's
    // currentIndex untouched.
    property int _previousItemCount: 0

    // If `hasMorePages` flips false while a pending target is still
    // ahead of the loaded slice, the watchdog branch in
    // `_commitPendingTarget` settles us on the loaded last item.
    // itemCount-change usually fires this path first, but this handler
    // covers the case where the flag is updated without an item delta
    // (e.g. a final empty append).
    onHasMorePagesChanged: {
        if (root.hasPendingTarget)
            root._commitPendingTarget();
    }

    onLoadingMoreChanged: {
        if (!root.loadingMore && root.hasPendingTarget)
            root._commitPendingTarget();
    }

    onItemCountChanged: {
        // Destroying the Repeater during suspension briefly reports zero before
        // itemCount rebinds to source count. That is not model shrinkage and
        // must not reset restored list selection.
        const sourceCountRetained = root.model && root.model.count >= root._previousItemCount;
        if (root.itemCount < root._previousItemCount && !sourceCountRetained) {
            // Model shed rows (reset, system change, path change). The
            // pending-target context no longer applies.
            root._clearPendingTarget();
        } else if (root.itemCount > root._previousItemCount) {
            // Pages were appended. If a wrap-target jump is pending,
            // try to commit it now; the helper chains another
            // `loadMoreRequested` if the target is still ahead, or
            // settles on the loaded last item if `hasMorePages`
            // says no more pages are coming.
            root._commitPendingTarget();
        }
        // Universal loaded-page invariant: whenever rows exist, currentIndex
        // points at one of them. Model replacement can invalidate a persisted
        // numeric index without taking the ordinary shrink branch (notably a
        // zero-to-populated reset); directional input used to repair this only
        // after the page had already painted with no focused item.
        if (root.itemCount > 0 && (root.currentIndex < 0 || root.currentIndex >= root.itemCount))
            root.currentIndex = 0;
        root._previousItemCount = root.itemCount;
    }

    clip: true

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.RightButton
        onClicked: root.emptyRightClicked()
        onWheel: wheel => root._handleWheel(wheel)
    }

    Item {
        id: track

        // One page wide. Cells whose `cellPage !== root.currentPage`
        // gate themselves invisible; nothing slides or scales.
        anchors.fill: parent

        Repeater {
            id: itemRepeater

            model: root.suspendDelegates ? null : root.model

            Item {
                id: cellItem

                required property int index
                required property string name
                // Every Browse model exposes `coverKey` — the relative path
                // under `resources/images/` without extension (e.g.
                // `systems/snes`, `categories/Consoles`). Tile resolves an
                // embedded PNG from the key or shows the procedural
                // fallback with `name` rendered large.
                required property string coverKey
                required property int favorite
                required property bool hidden
                // True while a tile's live precondition isn't currently met
                // (Hub only today). Required, same as `entryType`/
                // `fileCount` below -- a plain non-required property does
                // NOT automatically bind to a matching model role (Qt only
                // wires that up for `required property`), so every model
                // reaching this delegate declares this role even though
                // only Hub's `hubGridModel` ever sets it to `true`.
                required property bool disabled
                // Newline-joined disambiguating-tag tokens (empty for models
                // without variants). Every Browse model exposes this role.
                required property string disambiguatingTags
                // Round 11. Required, same as `disambiguatingTags` above --
                // a plain non-required property does NOT automatically bind
                // to a matching model role (Qt only wires that up for
                // `required property`), so every model reaching this
                // delegate, including Hub's hand-built `ListModel`
                // (HubScreen.qml's `hubGridModel`), sets both explicitly.
                required property string entryType
                required property int fileCount
                // Structural placeholder, not a real item — see
                // `root.emptyDelegate` above. Every model supplies this
                // (default `false` for models that never have one).
                required property bool isEmpty

                readonly property int cellPage: Math.floor(index / root.pageSize)
                readonly property int cellLocal: index % root.pageSize
                readonly property int cellRow: Math.floor(cellLocal / root.columns)
                readonly property int cellCol: cellLocal % root.columns
                readonly property bool isSelected: index === root.currentIndex
                readonly property string topLabel: typeof root.tileTopLabelProvider === "function" ? (root.tileTopLabelProvider(index) ?? "") : ""

                // Cover-decode gate AND delegate-materialisation gate.
                // PagedGrid's Repeater creates one cellItem per model
                // row at construction. Two-tier gate, both anchored on
                // distance from `root.currentPage`:
                //
                //   - decode range (current + configured lookahead pages):
                //     cells hand their real coverKey to Tile, forcing hidden
                //     next-page Image decode/QPixmapCache warm before the page
                //     cut when lookahead is enabled. Rust still owns byte-fetch priority
                //     via `prefetch_around`; this range only makes QML
                //     consume already-warmed bytes early enough.
                //   - retention range (current ± enough pages for one
                //     Sizing.visibleCovers span): cells inside this radius keep
                //     their TileLoader.active=true, AND cells that have already
                //     requested keep their coverKey set so Tile's Image keeps
                //     the decoded texture referenced. The active gate prevents
                //     per-press binding cost from growing with dataset size;
                //     only current and adjacent-page Tile delegates exist at
                //     normal grid densities. Retention doesn't trigger
                //     new cover requests; only the decode range does.
                //
                // Off-radius cells (outside retention) set
                // `TileLoader.active=false`, which destroys the
                // loaded Tile delegate (Image, name Text, focus ring,
                // favorite indicator) and detaches its binding tree.
                // Cells inside retention but never requested keep the
                // delegate alive but with coverKey="", so the cover
                // collapses to the procedural fallback and the
                // texture reference drops.
                //
                // Memory ceiling tracks visible cover density:
                // _coverRetentionPages around currentPage keeps adjacent
                // decoded pages warm for current UI scale. Re-decode
                // after crossing past the retention edge runs at
                // nice +10 (see media_image_provider.cpp) and is
                // invisible to the renderer.
                readonly property bool _coverInRange: root.coverRequestsEnabled && !root.rapidRenderMode && cellPage >= root.currentPage && cellPage <= root.currentPage + Math.max(0, root.coverLookaheadPages)
                readonly property bool _coverInRetentionRange: !root.rapidRenderMode && Math.abs(cellPage - root.currentPage) <= (root.coverLoadingPaused ? 1 : root._coverRetentionPages)
                property bool _coverEverRequested: false
                Binding on _coverEverRequested {
                    when: cellItem._coverInRange
                    value: true
                    restoreMode: Binding.RestoreNone
                }
                readonly property string _gatedCoverKey: (_coverInRange || (_coverEverRequested && _coverInRetentionRange)) ? coverKey : ""

                width: root.cellWidth
                height: root.cellHeight
                x: root.leftInset + root._cellBlockOffsetX + cellCol * (root.cellWidth + root.cellSpacingX)
                y: root.topInset + root._cellBlockOffsetY + cellRow * (root.cellHeight + root.cellSpacingY)
                // Selected tile draws on top so its focus treatment remains
                // authoritative at cell boundaries.
                z: isSelected ? 1 : 0
                visible: root.cellsVisible && cellPage === root.currentPage

                // Card-shaped placeholder painted behind the TileLoader,
                // plus the selection ring it draws during rapidRenderMode
                // (see the Loader's `active` comment below). Retention-gated
                // the same way `tileLoader` is: unconditionally building this
                // per row (a `PressableSurface` — 6 visual primitives, 2
                // `Behavior` animations, `clip: true` — plus its own implicit
                // MouseArea) for every model row, including the thousands a
                // bulk jump/restore can insert far from `currentPage`, was
                // the dominant residual cost behind a multi-second Qt-thread
                // stall even though `TileLoader` itself was already
                // retention-gated — see `games.rs::apply_append_page`'s
                // `bulk` comment for the Rust-side half of this fix. Off-
                // retention `cellItem`s now cost only the bare outer `Item`
                // (role bindings + the `_coverEverRequested` latch).
                //
                // When the loader's `active` is false (cell is outside the
                // retention window) or the Tile is still incubating
                // asynchronously after a retention-edge crossing, the user
                // sees this flat card slot instead of an empty pit. Once the
                // Tile finishes incubating it paints opaque on top with the
                // same color and radius, so the silhouette is hidden for
                // free without an explicit visibility gate. The only
                // selection-dependent work here is the focused placeholder
                // ring below; it stays limited to the current page by the
                // parent visibility gate.
                //
                // That "paints opaque on top" assumption is specific to
                // `delegate` — an `emptyDelegate` (`EmptySlot.qml`) is
                // deliberately NOT opaque (a genuinely blank slot, not a
                // card with nothing on it), so this skeleton would stay
                // permanently visible underneath it instead of being
                // covered for free. `isEmpty` rows skip it outright: there
                // is nothing to incubate for a structural placeholder.
                Loader {
                    id: placeholderCardLoader

                    anchors.fill: parent
                    // Retention window, OR'd with "on the current page" —
                    // NOT `&& !root.rapidRenderMode` the way `tileLoader`
                    // below is. `_coverInRetentionRange` already bakes in
                    // `!root.rapidRenderMode` internally (it's false for
                    // every cell during rapidRenderMode), but the selected
                    // cell's own placeholder + ring must survive
                    // rapidRenderMode — `tileLoader` goes inactive on
                    // purpose then, and this skeleton is what keeps
                    // selection visible while it's torn down (see the ring
                    // comment inside the loaded component). The selected
                    // cell is always on `currentPage`, so OR-ing in
                    // `cellPage === currentPage` keeps it (and the rest of
                    // the current page, cheaply) alive through
                    // rapidRenderMode without re-widening the window for
                    // every other off-page cell this Loader exists to skip.
                    active: cellItem._coverInRetentionRange || cellItem.cellPage === root.currentPage
                    // Cross-boundary values, resolved once here (same scope
                    // as `tileLoader`'s own property bindings below — not a
                    // new boundary) rather than inside the loaded component.
                    // A `Loader` never forwards its own properties onto the
                    // item it loads, so the component below reads these via
                    // `parent.X`, mirroring how `Tile.qml` reads
                    // `TileLoader`'s properties.
                    // `?? false`: only `Tile.qml` declares `cardPressed`.
                    // `EmptySlot` (any `emptyDelegate`) is a face-less Item
                    // with no press state, and reading a property it doesn't
                    // have yields `undefined`, which QML refuses to assign
                    // into a `bool` -- one warning per blank cell on every
                    // boot (the Hub's bootstrap page pads to a full grid).
                    property bool cardPressed: tileLoader.status === Loader.Ready && tileLoader.item ? (tileLoader.item.cardPressed ?? false) : false
                    property real tileOpacity: tileLoader.status === Loader.Ready && tileLoader.item ? tileLoader.item.opacity : 1
                    property bool ringVisible: cellItem.isSelected && root.focused && root.focusReady && (root.rapidRenderMode || tileLoader.status !== Loader.Ready)
                    property bool cellIsEmpty: cellItem.isEmpty

                    sourceComponent: PressableSurface {
                        id: placeholderCard

                        // Local captures of the Loader's properties, taken
                        // once here (mirrors Tile.qml's `delegateIsSelected:
                        // parent.isSelected` pattern) so the nested ring
                        // Rectangles below read plain same-component
                        // properties instead of repeating `parent.X`.
                        readonly property bool _cellIsEmpty: parent.cellIsEmpty
                        readonly property bool _ringVisible: parent.ringVisible

                        objectName: "pagedGridPlaceholderCard-" + cellItem.index
                        anchors.fill: parent
                        visible: !_cellIsEmpty
                        radius: root._cardRadius
                        faceColor: Theme.surfaceCard
                        edgeColor: Theme.tileEdge
                        // Track the loaded Tile's physical press so both opaque
                        // surfaces leave the same top gap. At skeleton time the
                        // placeholder stays raised until a real activation occurs.
                        pressed: parent.cardPressed
                        // Same tracking for the held blink (Hub Options -> Move,
                        // see Tile.qml's `_heldOpacity`): the "paints opaque on
                        // top" contract above only holds while the loaded Tile
                        // actually stays opaque. Without this, the Tile blinking
                        // to nothing left this skeleton (a blank card with no
                        // art or name) showing through underneath instead of the
                        // whole cell going with it.
                        opacity: parent.tileOpacity

                        // Standalone selected-cell ring for skeleton/rapid mode.
                        // Tile.qml owns the normal ring, but rapidRenderMode
                        // deliberately disables TileLoader to keep held d-pad
                        // navigation cheap. Draw the same filled-rect ring on
                        // the placeholder so selection never disappears while
                        // covers/delegates are paused.
                        Rectangle {
                            id: placeholderFocusRingOuter

                            anchors.fill: parent
                            anchors.margins: Sizing.pctH(0.4)
                            color: Theme.accent
                            radius: Math.max(0, root._cardRadius - Sizing.pctH(0.4))
                            antialiasing: Sizing.cornerAntialiasing
                            visible: placeholderCard._ringVisible
                        }

                        Rectangle {
                            anchors.fill: placeholderFocusRingOuter
                            anchors.margins: Sizing.focusRingWidth
                            color: placeholderCard.faceColor
                            radius: Math.max(0, placeholderFocusRingOuter.radius - Sizing.focusRingWidth)
                            antialiasing: Sizing.cornerAntialiasing
                            visible: placeholderFocusRingOuter.visible
                        }
                    }
                }

                TileLoader {
                    id: tileLoader

                    anchors.fill: parent
                    // A structural placeholder row (see `root.emptyDelegate`
                    // above) resolves to that component instead of the
                    // normal per-item delegate. `root.emptyDelegate === null`
                    // (every caller but the Hub) falls straight through to
                    // `root.delegate` regardless of `isEmpty` — unchanged
                    // behavior for every existing PagedGrid caller.
                    sourceComponent: cellItem.isEmpty && root.emptyDelegate ? root.emptyDelegate : root.delegate
                    // Bound delegate materialisation to the retention
                    // window. Cells outside +/-5 pages keep their
                    // cellItem (Repeater contract - it owns one item
                    // per model row) but don't construct a Tile, so
                    // the loaded delegate's binding tree (cover Image,
                    // focus ring, name Text, favorite indicator)
                    // doesn't fan out on every selection move. With
                    // ~110 active tiles instead of N, per-press
                    // binding cost stays roughly constant as the
                    // dataset grows.
                    active: cellItem._coverInRetentionRange && !root.rapidRenderMode
                    // Current-page delegates complete synchronously so
                    // tile content appears with the page instead of
                    // revealing icon/logo Images one-by-one as the
                    // Loader incubates across frames. Off-page retained
                    // delegates still incubate asynchronously so
                    // retention-edge warmup does not block input.
                    asynchronous: cellItem.cellPage !== root.currentPage
                    isSelected: cellItem.isSelected
                    isFocused: root.focused
                    name: cellItem.name
                    coverKey: cellItem._gatedCoverKey
                    topLabel: cellItem.topLabel
                    favorite: cellItem.favorite
                    hidden: cellItem.hidden
                    disabled: cellItem.disabled
                    disambiguatingTags: cellItem.disambiguatingTags
                    entryType: cellItem.entryType
                    fileCount: cellItem.fileCount
                    activatePulse: root.activatePulse
                    releasePulse: root.releasePulse
                    settling: root.screenSettling
                    focusReady: root.focusReady
                    loadFocusedCover: root.eagerFocusedCovers || cellItem.isSelected
                    held: cellItem.index === root.heldIndex
                    // Mirrors the `asynchronous` rule above, for the same
                    // reason: only the current page's tints are worth spending
                    // GUI-thread time on. Off-page retained delegates keep
                    // decoding on the reader thread so retention-edge warmup
                    // never blocks input, and held-d-pad rapidRenderMode does
                    // no synchronous work at all.
                    coverSynchronous: root.coverSynchronous && cellItem.cellPage === root.currentPage && !root.rapidRenderMode
                }

                // Click/hover hit area. Retention-gated the same as
                // `placeholderCardLoader` above, but WITHOUT the
                // `cellPage === currentPage` OR-term's rapidRenderMode
                // concern — a `MouseArea` was never rapidRenderMode-aware
                // (only `enabled: cellItem.visible` gated it), and
                // `cellItem.visible` already IS `cellPage === currentPage`
                // (folded in below), so gating construction on `visible`
                // directly reproduces the exact original condition: this
                // hit area only ever did anything on the current page, and
                // a disabled off-page MouseArea served no purpose anyway.
                Loader {
                    id: hitAreaLoader

                    anchors.fill: parent
                    active: cellItem.visible
                    property bool cellIsEmpty: cellItem.isEmpty
                    property int cellIndex: cellItem.index

                    sourceComponent: MouseArea {
                        anchors.fill: parent
                        hoverEnabled: true
                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                        cursorShape: Qt.PointingHandCursor

                        onEntered: {
                            // Mirrors the directional skip above: with
                            // `skipEmptyCells` set, a blank is not a landing
                            // spot for the mouse either.
                            if (root.skipEmptyCells && parent.cellIsEmpty)
                                return;
                            if (root.currentIndex !== parent.cellIndex)
                                root.currentIndex = parent.cellIndex;
                            root.itemHovered(parent.cellIndex);
                        }

                        onClicked: mouse => {
                            if (root.skipEmptyCells && parent.cellIsEmpty)
                                return;
                            if (root.currentIndex !== parent.cellIndex)
                                root.currentIndex = parent.cellIndex;
                            if (mouse.button === Qt.RightButton)
                                root.itemRightClicked(parent.cellIndex);
                            else
                                root.itemClicked(parent.cellIndex);
                        }

                        onWheel: wheel => root._handleWheel(wheel)
                    }
                }
            }
        }
    }
}
