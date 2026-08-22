// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Compact "where am I" page cue for a paged grid: an up/down chevron pair
// (the same ScrollUp/ScrollDown glyphs PagedGrid's old right-gutter used)
// plus "N / M", meant to sit in a screen's footer row. Replaces the
// gutter entirely -- grids are paged, not scrolled, so a proportional
// scrollbar thumb was never the right metaphor here; see docs/style.md ->
// "Tile aspect and grid blocks".
//
// Reserves its full width unconditionally: each chevron anchors off a
// fixed chain (up -> down -> text) so hiding one only stops it painting,
// it never moves the text or changes this Item's own width. That is the
// whole point -- the footer slot this sits in is a fixed one-third
// column (same discipline TopStatusStrip already uses), and arming Hub
// Move (which always reserves a second page) must change only the page
// TEXT here, never shift or resize the chevrons or anything else on
// screen.
//
// Two separate spacing tokens, not one shared `itemSpacing`, for a reason
// that only shows up once you measure the glyphs: ScrollUp.svg/
// ScrollDown.svg have identical ~22% transparent horizontal side bearing
// (ink spans x=5.307..18.693 of a 24-wide viewBox on both), scaled 1:1 with
// `chevronSize` by `PreserveAspectFit`. A single shared anchor gap between
// all three boxes therefore produces a LARGER visible ink-to-ink gap
// between the two chevrons (bearing paid on both sides of the gap) than
// between the second chevron and the text (bearing paid once) -- backwards
// from Gestalt proximity, which says the chevron PAIR should read as one
// control and sit tighter together than the gap to the separate page-count
// readout. `chevronSpacing` is deliberately near-zero: the icons' own
// baked-in padding already supplies a real visual gap between the two
// glyphs with no anchor margin added on top, while `itemSpacing` (kept, its
// prior default and meaning unchanged) still separates the pair from the
// text.
//
// Up/Down (not Left/Right) glyphs deliberately mirror PagedGrid's own
// vertical page-stacking model (`hasPagesAbove`/`hasPagesBelow`,
// `pageBy`) -- Down at the bottom row already advances a page through
// ordinary d-pad navigation, so the up/down metaphor is accurate, not
// just inherited from the old gutter's icons. Reusing ScrollUp/ScrollDown
// also avoids adding a new Left/Right pair to the baked icon atlas.
Item {
    id: root

    property int currentPage: 0
    property int totalPages: 1
    // False for cursor-paginated lists whose final page is unknown until
    // exhaustion (Favorites/Recents/Games). Shows bare "N" instead of a
    // denominator that would grow as more rows arrive.
    //
    // Mirrors TopStatusStrip's own known/unknown split (see that
    // component's `pageText`) rather than sharing an implementation with
    // it: this is a compact "N / M" with no "Page" word -- a different
    // string for a different context, since the chevrons alongside it
    // already establish "this is paging". The one thing that must never
    // drift between the two is the conditional itself -- never print a
    // growing denominator -- which is the single line of logic below.
    property bool pageTotalKnown: true
    property bool hasPagesAbove: false
    property bool hasPagesBelow: false
    property int chevronSize: Sizing.pctH(4)
    // Gap between the up and down chevrons -- see the doc comment above for
    // why this is its own token instead of sharing `itemSpacing`.
    property int chevronSpacing: 0
    property int itemSpacing: Sizing.pctW(0.6)

    signal pageRequested(int delta)

    // True the instant there's anywhere else to page to, in either
    // direction. Equivalent to `totalPages > 1` when `pageTotalKnown`, but
    // also covers the unknown-total case (a cursor list with nothing above
    // and nothing more to fetch below) the same way -- both mean "there is
    // only one page," and a "1 / 1" or bare "1" readout in that state says
    // nothing a user needs, just adds a number next to chevrons that were
    // never going to do anything. Text hides; the chevrons already hide
    // themselves off the same two properties, so nothing new to gate there.
    readonly property bool _hasMultiplePages: root.hasPagesAbove || root.hasPagesBelow
    readonly property string pageText: root.pageTotalKnown ? qsTr("%1 / %2").arg(root.currentPage + 1).arg(Math.max(1, root.totalPages)) : qsTr("%1").arg(root.currentPage + 1)
    // `NativeRendering` paints wider than `TextMetrics` reports -- see
    // ActiveLabel.qml's/TopStatusStrip.qml's identical `_slack` correction.
    readonly property int _slack: Theme.crtNativePath ? 0 : Sizing.px(2)
    readonly property int _textWidth: Math.ceil(Math.max(_textMetrics.advanceWidth, _textMetrics.boundingRect.width) + root._slack)
    // Absolute y-position of the page-count text's own baseline within this
    // Item, in this Item's own coordinate space. Lets a host that wants to
    // baseline-align this component against a Text of its own (see
    // TopStatusStrip.qml's `pageIndicatorMode`) compute the right `y` for
    // the whole Item without this component needing to know anything about
    // that host's layout.
    readonly property real textBaselineOffset: pageCountText.y + pageCountText.baselineOffset

    width: upChevron.width + root.chevronSpacing + downChevron.width + root.itemSpacing + root._textWidth
    height: root.chevronSize

    TextMetrics {
        id: _textMetrics
        text: root.pageText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
    }

    Image {
        id: upChevron
        source: Resources.iconUrl("ScrollUp", Theme.textPrimary)
        width: root.chevronSize
        height: root.chevronSize
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: root.hasPagesAbove

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            cursorShape: Qt.PointingHandCursor
            enabled: upChevron.visible
            onClicked: root.pageRequested(-1)
        }
    }

    Image {
        id: downChevron
        source: Resources.iconUrl("ScrollDown", Theme.textPrimary)
        width: root.chevronSize
        height: root.chevronSize
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.left: upChevron.right
        anchors.leftMargin: root.chevronSpacing
        anchors.verticalCenter: parent.verticalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: root.hasPagesBelow

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            cursorShape: Qt.PointingHandCursor
            enabled: downChevron.visible
            onClicked: root.pageRequested(1)
        }
    }

    Text {
        id: pageCountText
        objectName: "pageIndicatorText"
        anchors.left: downChevron.right
        anchors.leftMargin: root.itemSpacing
        anchors.verticalCenter: parent.verticalCenter
        // Explicit width (== the reserved `_textWidth`, slack included) so
        // an under-measure clips instead of overhanging the footer's right
        // margin -- `_textWidth` used to feed only the parent Item's
        // reserved `width` and left this Text sized to its own raw
        // implicitWidth instead.
        width: root._textWidth
        visible: root._hasMultiplePages
        elide: Text.ElideRight
        text: root.pageText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }
}
