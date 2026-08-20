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
    readonly property int faceBorderWidth: focused ? Sizing.focusBorderWidth : Sizing.cardBorderWidth

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
        border.color: root.focused ? Theme.accent : Theme.borderMid
        border.width: root.faceBorderWidth

        Behavior on y {
            enabled: Motion.enabled
            NumberAnimation {
                duration: Motion.dur(root.pressed ? Motion.pressMs : Motion.settleMs)
                easing.type: Easing.OutQuad
            }
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
