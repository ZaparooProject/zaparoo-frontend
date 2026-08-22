// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "UiPressableSurface"
    when: windowShown
    width: 640
    height: 480
    visible: true

    PressableSurface {
        id: surface
        width: 200
        height: 48
        pointerAcceptedButtons: Qt.LeftButton

        Rectangle {
            id: marker
            objectName: "pressableContentMarker"
            x: 10
            y: 10
            width: 8
            height: 8
        }
    }

    SignalSpy {
        id: pointerSpy
        target: surface
        signalName: "pointerClicked"
    }

    // Dedicated fixture for the ring-proportion test below: a tall surface
    // (roughly tile-sized) to compare against `surface`'s short,
    // menu-row-sized 48px. Kept separate from `surface` rather than
    // mutating its geometry so this test can't leak state into any other
    // test in this file.
    PressableSurface {
        id: tallSurface
        width: 200
        height: 480
    }

    function _linearChannel(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    function _relativeLuminance(value: color): real {
        return 0.2126 * _linearChannel(value.r) + 0.7152 * _linearChannel(value.g) + 0.0722 * _linearChannel(value.b);
    }

    function _contrastRatio(first: color, second: color): real {
        const firstLuminance = _relativeLuminance(first);
        const secondLuminance = _relativeLuminance(second);
        return (Math.max(firstLuminance, secondLuminance) + 0.05) / (Math.min(firstLuminance, secondLuminance) + 0.05);
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    function init(): void {
        Motion.enabled = false;
        surface.focused = false;
        surface.pressed = false;
        pointerSpy.clear();
    }

    function cleanup(): void {
        Motion.enabled = true;
    }

    function test_rest_and_pressed_geometry_snap_with_motion_disabled(): void {
        compare(surface.faceOffset, 0);
        compare(surface.visibleEdgeHeight, Sizing.pressEdgeHeight);
        const restY = marker.mapToItem(surface, 0, 0).y;

        surface.pressed = true;
        compare(surface.faceOffset, Sizing.pressEdgeHeight);
        compare(surface.visibleEdgeHeight, 0);
        compare(marker.mapToItem(surface, 0, 0).y, restY + Sizing.pressEdgeHeight);
        compare(surface.faceOffset, Math.round(surface.faceOffset));
    }

    function test_outer_edges_remain_clickable(): void {
        mouseClick(surface, Sizing.half(surface.width), surface.height - 1, Qt.LeftButton);
        compare(pointerSpy.count, 1);

        surface.pressed = true;
        mouseClick(surface, Sizing.half(surface.width), 0, Qt.LeftButton);
        compare(pointerSpy.count, 2);
    }

    // The face border no longer changes with focus — Tile-weight focus now
    // reads through the two-rect accent ring below, matching how a grid
    // tile's own focus ring works (see PressableSurface.qml and Tile.qml).
    function test_face_border_is_constant_regardless_of_focus(): void {
        const face = findChild(surface, "pressableFace");
        verify(face !== null);
        surface.focused = false;
        compare(face.border.width, Sizing.cardBorderWidth);
        compare(face.border.color, Theme.borderMid);
        surface.focused = true;
        compare(face.border.width, Sizing.cardBorderWidth);
        compare(face.border.color, Theme.borderMid);
    }

    // Focus ring: an outer accent-filled rounded rect with an inner
    // face-colored mask punched out of its center, snapping on/off with
    // `focused` rather than fading — mirrors Tile.qml's construction
    // byte-for-byte because thin rounded *borders* step visibly at the
    // corners under Qt's software adaptation (QTBUG-123210) while filled
    // rounded rects don't.
    function test_focus_ring_is_two_stacked_filled_rects(): void {
        const outer = findChild(surface, "pressableFocusRingOuter");
        const inner = findChild(surface, "pressableFocusRingInner");
        verify(outer !== null);
        verify(inner !== null);

        surface.focused = false;
        compare(outer.visible, false);
        compare(inner.visible, false);

        surface.focused = true;
        compare(outer.visible, true);
        compare(inner.visible, true);
        compare(outer.color, Theme.accent);
        compare(inner.color, surface.faceColor);
        // Inner mask sits strictly inside the outer ring by the ring width,
        // leaving a uniform accent border rather than covering it entirely.
        verify(inner.width < outer.width);
        verify(inner.height < outer.height);
        verify(_contrastRatio(outer.color, surface.faceColor) >= 3.0);
    }

    // The ring's weight is derived from the row's own resting border
    // (`Sizing.cardBorderWidth`), not an independent percentage of the
    // surface's height -- see PressableSurface.qml's doc comment. Two
    // earlier versions got this wrong in opposite directions: a
    // screen-relative percentage ate ~38% of a short row's height; a
    // surface-height-relative one floored to exactly 1px at every real
    // menu-row height, no heavier than the resting border it's meant to
    // stand out from. Deriving from `cardBorderWidth` fixes both: the ring
    // band is always exactly double the resting border's weight, by
    // construction, regardless of resolution or which PressableSurface
    // caller this is (`tallSurface` and `surface` -- a modal-button/menu-row
    // height and a tile-scale height -- read identically here on purpose,
    // since every real PressableSurface caller is a short row or button;
    // Tile.qml draws its own, separate ring rather than using this one).
    function test_focus_ring_is_derived_from_the_resting_border_weight(): void {
        compare(surface._ringGap, Sizing.cardBorderWidth);
        compare(surface._ringWidth, Sizing.cardBorderWidth * 2);
        verify(surface._ringWidth > Sizing.cardBorderWidth, "the focused ring must always be heavier than the row's own resting border");
        compare(tallSurface._ringGap, surface._ringGap, "ring weight tracks the shared cardBorderWidth token, not this surface's own height");
        compare(tallSurface._ringWidth, surface._ringWidth);
    }

    function test_default_geometry_uses_small_radius_and_opaque_edge(): void {
        compare(surface.radius, Sizing.radiusSm);
        compare(surface.edgeHeight, Sizing.pressEdgeHeight);
        compare(surface.edgeColor, Theme.controlEdge);
        compare(surface.edgeColor.a, 1);
    }

    // Runs over every preset, because the catalog carries light presets as well
    // as dark ones. The invariant is not "the control edge is lighter" — on a
    // light preset both edges are darker than their ground — but "the control
    // edge sits one step further along the accent ramp than the tile edge",
    // which is direction-agnostic.
    function test_edge_uses_contextual_middle_tone(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.tileEdge, palette.bgDeep) >= 1.8, id + " tile edge/background");
            verify(_contrastRatio(palette.controlEdge, palette.bgPanel) >= 2.0, id + " control edge/panel");
            const ground = _relativeLuminance(palette.bgDeep);
            const control = Math.abs(_relativeLuminance(palette.controlEdge) - ground);
            const tile = Math.abs(_relativeLuminance(palette.tileEdge) - ground);
            verify(control > tile, id + " control edge must sit further from the ground than the tile edge");
        }
    }

    function test_edge_has_square_top_and_overlaps_face_corners(): void {
        const edge = findChild(surface, "pressableEdge");
        verify(edge !== null);
        compare(edge.topLeftRadius, 0);
        compare(edge.topRightRadius, 0);
        compare(edge.bottomLeftRadius, surface.radius);
        compare(edge.bottomRightRadius, surface.radius);
        compare(edge.height, surface.edgeHeight + surface.radius);
    }
}
