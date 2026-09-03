// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property string title: ""
    // Stable identity for the focused row. Cover holding is scoped to this
    // identity so an async decode from the previous row can never flash under
    // the newly focused title.
    property string identity: ""
    property string coverKey: ""
    property string description: ""
    property bool showDescription: true
    property bool showTitle: true
    property string detailTags: ""
    property bool canPreviousImage: false
    property bool canNextImage: false
    // Reserve the carousel gutter even before can_next/can_prev are known,
    // so the cover footprint is stable from the first frame on screens that
    // support image cycling. Set to true by GamesScreen; Recents/Favorites
    // leave it false (they have no carousel wiring).
    property bool reserveImageNav: false
    property bool loading: false
    property bool detailSuppressed: false
    property bool showChrome: true
    property string loadingText: qsTr("Loading…")
    property int loadingDelayMs: 150
    property var layoutProfile: null

    readonly property var _detail: root.layoutProfile && root.layoutProfile.detail ? root.layoutProfile.detail : null
    readonly property var _surface: root.layoutProfile && root.layoutProfile.surface ? root.layoutProfile.surface : null
    readonly property int _panePaddingLeft: root._detail ? root._detail.panePaddingLeft : Sizing.pctW(2)
    readonly property int _panePaddingRight: root._detail ? root._detail.panePaddingRight : Sizing.pctW(2)
    readonly property int _panePaddingTop: root._detail ? root._detail.panePaddingTop : Sizing.pctH(2)
    readonly property int _panePaddingBottom: root._detail ? root._detail.panePaddingBottom : Sizing.pctH(2)
    readonly property int _imagePaddingLeft: root._detail ? root._detail.imagePaddingLeft : 0
    readonly property int _imagePaddingRight: root._detail ? root._detail.imagePaddingRight : 0
    readonly property int _imagePaddingTop: root._detail ? root._detail.imagePaddingTop : 0
    readonly property int _imagePaddingBottom: root._detail ? root._detail.imagePaddingBottom : 0
    readonly property int _metadataPaddingLeft: root._detail ? root._detail.metadataPaddingLeft : 0
    readonly property int _metadataPaddingRight: root._detail ? root._detail.metadataPaddingRight : 0
    readonly property int _metadataPaddingTop: root._detail ? root._detail.metadataPaddingTop : 0
    readonly property int _metadataPaddingBottom: root._detail ? root._detail.metadataPaddingBottom : 0
    readonly property int _metadataTopMargin: root._detail && root._detail.metadataTopMargin !== undefined ? root._detail.metadataTopMargin : 0
    readonly property int _metadataLeftMargin: root._detail && root._detail.metadataLeftMargin !== undefined ? root._detail.metadataLeftMargin : 0
    readonly property int _metadataRightMargin: root._detail && root._detail.metadataRightMargin !== undefined ? root._detail.metadataRightMargin : 0
    readonly property int _metadataHeightAdjustment: root._detail && root._detail.metadataHeightAdjustment !== undefined ? root._detail.metadataHeightAdjustment : 0
    readonly property int _sectionGap: root._detail ? root._detail.sectionGap : Sizing.pctH(2)
    readonly property bool _horizontalSections: root._detail && root._detail.contentAxis === "horizontal"
    readonly property real _imageShare: root._detail && root._detail.imageShare !== undefined ? root._detail.imageShare : 1
    readonly property real _metadataShare: root._detail && root._detail.metadataShare !== undefined ? root._detail.metadataShare : 1
    readonly property real _shareTotal: Math.max(1, root._imageShare + root._metadataShare)
    // Round 11: never shorter than the label/value text's own line height.
    // The per-theme profile value alone let a row box (e.g. `pctH:2.6` at
    // the 540/480 tiers) sit a few px shorter than Noto Sans's real
    // ascent+descent at `_tagTextSize`, so the last row's descender (the
    // tail of "Rating"'s g) crossed `tagTable`'s clip boundary. See
    // `tagFontMetrics` below.
    readonly property int _tagRowHeight: Math.max(root._detail ? root._detail.tagRowHeight : Sizing.pctH(3), Math.ceil(tagFontMetrics.height))
    readonly property int _tagRowSpacing: root._detail ? root._detail.tagRowSpacing : Sizing.pctH(0.55)
    readonly property bool _metadataBottomAligned: root._detail && root._detail.metadataBottomAligned === true
    readonly property int _titleBottomMargin: root._detail ? root._detail.titleBottomMargin : Sizing.pctH(2)
    readonly property real _imageHeightRatioWithTitle: root._detail && root._detail.imageHeightRatioWithTitle !== undefined ? root._detail.imageHeightRatioWithTitle : 48
    readonly property int _imageReservedWidth: root._detail && root._detail.imageReservedWidth !== undefined ? root._detail.imageReservedWidth : 0
    readonly property int _imageReservedHeight: root._detail && root._detail.imageReservedHeight !== undefined ? root._detail.imageReservedHeight : 0
    readonly property int _imageBottomMargin: root._detail && root._detail.imageBottomMargin !== undefined ? root._detail.imageBottomMargin : 0
    readonly property int _cardRadius: root._surface ? root._surface.cardRadius : Sizing.radiusMd
    // Reserve the side gutter whenever this screen supports image cycling
    // (reserveImageNav) OR when can_prev/can_next are already known, so the
    // cover footprint never changes when can_next flips async after meta loads.
    readonly property int _carouselGutter: (root.reserveImageNav || canPreviousImage || canNextImage) ? Sizing.pctW(4) : 0
    readonly property bool _coverPending: coverKey === "icons/Loading"
    // Round 11: no more hourglass overlay -- while pending, hold the last
    // good cover (coverHold below) instead of blanking after a delay. A
    // row with real art keeps showing it right up until the new one
    // decodes; a row that's never had art just stays blank, matching how
    // the grid tiles behave (Tile.qml's `_coverPending` swallows the same
    // sentinel to an empty source).
    readonly property bool _hasCurrentHeldCover: root.identity !== "" && root._lastGoodCoverIdentity === root.identity && root._lastGoodCoverSource !== ""
    readonly property url _coverSource: _coverPending && root._hasCurrentHeldCover ? root._lastGoodCoverSource : (_coverPending ? "" : Resources.coverUrl(coverKey, Theme.logoFocusPrimary, Theme.logoFocusSecondary, Theme.logoFocusShadow))
    // True whenever the cover Image is in flight (model pending, Qt async
    // decode, or any non-media-image provider still loading).
    readonly property bool _coverMediaImagePending: coverKey.startsWith("media-image/") && cover.status !== Image.Ready && cover.status !== Image.Error
    readonly property bool _coverBusy: root._coverPending || root._coverMediaImagePending || cover.status === Image.Loading
    readonly property bool _paneLoading: root.loading
    readonly property bool _delayedPaneLoading: root._paneLoading && root._paneLoadingDelayElapsed
    // Only fetched raster art gets the reveal fade (see `cover`'s
    // `updateReveal`) -- bundled glyphs (the File chip, system logos) load
    // from memory and would just look like flicker if faded, mirroring
    // Tile.qml's own `_coverIsRealArt` split.
    readonly property bool _coverIsRealArt: root.coverKey.startsWith("media-image/") || root.coverKey.startsWith("custom-image/")
    readonly property bool _detailVisible: !root.detailSuppressed
    readonly property bool _emptyPaneLoading: root._delayedPaneLoading && !root._coverBusy && root._coverSource === "" && root._displayRows.length === 0 && root.title === ""
    readonly property var _detailRows: _parseDetailTags(detailTags)
    readonly property int _tagRowCount: _displayRows.length
    readonly property int _tagTextSize: Sizing.fontSmall
    readonly property int _tagLabelGap: Sizing.pctW(1.4)
    readonly property int _metadataLabelMaxWidth: root._detail && root._detail.metadataLabelMaxWidth !== undefined ? root._detail.metadataLabelMaxWidth : 0
    readonly property int _labelColumnWidth: root._metadataLabelMaxWidth > 0 ? Math.min(root._labelColumnNaturalWidth, root._metadataLabelMaxWidth) : root._labelColumnNaturalWidth
    readonly property int _metadataNaturalHeight: _tagRowCount <= 0 ? 0 : (_tagRowCount * _tagRowHeight) + ((_tagRowCount - 1) * _tagRowSpacing)
    // Round 11: capped at the metadata slot's own real height
    // (`content.metadataHeight`, the vertical layout's actual secondary
    // span below the cover) rather than a flat 38% of the pane's total
    // height, which was disconnected from the profile's real image/
    // metadata share split and could let `detailBody` size itself taller
    // than the space `metadataSlot` (its clipping ancestor) actually has —
    // the mechanism behind the clipped last row. Still floors at the
    // natural fit so a short tag list never claims more height than it
    // needs; only clamps when the pane genuinely can't hold all six rows.
    readonly property int _compactMetadataHeight: Math.min(content.metadataHeight, _metadataNaturalHeight)
    // True for system-logo cover keys; used to select the wordmark fallback
    // instead of the generic File chip when no logo SVG exists.
    readonly property bool _isSystemCover: root.coverKey.startsWith("systems/")

    // Measured from the rows actually on screen, not accumulated across the
    // session. The old form was a `Math.max` fed by every delegate's
    // `Component.onCompleted` and `onAdvanceWidthChanged`, with nothing that
    // ever reset it -- safe only while the label set never varied. It does
    // vary: a systems row's table carries `Manufacturer`, a media row's does
    // not, so browsing Systems and then Games left the games table paying
    // for a column width no label in it needs. Same union-plus-slack figure
    // as before, just derived rather than remembered.
    readonly property int _labelColumnNaturalWidth: {
        let widest = 0;
        const rows = root._displayRows;
        for (let i = 0; i < rows.length; i++) {
            const text = rows[i].measureLabel ?? "";
            if (text === "")
                continue;
            widest = Math.max(widest, Math.ceil(Math.max(labelColumnMetrics.advanceWidth(text), labelColumnMetrics.boundingRect(text).width) + root._labelSlack));
        }
        return widest;
    }
    // advanceWidth measures cursor movement, not painted pixels: under
    // NativeRendering a fully hinted run can paint a px or two wider. Same
    // slack every other measurement site in the codebase carries
    // (ScrollingCaption, PageIndicator, TopStatusStrip, ContextMenu).
    readonly property int _labelSlack: Theme.crtNativePath ? 0 : Sizing.px(2)
    property bool _paneLoadingDelayElapsed: false
    // Holds the last resolved cover URL so the area does not blank while a
    // new one decodes -- see `_coverSource`/`coverHold` above.
    property url _lastGoodCoverSource: ""
    property string _lastGoodCoverIdentity: ""
    // The detail table tracks the focused row's metadata directly. The model
    // keeps `current_detail_tags` identity-correct on every move — an immediate
    // peek shows cached/local rows or a clean blank, never the previous row's
    // values — so no client-side hold is needed. (The cover keeps its own hold;
    // see `coverHold`/`_lastGoodCoverSource` below.)
    readonly property var _displayRows: root._detailRows

    onLoadingChanged: root._updatePaneLoadingDelay()
    onLoadingDelayMsChanged: root._updatePaneLoadingDelay()

    Timer {
        id: paneLoadingDelayTimer

        interval: Math.max(0, root.loadingDelayMs)
        repeat: false
        onTriggered: root._paneLoadingDelayElapsed = root._paneLoading
    }

    function _updatePaneLoadingDelay(): void {
        paneLoadingDelayTimer.stop();
        root._paneLoadingDelayElapsed = false;
        if (!root._paneLoading)
            return;
        if (root.loadingDelayMs <= 0) {
            root._paneLoadingDelayElapsed = true;
            return;
        }
        paneLoadingDelayTimer.restart();
    }

    // Label vocabulary lives in `Format` so this pane and the details
    // modal render one set of strings for one set of tag types. It used to
    // live here as a private ten-entry ladder, which is why the modal
    // (which never had access to it) shipped its own untranslated labels.
    //
    // `Format.metadataElidableLabel` packs the full label and its short
    // form behind U+009C, Qt's alternative-text separator: an eliding
    // `Text` renders the short form rather than truncating the long one
    // when the column is narrow. `metadataLabel` alone is what the column
    // is measured against, so the width tracks the full label.
    function _parseDetailTags(tags: string): var {
        if (tags === "")
            return [];
        return tags.split("\n").map(row => {
            const parts = row.split("\t");
            const rawLabel = parts.length > 0 ? parts[0] : "";
            return {
                "rawLabel": rawLabel,
                "label": Format.metadataElidableLabel(rawLabel),
                "measureLabel": Format.metadataLabel(rawLabel),
                "value": parts.length > 1 ? parts[1] : ""
            };
        });
    }

    // Backs `_tagRowHeight`'s floor -- see that property's doc comment.
    FontMetrics {
        id: tagFontMetrics
        font.family: Theme.fontUi
        font.pixelSize: root._tagTextSize
    }

    // Backs `_labelColumnNaturalWidth`. Measuring N fixed strings through
    // one FontMetrics is safe (and is what GameInfoModal does for the same
    // job); the round-8/9 pitfall documented in ContextMenu.qml is a *live
    // per-row weight*, which nothing here has.
    FontMetrics {
        id: labelColumnMetrics
        font.family: Theme.fontUi
        font.pixelSize: root._tagTextSize
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.width: Sizing.cardBorderWidth
        border.color: Theme.borderMid
        radius: root._cardRadius
        antialiasing: Sizing.cornerAntialiasing
        visible: root.showChrome
    }

    Item {
        id: content

        anchors.fill: parent
        anchors.leftMargin: root._panePaddingLeft
        anchors.rightMargin: root._panePaddingRight
        anchors.topMargin: root._panePaddingTop
        anchors.bottomMargin: root._panePaddingBottom
        clip: true

        readonly property int primarySpan: root._horizontalSections ? Math.floor((width - root._sectionGap) * root._imageShare / root._shareTotal) : Math.floor((height - root._sectionGap) * root._imageShare / root._shareTotal)
        readonly property int secondarySpan: root._horizontalSections ? Math.max(0, width - primarySpan - root._sectionGap) : Math.max(0, height - imageSlotHeight - root._sectionGap)
        readonly property int imageSlotX: root._horizontalSections ? root._carouselGutter : root._imagePaddingLeft + root._carouselGutter
        readonly property int imageSlotY: 0
        readonly property int imageSlotWidth: {
            if (root._horizontalSections)
                return Math.max(0, primarySpan - (2 * root._carouselGutter));
            const availableWidth = Math.max(0, width - (2 * root._carouselGutter) - root._imagePaddingLeft - root._imagePaddingRight);
            const maxWidth = Math.max(0, width - root._imagePaddingLeft - root._imagePaddingRight);
            return Math.max(0, Math.min(maxWidth, availableWidth + root._imageReservedWidth));
        }
        readonly property int imageSlotHeight: {
            if (root._horizontalSections)
                return height;
            const availableWidth = Math.max(0, width - (2 * root._carouselGutter) - root._imagePaddingLeft - root._imagePaddingRight);
            // Title visible: use the fixed ratio from the profile.
            // Title not visible (media screens): use the share-based primarySpan
            // so the cover footprint is stable from the first frame regardless of
            // whether metadata tags have loaded yet. _compactMetadataHeight
            // is metadata-driven and would cause a reflow on every move.
            const imageLimit = titleText.visible ? Math.floor((height * root._imageHeightRatioWithTitle) / 100) : Math.max(0, primarySpan - root._imageBottomMargin);
            return Math.max(0, Math.min(height, Math.min(availableWidth, imageLimit) + root._imageReservedHeight));
        }
        readonly property int metadataX: root._horizontalSections ? primarySpan + root._sectionGap : 0
        readonly property int metadataY: root._horizontalSections ? 0 : imageSlotHeight + root._sectionGap
        readonly property int metadataWidth: root._horizontalSections ? secondarySpan : width
        readonly property int metadataHeight: root._horizontalSections ? height : secondarySpan

        Item {
            id: imageSlot

            x: content.imageSlotX
            y: content.imageSlotY
            width: content.imageSlotWidth
            height: content.imageSlotHeight
            clip: true

            Item {
                anchors.fill: parent
                anchors.leftMargin: root._horizontalSections ? root._imagePaddingLeft : 0
                anchors.rightMargin: root._horizontalSections ? root._imagePaddingRight : 0
                anchors.topMargin: root._imagePaddingTop
                anchors.bottomMargin: root._imagePaddingBottom

                // Holds the previously decoded cover while the new one async-decodes.
                // Prevents the slot from blanking during the brief Qt pixmap-decode
                // window (typically < 150 ms for a cached JPEG) or while a fetch is
                // still in flight (`_coverPending`) -- round 11 dropped the hourglass
                // overlay in favor of the grid tile's own "stay blank, then fade"
                // treatment (see `cover`'s `updateReveal` below), so a row with
                // already-loaded art keeps showing it right up until the new one is
                // ready rather than ever flashing an hourglass.
                Image {
                    id: coverHold

                    objectName: "detailCoverHold"
                    anchors.fill: parent
                    source: root._lastGoodCoverSource
                    fillMode: Image.PreserveAspectFit
                    sourceSize.width: Sizing.detailCoverSourceWidth
                    smooth: true
                    asynchronous: false
                    cache: true
                    visible: root._hasCurrentHeldCover && root._lastGoodCoverSource !== root._coverSource && cover.status !== Image.Ready && !root.detailSuppressed && !root._isSystemCover
                }

                Image {
                    id: cover

                    objectName: "detailCoverImage"
                    anchors.fill: parent
                    source: root._coverSource
                    fillMode: Image.PreserveAspectFit
                    sourceSize.width: Sizing.detailCoverSourceWidth
                    smooth: true
                    asynchronous: true
                    visible: root._coverSource !== "" && status === Image.Ready && !root.detailSuppressed
                    // Real fetched art gets one brief reveal after decode, matching
                    // Tile.qml's `coverBase.revealOpacity`/`updateReveal` byte-for-
                    // byte -- bundled glyphs (the File chip, system logos) stay
                    // instant, only `_coverIsRealArt` keys fade.
                    property real revealOpacity: root._coverIsRealArt ? 0 : 1
                    opacity: cover.status === Image.Ready ? cover.revealOpacity : 0

                    NumberAnimation {
                        id: coverRevealAnimation
                        objectName: "detailCoverRevealAnimation"

                        target: cover
                        property: "revealOpacity"
                        from: 0
                        to: 1
                        duration: Motion.dur(Motion.pressMs)
                        easing.type: Easing.OutQuad
                    }

                    function updateReveal(): void {
                        coverRevealAnimation.stop();
                        if (cover.status === Image.Ready && root._coverIsRealArt) {
                            cover.revealOpacity = 0;
                            coverRevealAnimation.restart();
                        } else {
                            cover.revealOpacity = cover.status === Image.Ready ? 1 : 0;
                        }
                    }

                    Component.onCompleted: cover.updateReveal()

                    // Record the decoded cover URL so coverHold can display it
                    // while the next cover async-decodes after a d-pad move.
                    onStatusChanged: {
                        cover.updateReveal();
                        if (status === Image.Ready && root.identity !== "") {
                            root._lastGoodCoverSource = source;
                            root._lastGoodCoverIdentity = root.identity;
                        }
                    }
                }

                Image {
                    id: placeholderIcon

                    objectName: "detailPlaceholderIcon"
                    x: Sizing.center(parent.width, width)
                    y: Sizing.center(parent.height, height)
                    // Size the chip to ~50% of the cover-slot width so it reads
                    // as a modest accent rather than a large placeholder icon.
                    width: Sizing.px(parent.width * 0.5)
                    height: width
                    source: Resources.coverUrl("icons/File", Theme.logoFocusPrimary, Theme.logoFocusSecondary, Theme.logoFocusShadow)
                    sourceSize.width: Sizing.px(width)
                    sourceSize.height: Sizing.px(height)
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                    asynchronous: false
                    // Round 11: no hourglass branch -- while a cover is busy
                    // (pending fetch or still decoding) the slot stays blank
                    // (coverHold/nothing), same as the grid tiles. Only a
                    // *confirmed* no-cover state (empty resolved source, or a
                    // terminal decode error) shows the File chip.
                    visible: !root.detailSuppressed && !root._isSystemCover && !root._coverBusy && !root._hasCurrentHeldCover && (root._coverSource === "" || cover.status === Image.Error)
                }

                // Wordmark fallback for system entries with no curated logo SVG.
                // Mirrors the grid Tile's fitted-text treatment: DemiBold, logo-focus
                // tint, shrinks to fill. It appears only after terminal Image.Error,
                // never during Null/Loading. The File chip above is suppressed for
                // system keys so exactly one fallback can show.
                Text {
                    objectName: "detailLogoWordmark"

                    anchors.fill: parent
                    anchors.margins: Sizing.pctH(1)
                    text: root.title
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontSize(5.8)
                    fontSizeMode: Text.Fit
                    minimumPixelSize: Sizing.fontSize(2.8)
                    font.weight: Font.DemiBold
                    color: Theme.logoFocusPrimary
                    wrapMode: Text.Wrap
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    renderType: Text.NativeRendering
                    visible: root._isSystemCover && cover.status === Image.Error && root.title !== "" && !root.detailSuppressed
                    clip: true
                }
            }
        }

        Image {
            source: Resources.iconUrl("NavLeft", Theme.textPrimary)
            width: Sizing.pctH(4)
            height: width
            sourceSize.width: Sizing.px(width)
            sourceSize.height: Sizing.px(height)
            anchors.left: parent.left
            anchors.verticalCenter: imageSlot.verticalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: root._detailVisible && root.canPreviousImage
        }

        Image {
            source: Resources.iconUrl("NavRight", Theme.textPrimary)
            width: Sizing.pctH(4)
            height: width
            sourceSize.width: Sizing.px(width)
            sourceSize.height: Sizing.px(height)
            anchors.right: parent.right
            anchors.verticalCenter: imageSlot.verticalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: root._detailVisible && root.canNextImage
        }

        Item {
            id: metadataSlot

            x: content.metadataX
            y: content.metadataY
            width: content.metadataWidth
            height: content.metadataHeight
            clip: true

            Item {
                id: metadataInner

                anchors.fill: parent
                anchors.leftMargin: root._horizontalSections ? root._metadataPaddingLeft : root._metadataLeftMargin
                anchors.rightMargin: root._horizontalSections ? root._metadataPaddingRight : root._metadataRightMargin
                anchors.topMargin: root._horizontalSections ? root._metadataPaddingTop : 0
                anchors.bottomMargin: root._horizontalSections ? root._metadataPaddingBottom : 0
                clip: true

                Text {
                    id: titleText

                    objectName: "detailTitleText"
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    text: root.title
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontTitle
                    wrapMode: Text.Wrap
                    maximumLineCount: 3
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    renderType: Text.NativeRendering
                    visible: root._detailVisible && root.showTitle && root.title !== ""
                }

                Item {
                    id: detailBody

                    readonly property int _bodyTopOffset: titleText.visible ? titleText.y + titleText.height + root._titleBottomMargin : 0
                    readonly property bool _bottomAlignedCompactMetadata: !titleText.visible && !root._horizontalSections && root._metadataBottomAligned

                    x: 0
                    y: _bodyTopOffset + (_bottomAlignedCompactMetadata ? 0 : root._metadataTopMargin)
                    width: parent.width
                    height: {
                        if (root._horizontalSections)
                            return Math.max(0, parent.height - y);
                        if (titleText.visible)
                            return Math.max(0, parent.height - y + root._metadataHeightAdjustment);
                        if (_bottomAlignedCompactMetadata)
                            return Math.max(0, Math.min(root._compactMetadataHeight + root._metadataTopMargin, parent.height));
                        return Math.max(0, Math.min(root._compactMetadataHeight, parent.height - y));
                    }
                    clip: true

                    Column {
                        id: tagTable

                        objectName: "detailTagTable"
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: root._metadataBottomAligned && !titleText.visible ? undefined : parent.top
                        anchors.bottom: root._metadataBottomAligned && !titleText.visible ? parent.bottom : undefined
                        spacing: root._tagRowSpacing
                        clip: true
                        visible: root._detailVisible && root._displayRows.length > 0

                        Repeater {
                            model: root._displayRows

                            delegate: Item {
                                id: tagRow

                                required property var modelData

                                width: tagTable.width
                                height: root._tagRowHeight

                                readonly property string label: modelData.label ?? ""
                                readonly property string measureLabel: modelData.measureLabel ?? tagRow.label
                                readonly property string value: modelData.value ?? ""

                                Text {
                                    anchors.left: parent.left
                                    height: parent.height
                                    width: root._labelColumnWidth
                                    text: tagRow.label
                                    color: Theme.textLabel
                                    font.family: Theme.fontUi
                                    font.pixelSize: root._tagTextSize
                                    textFormat: Text.PlainText
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignRight
                                    verticalAlignment: Text.AlignVCenter
                                    renderType: Text.NativeRendering
                                }

                                Text {
                                    anchors.left: parent.left
                                    anchors.leftMargin: root._labelColumnWidth + root._tagLabelGap
                                    anchors.right: parent.right
                                    height: parent.height
                                    text: tagRow.value
                                    // Core data straight from a scraper.
                                    // The label above has always been
                                    // PlainText; the value, which is the
                                    // field that actually carries
                                    // third-party text, did not, so a
                                    // stray `<` or `&` rendered as markup.
                                    textFormat: Text.PlainText
                                    color: Theme.textPrimary
                                    font.family: Theme.fontUi
                                    font.pixelSize: root._tagTextSize
                                    wrapMode: Text.NoWrap
                                    maximumLineCount: 1
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignLeft
                                    verticalAlignment: Text.AlignVCenter
                                    renderType: Text.NativeRendering
                                }
                            }
                        }
                    }
                }
            }
        }

        LoadingIndicator {
            objectName: "detailLoadingIndicator"
            visible: root._emptyPaneLoading && !root.detailSuppressed
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            text: root.loadingText
        }
    }
}
