// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton methods aren't marked final so Browse.* calls trip
// "Member can be shadowed"; structural, suppress compiler.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Browse as Browse
import Zaparoo.Screens
import Zaparoo.Theme

// Guards the selection-persist contract around a model replacement.
//
// The bug these were written for: backing out of a folder wrote the CHILD
// folder's first row into the PARENT's saved-selection slot. `set_path` flips
// `loading` synchronously, MediaListScreen answers that edge with
// `prepareForModelReplacement()`, and that snaps `currentIndex` to 0 — which
// fires `onCurrentIndexChanged` -> `_persistFocus()` while the model still
// holds the outgoing folder's rows, because `start_initial_browse`
// deliberately keeps them mounted until the new ones land. The router's
// `suppressSelectionPersist` could not cover it: that is only set once rows
// have arrived, several frames later.
//
// Downstream, the poisoned slot named a path that does not exist in the
// parent listing, so the next return to it resolved index -1, fell back to
// row 0 and started a full-folder restore walk hunting for a path that was
// never there. That walk is what testers saw as "Loading games…" plus a reset
// to the first entry.
TestCase {
    id: testCase

    name: "UiMediaListPersist"
    when: windowShown
    width: 1280
    height: 720
    visible: true

    property var persisted: []
    property int discardCount: 0

    // The real GamesModel needs a live Core, and a bare ListModel has no
    // `loading`. A ListModel carrying the extra property and the index-based
    // lookups the shell calls is the smallest thing that reproduces the edge
    // while still giving the grid real rows — with a QtObject stand-in the
    // grid has no items, `currentIndex` never leaves 0, and every assertion
    // below would pass vacuously.
    ListModel {
        id: mediaModel

        property bool loading: false
        property string error_message: ""

        function path_at(index: int): string {
            return index >= 0 && index < mediaModel.count ? mediaModel.get(index).path : "";
        }

        function name_at(index: int): string {
            return index >= 0 && index < mediaModel.count ? mediaModel.get(index).name : "";
        }

        function index_for_path(path: string): int {
            for (let i = 0; i < mediaModel.count; i++) {
                if (mediaModel.get(i).path === path)
                    return i;
            }
            return -1;
        }
    }

    MediaListScreen {
        id: screen

        anchors.fill: parent
        mediaModel: mediaModel
        emptyText: "No entries"
        loadingText: "Loading entries"
        showTopStrip: false
        detailShowTitle: false
        gridColumnsOverride: 2
        gridRowsOverride: 2
        totalItemsOverride: mediaModel.count
        persistSelectionPath: path => testCase.persisted.push(path)
        discardSelectionPersist: () => testCase.discardCount++
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        Browse.Settings.current_games_browse_layout = "grid";
        testCase.persisted = [];
        testCase.discardCount = 0;
        mediaModel.loading = false;
        screen.suppressSelectionPersist = false;
        mediaModel.clear();
        for (let i = 0; i < 8; i++) {
            mediaModel.append({
                "name": "Game " + i,
                "fileStem": "Game " + i,
                "coverKey": "",
                "favorite": false,
                "disambiguatingTags": "",
                "entryType": "media",
                "fileCount": 0,
                "path": "/child/game" + i
            });
        }
        screen.mediaGrid.currentIndex = 0;
        testCase.persisted = [];
    }

    // Baseline: an ordinary user move must still reach disk. Without this the
    // fix could "pass" by never persisting anything at all.
    function test_user_move_still_persists_its_row(): void {
        screen.mediaGrid.currentIndex = 3;
        compare(testCase.persisted.length, 1);
        compare(testCase.persisted[0], "/child/game3");
        compare(testCase.discardCount, 0);
    }

    // The regression itself. Entering the loading state must not schedule the
    // outgoing rows' row 0 — the index snap is bookkeeping, not a selection.
    function test_model_replacement_does_not_persist_the_index_snap(): void {
        screen.mediaGrid.currentIndex = 5;
        compare(testCase.persisted.length, 1);

        mediaModel.loading = true;

        compare(screen.mediaGrid.currentIndex, 0, "replacement must snap the grid to index 0");
        compare(testCase.persisted.length, 1, "the snap to index 0 must not persist the outgoing folder's first row");
        compare(testCase.persisted[0], "/child/game5", "the user's own last move must survive untouched");
    }

    // The snap is only half of it. A debounce armed by the user's last move
    // moments earlier is still in flight on the paths that don't flush, and
    // would land against the new scope's slot. The screen owning the debounce
    // is told to drop it.
    function test_model_replacement_discards_a_pending_write(): void {
        screen.mediaGrid.currentIndex = 2;
        compare(testCase.discardCount, 0);

        mediaModel.loading = true;

        compare(testCase.discardCount, 1, "entering loading must discard any scheduled write");
    }

    // Leaving the loading state must restore normal behavior — a guard that
    // latched on would silently stop persisting for the rest of the session.
    function test_persist_resumes_after_the_replacement_settles(): void {
        mediaModel.loading = true;
        mediaModel.loading = false;
        testCase.persisted = [];

        screen.mediaGrid.currentIndex = 4;

        compare(testCase.persisted.length, 1);
        compare(testCase.persisted[0], "/child/game4");
    }

    // The router's own guard is unrelated and must keep working on its own;
    // the new one is additive, not a replacement.
    function test_router_suppression_still_blocks_persist(): void {
        screen.suppressSelectionPersist = true;
        screen.mediaGrid.currentIndex = 6;
        compare(testCase.persisted.length, 0);
    }
}
