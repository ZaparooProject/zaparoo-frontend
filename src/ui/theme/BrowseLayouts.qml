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
        readonly property bool scrollThumbRightAligned: false
        readonly property int scrollArrowSize: Math.min(gridGutterWidth, Sizing.pctH(4))
        readonly property bool packHorizontalRemainderAfterGutter: false
        readonly property int activeLabelHeight: Sizing.pctH(7)
        readonly property int activeLabelBottomMargin: Sizing.pctH(8)
        readonly property int bottomStatusLeftMargin: Sizing.pctW(5)
        readonly property int bottomStatusRightMargin: Sizing.pctW(5)
        readonly property int bottomUnsafeHeight: Sizing.pctH(6) + Sizing.pctH(2)
        readonly property int tileCornerRadius: Sizing.cornerRadius
        readonly property int listCardSideMargin: Sizing.pctW(5)
        readonly property int listDividerOffsetX: 0
        readonly property int listStripHeight: Sizing.pctH(7)
        readonly property int listStripSlotMargin: Sizing.pctW(5)
        readonly property int listCardPaddingLeft: Sizing.pctW(2)
        readonly property int listCardPaddingRight: Sizing.pctW(2)
        readonly property int listCardPaddingTop: Sizing.pctH(2)
        readonly property int listCardPaddingBottom: Sizing.pctH(2)
        readonly property int listRowHeight: 0
        readonly property int listRowSpacing: Sizing.pctH(0.7)
        readonly property int listCenterSlot: -1
        readonly property int listScrollbarGap: Sizing.pctW(1.5)
        readonly property int listSelectionAccentWidth: Sizing.pctW(0.45)
        readonly property int detailMetadataYOffset: 0
        readonly property int detailMetadataExtraHeight: 0
        readonly property int detailMetadataLeftInset: 0
        readonly property int detailMetadataRightInset: 0
        readonly property int detailPanePaddingLeft: Sizing.pctW(2)
        readonly property int detailPanePaddingRight: Sizing.pctW(2)
        readonly property int detailPanePaddingTop: Sizing.pctH(2)
        readonly property int detailPanePaddingBottom: Sizing.pctH(2)
        readonly property int detailImageXOffset: 0
        readonly property int detailImageLeftInset: 0
        readonly property int detailImageRightInset: 0
        readonly property int detailImageExtraWidth: 0
        readonly property int detailImageExtraHeight: 0
        readonly property int detailImageBottomGap: 0
        readonly property int detailTagRowHeight: Sizing.pctH(3)
        readonly property int detailTagRowSpacing: Sizing.pctH(0.55)
        readonly property int listRowTextLeftPadding: Sizing.pctW(1.6)
        readonly property int listRowTextRightPadding: Sizing.pctW(1.6)
        readonly property int listFavoriteRightPadding: Sizing.pctW(1.6)
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
        readonly property bool scrollThumbRightAligned: false
        readonly property int scrollArrowSize: 8
        readonly property bool packHorizontalRemainderAfterGutter: true
        readonly property int activeLabelHeight: 8
        readonly property int activeLabelBottomMargin: Sizing.pctH(6)
        readonly property int bottomStatusLeftMargin: 4
        readonly property int bottomStatusRightMargin: Sizing.pctW(5)
        readonly property int bottomUnsafeHeight: 16
        readonly property int tileCornerRadius: 4
        readonly property int listCardSideMargin: 4
        readonly property int listDividerOffsetX: -16
        readonly property int listStripHeight: 8
        readonly property int listStripSlotMargin: Sizing.headerSideMargin
        readonly property int listCardPaddingLeft: 3
        readonly property int listCardPaddingRight: 2
        readonly property int listCardPaddingTop: 3
        readonly property int listCardPaddingBottom: 2
        readonly property int listRowHeight: 12
        readonly property int listRowSpacing: 0
        readonly property int listCenterSlot: 7
        readonly property int listScrollbarGap: 2
        readonly property int listSelectionAccentWidth: 2
        readonly property int detailMetadataYOffset: -14
        readonly property int detailMetadataExtraHeight: 2
        readonly property int detailMetadataLeftInset: 2
        readonly property int detailMetadataRightInset: 1
        readonly property int detailPanePaddingLeft: 1
        readonly property int detailPanePaddingRight: 1
        readonly property int detailPanePaddingTop: 3
        readonly property int detailPanePaddingBottom: 2
        readonly property int detailImageXOffset: 0
        readonly property int detailImageLeftInset: 2
        readonly property int detailImageRightInset: 2
        readonly property int detailImageExtraWidth: 16
        readonly property int detailImageExtraHeight: 0
        readonly property int detailImageBottomGap: 2
        readonly property int detailTagRowHeight: 9
        readonly property int detailTagRowSpacing: 0
        readonly property int listRowTextLeftPadding: 4
        readonly property int listRowTextRightPadding: 2
        readonly property int listFavoriteRightPadding: 2
    }
}
