// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "BrowseDetailPane"
    when: windowShown
    width: 320
    height: 240
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    BrowseDetailPane {
        id: pane

        width: 320
        height: 240
        loadingDelayMs: 150
        showTitle: true
    }

    function resetPane(): void {
        pane.loading = false;
        pane.detailSuppressed = false;
        pane.title = "";
        pane.detailTags = "";
        pane.coverKey = "";
        wait(1);
    }

    function init(): void {
        resetPane();
    }

    function cleanup(): void {
        resetPane();
    }

    function test_metadata_stays_visible_while_loading(): void {
        pane.title = "Selected Game";
        pane.detailTags = "Year\t1990\nGenre\tAction";
        pane.coverKey = "icons/Loading";
        pane.loading = true;
        wait(1);

        verify(findChild(pane, "detailTitleText").visible);
        verify(findChild(pane, "detailTagTable").visible);
        verify(findChild(pane, "detailPlaceholderIcon").visible);
        verify(!findChild(pane, "detailLoadingIndicator").visible);
    }

    function test_loading_icon_survives_media_image_handoff(): void {
        pane.coverKey = "icons/Loading";
        wait(1);
        verify(findChild(pane, "detailPlaceholderIcon").visible);

        pane.coverKey = "media-image/not-ready";
        verify(findChild(pane, "detailPlaceholderIcon").visible);
    }

    function test_suppressed_detail_still_hides_metadata(): void {
        pane.title = "Selected Game";
        pane.detailTags = "Year\t1990";
        pane.detailSuppressed = true;
        wait(1);

        verify(!findChild(pane, "detailTitleText").visible);
        verify(!findChild(pane, "detailTagTable").visible);
    }
}
