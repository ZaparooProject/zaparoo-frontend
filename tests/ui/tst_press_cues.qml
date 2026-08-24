// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase

    name: "PressCues"
    when: windowShown

    Component {
        id: tileHost

        Item {
            width: 180
            height: 220
            property bool isSelected: true
            property bool isFocused: true
            property string name: "Test tile"
            property string coverKey: ""
            property string topLabel: ""
            property int favorite: 0
            property bool hidden: false
            property string disambiguatingTags: ""
            property int activatePulse: 0
            property int releasePulse: 0
            property bool settling: false
            property bool focusReady: true
            property bool held: false

            Tile {
                anchors.fill: parent
            }
        }
    }

    Component {
        id: toggleHost

        SettingsField {
            objectName: "toggleField"
            width: 420
            height: 48
            label: "Toggle"
            value: ""
            control: "toggle"
            checked: true
        }
    }

    Component {
        id: pickerFieldHost

        SettingsField {
            objectName: "pickerField"
            width: 420
            height: 48
            label: "Picker"
            value: "Value"
            control: "picker"
            isFocused: true
        }
    }

    Component {
        id: listHost

        Item {
            width: 420
            height: 240

            ListModel {
                id: entries

                ListElement {
                    name: "Alpha"
                    fileStem: "alpha"
                    coverKey: ""
                    favorite: 0
                    disambiguatingTags: ""
                    entryType: "media"
                    fileCount: 0
                }
                ListElement {
                    name: "Beta"
                    fileStem: "beta"
                    coverKey: ""
                    favorite: 1
                    disambiguatingTags: "US"
                    entryType: "media"
                    fileCount: 0
                }
            }

            BrowseList {
                id: list

                objectName: "browseList"
                anchors.fill: parent
                model: entries
                currentIndex: 0
                targetVisibleRowCount: 2
            }
        }
    }

    Component {
        id: contextMenuHost

        Item {
            width: 420
            height: 240

            ContextMenu {
                id: menu

                objectName: "contextMenu"
                anchors.fill: parent
                open: true
                anchorRect: Qt.rect(10, 10, 40, 40)
                entries: [
                    {
                        "id": "one",
                        "label": "One"
                    },
                    {
                        "id": "two",
                        "label": "Two"
                    }
                ]
            }
        }
    }

    Component {
        id: listPickerHost

        Item {
            width: 420
            height: 240

            ListPickerModal {
                id: picker

                objectName: "listPicker"
                anchors.fill: parent
                open: true
                entries: [
                    {
                        "id": "one",
                        "label": "One"
                    },
                    {
                        "id": "two",
                        "label": "Two"
                    }
                ]
            }
        }
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

    function init(): void {
        Motion.enabled = true;
        Sizing.screenWidth = 960;
        Sizing.screenHeight = 540;
        Sizing.crtNativePath = false;
        Sizing.swapPercentageAxes = false;
    }

    function cleanup(): void {
        Motion.enabled = true;
    }

    function test_toggle_knob_geometry(): void {
        const field = createTemporaryObject(toggleHost, testCase);
        verify(field !== null);

        const track = findChild(field, "settingsToggleTrack");
        const knob = findChild(field, "settingsToggleKnob");
        verify(track !== null);
        verify(knob !== null);
        // Geometry is unaffected by which row register the toggle sits in —
        // knob size/travel are the primary on/off cue in both.
        compare(knob.width, track.height - Sizing.pctH(0.9) + 1);
        compare(knob.height, knob.width);
        compare(knob.y, Sizing.center(track.height, knob.height));
        compare(track.width - knob.x - knob.width, knob.y);
    }

    // The track alone carries on/off state, at maximum contrast against the
    // row's own current background; the knob always matches that same
    // background, reading as a hole punched through the track rather than a
    // second state signal — see SettingsField.qml and docs/style.md ->
    // "Toggle rows". This is deliberately the same rule in both row
    // registers: only which background the track/knob resolve against
    // changes.
    //
    // Round 5 adds a border to the knob, in the track's own "on" color for
    // that register — this is what stops the knob from visually merging
    // into a selected row (whose own background is the same solid
    // `Theme.accent` the knob's fill uses). The border reuses colors the
    // semantic-tier guardrail tests already guarantee clear >=4.5:1 against
    // the fill, so no new color derivation was needed.
    function test_toggle_track_carries_state_knob_matches_row_background(): void {
        const field = createTemporaryObject(toggleHost, testCase);
        verify(field !== null);

        const track = findChild(field, "settingsToggleTrack");
        const knob = findChild(field, "settingsToggleKnob");
        verify(track !== null);
        verify(knob !== null);

        field.isFocused = false;
        field.checked = true;
        compare(track.color, Theme.accent);
        compare(knob.color, Theme.surfaceCard);
        compare(knob.border.color, Theme.accent);
        verify(_contrastRatio(knob.color, track.color) >= 3.0);
        verify(_contrastRatio(knob.border.color, knob.color) >= 3.0);

        field.checked = false;
        compare(track.color, Theme.borderMid);
        compare(knob.color, Theme.surfaceCard);
        compare(knob.border.color, Theme.accent);
        // Track/knob-fill contrast is deliberately low here: `borderMid` is
        // itself a subtle, near-card neutral, so a "hole" cut through it
        // reveals a similarly subtle color. The off state's affordance
        // comes from knob position, not track/fill color contrast — see
        // the geometry test above. The knob's own border still separates
        // it from its fill regardless.

        field.isFocused = true;
        field.checked = true;
        compare(track.color, Theme.onAccent);
        compare(knob.color, Theme.accent);
        compare(knob.border.color, Theme.onAccent);
        verify(_contrastRatio(knob.color, track.color) >= 3.0);
        verify(_contrastRatio(knob.border.color, knob.color) >= 3.0);

        field.checked = false;
        compare(track.color, Theme.onAccentMuted);
        compare(knob.color, Theme.accent);
        compare(knob.border.color, Theme.onAccent);
        verify(_contrastRatio(knob.color, track.color) >= 3.0);
        verify(_contrastRatio(knob.border.color, knob.color) >= 3.0);
    }

    function test_tile_uses_physical_press_without_scaling(): void {
        const host = createTemporaryObject(tileHost, testCase);
        verify(host !== null);
        wait(1);

        const tile = findChild(host, "tile");
        const surface = findChild(host, "tileSurface");
        verify(tile !== null);
        verify(surface !== null);
        compare(tile.cardPressed, false);
        compare(tile.scale, 1.0);
        compare(surface.edgeColor, Theme.tileEdge);

        host.activatePulse++;
        compare(tile.cardPressed, true);
        tryCompare(surface, "faceOffset", Sizing.pressEdgeHeight, Motion.pressMs + 100);
        compare(tile.scale, 1.0);

        host.releasePulse++;
        compare(tile.cardPressed, false);
        tryCompare(surface, "faceOffset", 0, Motion.settleMs + 100);
    }

    // Held (Hub Options -> Move) is NOT pressed and touches nothing about
    // the face/edge at all — see Tile.qml's `_heldOpacity`. The whole tile
    // blinks completely out of existence and back via `opacity` on the
    // tile's own root item (0/1, never animated — cascades to every
    // child, taking cover art, caption, AND focus ring with it; opacity
    // rather than `visible` because a `visible: <bound expression>`
    // binding did not reliably reflect changes in testing on a related
    // element, so opacity is used everywhere a hard binary cut is needed
    // here). Motion is disabled for this test: with motion off, the tile
    // shows normally (opacity 1) instead of blinking — freezing on
    // "invisible" would hide the very thing a reduce-motion user needs to
    // see, and there's no static "half blinked" state to freeze on the
    // way a color cue would have one.
    function test_tile_held_is_not_pressed(): void {
        Motion.enabled = false;
        const host = createTemporaryObject(tileHost, testCase);
        verify(host !== null);
        wait(1);

        const tile = findChild(host, "tile");
        const surface = findChild(host, "tileSurface");
        verify(tile !== null);
        verify(surface !== null);
        compare(tile.cardPressed, false);
        compare(surface.edgeColor, Theme.tileEdge);
        compare(surface.faceOffset, 0);
        compare(tile.opacity, 1);

        host.held = true;
        compare(tile.cardPressed, false, "held must not claim the pressed face gap");
        compare(surface.faceOffset, 0, "held must not sink the face — that's pressed's job");
        compare(surface.edgeColor, Theme.tileEdge, "held must not recolor the edge — the whole tile blinks instead");
        compare(tile.opacity, 1, "with motion disabled the tile shows normally instead of blinking");

        host.held = false;
        compare(tile.opacity, 1, "releasing held leaves the tile fully visible");
    }

    // With motion ON, held is a hard on/off cut (a `Timer` toggling the
    // tile's own `opacity` between exactly 0 and exactly 1, never an
    // intermediate value) — see Tile.qml's `_heldBlinkOn`. The whole tile
    // alternates between showing normally and being completely gone — no
    // color, no tint, nothing painted at all; nothing else changes at the
    // same time.
    function test_tile_held_blinks_the_whole_tile_out_of_existence(): void {
        const host = createTemporaryObject(tileHost, testCase);
        verify(host !== null);
        wait(1);

        const tile = findChild(host, "tile");
        const surface = findChild(host, "tileSurface");
        verify(tile !== null);
        verify(surface !== null);

        host.held = true;
        // Starts visible immediately (no fade-in to wait out).
        compare(tile.opacity, 1);
        // The face/edge never move or recolor at any point in the cycle —
        // only the tile's own opacity toggles.
        compare(surface.edgeColor, Theme.tileEdge);
        compare(surface.faceOffset, 0);
        // The timer snaps it off, then back on, on a hard cut — never
        // anything between 0 and 1.
        tryCompare(tile, "opacity", 0, 900);
        tryCompare(tile, "opacity", 1, 900);
    }

    function test_list_activation_flashes_selected_row(): void {
        const host = createTemporaryObject(listHost, testCase);
        verify(host !== null);
        wait(1);

        const list = findChild(host, "browseList");
        const firstRow = findChild(host, "browseListRow-0");
        verify(list !== null);
        verify(firstRow !== null);
        const bar = findChild(firstRow, "selectionBar");
        verify(bar !== null);
        compare(bar.flashing, false);
        compare(firstRow.scale, 1.0);

        list.activatePulse++;
        compare(bar.flashing, true);
        compare(firstRow.scale, 1.0);
        tryCompare(bar, "flashing", false, Motion.pressMs + 100);
    }

    // Regression test for item 2: the row title's inline tag suffix uses a
    // separate `variantColor` from the name's own `nameColor`, and it went
    // unset when the inverse-video row background flipped to accent -- there
    // was no coverage for this at all, which is how it shipped illegible.
    function test_list_tag_suffix_color_flips_with_selected_row(): void {
        const host = createTemporaryObject(listHost, testCase);
        verify(host !== null);
        // Under load (the full suite, not this file run in isolation) the
        // second delegate can take longer than the first to be created —
        // wait for it explicitly rather than assuming one event-loop tick
        // is always enough.
        tryVerify(() => findChild(host, "browseListRow-1") !== null, 1000);

        const firstRow = findChild(host, "browseListRow-0");
        const secondRow = findChild(host, "browseListRow-1");
        verify(firstRow !== null);
        verify(secondRow !== null);
        const secondSuffix = findChild(secondRow, "scrollingCaptionSuffixText");
        verify(secondSuffix !== null);

        // Compared as hex strings, not `color` values directly: `Text.color`
        // round-trips through Qt Quick's 8-bit-per-channel paint path, so a
        // value read back can differ from the assigned `color` property by a
        // sub-visible amount despite both displaying (and printing) the same
        // `#rrggbb` -- a direct `color` compare() is comparing precision the
        // rendering pipeline itself doesn't preserve.
        const list = findChild(host, "browseList");
        list.currentIndex = 0;
        compare(secondSuffix.color.toString(), Theme.textVariant.toString());

        list.currentIndex = 1;
        compare(secondSuffix.color.toString(), Theme.onAccentMuted.toString());
    }

    function test_list_selection_change_cuts_flash_short(): void {
        const host = createTemporaryObject(listHost, testCase);
        verify(host !== null);
        wait(1);

        const list = findChild(host, "browseList");
        const firstRow = findChild(host, "browseListRow-0");
        verify(list !== null);
        verify(firstRow !== null);
        const bar = findChild(firstRow, "selectionBar");
        verify(bar !== null);

        list.activatePulse++;
        compare(bar.flashing, true);
        list.currentIndex = 1;
        compare(bar.flashing, false);
    }

    // SettingsField mounts the same SelectionBar BrowseList does (see
    // SelectionBar.qml) so the two lists cannot drift apart.
    function test_settings_field_activation_flashes_selected_row(): void {
        const field = createTemporaryObject(pickerFieldHost, testCase);
        verify(field !== null);
        wait(1);

        const bar = findChild(field, "selectionBar");
        verify(bar !== null);
        compare(bar.flashing, false);

        field.activatePulse++;
        compare(bar.flashing, true);
        tryCompare(bar, "flashing", false, Motion.pressMs + 100);
    }

    // Toggle rows are exempt from the flash — the knob slide is their own
    // activation feedback (see SettingsField.qml's onActivatePulseChanged
    // guard on `control !== "toggle"`), so an inverse blink on top of a
    // sliding knob would double up the cue.
    function test_settings_field_toggle_row_does_not_flash(): void {
        const field = createTemporaryObject(toggleHost, testCase);
        verify(field !== null);
        field.isFocused = true;
        wait(1);

        const bar = findChild(field, "selectionBar");
        verify(bar !== null);

        field.activatePulse++;
        wait(Motion.pressMs + 100);
        compare(bar.flashing, false);
    }

    // ContextMenu rows moved from PressableSurface to SelectionBar — see
    // docs/style.md -> "Two registers". Accept plays the same inverse-video
    // flash BrowseList/SettingsField rows do, not a push-in.
    function test_context_menu_activation_flashes_selected_row(): void {
        const host = createTemporaryObject(contextMenuHost, testCase);
        verify(host !== null);
        wait(1);

        const firstRow = findChild(host, "contextMenuRow-0");
        verify(firstRow !== null);
        const bar = findChild(firstRow, "contextMenuSelectionBar");
        verify(bar !== null);
        compare(bar.active, true);
        compare(bar.flashing, false);

        const menu = findChild(host, "contextMenu");
        menu.handleAction("accept");
        compare(bar.flashing, true);
        tryCompare(bar, "flashing", false, Motion.pressMs + 100);
    }

    // Same move for ListPickerModal rows — see the ContextMenu test above.
    function test_list_picker_activation_flashes_selected_row(): void {
        const host = createTemporaryObject(listPickerHost, testCase);
        verify(host !== null);
        wait(1);

        const firstRow = findChild(host, "listPickerRow-0");
        verify(firstRow !== null);
        const bar = findChild(firstRow, "listPickerSelectionBar");
        verify(bar !== null);
        compare(bar.active, true);
        compare(bar.flashing, false);

        const picker = findChild(host, "listPicker");
        picker.handleAction("accept");
        compare(bar.flashing, true);
        tryCompare(bar, "flashing", false, Motion.pressMs + 100);
    }
}
