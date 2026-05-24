// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Browse as Browse

// Recently Played screen — flat paged grid driven by
// `Browse.RecentsModel`. Pure input dispatcher: emits
// `requestHubScreen()` on Escape and launches the highlighted entry on
// Accept by calling the model's `launch_at` (which fans out to Core's
// `run` endpoint).
//
// History is a flat list — no folder navigation, no card-write flow —
// so it reuses the shared `MediaListScreen` shell with the
// recents-specific model, persisted selection state, and copy.
MediaListScreen {
    id: recents

    property alias recentsGrid: recents.mediaGrid

    mediaModel: Browse.RecentsModel
    mediaState: Browse.RecentsState
    screenTitle: qsTr("Recently Played")
    emptyText: qsTr("Nothing played yet")
    loadingText: qsTr("Loading recently played…")
}
