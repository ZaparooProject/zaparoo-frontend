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
        screen.currentPage = screen.pageAppearance;
        const fields = screen.fields;
        let idx = -1;
        for (let i = 0; i < fields.length; i++) {
            if (fields[i].id === "colorScheme") {
                idx = i;
                break;
            }
        }
        verify(idx >= 0, "colorScheme field not found on the Appearance page");
        screen.currentIndex = idx;
        compare(screen.focusedFieldIsPicker, true);
    }

    // Round 9, item 5: the "Discover arcade alternate versions" toggle was
    // removed -- the feature it gated is now permanently on. Every id chain
    // `_fieldControl` used to recognise it through must no longer route it
    // to "toggle" (the id simply isn't a field anywhere any more, so it
    // falls through to the function's default "picker" -- this test exists
    // to catch a future re-add of the id to any one of those chains without
    // the others, not to assert a meaningful return value for a dead id).
    function test_discover_arcade_alternate_versions_field_is_gone(): void {
        let found = false;
        for (let i = 0; i < screen.libraryDataFields.length; i++) {
            if (screen.libraryDataFields[i].id === "discoverArcadeAlternateVersions")
                found = true;
        }
        verify(!found, "discoverArcadeAlternateVersions must no longer be a registered field");
    }

    // Round 9, item 7: the shared hint band's two-line reservation
    // (`settingsCard._hintTextHeight`) used to be `2 * Sizing.px(Sizing.fontBody
    // * 1.35)` -- Noto Sans's real line spacing is ~1.362 em, so that
    // reservation was consistently a couple px short of what two actual
    // lines need, and `Text`'s own "drop the line that doesn't fit, elide
    // the last one that does" behavior silently collapsed every
    // two-line-worthy description down to one truncated line -- at this
    // test's 480x-tier size the old formula reserved ~44px against a real
    // ~46px need. Now driven off a real `FontMetrics.lineSpacing`
    // measurement. `region`'s description is one of the strings that needs
    // (and, at this size, only needs) the second line.
    function test_hint_band_reserves_a_real_second_line(): void {
        screen._switchPage(screen.pageLanguage);
        const fields = screen.fields;
        let idx = -1;
        for (let i = 0; i < fields.length; i++) {
            if (fields[i].id === "region") {
                idx = i;
                break;
            }
        }
        verify(idx >= 0, "region field not found on the Language page");
        screen.currentIndex = idx;

        const hint = findChild(screen, "settingsHintText");
        verify(hint !== null);
        compare(hint.text, fields[idx].description);
        verify(!hint.truncated, "a description long enough to need two lines must not be elided down to one");
    }

    // Round 9, item 8: sub-page <-> parent focus is remembered for the rest
    // of a visit (SettingsScreen instance survives Settings <-> Hub, see
    // MainLayout.qml's settingsScreenLoader), but leaving Settings for the
    // Hub must reset back to the root category grid -- `_goBack()`'s
    // root-exit branch clears `_pageIndexes` and `currentIndex` before
    // emitting `requestHubScreen`.
    function test_going_back_from_root_resets_page_memory(): void {
        screen._switchPage(screen.pageLibraryData);
        screen.currentIndex = Math.min(2, screen.fieldCount - 1);
        screen._switchPage(screen.pageRoot);
        verify(screen.currentPage === screen.pageRoot);

        // Re-enter the same sub-page mid-visit: focus is remembered.
        screen._switchPage(screen.pageLibraryData);
        compare(screen.currentIndex, Math.min(2, screen.fieldCount - 1));
        screen._switchPage(screen.pageRoot);

        // Now leave Settings entirely from the root grid.
        let backSignalCount = 0;
        const onBack = () => {
            backSignalCount++;
        };
        screen.requestHubScreen.connect(onBack);
        screen._goBack();
        screen.requestHubScreen.disconnect(onBack);
        compare(backSignalCount, 1, "_goBack must emit requestHubScreen from the root grid");
        compare(screen.currentPage, screen.pageRoot);
        compare(Object.keys(screen._pageIndexes).length, 0, "_pageIndexes must be cleared on exit");

        // Re-entering the same sub-page after the reset starts fresh, not
        // at the previously-remembered row.
        screen._switchPage(screen.pageLibraryData);
        compare(screen.currentIndex, screen._firstNavigableIndex());
    }
}
