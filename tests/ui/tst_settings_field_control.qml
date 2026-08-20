// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton methods aren't marked final so Browse.* calls trip
// "Member can be shadowed" — same convention every other screen-level test
// file in this suite uses. Structural; suppress compiler.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Screens

TestCase {
    id: testCase
    name: "UiSettingsFieldControl"
    when: windowShown
    width: 640
    height: 480
    visible: true

    SettingsScreen {
        id: screen
        anchors.fill: parent
    }

    // Regression coverage for the `_fieldControl` bug (#225, `6fd1d2f`): an
    // accidentally-nested conditional deleted `return "navigate"` and made
    // the action-row check unreachable, so 17 non-toggle rows rendered as
    // blank-valued pickers with no chevron and no status caption for two
    // months before anyone noticed. One assertion per control kind so a
    // future regression here fails loudly instead of silently.
    function test_field_control_covers_every_row_kind(): void {
        compare(screen._fieldControl("showHidden"), "toggle");
        compare(screen._fieldControl("reduceMotion"), "toggle");
        compare(screen._fieldControl("aboutLicense"), "navigate");
        compare(screen._fieldControl("crtCalibration"), "navigate");
        compare(screen._fieldControl("pageDisplayInterface"), "navigate");
        compare(screen._fieldControl("updateMediaDb"), "action");
        compare(screen._fieldControl("runScraper"), "action");
        compare(screen._fieldControl("uploadLog"), "action");
        compare(screen._fieldControl("resolution"), "picker");
        compare(screen._fieldControl("colorScheme"), "picker");
    }

    // `colorScheme` is a picker (left/right is a no-op, Accept opens the
    // list-picker modal) — it was missing from this list, so the row
    // advertised "Change" instead of "Open".
    function test_focused_field_is_picker_includes_color_scheme(): void {
        screen.currentPage = screen.pageDisplayInterface;
        const fields = screen.fields;
        let idx = -1;
        for (let i = 0; i < fields.length; i++) {
            if (fields[i].id === "colorScheme") {
                idx = i;
                break;
            }
        }
        verify(idx >= 0, "colorScheme field not found on the Display page");
        screen.currentIndex = idx;
        compare(screen.focusedFieldIsPicker, true);
    }
}
