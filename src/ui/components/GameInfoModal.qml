// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 Browse singleton methods lack isFinal in the qmltypes schema so
// every access trips "Member can be shadowed". Structural; suppress compiler.
// qmllint disable compiler
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

Item {
    id: root

    property bool open: false
    readonly property bool _hasContentAbove: flick.contentY > 1
    readonly property bool _hasContentBelow: flick.contentY + flick.height < flick.contentHeight - 1
    // Round 10: scroll chevrons only paint when there's genuinely more
    // than one screenful -- see `_hasContentAbove`/`_hasContentBelow`
    // below, which mirror the same "hide when there's nothing to page/
    // scroll to" rule PageIndicator.qml applies to grid paging.
    readonly property bool _scrollable: flick.contentHeight > flick.height
    // Round 10: fixed label-column width, sized once against the widest
    // label in the known vocabulary rather than a live per-open
    // accumulator. The old `_labelColumnWidth` grew across the whole
    // modal's lifetime (Component.onCompleted + onAdvanceWidthChanged per
    // row) and never shrank even after switching to a game with shorter
    // tags, so the column could stay needlessly wide. Passthrough tag
    // types outside this set simply elide (`Text.ElideRight` on the label
    // below) rather than growing the column further.
    //
    // The vocabulary is the canonical tag *types* the models emit; the
    // labels come from `Format.metadataLabel`, which is where they get
    // their `qsTr()`. Measuring translated labels rather than a hardcoded
    // English list is what keeps the column correct in every language --
    // "Release date" and "Veröffentlichungsdatum" are not the same width.
    readonly property var _knownTagTypes: ["system", "platform", "year", "release_date", "genre", "players", "play_mode", "cooperative", "developer", "publisher", "rating", "filename"]
    readonly property int _labelColumnWidth: Sizing.px(root._knownTagTypes.reduce((widest, tagType) => Math.max(widest, tagLabelMetrics.advanceWidth(Format.metadataLabel(tagType))), 0) + Sizing.stroke(2))

    // Measured title box, so the title can be centered by item position
    // instead of `Text.AlignHCenter` -- a centered glyph run lands on a
    // half pixel and softens, which CLAUDE.md rules out on user-visible
    // text at any resolution. The slack matches every other measurement
    // site in the codebase (ContextMenu, ListPickerModal): advanceWidth
    // measures cursor movement, not painted pixels, and under
    // NativeRendering a hinted run can paint a couple of px wider.
    readonly property int _titleNaturalWidth: Sizing.px(Math.ceil(Math.max(titleMetrics.advanceWidth, titleMetrics.boundingRect.width)) + Sizing.stroke(2))
    readonly property int _titleAvailableWidth: Math.max(0, panel.width - 2 * Sizing.pctW(4))
    // A title too long to fit fills the width and elides, at which point
    // there is no alignment question left to answer.
    readonly property bool _titleFits: root._titleNaturalWidth > 0 && root._titleNaturalWidth <= root._titleAvailableWidth

    signal closeRequested

    visible: open
    enabled: visible
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        if (root.open)
            flick.contentY = 0;
    }

    function _scrollBody(delta: int): void {
        if (!flick.visible)
            return;
        const maxY = Math.max(0, flick.contentHeight - flick.height);
        flick.contentY = Math.max(0, Math.min(maxY, flick.contentY + delta));
    }

    function handleAction(action: string): void {
        if (action === "cancel" || action === "accept")
            root.closeRequested();
        else if (action === "left" && Browse.GameInfo.image_count > 1)
            Browse.GameInfo.cycle_image(-1);
        else if (action === "right" && Browse.GameInfo.image_count > 1)
            Browse.GameInfo.cycle_image(1);
        else if (action === "up")
            root._scrollBody(-Sizing.pctH(8));
        else if (action === "down")
            root._scrollBody(Sizing.pctH(8));
        else if (action === "page_prev")
            root._scrollBody(-Math.max(Sizing.pctH(12), flick.height - Sizing.pctH(8)));
        else if (action === "page_next")
            root._scrollBody(Math.max(Sizing.pctH(12), flick.height - Sizing.pctH(8)));
    }

    // Fixed-weight measurement of the known tag-label vocabulary above --
    // FontMetrics + an invokable `.advanceWidth(text)` call is safe here
    // (unlike a per-row *live* weight, the round-8/9 pitfall documented in
    // ContextMenu.qml/ListPickerModal.qml) because the measured set is
    // fixed for the session: `_knownTagTypes` is a literal, and the
    // `Format.metadataLabel` strings it resolves to only change with the
    // interface language, which restarts the frontend (see
    // SettingsScreen's `language` row). Nothing here changes under the
    // binding.
    FontMetrics {
        id: tagLabelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
    }

    TextMetrics {
        id: titleMetrics

        text: Browse.GameInfo.title
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontTitle
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.scrim

        // Swallowed, not a dismiss. `Modal.qml`'s scrim deliberately eats
        // the click too, and two modals on the same scrim answering an
        // outside click differently is the kind of inconsistency a d-pad
        // user with mouse support on has no way to predict. B closes.
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.AllButtons
        }

        Rectangle {
            id: panel

            objectName: "gameInfoPanel"
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            // Same 92% ceiling `Modal.qml` clamps every other modal to.
            // This panel used to sit at `parent.width - pctW(6)` (~94%),
            // wider than any other modal is allowed to be.
            width: Sizing.px(Math.min(Math.min(parent.width * 0.92, Sizing.pctH(150)), parent.width - Sizing.pctW(6)))
            // Content-driven, capped at the old fixed height rather than
            // pinned to it. A game with a cover and four tags used to get
            // ~84% of the screen as mostly empty panel, while every other
            // modal in the app sizes to what it holds. `_chromeHeight` is
            // the title band plus the flickable's own top/bottom margins,
            // the two fixed bands the content sits between.
            readonly property int _chromeHeight: titleText.y + titleText.height + Sizing.pctH(4) + Sizing.pctH(4)
            readonly property int _maxHeight: Sizing.px(parent.height - Sizing.pctH(16))
            height: Sizing.px(Math.min(panel._maxHeight, panel._chromeHeight + contentColumn.height))
            color: Theme.bgPanel
            radius: Sizing.radiusMd
            antialiasing: Sizing.cornerAntialiasing

            MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.AllButtons
            }

            Text {
                id: titleText

                objectName: "gameInfoTitle"
                // Centered like every other modal title (Modal.qml), but by
                // item position rather than `Text.AlignHCenter`: a centered
                // glyph run straddles a half pixel and softens, which
                // CLAUDE.md rules out on user-visible text at any
                // resolution. A title too long to fit takes the full width
                // and elides, where alignment no longer means anything.
                x: root._titleFits ? Sizing.center(parent.width, width) : Sizing.pctW(4)
                width: root._titleFits ? root._titleNaturalWidth : root._titleAvailableWidth
                anchors.top: parent.top
                anchors.topMargin: Sizing.pctH(4)
                text: Browse.GameInfo.title
                textFormat: Text.PlainText
                color: Theme.textPrimary
                font.family: Theme.fontUi
                // `Sizing.fontTitle`, the same rung `Modal.qml`'s shell gives
                // every other modal title, rather than a bespoke `fontSize(3.4)`
                // that sits off the ladder. `Font.Medium` is gone with it: the
                // shell sets no weight, and weight is a no-op in bitmap mode
                // anyway (one face, see docs/style.md -> "Settings section
                // headers"), so it was a signal that silently disappeared at
                // the CRT and 240p tiers.
                font.pixelSize: Sizing.fontTitle
                elide: Text.ElideRight
                maximumLineCount: 1
                horizontalAlignment: Text.AlignLeft
                renderType: Text.NativeRendering
            }

            LoadingIndicator {
                visible: Browse.GameInfo.loading
                x: Sizing.center(parent.width, width)
                y: Sizing.center(parent.height, height)
                text: qsTr("Loading details…")
            }

            Text {
                visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message !== ""
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                anchors.topMargin: Sizing.pctH(4)
                text: qsTr("Could not load details. Check Zaparoo Core and try again.")
                textFormat: Text.PlainText
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontBody
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignLeft
                renderType: Text.NativeRendering
            }

            Flickable {
                id: flick

                visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message === ""
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                // Must clear the up chevron, which hangs ABOVE this anchor
                // (`gameInfoScrollUp` is `anchors.bottom: flick.top`): it needs
                // its own `pctH(3)` height plus its `pctH(0.5)` margin, so a
                // gap under `pctH(3.5)` leaves it drawing through the title.
                // Matches the bottom margin below, where the down chevron has
                // always had the room it needs -- keep the two in step so the
                // chevrons sit symmetrically.
                anchors.topMargin: Sizing.pctH(4)
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Sizing.pctH(4)
                contentWidth: width
                contentHeight: contentColumn.height
                boundsBehavior: Flickable.StopAtBounds
                clip: true

                Column {
                    id: contentColumn

                    width: flick.width
                    spacing: Sizing.pctH(2.4)

                    // Cover/screenshot slot. No card plate and no border:
                    // docs/style.md's card recipe (surfaceCard fill +
                    // borderMid edge + radiusMd) describes a *selectable*
                    // surface, and this art is neither selectable nor
                    // sitting on a background that needs contrast help --
                    // the panel behind it is already an opaque bgPanel. A
                    // full-column-width plate behind PreserveAspectFit art
                    // also framed every portrait cover in wide gutters,
                    // which is what read as "a border around the image".
                    //
                    // The carousel gutters are reserved unconditionally
                    // (`_carouselGutter`, same idea as BrowseDetailPane's),
                    // so the arrows never paint over the artwork and the
                    // cover footprint does not jump when `image_can_next`
                    // flips async after the meta response lands.
                    Item {
                        id: coverSlot

                        objectName: "gameInfoCoverSlot"
                        readonly property int _carouselGutter: Browse.GameInfo.image_count > 1 ? Sizing.pctW(4) : 0

                        width: parent.width
                        height: Browse.GameInfo.image_count > 0 ? Sizing.pctH(32) : 0
                        visible: height > 0

                        Item {
                            id: coverInner

                            anchors.fill: parent
                            anchors.leftMargin: coverSlot._carouselGutter
                            anchors.rightMargin: coverSlot._carouselGutter

                            Image {
                                anchors.fill: parent
                                source: Browse.GameInfo.image_key !== "" ? Resources.coverUrl(Browse.GameInfo.image_key, Theme.textPrimary, Theme.bgPanel) : ""
                                sourceSize.width: Sizing.px(parent.width)
                                sourceSize.height: Sizing.px(parent.height)
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                asynchronous: true
                            }

                            LoadingIndicator {
                                visible: Browse.GameInfo.image_key === ""
                                x: Sizing.center(parent.width, width)
                                y: Sizing.center(parent.height, height)
                                text: qsTr("Loading image…")
                                glyphSize: Sizing.fontCaption
                            }
                        }

                        // Dimmed rather than hidden when that direction has
                        // nothing left, matching the scroll chevrons below
                        // and PageIndicator's rule: a bare glance should
                        // still say "this cycles", with only the live
                        // direction reading as actionable. A colour swap,
                        // not an opacity one -- a translucent node repaints
                        // everything under it every frame in software
                        // rendering.
                        Image {
                            source: Resources.iconUrl("NavLeft", Browse.GameInfo.image_can_prev ? Theme.textPrimary : Theme.textLabel)
                            width: Sizing.pctH(4)
                            height: width
                            sourceSize.width: Sizing.px(width)
                            sourceSize.height: Sizing.px(height)
                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                            visible: Browse.GameInfo.image_count > 1
                        }

                        Image {
                            source: Resources.iconUrl("NavRight", Browse.GameInfo.image_can_next ? Theme.textPrimary : Theme.textLabel)
                            width: Sizing.pctH(4)
                            height: width
                            sourceSize.width: Sizing.px(width)
                            sourceSize.height: Sizing.px(height)
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                            visible: Browse.GameInfo.image_count > 1
                        }
                    }

                    // The file behind this row is gone. Core still has it
                    // indexed, so without saying so the modal looks
                    // completely normal right up until a launch fails.
                    Text {
                        width: parent.width
                        visible: Browse.GameInfo.media_missing
                        text: qsTr("This game's file is missing.")
                        textFormat: Text.PlainText
                        color: Theme.error
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignLeft
                        renderType: Text.NativeRendering
                    }

                    Column {
                        id: tagTable

                        width: parent.width
                        spacing: Sizing.pctH(0.8)
                        visible: Browse.GameInfo.detail_tags !== ""

                        Repeater {
                            model: Browse.GameInfo.detail_tags === "" ? [] : Browse.GameInfo.detail_tags.split("\n")

                            delegate: Item {
                                id: tagRow

                                required property string modelData
                                required property int index

                                width: tagTable.width
                                // `paintedHeight` is a real, so the sum has
                                // to land back on an integer pixel before
                                // it drives geometry (CLAUDE.md's Sizing
                                // rule). Values are capped at two lines so
                                // one pathological scraped string can't
                                // reflow the whole table.
                                height: Sizing.px(Math.max(Sizing.pctH(3), tagValue.paintedHeight) + (tagRow.index > 0 ? Sizing.pctH(0.8) : 0))

                                readonly property list<string> parts: modelData.split("\t")
                                // The model emits a canonical tag *type*
                                // (`release_date`); the label comes from
                                // Format, which is where it gets its
                                // qsTr(). game_info.rs used to bake a
                                // title-cased English label in, so this
                                // whole table shipped untranslated.
                                readonly property string label: parts.length > 0 ? Format.metadataLabel(parts[0]) : ""
                                readonly property string value: parts.length > 1 ? parts[1] : ""

                                // Hairline row divider — every row but the
                                // first, so adjacent tags read as
                                // scannable rows instead of one undivided
                                // block.
                                Rectangle {
                                    visible: tagRow.index > 0
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    height: Sizing.stroke(1)
                                    color: Theme.borderSubtle
                                }

                                readonly property int _rowTop: tagRow.index > 0 ? Sizing.pctH(0.8) : 0

                                Text {
                                    anchors.left: parent.left
                                    anchors.top: parent.top
                                    anchors.topMargin: tagRow._rowTop
                                    width: root._labelColumnWidth
                                    text: tagRow.label
                                    textFormat: Text.PlainText
                                    color: Theme.textLabel
                                    font.family: Theme.fontUi
                                    font.pixelSize: Sizing.fontBody
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignLeft
                                    renderType: Text.NativeRendering
                                }

                                Text {
                                    id: tagValue

                                    anchors.left: parent.left
                                    anchors.leftMargin: root._labelColumnWidth + Sizing.pctW(1.4)
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.topMargin: tagRow._rowTop
                                    text: tagRow.value
                                    // Core data, straight from a scraper:
                                    // PlainText or a stray `<` or `&` in a
                                    // third-party string renders as markup.
                                    textFormat: Text.PlainText
                                    color: Theme.textPrimary
                                    font.family: Theme.fontUi
                                    font.pixelSize: Sizing.fontBody
                                    wrapMode: Text.Wrap
                                    maximumLineCount: 2
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignLeft
                                    renderType: Text.NativeRendering
                                }
                            }
                        }
                    }

                    // Description block -- its own section heading (the
                    // shared SectionHeader, same module, no import needed)
                    // rather than just extra Column spacing, so it reads as
                    // a distinct block from the tag table above it.
                    SectionHeader {
                        width: parent.width
                        // The tag table and the description below both sit
                        // flush at x: 0 in this column, so the heading has
                        // to as well -- the shared inset exists to line a
                        // heading up with the rows it heads, and here those
                        // rows have none.
                        labelInset: 0
                        visible: Browse.GameInfo.description !== ""
                        label: qsTr("Description")
                    }

                    Text {
                        width: parent.width
                        visible: Browse.GameInfo.description !== ""
                        text: Browse.GameInfo.description
                        // Scraped third-party prose; see the tag value above.
                        textFormat: Text.PlainText
                        color: Theme.textPrimary
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignLeft
                        renderType: Text.NativeRendering
                    }
                }
            }

            // Round 9: dims (Theme.textLabel) rather than hides when the
            // panel doesn't overflow in that direction, matching
            // PageIndicator.qml's treatment. Round 10: both now hide
            // entirely (not just their dim state) when `flick`'s content
            // doesn't overflow at all -- a description-only game with no
            // tag table might not fill the flickable, and two permanently
            // dim arrows pointing at nothing to scroll to said nothing
            // useful.
            Image {
                objectName: "gameInfoScrollUp"
                source: Resources.iconUrl("ScrollUp", root._hasContentAbove ? Theme.textPrimary : Theme.textLabel)
                width: Sizing.pctH(3)
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.bottom: flick.top
                anchors.bottomMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._scrollable
            }

            Image {
                objectName: "gameInfoScrollDown"
                source: Resources.iconUrl("ScrollDown", root._hasContentBelow ? Theme.textPrimary : Theme.textLabel)
                width: Sizing.pctH(3)
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.top: flick.bottom
                anchors.topMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._scrollable
            }
        }
    }
}
