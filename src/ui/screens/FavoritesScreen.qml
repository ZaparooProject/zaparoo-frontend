// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

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
// favorites-specific model, persisted selection state, and copy.
MediaListScreen {
    id: favorites

    property alias favoritesGrid: favorites.mediaGrid

    mediaModel: Browse.FavoritesModel
    mediaState: Browse.FavoritesState
    screenTitle: qsTr("Favorites")
    emptyText: qsTr("No favorites yet")
    loadingText: qsTr("Loading favorites…")
}
