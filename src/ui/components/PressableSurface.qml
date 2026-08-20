// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Opaque, software-rendering-safe pressable surface. Contrasting strip is the
// material's front face, not a shadow: it stays solid and uses a contextual
// theme ramp for its background. Pressing translates the face into that depth and
// collapses the exposed strip.
Item {
    id: root

    property color faceColor: Theme.surfaceCard
    property color edgeColor: Theme.controlEdge
    property int radius: Sizing.radiusSm
    property int edgeHeight: Sizing.pressEdgeHeight
    property bool focused: false
    property bool pressed: false
    property int pointerAcceptedButtons: Qt.NoButton
    property bool pointerHoverEnabled: false
    default property alias contentData: faceContent.data

    signal pointerEntered
    signal pointerClicked(int button)

    readonly property real faceOffset: face.y
    readonly property int visibleEdgeHeight: root.pressed ? 0 : Math.min(root.edgeHeight, root.height)
    // Ring inset and width reuse the same tokens Tile.qml's focus ring uses
    // (see the ring construction below), so a modal button and a grid tile
    // read as the same weight of "this is focused."
    readonly property int _ringGap: Sizing.focusBorderWidth
    readonly property int _ringWidth: Sizing.focusRingWidth

    clip: true

    Rectangle {
        id: edge

        objectName: "pressableEdge"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: root.pressed ? 0 : Math.min(root.edgeHeight + root.radius, root.height)
        color: root.edgeColor
        topLeftRadius: 0
        topRightRadius: 0
        bottomLeftRadius: root.radius
        bottomRightRadius: root.radius

        Behavior on height {
            enabled: Motion.enabled
            NumberAnimation {
                duration: Motion.dur(root.pressed ? Motion.pressMs : Motion.settleMs)
                easing.type: Easing.OutQuad
            }
        }
    }

    Rectangle {
        id: face

        objectName: "pressableFace"
        x: 0
        y: root.pressed ? Math.min(root.edgeHeight, root.height) : 0
        width: root.width
        height: Math.max(0, root.height - Math.min(root.edgeHeight, root.height))
        color: root.faceColor
        radius: root.radius
        border.color: Theme.borderMid
        border.width: Sizing.cardBorderWidth

        Behavior on y {
            enabled: Motion.enabled
            NumberAnimation {
                duration: Motion.dur(root.pressed ? Motion.pressMs : Motion.settleMs)
                easing.type: Easing.OutQuad
            }
        }

        // Focus ring — two stacked *filled* rounded rectangles rather than a
        // thicker border: thin rounded borders are tessellated without
        // subpixel AA coverage under Qt's software adaptation and step
        // visibly at the corners (QTBUG-123210), while filled rounded rects
        // honor the AA path. See Tile.qml's identical construction. Inset
        // within `face` (not the root Item, which clips) so the ring stays
        // inside the button's own bounds through the press translation, and
        // drawn before `faceContent` so caller content paints on top of it.
        Rectangle {
            id: focusRingOuter
            objectName: "pressableFocusRingOuter"

            anchors.fill: parent
            anchors.margins: root._ringGap
            color: Theme.accent
            radius: Math.max(0, face.radius - root._ringGap)
            antialiasing: true
            visible: root.focused
        }

        Rectangle {
            objectName: "pressableFocusRingInner"
            anchors.fill: focusRingOuter
            anchors.margins: root._ringWidth
            color: root.faceColor
            radius: Math.max(0, focusRingOuter.radius - root._ringWidth)
            antialiasing: true
            visible: root.focused
        }

        Item {
            id: faceContent
            anchors.fill: parent
        }
    }

    // Pointer handling stays on the fixed outer bounds rather than moving with
    // faceContent. Every visible part of the physical control remains a hit
    // target, including the exposed front edge and pressed-state top gap.
    MouseArea {
        anchors.fill: parent
        z: 1
        enabled: root.pointerAcceptedButtons !== Qt.NoButton
        acceptedButtons: root.pointerAcceptedButtons
        hoverEnabled: root.pointerHoverEnabled
        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onEntered: root.pointerEntered()
        onClicked: mouse => root.pointerClicked(mouse.button)
    }
}
