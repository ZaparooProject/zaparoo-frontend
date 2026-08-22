// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Segmented block progress track for the header status line. Replaces
// CoreStatusPill's 4-dot spinner: that spinner ran a hardcoded 140ms
// timer with no way to show real progress, painted twice per tick
// (normal + inverted foreground copies), and kept spinning under Reduce
// Motion. This is one visual vocabulary for both cases instead:
//
// - Determinate (`totalKnown`, e.g. indexing/scraping systems): cells up
//   to the fill fraction paint `Theme.accent`, the rest `Theme.borderSubtle`.
// - Indeterminate (optimize/vacuum, or a total Core hasn't reported yet):
//   one lit cell marches along an otherwise dark track.
//
// Either way, exactly one cell -- the fill's leading edge in determinate
// mode, the marching cell in indeterminate mode -- blinks between accent
// and its dim color every Motion.pulseMs. This is a hard cut each tick,
// not a crossfade: no ColorAnimation, no Behavior on color, just a Timer
// flipping a bool that `color` reads directly. A blink is the actual
// spec here -- on, then off, then on -- not a breathing/throbbing fade
// between the two; see docs/style.md -> "Header status line" for why
// that distinction matters (it's also cheaper: a solid-color repaint per
// tick instead of N interpolated frames per cycle). This is the one
// exception to the no-persistent-motion rule: it's header chrome, never
// painted over content; its dirty rect is a single small cell; it only
// runs while `active` and `!paused`; and it stops outright under Reduce
// Motion rather than collapsing to a 0ms loop. CLAUDE.md's own animation
// rules already sanction "page-dot pulse, focus-ring blink" as small
// enough to be cheap on the software renderer -- this is that same
// exemption, just continuous because a background task has no natural
// per-frame "done" edge the way a one-shot press/release cue does. See
// Motion.qml's `pulseMs` doc comment.
//
// Cell width/gap are chosen first as integers so the track's total width
// is always a whole-pixel sum -- a continuous fill bar's right edge would
// land on a fractional pixel and soften at 240p, exactly the class of bug
// docs/qml-gotchas.md's "Integer-pixel rules" exists to prevent.
Item {
    id: root

    // Whether there is a task to show at all. Cells fall back to a flat
    // `borderSubtle` row when false rather than resetting a fill to zero
    // -- callers hide this component entirely when idle (see
    // StatusLine.qml); this flag exists so the component itself is inert
    // and testable standalone.
    property bool active: false
    // Freezes the pulse (cells still show their static fill) without
    // losing progress -- Core pauses indexing around a running game, and
    // "the bar stops breathing" is a subtler, cheaper cue than adding a
    // second color state for paused cells.
    property bool paused: false
    // False when Core hasn't reported a total yet (or never will, e.g.
    // optimize/vacuum) -- see PageIndicator.qml's identical split for the
    // footer's page cue. `currentStep`/`totalSteps` are ignored in this
    // mode; one cell marches instead of a fraction filling.
    property bool totalKnown: true
    property int currentStep: 0
    property int totalSteps: 0
    property int cellCount: Theme.crtNativePath ? 8 : 12
    property int cellHeight: Math.max(Sizing.pctH(1.6), Sizing.stroke(6))
    property int cellGap: Sizing.pctW(0.3)

    readonly property int cellWidth: Math.max(Sizing.stroke(2), Sizing.px(root.cellHeight * 1.6))
    readonly property int _filledCells: {
        if (!root.active || !root.totalKnown || root.totalSteps <= 0)
            return 0;
        const fraction = Math.max(0, Math.min(1, root.currentStep / root.totalSteps));
        return Math.min(root.cellCount, Math.round(fraction * root.cellCount));
    }
    // The one cell that flashes: the fill's leading edge when determinate,
    // the marching cell when not. `Math.max(0, ...)` covers the
    // just-started 0-filled frame -- cell 0 blinks before anything is
    // technically "filled" yet, which reads as "starting up."
    readonly property int _pulseCellIndex: root.totalKnown ? Math.max(0, root._filledCells - 1) : root._marchIndex
    readonly property bool _pulsing: root.active && !root.paused && Motion.enabled
    property int _marchIndex: 0
    // The blink's own on/off state -- flipped by the Timer below, read
    // directly by each cell's `color` binding. No animation object
    // touches this; the value itself is the whole state machine.
    property bool _blinkOn: true

    width: root.cellCount * root.cellWidth + Math.max(0, root.cellCount - 1) * root.cellGap
    height: root.cellHeight

    // Marches the indeterminate cell along the track. Gated on
    // Motion.enabled like the pulse itself -- under Reduce Motion the
    // track simply rests on cell 0 lit and static rather than sweeping.
    Timer {
        interval: Motion.dur(Motion.pulseMs * 2)
        running: root.active && !root.totalKnown && !root.paused && Motion.enabled
        repeat: true
        onTriggered: root._marchIndex = (root._marchIndex + 1) % root.cellCount
    }

    // Drives the blink: a hard on/off toggle, not a fade. `color` reads
    // `_blinkOn` straight through with no Behavior in between, so every
    // tick is an instant one-frame cut.
    Timer {
        interval: Motion.dur(Motion.pulseMs)
        running: root._pulsing
        repeat: true
        onTriggered: root._blinkOn = !root._blinkOn
    }

    Row {
        anchors.fill: parent
        spacing: root.cellGap

        Repeater {
            model: root.cellCount

            delegate: Rectangle {
                id: cell

                required property int index

                readonly property bool _isFilled: root.active && (index < root._filledCells || (!root.totalKnown && index === root._marchIndex))
                readonly property bool _isPulseCell: root._pulsing && index === root._pulseCellIndex

                width: root.cellWidth
                height: root.cellHeight
                radius: Sizing.radiusSm
                // No Behavior on color anywhere in this delegate -- every
                // state change here, blink included, is an instant cut.
                color: cell._isPulseCell ? (root._blinkOn ? Theme.accent : Theme.borderSubtle) : (cell._isFilled ? Theme.accent : Theme.borderSubtle)
            }
        }
    }
}
