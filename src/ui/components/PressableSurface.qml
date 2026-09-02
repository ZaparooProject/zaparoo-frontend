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
    // Press-progress driver, animated instead of `edge.height`/`face.y`
    // directly (see the two Behaviors below, now on this property alone).
    // `root.height` is 0 at construction, before the cell's real layout
    // pass lands -- with the old per-geometry Behaviors, `edge.height`'s
    // own binding (`Math.min(root.edgeHeight + root.radius, root.height)`)
    // captured that 0 and then animated 0 -> full when the real height
    // arrived a moment later, because the Behavior can't distinguish a
    // layout change from an actual press. On a loaded MiSTer boot the
    // render loop can stall long enough to stretch that nominal
    // `Motion.settleMs` settle across roughly a second of wall clock --
    // every Hub tile's bottom edge (this surface's front face) painted
    // missing for that whole span. Animating `_pressT` (a pure 0..1
    // scalar with no geometry read inside its own binding) instead means
    // a layout pass is always instant, and only `root.pressed` flipping
    // still animates.
    property real _pressT: root.pressed ? 1 : 0
    readonly property int _edgeBand: Math.min(root.edgeHeight + root.radius, root.height)
    readonly property int _pressDepth: Math.min(root.edgeHeight, root.height)

    Behavior on _pressT {
        enabled: Motion.enabled
        NumberAnimation {
            duration: Motion.dur(root.pressed ? Motion.pressMs : Motion.settleMs)
            easing.type: Easing.OutQuad
        }
    }
    // Ring inset and width were first tried as a percentage of this
    // surface's own `height` (fixing an earlier screen-relative version
    // that ate ~38% of a short row's height -- see git history), but that
    // independent percentage floored to exactly 1px at every resolution
    // tier for a menu-row-sized surface, which is thinner than the row's
    // OWN resting `border.width` (`face`'s `Sizing.cardBorderWidth`,
    // itself 1-2px) -- a focus ring that's no heavier than the idle chrome
    // reads as barely focused at all. Deriving from `cardBorderWidth`
    // instead fixes that by construction: the ring band is always exactly
    // double the resting border's weight, and both scale together off the
    // same token, so they can't drift out of relative proportion again at
    // some resolution neither was tested at. See docs/style.md -> "Tile
    // focus ring" and PressableSurface's face `Text` color split below for
    // the other half of this fix -- brightening the focused row's own
    // content, the way Tile.qml's caption/logo already do, matters at
    // least as much as the ring itself.
    readonly property int _ringGap: Sizing.cardBorderWidth
    readonly property int _ringWidth: Sizing.cardBorderWidth * 2

    clip: true

    // A clipping wrapper, not a resized Rectangle -- see `edge` below for
    // why. This Item is the thing that actually shrinks as `_pressT`
    // increases; the Rectangle inside it never does.
    Item {
        id: edgeClip

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        // Driven off `root._pressT`, not `root.pressed` directly -- see
        // that property's doc comment. Settles to the identical values
        // this used to compute inline (0 pressed, `_edgeBand` at rest).
        height: Sizing.px((1 - root._pressT) * root._edgeBand)
        clip: true

        // Round 10: height is a CONSTANT (`_edgeBand`), never `_pressT`-
        // driven. The old version put `(1 - root._pressT) * root._edgeBand`
        // directly on this Rectangle's own `height`, which shrank the
        // radiused rect itself across the press animation -- Qt clamps
        // `bottomLeftRadius`/`bottomRightRadius` to `height / 2` once
        // `height` drops below `2 * radius`, so for most of the animation
        // this rectangle's corners were less round than `face`'s (whose
        // radius is evaluated against a height that never changes), and
        // only matched up again at the very last frame (`edgeClip.height`
        // reaching 0). Anchoring a full-height, full-radius Rectangle to
        // `edgeClip`'s bottom and clipping the shrinking container instead
        // means the corners never move, never resize, and are never
        // reclamped -- only the flat, unrounded top of the strip (already
        // hidden under `face`'s own curve at rest) gets clipped away.
        Rectangle {
            id: edge

            objectName: "pressableEdge"
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: root._edgeBand
            color: root.edgeColor
            topLeftRadius: 0
            topRightRadius: 0
            bottomLeftRadius: root.radius
            bottomRightRadius: root.radius
            antialiasing: Sizing.cornerAntialiasing
        }
    }

    Rectangle {
        id: face

        objectName: "pressableFace"
        x: 0
        // Driven off `root._pressT` -- see that property's doc comment.
        // Settles to the identical values this used to compute inline
        // (`_pressDepth` pressed, 0 at rest).
        y: Sizing.px(root._pressT * root._pressDepth)
        width: root.width
        height: Math.max(0, root.height - root._pressDepth)
        color: root.faceColor
        radius: root.radius
        antialiasing: Sizing.cornerAntialiasing
        border.color: Theme.borderMid
        border.width: Sizing.cardBorderWidth

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
            antialiasing: Sizing.cornerAntialiasing
            visible: root.focused
        }

        Rectangle {
            objectName: "pressableFocusRingInner"
            anchors.fill: focusRingOuter
            anchors.margins: root._ringWidth
            color: root.faceColor
            radius: Math.max(0, focusRingOuter.radius - root._ringWidth)
            antialiasing: Sizing.cornerAntialiasing
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
