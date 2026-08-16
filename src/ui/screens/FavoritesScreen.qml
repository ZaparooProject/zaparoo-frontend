// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton members aren't marked final, so every Browse.* read
// trips "can be shadowed". Structural; suppress compiler file-wide as the
// other screens do.
// qmllint disable compiler

import QtQuick
import Zaparoo.Browse as Browse

// Favorites screen — flat paged grid driven by
// `Browse.FavoritesModel`. Pure input dispatcher: emits
// `requestHubScreen()` on Escape and launches the highlighted entry on
// Accept by calling the model's `launch_at` (which fans out to Core's
// `run` endpoint).
//
// Favorites is a flat list — no folder navigation, no card-write flow —
// so it reuses the shared `MediaListScreen` shell with the
// favorites-specific model, persisted selection state, and copy. View adds
// Core-backed ordering without materializing full list in frontend.
MediaListScreen {
    id: favorites

    property alias favoritesGrid: favorites.mediaGrid
    property string selectedSystemId: ""
    readonly property int favoriteTotal: Browse.FavoritesModel.total_items

    mediaModel: Browse.FavoritesModel
    mediaState: Browse.FavoritesState
    screenTitle: qsTr("Favorites")
    emptyText: qsTr("No favorites yet")
    loadingText: qsTr("Loading favorites…")
    detailShowTitle: false
    totalItemsOverride: favorites.favoriteTotal > 0 ? favorites.favoriteTotal : -1
    gridTotalItemsOverride: favorites.favoriteTotal > 0 ? favorites.favoriteTotal : -1
    gridHasMorePages: Browse.FavoritesModel.has_next_page
    paginationTotalKnown: false
    gridTileTopLabelProvider: favorites.selectedSystemId === "" ? (index => Browse.FavoritesModel.system_name_at(index)) : null
    topStripTotalPagesProvider: () => favorites.mediaGrid.totalPageCount
    topStripTotalTextProvider: () => favorites.favoriteTotal >= 0 ? qsTr("%n favorite(s)", "", favorites.favoriteTotal) : ""
    pageMenuEnabledWhenEmpty: true

    onSelectedSystemIdChanged: Browse.FavoritesModel.set_system(favorites.selectedSystemId)
    Component.onCompleted: Browse.FavoritesModel.set_system(favorites.selectedSystemId)

    Connections {
        target: Browse.FavoritesModel

        function onLoadingChanged(): void {
            if (!Browse.FavoritesModel.loading)
                favorites.restoreSelection();
        }
    }
}
