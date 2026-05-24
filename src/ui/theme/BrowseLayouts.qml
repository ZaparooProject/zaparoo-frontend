// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Shared browse-screen layout profiles. These only describe geometry and
// slot placement; screen behavior stays in the screens/components that
// consume them.
QtObject {
    readonly property QtObject defaultTile: QtObject {
        readonly property bool showTopStrip: true
        readonly property bool showHeaderTitleInHeader: false
        readonly property bool showBottomStatusRow: false
        readonly property bool headerHudBottomAligned: false
        readonly property bool headerStatusPillPinnedTop: false
        readonly property int gridLeftInset: Sizing.pctW(5)
        readonly property int gridRightInset: Sizing.pctW(5)
        readonly property int gridGutterWidth: Sizing.pctW(3)
        readonly property int gridGutterGap: Sizing.pctW(1.5)
        readonly property int gridColumnGap: Sizing.pctW(3)
        readonly property int gridTopInset: Sizing.pctH(2)
        readonly property int gridBottomInset: Sizing.pctH(2)
        readonly property int gridRowGap: Sizing.pctH(4)
        readonly property int scrollThumbWidth: Sizing.pctW(1.2)
        readonly property int scrollThumbRightInset: 0
        readonly property int scrollArrowSize: Math.min(gridGutterWidth, Sizing.pctH(4))
        readonly property bool packHorizontalRemainderAfterGutter: false
        readonly property int activeLabelHeight: Sizing.pctH(7)
        readonly property int activeLabelBottomMargin: Sizing.pctH(8)
        readonly property int bottomStatusLeftMargin: Sizing.pctW(5)
        readonly property int bottomStatusRightMargin: Sizing.pctW(5)
        readonly property int bottomUnsafeHeight: Sizing.pctH(6) + Sizing.pctH(2)
        readonly property int tileCornerRadius: Sizing.cornerRadius
    }

    readonly property QtObject crtTile: QtObject {
        readonly property bool showTopStrip: false
        readonly property bool showHeaderTitleInHeader: true
        readonly property bool showBottomStatusRow: true
        readonly property bool headerHudBottomAligned: true
        readonly property bool headerStatusPillPinnedTop: true
        readonly property int gridLeftInset: 4
        readonly property int gridRightInset: 0
        readonly property int gridGutterWidth: 8
        readonly property int gridGutterGap: 4
        readonly property int gridColumnGap: 4
        readonly property int gridTopInset: 2
        readonly property int gridBottomInset: 4
        readonly property int gridRowGap: 4
        readonly property int scrollThumbWidth: 4
        readonly property int scrollThumbRightInset: 2
        readonly property int scrollArrowSize: 8
        readonly property bool packHorizontalRemainderAfterGutter: true
        readonly property int activeLabelHeight: 8
        readonly property int activeLabelBottomMargin: Sizing.pctH(6) + 4
        readonly property int bottomStatusLeftMargin: 4
        readonly property int bottomStatusRightMargin: Sizing.pctW(5)
        readonly property int bottomUnsafeHeight: 16
        readonly property int tileCornerRadius: 4
    }
}
