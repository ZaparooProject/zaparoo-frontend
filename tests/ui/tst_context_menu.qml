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
        entries: [{
            "id": "one",
            "label": "One"
        }]
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
}
