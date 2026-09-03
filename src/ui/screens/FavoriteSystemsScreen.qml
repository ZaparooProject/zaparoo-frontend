// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// Favorite Systems screen — paged grid driven by `Browse.FavoriteSystemsModel`.
// Pure input dispatcher: emits a system id on Accept and the shared Hub signal
// on Back. Main.qml owns destination choice and transition orchestration.
MediaListScreen {
    id: favoriteSystems

    property alias favoriteSystemsGrid: favoriteSystems.mediaGrid

    signal requestAccept(string systemId)

    mediaModel: Browse.FavoriteSystemsModel
    mediaState: Browse.FavoriteSystemsState
    // Structurally a MediaListScreen but semantically a systems screen —
    // follows the Systems layout preference, not the Games one.
    layoutScope: "systems"
    screenTitle: qsTr("Favorites")
    gridViewId: "systemsGrid"
    listViewId: "systemsList"
    tateListViewId: "systemsListTate"
    showTopStrip: true
    activeLabelAtBottom: false
    gridBottomMargin: Sizing.pctH(8) + Sizing.pctH(7)
    topStripTitleProvider: () => qsTr("Favorites")
    topStripTotalTextProvider: () => favoriteSystems.mediaGrid.itemCount > 0 ? qsTr("%n system(s) with favorites", "", Browse.FavoriteSystemsModel.count) : ""
    // Shared list chrome shows focused-item/total-items on the right;
    // FavoriteSystems needs no text override there.
    activeLabelTextProvider: () => favoriteSystems.mediaGrid.itemCount > 0 ? Browse.FavoriteSystemsModel.name_at(favoriteSystems.mediaGrid.currentIndex) : ""
    activeLabelTagsProvider: () => {
        // Reads Browse.FavoriteSystemsModel.media_counts_revision explicitly
        // as a dependency: media_count_for_system is a qinvokable METHOD,
        // not a qproperty, so calling it below does not itself register a
        // reactive dependency -- revision is what makes this binding
        // re-evaluate after Core refreshes a system's media count in the
        // background (see FavoriteSystemsModel's header comment).
        const _rev = Browse.FavoriteSystemsModel.media_counts_revision;
        if (favoriteSystems.mediaGrid.itemCount <= 0)
            return "";
        const systemId = Browse.FavoriteSystemsModel.system_id_at(favoriteSystems.mediaGrid.currentIndex);
        const count = Browse.FavoriteSystemsModel.media_count_for_system(systemId);
        return count >= 0 ? qsTr("%n favorite(s)", "", count) : "";
    }
    gridColumnsOverride: Sizing.systemsGridShape(Sizing.screenWidth, Sizing.screenHeight).columns
    gridRowsOverride: Sizing.systemsGridShape(Sizing.screenWidth, Sizing.screenHeight).rows
    gridShowCaption: false
    emptyText: qsTr("No favorites yet")
    loadingText: qsTr("Loading favorite systems…")
    detailShowTitle: false
    detailShowDescription: false
    detailPlaceholderKey: "icons/Console"
    pageMenuEnabledWhenEmpty: true
    retryAction: () => Browse.FavoriteSystemsModel.retry()

    acceptAction: index => {
        if (favoriteSystems.mediaModel === null || favoriteSystems.mediaGrid.itemCount <= 0 || pressCommit.running)
            return;
        favoriteSystems.pulseActivate();
        pressCommit._systemId = Browse.FavoriteSystemsModel.system_id_at(index);
        pressCommit.arm();
    }
    cancelAction: () => {
        pressCommit.stop();
        favoriteSystems.requestHubScreen();
    }

    DeferredAction {
        id: pressCommit

        property string _systemId: ""
        onDeferred: {
            const systemId = _systemId;
            _systemId = "";
            favoriteSystems.requestAccept(systemId);
        }
    }
}
