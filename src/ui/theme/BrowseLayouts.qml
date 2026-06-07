// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick
import Zaparoo.Theme

// Built-in browse layout profiles. Data stays in one place, but does not rely
// on runtime file loading. Values are semantic view tokens, resolved into
// integer geometry through Sizing.
QtObject {
    readonly property string currentThemeId: Theme.crtNativePath ? "crt" : "default"
    readonly property var _themes: _builtInThemes()

    function _builtInThemes(): var {
        return {
            "default": {
                "systemsGrid": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "grid": {
                        "leftInset": "pctW:5",
                        "rightInset": "pctW:5",
                        "gutterWidth": "pctW:3",
                        "gutterGap": "pctW:1.5",
                        "columnGap": "pctW:3",
                        "topInset": "pctH:2",
                        "bottomInset": "pctH:2",
                        "rowGap": "pctH:4",
                        "scrollThumbWidth": "pctW:1.2",
                        "scrollThumbRightInset": 0,
                        "scrollThumbRightAligned": false,
                        "scrollArrowSize": "min(pctW:3,pctH:4)",
                        "gutterFollowsContentWidth": false
                    },
                    "footer": {
                        "activeLabelHeight": "pctH:7",
                        "activeLabelBottomMargin": "pctH:8",
                        "bottomStatusVisible": false,
                        "bottomStatusLeftMargin": "pctW:5",
                        "bottomStatusRightMargin": "pctW:5",
                        "gridBottomMargin": "sum(pctH:8,pctH:7)",
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                },
                "systemsList": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "horizontal",
                        "listShare": 2,
                        "detailShare": 1,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": "pctW:5",
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": "pctW:2",
                        "cardPaddingRight": "pctW:2",
                        "cardPaddingTop": "pctH:2",
                        "cardPaddingBottom": "pctH:2",
                        "rowHeight": 0,
                        "rowSpacing": "pctH:0.7",
                        "centerSlot": -1,
                        "scrollbarGap": "pctW:1.5",
                        "selectionAccentWidth": "pctW:0.45",
                        "rowTextLeftPadding": "pctW:1.6",
                        "rowTextRightPadding": "pctW:1.6",
                        "favoriteRightPadding": "pctW:1.6",
                        "overlayBottomMargin": "pctH:15"
                    },
                    "detail": {
                        "contentAxis": "vertical",
                        "sectionGap": "pctH:2",
                        "imageShare": 2,
                        "metadataShare": 1,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 0,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 0,
                        "panePaddingLeft": "pctW:2",
                        "panePaddingRight": "pctW:2",
                        "panePaddingTop": "pctH:2",
                        "panePaddingBottom": "pctH:2",
                        "imagePaddingLeft": 0,
                        "imagePaddingRight": 0,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 0,
                        "metadataPaddingLeft": 0,
                        "metadataPaddingRight": 0,
                        "metadataPaddingTop": 0,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": false,
                        "titleBottomMargin": "pctH:2",
                        "tagRowHeight": "pctH:3",
                        "tagRowSpacing": "pctH:0.55"
                    },
                    "footer": {
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                },
                "systemsListTate": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "vertical",
                        "listShare": 11,
                        "detailShare": 5,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": "pctW:5",
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": "pctW:2",
                        "cardPaddingRight": "pctW:2",
                        "cardPaddingTop": "pctH:2",
                        "cardPaddingBottom": "pctH:2",
                        "rowHeight": 0,
                        "rowSpacing": "pctH:0.3",
                        "centerSlot": -1,
                        "scrollbarGap": "pctW:1.5",
                        "selectionAccentWidth": "pctW:0.45",
                        "rowTextLeftPadding": "pctW:1.6",
                        "rowTextRightPadding": "pctW:1.6",
                        "favoriteRightPadding": "pctW:1.6",
                        "overlayBottomMargin": "pctH:15"
                    },
                    "detail": {
                        "contentAxis": "horizontal",
                        "sectionGap": "pctW:3",
                        "imageShare": 4,
                        "metadataShare": 8,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 0,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 0,
                        "panePaddingLeft": "pctW:3",
                        "panePaddingRight": "pctW:3",
                        "panePaddingTop": "pctH:1.2",
                        "panePaddingBottom": "pctH:1.2",
                        "imagePaddingLeft": 0,
                        "imagePaddingRight": "pctW:1",
                        "imagePaddingTop": "pctH:0.8",
                        "imagePaddingBottom": "pctH:0.8",
                        "metadataPaddingLeft": "pctW:1.5",
                        "metadataPaddingRight": "pctW:0.5",
                        "metadataPaddingTop": "pctH:0.8",
                        "metadataPaddingBottom": "pctH:0.8",
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": false,
                        "titleBottomMargin": "pctH:1",
                        "tagRowHeight": "pctH:2.6",
                        "tagRowSpacing": "pctH:0.35"
                    },
                    "footer": {
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                },
                "gamesGrid": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "grid": {
                        "leftInset": "pctW:5",
                        "rightInset": "pctW:5",
                        "gutterWidth": "pctW:3",
                        "gutterGap": "pctW:1.5",
                        "columnGap": "pctW:3",
                        "topInset": "pctH:2",
                        "bottomInset": "pctH:2",
                        "rowGap": "pctH:4",
                        "scrollThumbWidth": "pctW:1.2",
                        "scrollThumbRightInset": 0,
                        "scrollThumbRightAligned": false,
                        "scrollArrowSize": "min(pctW:3,pctH:4)",
                        "gutterFollowsContentWidth": false
                    },
                    "footer": {
                        "activeLabelHeight": "pctH:7",
                        "activeLabelBottomMargin": "pctH:8",
                        "bottomStatusVisible": false,
                        "bottomStatusLeftMargin": "pctW:5",
                        "bottomStatusRightMargin": "pctW:5",
                        "gridBottomMargin": "sum(pctH:8,pctH:7)",
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                },
                "gamesList": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "horizontal",
                        "listShare": 2,
                        "detailShare": 1,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": "pctW:5",
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": "pctW:2",
                        "cardPaddingRight": "pctW:2",
                        "cardPaddingTop": "pctH:2",
                        "cardPaddingBottom": "pctH:2",
                        "rowHeight": 0,
                        "rowSpacing": "pctH:0.7",
                        "centerSlot": -1,
                        "scrollbarGap": "pctW:1.5",
                        "selectionAccentWidth": "pctW:0.45",
                        "rowTextLeftPadding": "pctW:1.6",
                        "rowTextRightPadding": "pctW:1.6",
                        "favoriteRightPadding": "pctW:1.6",
                        "overlayBottomMargin": "pctH:15"
                    },
                    "detail": {
                        "contentAxis": "vertical",
                        "sectionGap": "pctH:2",
                        "imageShare": 2,
                        "metadataShare": 1,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 0,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 0,
                        "panePaddingLeft": "pctW:2",
                        "panePaddingRight": "pctW:2",
                        "panePaddingTop": "pctH:2",
                        "panePaddingBottom": "pctH:2",
                        "imagePaddingLeft": 0,
                        "imagePaddingRight": 0,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 0,
                        "metadataPaddingLeft": 0,
                        "metadataPaddingRight": 0,
                        "metadataPaddingTop": 0,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": true,
                        "titleBottomMargin": "pctH:2",
                        "tagRowHeight": "pctH:3",
                        "tagRowSpacing": "pctH:0.55"
                    },
                    "footer": {
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                },
                "gamesListTate": {
                    "header": {
                        "titleInHeader": false,
                        "hudBottomAligned": false,
                        "statusPillPinnedTop": false
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": "pctH:7",
                        "slotMargin": "pctW:5",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "vertical",
                        "listShare": 11,
                        "detailShare": 5,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": "pctW:5",
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": "pctW:2",
                        "cardPaddingRight": "pctW:2",
                        "cardPaddingTop": "pctH:2",
                        "cardPaddingBottom": "pctH:2",
                        "rowHeight": 0,
                        "rowSpacing": "pctH:0.3",
                        "centerSlot": -1,
                        "scrollbarGap": "pctW:1.5",
                        "selectionAccentWidth": "pctW:0.45",
                        "rowTextLeftPadding": "pctW:1.6",
                        "rowTextRightPadding": "pctW:1.6",
                        "favoriteRightPadding": "pctW:1.6",
                        "overlayBottomMargin": "pctH:15"
                    },
                    "detail": {
                        "contentAxis": "horizontal",
                        "sectionGap": "pctW:3",
                        "imageShare": 4,
                        "metadataShare": 8,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 0,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 0,
                        "panePaddingLeft": "pctW:3",
                        "panePaddingRight": "pctW:3",
                        "panePaddingTop": "pctH:1.2",
                        "panePaddingBottom": "pctH:1.2",
                        "imagePaddingLeft": 0,
                        "imagePaddingRight": "pctW:1",
                        "imagePaddingTop": "pctH:0.8",
                        "imagePaddingBottom": "pctH:0.8",
                        "metadataPaddingLeft": "pctW:1.5",
                        "metadataPaddingRight": "pctW:0.5",
                        "metadataPaddingTop": "pctH:0.8",
                        "metadataPaddingBottom": "pctH:0.8",
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": false,
                        "titleBottomMargin": "pctH:1",
                        "tagRowHeight": "pctH:2.6",
                        "tagRowSpacing": "pctH:0.35"
                    },
                    "footer": {
                        "bottomUnsafeHeight": "sum(pctH:6,pctH:2)"
                    },
                    "surface": {
                        "cornerRadius": "cornerRadius"
                    }
                }
            },
            "crt": {
                "systemsGrid": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": false,
                        "stripHeight": 0,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "grid": {
                        "leftInset": 4,
                        "rightInset": 0,
                        "gutterWidth": 8,
                        "gutterGap": 4,
                        "columnGap": 4,
                        "topInset": 2,
                        "bottomInset": 4,
                        "rowGap": 4,
                        "scrollThumbWidth": 4,
                        "scrollThumbRightInset": 2,
                        "scrollThumbRightAligned": false,
                        "scrollArrowSize": 8,
                        "gutterFollowsContentWidth": true
                    },
                    "footer": {
                        "activeLabelHeight": 8,
                        "activeLabelBottomMargin": "pctH:6",
                        "bottomStatusVisible": true,
                        "bottomStatusLeftMargin": 4,
                        "bottomStatusRightMargin": "pctW:5",
                        "gridBottomMargin": "sum(pctH:6,8)",
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                },
                "systemsList": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": 8,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "horizontal",
                        "listShare": 2,
                        "detailShare": 1,
                        "dividerWidth": 1,
                        "dividerMargin": -16,
                        "cardSideMargin": 4,
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": 3,
                        "cardPaddingRight": 2,
                        "cardPaddingTop": 3,
                        "cardPaddingBottom": 2,
                        "rowHeight": 12,
                        "rowSpacing": 0,
                        "centerSlot": 7,
                        "scrollbarGap": 2,
                        "selectionAccentWidth": 2,
                        "rowTextLeftPadding": 4,
                        "rowTextRightPadding": 2,
                        "favoriteRightPadding": 2,
                        "overlayBottomMargin": "pctH:14"
                    },
                    "detail": {
                        "contentAxis": "vertical",
                        "sectionGap": 2,
                        "imageShare": 2,
                        "metadataShare": 1,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 16,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 2,
                        "panePaddingLeft": 1,
                        "panePaddingRight": 1,
                        "panePaddingTop": 3,
                        "panePaddingBottom": 2,
                        "imagePaddingLeft": 2,
                        "imagePaddingRight": 2,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 2,
                        "metadataPaddingLeft": 2,
                        "metadataPaddingRight": 1,
                        "metadataPaddingTop": 0,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 0,
                        "metadataLeftMargin": 2,
                        "metadataRightMargin": 1,
                        "metadataHeightAdjustment": 2,
                        "metadataBottomAligned": false,
                        "metadataLabelMaxWidth": 24,
                        "titleBottomMargin": 2,
                        "tagRowHeight": 9,
                        "tagRowSpacing": 0
                    },
                    "footer": {
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                },
                "systemsListTate": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": 8,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "vertical",
                        "listShare": 11,
                        "detailShare": 5,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": 4,
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": 3,
                        "cardPaddingRight": 2,
                        "cardPaddingTop": 3,
                        "cardPaddingBottom": 2,
                        "rowHeight": 12,
                        "rowSpacing": 0,
                        "centerSlot": 7,
                        "scrollbarGap": 2,
                        "selectionAccentWidth": 2,
                        "rowTextLeftPadding": 4,
                        "rowTextRightPadding": 2,
                        "favoriteRightPadding": 2,
                        "overlayBottomMargin": "pctH:14"
                    },
                    "detail": {
                        "contentAxis": "horizontal",
                        "sectionGap": 2,
                        "imageShare": 4,
                        "metadataShare": 8,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 16,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 2,
                        "panePaddingLeft": 2,
                        "panePaddingRight": 1,
                        "panePaddingTop": 1,
                        "panePaddingBottom": 1,
                        "imagePaddingLeft": 1,
                        "imagePaddingRight": 2,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 1,
                        "metadataPaddingLeft": 2,
                        "metadataPaddingRight": 1,
                        "metadataPaddingTop": 1,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": false,
                        "titleBottomMargin": 1,
                        "tagRowHeight": 8,
                        "tagRowSpacing": 0
                    },
                    "footer": {
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                },
                "gamesGrid": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": false,
                        "stripHeight": 0,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "grid": {
                        "leftInset": 4,
                        "rightInset": 0,
                        "gutterWidth": 8,
                        "gutterGap": 4,
                        "columnGap": 4,
                        "topInset": 2,
                        "bottomInset": 4,
                        "rowGap": 4,
                        "scrollThumbWidth": 4,
                        "scrollThumbRightInset": 2,
                        "scrollThumbRightAligned": false,
                        "scrollArrowSize": 8,
                        "gutterFollowsContentWidth": true
                    },
                    "footer": {
                        "activeLabelHeight": 8,
                        "activeLabelBottomMargin": "pctH:6",
                        "bottomStatusVisible": true,
                        "bottomStatusLeftMargin": 4,
                        "bottomStatusRightMargin": "pctW:5",
                        "gridBottomMargin": "sum(pctH:6,8)",
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                },
                "gamesList": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": 8,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "horizontal",
                        "listShare": 2,
                        "detailShare": 1,
                        "dividerWidth": 1,
                        "dividerMargin": -16,
                        "cardSideMargin": 4,
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": 3,
                        "cardPaddingRight": 2,
                        "cardPaddingTop": 3,
                        "cardPaddingBottom": 2,
                        "rowHeight": 12,
                        "rowSpacing": 0,
                        "centerSlot": 7,
                        "scrollbarGap": 2,
                        "selectionAccentWidth": 2,
                        "rowTextLeftPadding": 4,
                        "rowTextRightPadding": 2,
                        "favoriteRightPadding": 2,
                        "overlayBottomMargin": "pctH:14"
                    },
                    "detail": {
                        "contentAxis": "vertical",
                        "sectionGap": 2,
                        "imageShare": 2,
                        "metadataShare": 1,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 16,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 2,
                        "panePaddingLeft": 1,
                        "panePaddingRight": 1,
                        "panePaddingTop": 3,
                        "panePaddingBottom": 2,
                        "imagePaddingLeft": 2,
                        "imagePaddingRight": 2,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 2,
                        "metadataPaddingLeft": 2,
                        "metadataPaddingRight": 1,
                        "metadataPaddingTop": 0,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 0,
                        "metadataLeftMargin": 2,
                        "metadataRightMargin": 1,
                        "metadataHeightAdjustment": 2,
                        "metadataBottomAligned": true,
                        "metadataLabelMaxWidth": 24,
                        "titleBottomMargin": 2,
                        "tagRowHeight": 9,
                        "tagRowSpacing": 0
                    },
                    "footer": {
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                },
                "gamesListTate": {
                    "header": {
                        "titleInHeader": true,
                        "hudBottomAligned": true,
                        "statusPillPinnedTop": true
                    },
                    "status": {
                        "topStripVisible": true,
                        "stripHeight": 8,
                        "slotMargin": "headerSideMargin",
                        "topMargin": "pctH:1"
                    },
                    "list": {
                        "contentAxis": "vertical",
                        "listShare": 11,
                        "detailShare": 5,
                        "dividerWidth": 1,
                        "dividerMargin": 0,
                        "cardSideMargin": 4,
                        "cardTopMargin": "pctH:2",
                        "cardBottomMargin": "pctH:8",
                        "cardPaddingLeft": 3,
                        "cardPaddingRight": 2,
                        "cardPaddingTop": 3,
                        "cardPaddingBottom": 2,
                        "rowHeight": 12,
                        "rowSpacing": 0,
                        "centerSlot": 7,
                        "scrollbarGap": 2,
                        "selectionAccentWidth": 2,
                        "rowTextLeftPadding": 4,
                        "rowTextRightPadding": 2,
                        "favoriteRightPadding": 2,
                        "overlayBottomMargin": "pctH:14"
                    },
                    "detail": {
                        "contentAxis": "horizontal",
                        "sectionGap": 2,
                        "imageShare": 4,
                        "metadataShare": 8,
                        "imageHeightRatioWithTitle": 48,
                        "imageReservedWidth": 16,
                        "imageReservedHeight": 0,
                        "imageBottomMargin": 2,
                        "panePaddingLeft": 2,
                        "panePaddingRight": 1,
                        "panePaddingTop": 1,
                        "panePaddingBottom": 1,
                        "imagePaddingLeft": 1,
                        "imagePaddingRight": 2,
                        "imagePaddingTop": 0,
                        "imagePaddingBottom": 1,
                        "metadataPaddingLeft": 2,
                        "metadataPaddingRight": 1,
                        "metadataPaddingTop": 1,
                        "metadataPaddingBottom": 0,
                        "metadataTopMargin": 12,
                        "metadataLeftMargin": 0,
                        "metadataRightMargin": 0,
                        "metadataHeightAdjustment": 0,
                        "metadataBottomAligned": false,
                        "titleBottomMargin": 1,
                        "tagRowHeight": 8,
                        "tagRowSpacing": 0
                    },
                    "footer": {
                        "bottomUnsafeHeight": 16
                    },
                    "surface": {
                        "cornerRadius": 4
                    }
                }
            }
        };
    }

    function currentProfile(viewId: string): var {
        return BrowseLayouts.themeProfile(BrowseLayouts.currentThemeId, viewId);
    }

    function themeProfile(themeId: string, viewId: string): var {
        const theme = BrowseLayouts._themes[themeId];
        if (theme === undefined || theme === null || !(viewId in theme))
            return null;
        return BrowseLayouts._resolveValue(theme, theme[viewId], {});
    }

    function boolValue(profile: var, path: string, fallback: bool): bool {
        const value = BrowseLayouts._lookup(profile, path);
        return typeof value === "boolean" ? value : fallback;
    }

    function numberValue(profile: var, path: string, fallback: int): int {
        const value = BrowseLayouts._lookup(profile, path);
        return typeof value === "number" && isFinite(value) ? value : fallback;
    }

    function stringValue(profile: var, path: string, fallback: string): string {
        const value = BrowseLayouts._lookup(profile, path);
        return typeof value === "string" ? value : fallback;
    }

    function _lookup(object: var, path: string): var {
        if (object === null || typeof object !== "object" || path === "")
            return undefined;
        const parts = path.split(".");
        let current = object;
        for (let i = 0; i < parts.length; i++) {
            if (current === null || typeof current !== "object" || !(parts[i] in current))
                return undefined;
            current = current[parts[i]];
        }
        return current;
    }

    function _resolveValue(theme: var, value: var, seenRefs: var): var {
        if (value === null || value === undefined)
            return value;
        if (Array.isArray(value))
            return value.map(entry => BrowseLayouts._resolveValue(theme, entry, seenRefs));
        if (typeof value === "object") {
            const out = {};
            for (const key in value)
                out[key] = BrowseLayouts._resolveValue(theme, value[key], seenRefs);
            return out;
        }
        if (typeof value !== "string")
            return value;

        if (value.startsWith("pctW:"))
            return Sizing.pctW(Number(value.substring("pctW:".length)));
        if (value.startsWith("pctH:"))
            return Sizing.pctH(Number(value.substring("pctH:".length)));
        if (value.startsWith("fontSize:"))
            return Sizing.fontSize(Number(value.substring("fontSize:".length)));
        if (value === "cornerRadius")
            return Sizing.cornerRadius;
        if (value === "headerSideMargin")
            return Sizing.headerSideMargin;
        if (value.startsWith("ref:")) {
            const refPath = value.substring("ref:".length);
            if (seenRefs[refPath] === true)
                return undefined;
            const nextSeen = Object.assign({}, seenRefs);
            nextSeen[refPath] = true;
            return BrowseLayouts._resolveValue(theme, BrowseLayouts._lookup(theme, refPath), nextSeen);
        }

        const fnMatch = value.match(/^([a-z]+)\((.*)\)$/);
        if (fnMatch !== null) {
            const fnName = fnMatch[1];
            const args = BrowseLayouts._splitArgs(fnMatch[2]).map(arg => BrowseLayouts._resolveValue(theme, arg, seenRefs));
            if (fnName === "min" && args.length === 2)
                return Math.min(args[0], args[1]);
            if (fnName === "max" && args.length === 2)
                return Math.max(args[0], args[1]);
            if (fnName === "sum")
                return args.reduce((total, entry) => total + entry, 0);
        }

        const numeric = Number(value);
        if (!isNaN(numeric))
            return numeric;
        return value;
    }

    function _splitArgs(text: string): var {
        const parts = [];
        let start = 0;
        let depth = 0;
        for (let i = 0; i < text.length; i++) {
            const ch = text[i];
            if (ch === "(")
                depth++;
            else if (ch === ")")
                depth--;
            else if (ch === "," && depth === 0) {
                parts.push(text.substring(start, i).trim());
                start = i + 1;
            }
        }
        parts.push(text.substring(start).trim());
        return parts;
    }
}
