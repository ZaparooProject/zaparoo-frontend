// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so reads of `Browse.BuildInfo`
// fields trip qmllint's "Member can be shadowed" check. Suppress just
// the compiler category file-wide; matches the pattern used in
// CommercialNoticeModal.qml.
// qmllint disable compiler

// About / License screen — static, scrollable info page reachable from
// Settings → About / License. Pure input dispatcher: emits
// `requestSettingsScreen()` on cancel; Up/Down scroll the Flickable.
//
// Build provenance (git commit, build channel, official-build marker)
// will plug into the version line in a follow-up round; for now only
// the hardcoded `Qt.application.version` is shown.
Item {
    id: about

    Component.onCompleted: console.debug("startup/qml component AboutScreen completed")

    // Bound by MainLayout to `root.pendingTransition !== ""`. About is
    // a destination, never a source — kept for parity with the other
    // screens.
    property bool transitioning: false

    signal requestSettingsScreen

    // True when the body Column overflows the Flickable viewport, so
    // the help bar can show the Up/Down scroll cue only when it's
    // actually meaningful. Per the minimal-help-bar policy, hints
    // shouldn't promise a press that no-ops.
    readonly property bool contentOverflows: body.implicitHeight > flickable.height

    // Drive the top/bottom scroll chevrons. The 1-px epsilon swallows
    // sub-pixel rounding so the chevrons don't flicker on exact-fit
    // content.
    readonly property bool _hasContentAbove: flickable.contentY > 1
    readonly property bool _hasContentBelow: flickable.contentY + flickable.height < flickable.contentHeight - 1

    function _scrollBy(delta: int): void {
        const maxY = Math.max(0, flickable.contentHeight - flickable.height);
        flickable.contentY = Math.max(0, Math.min(maxY, flickable.contentY + delta));
    }

    function handleAction(action: string): void {
        if (action === "up")
            about._scrollBy(-Sizing.pctH(8));
        else if (action === "down")
            about._scrollBy(Sizing.pctH(8));
        else if (action === "cancel")
            about.requestSettingsScreen();
    // accept and left/right are no-ops on a static page.
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    TopStatusStrip {
        id: topStrip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom
        height: Sizing.pctH(7)
        title: qsTr("About / License")
        currentPage: 0
        totalPages: 0
        totalText: ""
    }

    // Body lives in a Flickable so the static content can grow past a
    // single screen on MiSTer 240p without dropping off-frame. Width
    // is capped tighter than Settings's pctW(70) — prose reads better
    // at narrow line lengths, and the cap also keeps the logo from
    // having to scale up past its 600px native width on widescreen.
    // Bottom margin clears the help bar (pctH(6)) plus a small gap.
    //
    // Card stays put; the Flickable sits inside the card and content
    // scrolls within it. Putting the Flickable outside the card would
    // scroll the card itself, which reads wrong. Internal padding
    // matches the Settings row recipe (pctW(3) / pctH(3)) so the body
    // text doesn't kiss the card edge.
    Rectangle {
        id: card

        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(parent.width - Sizing.pctW(10), Sizing.pctW(50))
        color: Theme.surfaceCard
        radius: Sizing.radiusMd
        antialiasing: Sizing.cornerAntialiasing
        border.color: Theme.borderMid
        border.width: Sizing.cardBorderWidth

        Flickable {
            id: flickable

            // top/bottomMargin is sized to leave a clear band inside
            // the card for the scroll chevrons to sit outside the
            // scrollable area (chevron pctH(3) + breathing room).
            anchors.fill: parent
            anchors.leftMargin: Sizing.pctW(3)
            anchors.rightMargin: Sizing.pctW(3)
            anchors.topMargin: Sizing.pctH(4)
            anchors.bottomMargin: Sizing.pctH(4)
            contentWidth: width
            contentHeight: body.implicitHeight
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: body

                width: parent.width
                spacing: Sizing.pctH(2)

                // No leading spacer -- `flickable`'s own `topMargin`
                // (pctH(4), above) already reserves the full chevron band
                // (pctH(3) glyph + pctH(0.5) margin) above the scrollable
                // content, so a spacer here only doubled that whitespace.
                // Round 8 added `pctH(2)` of spacer plus the Column's own
                // `pctH(2)` gap before the logo, stacking to `pctH(8)` of
                // total top inset against `pctW(3)` of side inset --
                // visibly unbalanced. Round 9 removed both spacers,
                // leaving `pctH(4)`, the actual minimum the chevrons need.
                //
                // Logo width is capped at a screen-height-relative size so
                // the brand mark stays a header element across 240p →
                // 1080p without ballooning. Uses the same pre-sized ladder
                // as HeaderBar (item 9d) so the decode is close to the
                // painted size instead of always downsampling the 600px
                // master; height is derived from width via the image's
                // intrinsic aspect.
                Image {
                    id: aboutLogo

                    readonly property real _paintedWidth: Math.min(parent.width, Sizing.pctH(35))

                    anchors.horizontalCenter: parent.horizontalCenter
                    source: Resources.logoUrl(aboutLogo._paintedWidth)
                    fillMode: Image.PreserveAspectFit
                    width: aboutLogo._paintedWidth
                    height: Sizing.px(width * 135 / 600)
                    sourceSize.width: Sizing.px(width)
                    sourceSize.height: Sizing.px(height)
                }

                Text {
                    x: Sizing.center(parent.width, width)
                    text: qsTr("Zaparoo Frontend")
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontHero
                    font.weight: Font.Medium
                    renderType: Text.NativeRendering
                }

                Text {
                    x: Sizing.center(parent.width, width)
                    text: qsTr("Version %1 · %2 · %3").arg(Qt.application.version).arg(Browse.BuildInfo.commit).arg(Browse.BuildInfo.channel)
                    color: Theme.textLabel
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontCaption
                    renderType: Text.NativeRendering
                }

                Text {
                    x: Sizing.center(parent.width, width)
                    text: qsTr("Built %1").arg(Browse.BuildInfo.build_date)
                    color: Theme.textLabel
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontSmall
                    renderType: Text.NativeRendering
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: qsTr("Copyright 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.")
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontBody
                    renderType: Text.NativeRendering
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: qsTr("Source available under the PolyForm Noncommercial License 1.0.0. Free for personal, non-commercial use. Commercial use requires a separate license.")
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontBody
                    renderType: Text.NativeRendering
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: qsTr("Commercial licensing: legal@zaparoo.com")
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontBody
                    renderType: Text.NativeRendering
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: qsTr("Project: https://zaparoo.org")
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontBody
                    renderType: Text.NativeRendering
                }

                Text {
                    x: Sizing.center(parent.width, width)
                    text: qsTr("Created by")
                    color: Theme.textLabel
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontCaption
                    renderType: Text.NativeRendering
                }

                // Contributor names are not translated — they're proper
                // names. Each is its own Text item, individually centered
                // via Sizing.center() -- a single multi-line Text centered
                // with AlignHCenter leaves shorter lines on a sub-pixel
                // offset within the block (docs/qml-gotchas.md ->
                // "Integer-pixel rules").
                Column {
                    id: creditsColumn

                    x: Sizing.center(parent.width, width)
                    spacing: 0

                    Repeater {
                        model: ["Andrea Bogazzi", "BossRighteous", "Carlos R.", "devilschile2", "Giancarlo Erra", "José Manuel Barroso Galindo", "Peter Brittain", "Tim Wilsie", "Wilfried Jeanniard", "Wizzo"]

                        Text {
                            required property string modelData

                            x: Sizing.center(creditsColumn.width, width)
                            text: modelData
                            color: Theme.textPrimary
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontBody
                            renderType: Text.NativeRendering
                        }
                    }
                }

                Text {
                    x: Sizing.center(parent.width, width)
                    text: qsTr("Translations")
                    color: Theme.textLabel
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontCaption
                    renderType: Text.NativeRendering
                }

                // Translator names are proper names; one translatable
                // string with embedded newlines so translators localize
                // language labels without touching the credits layout,
                // split into one Text per line so each can be centered
                // individually (same reasoning as creditsColumn above).
                Column {
                    id: translatorsColumn

                    x: Sizing.center(parent.width, width)
                    spacing: 0

                    Repeater {
                        model: qsTr("Italiano - Andrea Bogazzi\nEspañol - Carlos R.\nEuskara - devilschile2\nFrançais - Wilfried").split("\n")

                        Text {
                            required property string modelData

                            x: Sizing.center(translatorsColumn.width, width)
                            text: modelData
                            color: Theme.textPrimary
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontBody
                            renderType: Text.NativeRendering
                        }
                    }
                }

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: qsTr("Full license text in COPYING.")
                    color: Theme.textLabel
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontCaption
                    renderType: Text.NativeRendering
                }
            }
        }

        // Top/bottom scroll chevrons — mirror the PageIndicator recipe
        // (same SVG icons, `PreserveAspectFit` + `smooth: true` +
        // `sourceSize` pinned to the painted size) but centered on the
        // viewport in the card's chrome gap *above* and *below* the
        // Flickable, not inside its visible band. Sitting outside the
        // scrolled area means the chevrons never overlap moving content
        // as the user scrolls. Round 9: dims (Theme.textLabel) rather than
        // hides when the page doesn't overflow in that direction, matching
        // PageIndicator.qml's treatment. Round 10: both also hide entirely
        // when the page doesn't overflow at all (`!contentOverflows`) — a
        // short page shows neither chevron rather than two permanently
        // dim arrows.
        Image {
            source: Resources.iconUrl("ScrollUp", about._hasContentAbove ? Theme.textPrimary : Theme.textLabel)
            width: Sizing.pctH(3)
            height: width
            sourceSize.width: Sizing.px(width)
            sourceSize.height: Sizing.px(height)
            anchors.bottom: flickable.top
            anchors.bottomMargin: Sizing.pctH(0.5)
            anchors.horizontalCenter: flickable.horizontalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: about.contentOverflows
        }

        Image {
            source: Resources.iconUrl("ScrollDown", about._hasContentBelow ? Theme.textPrimary : Theme.textLabel)
            width: Sizing.pctH(3)
            height: width
            sourceSize.width: Sizing.px(width)
            sourceSize.height: Sizing.px(height)
            anchors.top: flickable.bottom
            anchors.topMargin: Sizing.pctH(0.5)
            anchors.horizontalCenter: flickable.horizontalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: about.contentOverflows
        }
    }
}
