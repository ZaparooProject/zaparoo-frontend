// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "UiContextMenu"
    when: windowShown
    width: 640
    height: 480
    visible: true

    ContextMenu {
        id: menu
        open: true
        entries: [
            {
                "id": "one",
                "label": "One"
            }
        ]
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    function init(): void {
        menu.anchorRect = Qt.rect(0, 0, 0, 0);
        menu.anchorRadius = 0;
    }

    function _corner(name: string): var {
        return findChild(menu, name);
    }

    // A zero radius (today's default for anchors we haven't confirmed as
    // PressableSurface, e.g. hub_favorites' action tile) must keep the
    // square hole byte-identical: no corner piece drawn at all.
    function test_zero_radius_hides_all_corners(): void {
        menu.anchorRect = Qt.rect(40, 40, 200, 80);
        menu.anchorRadius = 0;
        verify(!menu._canCutCorners);
        verify(!_corner("contextMenuCornerTl").visible);
        verify(!_corner("contextMenuCornerTr").visible);
        verify(!_corner("contextMenuCornerBl").visible);
        verify(!_corner("contextMenuCornerBr").visible);
    }

    // A large enough anchor with a nonzero radius must show all four
    // corners, each sourced from the matching baked mask.
    function test_nonzero_radius_shows_all_corners(): void {
        menu.anchorRect = Qt.rect(40, 40, 200, 80);
        menu.anchorRadius = 6;
        verify(menu._canCutCorners);
        const tl = _corner("contextMenuCornerTl");
        const tr = _corner("contextMenuCornerTr");
        const bl = _corner("contextMenuCornerBl");
        const br = _corner("contextMenuCornerBr");
        verify(tl.visible);
        verify(tr.visible);
        verify(bl.visible);
        verify(br.visible);
        compare(tl.source.toString(), Resources.cornerCutUrl(6, "tl", Theme.scrim).toString());
        compare(tr.source.toString(), Resources.cornerCutUrl(6, "tr", Theme.scrim).toString());
        compare(bl.source.toString(), Resources.cornerCutUrl(6, "bl", Theme.scrim).toString());
        compare(br.source.toString(), Resources.cornerCutUrl(6, "br", Theme.scrim).toString());
    }

    // Corner pieces sit flush against the hole edges, not just somewhere
    // inside the anchor rect -- a stray offset would reopen the notch gap
    // this component exists to close.
    function test_corners_are_flush_with_the_hole_edges(): void {
        menu.anchorRect = Qt.rect(40, 40, 200, 80);
        menu.anchorRadius = 6;
        compare(_corner("contextMenuCornerTl").x, menu._holeLeft);
        compare(_corner("contextMenuCornerTl").y, menu._holeTop);
        compare(_corner("contextMenuCornerTr").x, menu._holeRight - menu.anchorRadius);
        compare(_corner("contextMenuCornerTr").y, menu._holeTop);
        compare(_corner("contextMenuCornerBl").x, menu._holeLeft);
        compare(_corner("contextMenuCornerBl").y, menu._holeBottom - menu.anchorRadius);
        compare(_corner("contextMenuCornerBr").x, menu._holeRight - menu.anchorRadius);
        compare(_corner("contextMenuCornerBr").y, menu._holeBottom - menu.anchorRadius);
    }

    // An anchor narrower than two radii on either axis would make the two
    // corner masks on that axis overlap; skip the pieces instead and fall
    // back to the square hole rather than drawing garbled corners.
    function test_anchor_too_small_for_two_radii_skips_corners(): void {
        menu.anchorRect = Qt.rect(40, 40, 10, 80);
        menu.anchorRadius = 6;
        verify(!menu._canCutCorners);
        verify(!_corner("contextMenuCornerTl").visible);

        menu.anchorRect = Qt.rect(40, 40, 200, 10);
        menu.anchorRadius = 6;
        verify(!menu._canCutCorners);
        verify(!_corner("contextMenuCornerTl").visible);
    }

    // Panel width tracks the longest entry rather than sitting on the old
    // large fixed floor (24% of width), so a one-word menu no longer opens
    // a panel sized for a much longer label.
    function test_panel_width_tracks_longest_label(): void {
        menu.entries = [
            {
                "id": "one",
                "label": "Go"
            }
        ];
        const shortWidth = menu.panelWidth;
        verify(shortWidth >= menu._minPanelWidth);
        verify(shortWidth < Sizing.pctW(24), "short label must not hit the old fixed floor");

        menu.entries = [
            {
                "id": "one",
                "label": "A Considerably Longer Menu Entry Label"
            }
        ];
        const longWidth = menu.panelWidth;
        verify(longWidth > shortWidth);
    }

    // Round 7: rows moved from PressableSurface to the same inverse-video
    // SelectionBar as browse/settings rows -- see docs/style.md -> "Two
    // registers". The focused row's bar is active and its label reads
    // `onAccent`; every other row stays flat with a plain `textPrimary`
    // label.
    function test_selected_row_uses_inverse_video(): void {
        menu.entries = [
            {
                "id": "one",
                "label": "One"
            },
            {
                "id": "two",
                "label": "Two"
            }
        ];
        menu.currentIndex = 0;

        const firstRow = findChild(menu, "contextMenuRow-0");
        const secondRow = findChild(menu, "contextMenuRow-1");
        verify(firstRow !== null);
        verify(secondRow !== null);
        const firstBar = findChild(firstRow, "contextMenuSelectionBar");
        const secondBar = findChild(secondRow, "contextMenuSelectionBar");
        verify(firstBar !== null);
        verify(secondBar !== null);
        verify(firstBar.active);
        verify(!secondBar.active);

        menu.currentIndex = 1;
        verify(!firstBar.active);
        verify(secondBar.active);
    }

    // Round 9: `_textWidth` reads `rowLabelMetrics.advanceWidth`/
    // `.boundingRect` as *properties* of a `TextMetrics` bound to
    // `font.weight: bar.contentWeight` -- not a `FontMetrics` fed through
    // a Q_INVOKABLE `advanceWidth(text)` call, the round-8 shape that let
    // a selected row's label repaint bold while its own centering box
    // stayed pinned to the stale Font.Normal measurement (see
    // ListPickerModal.qml's identical fix and its own regression test for
    // why the "pass it as an unused parameter" alternative doesn't
    // reliably work under this project's AOT-compiled QML). Confirms the
    // label's own width tracks a fresh measurement in both directions,
    // not just whichever weight was current when the row was built.
    function test_selected_row_label_width_never_goes_stale(): void {
        menu.entries = [
            {
                "id": "one",
                "label": "One"
            },
            {
                "id": "two",
                "label": "A Considerably Longer Menu Entry Label"
            }
        ];
        menu.currentIndex = 0;

        const row = findChild(menu, "contextMenuRow-1");
        verify(row !== null);
        const label = findChild(row, "contextMenuRowLabel");
        verify(label !== null);
        const metrics = findChild(row, "contextMenuRowLabelMetrics");
        verify(metrics !== null);

        function expectedWidth() {
            const available = Math.max(0, row.width - 2 * menu.horizontalPadding);
            return Math.min(Math.ceil(Math.max(metrics.advanceWidth, metrics.boundingRect.width)) + Sizing.stroke(2), available);
        }

        compare(metrics.font.weight, Font.Normal, "unselected row must measure at Font.Normal");
        compare(label.width, expectedWidth(), "resting width must match a fresh measurement");

        menu.currentIndex = 1;
        compare(metrics.font.weight, Font.Medium, "selecting the row must flip its metrics to Font.Medium");
        compare(label.width, expectedWidth(), "selected width must match a fresh measurement, not the stale resting one");

        menu.currentIndex = 0;
        compare(metrics.font.weight, Font.Normal, "deselecting the row must flip its metrics back to Font.Normal");
        compare(label.width, expectedWidth(), "width after deselecting must match a fresh measurement too");
    }

    function _manyEntries(count: int): var {
        const entries = [];
        for (let i = 0; i < count; i++)
            entries.push({
                "id": "entry-" + i,
                "label": "Entry " + i
            });
        return entries;
    }

    // A data-driven menu (e.g. "Discover alt. versions", up to
    // MAX_ALT_RESULTS in alternate_versions.rs) can exceed the panel's
    // available height. `panel`'s `clip: true` used to be the only thing
    // stopping overflow rows from painting past the rounded corners --
    // `move()` could still select a row that was clipped out of view
    // entirely. `rowViewport` must keep the focused row inside its visible
    // band instead.
    function test_more_entries_than_fit_scrolls_the_focused_row_into_view(): void {
        menu.entries = _manyEntries(30);
        menu.currentIndex = 0;

        verify(menu._scrollable, "30 short entries must exceed the panel's available height in this test window");

        const viewport = findChild(menu, "contextMenuRowViewport");
        verify(viewport !== null);
        compare(viewport.contentY, 0, "menu opens scrolled to the top");

        // Column stacks children top to bottom with `spacing` between them
        // -- a documented, stable property of Column's own layout, not an
        // implementation detail of ContextMenu.qml (`_scrollCurrentIntoView`
        // relies on exactly this to compute a row's position without
        // touching the rendered tree). Used here as the test's independent
        // oracle instead of reading a delegate's own `y`, which Column only
        // commits on the next polish pass and would otherwise still read
        // stale mid-loop with no `wait()` between moves.
        const stride = menu.rowHeight + menu.rowSpacing;

        // Walk to the last entry one row at a time -- the same path real
        // input takes -- and confirm the focused row's position never
        // falls outside the viewport's visible band.
        for (let i = 1; i < menu.entries.length; i++) {
            menu.move(1);
            const rowTop = menu.currentIndex * stride;
            const rowBottom = rowTop + menu.rowHeight;
            verify(rowTop >= viewport.contentY, "row " + menu.currentIndex + " top must not sit above the visible band");
            verify(rowBottom <= viewport.contentY + viewport.height, "row " + menu.currentIndex + " bottom must not sit below the visible band");
        }
        compare(menu.currentIndex, menu.entries.length - 1);
        verify(viewport.contentY > 0, "reaching the last row must have scrolled the viewport");

        // Wrapping back to the first entry must scroll all the way back to
        // the top, not leave the last row's scroll position behind.
        menu.move(1);
        compare(menu.currentIndex, 0);
        compare(viewport.contentY, 0, "wrapping to the first entry must scroll back to the top");
    }

    // A menu short enough to fit must render exactly as it did before
    // `rowViewport` existed: unscrollable, contentY pinned at 0.
    function test_few_entries_never_scrolls(): void {
        menu.entries = _manyEntries(3);
        menu.currentIndex = 0;

        verify(!menu._scrollable);
        const viewport = findChild(menu, "contextMenuRowViewport");
        verify(viewport !== null);
        compare(viewport.contentY, 0);

        menu.move(1);
        menu.move(1);
        compare(viewport.contentY, 0, "a menu that already fits must never scroll");
    }

    // Swapping to a new, still-long entry list on an already-open menu
    // (the "Discover alt. versions" submenu replaces the main list in
    // place) must not carry over a scroll position left by the previous
    // list.
    function test_swapping_entries_while_open_resets_scroll(): void {
        menu.entries = _manyEntries(30);
        menu.currentIndex = menu.entries.length - 1;
        const viewport = findChild(menu, "contextMenuRowViewport");
        verify(viewport.contentY > 0, "selecting the last of 30 entries must have scrolled down");

        menu.entries = _manyEntries(30);
        compare(viewport.contentY, 0, "a fresh entries array must reset scroll even if the count is unchanged");
    }
}
