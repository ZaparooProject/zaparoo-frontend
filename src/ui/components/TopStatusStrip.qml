// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Three-slot top strip shared by the Systems and Games screens. Owns
// layout only — callers compute and pass `title`, `currentPage`,
// `totalPages`, and `totalText` from their own model. Each slot is
// capped at one third of the parent width with `elide: ElideRight` so
// long strings (3-digit page counts, 5-digit file totals, multi-word
// titles) can't collide on a 240p MiSTer screen.
// Slots:
//   left   — total-count badge (visible when `totalText !== ""`)
//   center — screen title (category / system name)
//   right  — "Page N / M" for bounded results, or "Page N" when final
//            page count is unknown

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item + Text, no transforms, no shaders.
Item {
    id: status

    property string title: ""
    property int currentPage: 0 // 0-indexed; displayed as N+1
    property int totalPages: 1
    // False for cursor chains whose final page is unknown until exhaustion.
    // Such screens show only "Page N" rather than a denominator that grows as
    // more rows arrive.
    property bool pageTotalKnown: true
    property string totalText: "" // formatted; empty hides the slot
    property string rightTextOverride: "" // formatted; non-empty replaces page text
    // List layouts show paging as plain "Page N / M" text here (unaffected
    // by any of this -- they have their own separate chevron-band scroll
    // chrome elsewhere). Grid layout's cue lives in the footer on CRT
    // (`footer.pageCueInFooter`, BrowseLayouts.qml) but up here, alongside
    // the count badge, on every other theme -- see `pageIndicatorMode`.
    // Default true keeps every existing (list-layout) caller byte-identical.
    property bool showPageCounter: true
    // True mounts a `PageIndicator` (chevrons + "N / M") in the right slot
    // instead of the plain page-count Text -- the grid-layout page cue,
    // relocated here from the footer. False (default, every list-layout
    // caller) keeps this slot exactly as it always was. See
    // SystemsScreen.qml / MediaListScreen.qml for the callers.
    property bool pageIndicatorMode: false
    property bool hasPagesAbove: false
    property bool hasPagesBelow: false
    property int pageIndicatorChevronSize: Sizing.pctH(4)
    signal pageRequested(int delta)
    readonly property string pageText: status.rightTextOverride !== "" ? status.rightTextOverride : (status.pageTotalKnown ? qsTr("Page %1 / %2").arg(status.currentPage + 1).arg(status.totalPages) : qsTr("Page %1").arg(status.currentPage + 1))
    property int slotMargin: Sizing.pctW(3)
    readonly property int _slotWidth: Sizing.px(status.width / 3)
    readonly property int _textMeasureSlack: Theme.crtNativePath ? 0 : 2
    readonly property int _titleMeasuredWidth: Math.ceil(Math.max(titleMetrics.advanceWidth, titleMetrics.boundingRect.width) + status._textMeasureSlack)
    readonly property int _titleTextWidth: Math.min(status._slotWidth, status._titleMeasuredWidth)

    // Page counter and total badge sit on the title's own text baseline —
    // not just its box's bottom edge, which the title's `height:
    // Sizing.fontHero` pins independently of the side texts' own implicit
    // height, so a bottom-edge anchor left the visible glyph baselines
    // offset by the descent difference between the two font sizes (round
    // 6, item 9). `anchors.baseline` is what "matching the bottom of the
    // screen title" actually means. Counter/total drop one step in font
    // size so the title stays the visual anchor.
    Text {
        id: totalBadge

        visible: status.totalText !== ""
        anchors.left: parent.left
        anchors.leftMargin: status.slotMargin
        anchors.baseline: titleText.baseline
        width: status._slotWidth - status.slotMargin
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        text: status.totalText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    TextMetrics {
        id: titleMetrics

        text: status.title
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontHero
        font.weight: Font.Medium
    }

    Text {
        id: titleText

        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        width: status._titleTextWidth
        height: Sizing.fontHero
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        text: status.title
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontHero
        font.weight: Font.Medium
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    Text {
        id: pageCounter

        visible: !status.pageIndicatorMode && status.showPageCounter && (status.rightTextOverride !== "" || !status.pageTotalKnown || status.totalPages > 1)
        anchors.right: parent.right
        anchors.rightMargin: status.slotMargin
        anchors.baseline: titleText.baseline
        width: status._slotWidth - status.slotMargin
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        text: status.pageText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    // Grid layout's relocated page cue -- see `pageIndicatorMode` above.
    // Baseline-aligned the same way `pageCounter` is, but `PageIndicator`
    // is a plain Item (icons + text, not one Text), so it has no `baseline`
    // anchor line of its own to bind to directly; `y` is computed instead
    // from `titleText`'s real baseline position and this component's own
    // exposed `textBaselineOffset` (the same font as `pageCounter`, so the
    // two line up exactly).
    PageIndicator {
        id: pageIndicatorRight

        visible: status.pageIndicatorMode && status.showPageCounter
        anchors.right: parent.right
        anchors.rightMargin: status.slotMargin
        y: titleText.y + titleText.baselineOffset - pageIndicatorRight.textBaselineOffset
        chevronSize: status.pageIndicatorChevronSize
        currentPage: status.currentPage
        totalPages: status.totalPages
        pageTotalKnown: status.pageTotalKnown
        hasPagesAbove: status.hasPagesAbove
        hasPagesBelow: status.hasPagesBelow
        onPageRequested: delta => status.pageRequested(delta)
    }
}
