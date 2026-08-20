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
                }
                ListElement {
                    name: "Beta"
                    fileStem: "beta"
                    coverKey: ""
                    favorite: 1
                    disambiguatingTags: ""
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

    function test_toggle_knob_is_larger_centered_and_contrasts_when_checked(): void {
        const field = createTemporaryObject(toggleHost, testCase);
        verify(field !== null);

        const track = findChild(field, "settingsToggleTrack");
        const knob = findChild(field, "settingsToggleKnob");
        verify(track !== null);
        verify(knob !== null);
        compare(knob.width, track.height - Sizing.pctH(0.9) + 1);
        compare(knob.height, knob.width);
        compare(knob.y, Sizing.center(track.height, knob.height));
        compare(track.width - knob.x - knob.width, knob.y);
        compare(knob.color, Theme.bgDeep);
        verify(_contrastRatio(knob.color, track.color) >= 3.0);
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

    function test_list_activation_latches_cursor_inward(): void {
        const host = createTemporaryObject(listHost, testCase);
        verify(host !== null);
        wait(1);

        const list = findChild(host, "browseList");
        const firstRow = findChild(host, "browseListRow-0");
        verify(list !== null);
        verify(firstRow !== null);
        compare(firstRow._latchOffset, 0);
        compare(firstRow.scale, 1.0);

        list.activatePulse++;
        tryCompare(firstRow, "_latchOffset", Sizing.focusRingWidth, Motion.pressMs + 100);
        compare(firstRow.scale, 1.0);

        list.releasePulse++;
        tryCompare(firstRow, "_latchOffset", 0, Motion.settleMs + 100);
    }

    function test_list_selection_change_releases_old_latch(): void {
        const host = createTemporaryObject(listHost, testCase);
        verify(host !== null);
        wait(1);

        const list = findChild(host, "browseList");
        const firstRow = findChild(host, "browseListRow-0");
        verify(list !== null);
        verify(firstRow !== null);

        list.activatePulse++;
        tryCompare(firstRow, "_latchOffset", Sizing.focusRingWidth, Motion.pressMs + 100);
        list.currentIndex = 1;
        compare(firstRow._latchOffset, 0);
    }
}
