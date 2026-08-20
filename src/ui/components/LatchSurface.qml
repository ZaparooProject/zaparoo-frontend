// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Recessed-slot row highlight, shared by BrowseList and SettingsField so the
// two lists cannot drift apart. Paints nothing at rest — the containing list
// or form supplies the card behind it. When `active`, fills with the
// selection surface and draws one shaded keyline along the bottom (the
// recess's near wall falls into shade because the whole UI is lit low and
// from the front — see PressableSurface's front-edge comment) plus an accent
// rail on the leading edge.
//
// `activatePulse`/`releasePulse` drive a one-shot inward latch: the rail
// slides in by `latchOffset` and back out, like a cursor engaging a slot.
// The row's own surface never moves — only the rail and whatever the host
// binds to `latchOffset` (typically its label's leading inset) do.
Item {
    id: root

    property bool active: false
    // Pulse counters the host forwards from its own screen-level counters.
    // Only the active instance reacts; see the change handlers below.
    property int activatePulse: 0
    property int releasePulse: 0
    // Releases a held latch without animating, e.g. while the host screen is
    // off-screen and about to be reused for different content.
    property bool screenSettling: false
    property int radius: Sizing.radiusSm
    property int railWidth: Sizing.pctW(0.45)

    readonly property int _latchDistance: Sizing.focusRingWidth
    readonly property int latchOffset: Sizing.px(root._latchProgress * root._latchDistance)
    property real _latchProgress: 0

    onActiveChanged: {
        if (!root.active) {
            activateAnim.stop();
            releaseAnim.stop();
            root._latchProgress = 0.0;
        }
    }

    onActivatePulseChanged: {
        if (root.active)
            activateAnim.restart();
    }

    onReleasePulseChanged: {
        if (root.active) {
            activateAnim.stop();
            releaseAnim.restart();
        }
    }

    onScreenSettlingChanged: {
        if (root.screenSettling) {
            activateAnim.stop();
            releaseAnim.stop();
            root._latchProgress = 0.0;
        }
    }

    NumberAnimation {
        id: activateAnim
        target: root
        property: "_latchProgress"
        to: 1.0
        duration: Motion.dur(Motion.pressMs)
        easing.type: Easing.OutQuad
    }

    NumberAnimation {
        id: releaseAnim
        target: root
        property: "_latchProgress"
        to: 0.0
        duration: Motion.dur(Motion.settleMs)
        easing.type: Easing.OutQuad
    }

    Item {
        anchors.fill: parent
        visible: root.active

        Rectangle {
            anchors.fill: parent
            color: Theme.selectionSurface
            radius: root.radius
        }

        // Squares off the fill's left rounded corners against the
        // containing card's own edge so two adjacent selected/deselected
        // rows never show a rounded notch mid-list.
        Rectangle {
            anchors.left: parent.left
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: root.radius
            color: Theme.selectionSurface
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Sizing.cardBorderWidth
        color: Theme.selectionShade
        visible: root.active
    }

    Rectangle {
        x: root.latchOffset
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: root.railWidth
        color: Theme.accent
        visible: root.active
        radius: Math.max(0, Sizing.px(width / 3))
    }
}
