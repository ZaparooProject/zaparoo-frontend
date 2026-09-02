// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Inverse-video row highlight, shared by BrowseList and SettingsField so the
// two lists cannot drift apart. Paints nothing at rest -- the containing list
// or form supplies the card behind it. When `active`, fills solid with
// `Theme.accent` and exposes `contentColor` (`Theme.onAccent`, the semantic
// tier role guaranteed >=4.5:1 against accent -- see ColorSchemes.qml) for
// the host to bind its own text/icon colors to, so a selected row reads as a
// line of text that flips foreground and background rather than an object
// that lifts off the page. This is the typographic half of the two-register
// design language -- see docs/style.md -> "Two registers"; the physical half
// is PressableSurface, used by tiles, buttons, and menu/picker rows.
//
// `activatePulse` fires a one-shot inverse blink: the bar and its content
// swap colors for `Motion.pressMs`, then swap back. Nothing moves -- no rail,
// no inset, no push-in. That is deliberate: a text row is not a button.
Item {
    id: root

    property bool active: false
    // Pulse counters the host forwards from its own screen-level counters.
    // Only the active instance reacts; see the change handlers below.
    property int activatePulse: 0
    property int releasePulse: 0
    // Cuts a held blink short without animating, e.g. while the host screen
    // is off-screen and about to be reused for different content.
    property bool screenSettling: false
    property int radius: Sizing.radiusSm

    readonly property bool flashing: root._flashing
    readonly property color barColor: root._flashing ? Theme.onAccent : Theme.selectionFill
    readonly property color contentColor: root._flashing ? Theme.selectionFill : Theme.onAccent
    // Selected-row text weight. Dark-on-light (inverted) text suffers
    // irradiation — it reads as thinner than light-on-dark text at the
    // identical weight — so a selected row steps up one notch to read at
    // parity with a resting row, not to read as emphasised. Medium (500)
    // is a real cut on the variable NotoSans.ttf ships (has an `fvar`
    // table), not a synthesised weight. No-op under `Theme.bitmapType`:
    // the CRT/240p bitmap face has a single strike, and unantialiased
    // 1-bit text has no irradiation to correct. See docs/style.md's
    // "Inverse-video rows".
    readonly property int contentWeight: root.active ? Font.Medium : Font.Normal
    property bool _flashing: false

    onActiveChanged: {
        if (!root.active) {
            flashAnim.stop();
            root._flashing = false;
        }
    }

    onActivatePulseChanged: {
        if (root.active)
            flashAnim.restart();
    }

    onReleasePulseChanged: {
        if (root.active) {
            flashAnim.stop();
            root._flashing = false;
        }
    }

    onScreenSettlingChanged: {
        if (root.screenSettling) {
            flashAnim.stop();
            root._flashing = false;
        }
    }

    SequentialAnimation {
        id: flashAnim
        PropertyAction {
            target: root
            property: "_flashing"
            value: true
        }
        PauseAnimation {
            duration: Motion.dur(Motion.pressMs)
        }
        PropertyAction {
            target: root
            property: "_flashing"
            value: false
        }
    }

    Rectangle {
        anchors.fill: parent
        visible: root.active
        color: root.barColor
        radius: root.radius
        antialiasing: Sizing.cornerAntialiasing
    }
}
