// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Browse as Browse

// qmllint disable compiler

// CRT screen-position calibration. A 240p-test-suite style border
// pattern: a 1-px frame at the extreme framebuffer edge, outline
// rectangles at the 90% (action-safe) and 80% (title-safe) areas, and
// a crosshatch grid. Arrow presses nudge the centering trims through
// Browse.CrtVideo, which pokes the DDR control word live so the user
// sees the picture move in real time; accept/cancel persist the values
// and close.
//
// Geometry deliberately bypasses the Sizing pct helpers: this pattern
// is mounted OUTSIDE the safe-area inset (sibling of `scene` in
// MainLayout) and must address true framebuffer pixels - its whole job
// is to show where those edges land on the tube. Integer rounding via
// Math.round/Sizing keeps every edge on a whole pixel.
//
// The opaque full-bleed Rectangle is a justified exemption from the
// "no full-screen background" rule: a test pattern needs a clean
// field, and an opaque surface lets the software renderer subtract
// everything underneath instead of repainting it.
Item {
    id: calibration

    property bool open: false
    visible: calibration.open

    signal closeRequested

    // Pure dispatcher per the routing contract: Main.qml forwards
    // actions while this modal is on top of the stack.
    function handleAction(action: string): void {
        const h = Browse.CrtVideo.h_offset;
        const v = Browse.CrtVideo.v_offset;
        if (action === "left")
            Browse.CrtVideo.set_offsets(h - 1, v);
        else if (action === "right")
            Browse.CrtVideo.set_offsets(h + 1, v);
        else if (action === "up")
            Browse.CrtVideo.set_offsets(h, v - 1);
        else if (action === "down")
            Browse.CrtVideo.set_offsets(h, v + 1);
        else if (action === "accept" || action === "cancel") {
            Browse.CrtVideo.commit_offsets();
            calibration.closeRequested();
        }
    }

    readonly property int _stroke: Sizing.stroke(1)
    // Safe-area insets per side: 5% = action safe (90% area), 10% =
    // title safe (80% area). SMPTE SD practice.
    readonly property int _actionInsetX: Math.round(calibration.width * 0.05)
    readonly property int _actionInsetY: Math.round(calibration.height * 0.05)
    readonly property int _titleInsetX: Math.round(calibration.width * 0.10)
    readonly property int _titleInsetY: Math.round(calibration.height * 0.10)
    readonly property int _gridColumns: 16
    readonly property int _gridRows: 12

    Rectangle {
        anchors.fill: parent
        color: "#000000"
    }

    // Crosshatch grid. Two Repeaters of 1-px lines on a fixed integer
    // grid; endpoints land on the extreme-edge frame so the hatch
    // meets the border cleanly.
    Repeater {
        model: calibration._gridColumns + 1

        Rectangle {
            id: gridColumn
            required property int index
            x: Math.min(calibration.width - calibration._stroke, Math.round(gridColumn.index * (calibration.width - calibration._stroke) / calibration._gridColumns))
            y: 0
            width: calibration._stroke
            height: calibration.height
            color: Theme.textLabel
        }
    }

    Repeater {
        model: calibration._gridRows + 1

        Rectangle {
            id: gridRow
            required property int index
            x: 0
            y: Math.min(calibration.height - calibration._stroke, Math.round(gridRow.index * (calibration.height - calibration._stroke) / calibration._gridRows))
            width: calibration.width
            height: calibration._stroke
            color: Theme.textLabel
        }
    }

    // Extreme-edge frame: the outermost row/column of framebuffer
    // pixels. On a correctly trimmed set this sits right at (or just
    // past) the visible edge of the tube.
    Rectangle {
        anchors.fill: parent
        color: "transparent"
        border.width: calibration._stroke
        border.color: Theme.textPrimary
    }

    // Action-safe (90%) outline: all interactive/meaningful content
    // stays inside this.
    Rectangle {
        x: calibration._actionInsetX
        y: calibration._actionInsetY
        width: calibration.width - 2 * calibration._actionInsetX
        height: calibration.height - 2 * calibration._actionInsetY
        color: "transparent"
        border.width: calibration._stroke
        border.color: Theme.accent
    }

    // Title-safe (80%) outline: text that must be readable stays
    // inside this.
    Rectangle {
        x: calibration._titleInsetX
        y: calibration._titleInsetY
        width: calibration.width - 2 * calibration._titleInsetX
        height: calibration.height - 2 * calibration._titleInsetY
        color: "transparent"
        border.width: calibration._stroke
        border.color: Theme.textPrimary
    }

    // Readout + help, on a small opaque plate so the hatch doesn't
    // run through the glyphs. Centered per the integer-pixel rules:
    // item centered with Sizing.center, glyphs left-aligned.
    Rectangle {
        id: readoutPlate

        x: Sizing.center(calibration.width, width)
        y: Sizing.center(calibration.height, height)
        width: Math.round(readoutColumn.width + 2 * Sizing.pctW(2))
        height: Math.round(readoutColumn.height + 2 * Sizing.pctH(2))
        color: "#000000"
        border.width: calibration._stroke
        border.color: Theme.textPrimary

        Column {
            id: readoutColumn
            x: Sizing.center(readoutPlate.width, width)
            y: Sizing.center(readoutPlate.height, height)
            spacing: Sizing.pctH(1)

            Text {
                x: Sizing.center(readoutColumn.width, width)
                text: qsTr("H %1   V %2").arg(Browse.CrtVideo.h_offset).arg(Browse.CrtVideo.v_offset)
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(3.2)
                renderType: Text.NativeRendering
            }

            Text {
                x: Sizing.center(readoutColumn.width, width)
                text: qsTr("Arrows adjust position")
                color: Theme.textLabel
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(2.6)
                renderType: Text.NativeRendering
            }

            Text {
                x: Sizing.center(readoutColumn.width, width)
                text: qsTr("Press any button to save")
                color: Theme.textLabel
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(2.6)
                renderType: Text.NativeRendering
            }
        }
    }
}
