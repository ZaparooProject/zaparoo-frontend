// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Single row in a `SettingsScreen.qml` form. Label on the left, an
// accessory cluster on the right whose shape is selected by `control`:
//   "picker"   — current value text. Accept opens a list-picker modal.
//   "toggle"   — compact square-cornered on/off control.
//   "action"   — trigger row. Centered label, status caption on a second
//                centered line when an operation is in flight; nothing
//                while idle. See docs/style.md's "Inverse-video rows".
//   "navigate" — `›` chevron. Reserved for rows that open another
//                screen (subpages, About / License).
//
// A per-row description string, when the model provides one, is not
// rendered here — `SettingsScreen.qml` reads it directly for the
// screen-level hint band pinned under the settings card (one shared band,
// not a per-row line: see docs/style.md's Settings hint-band note and
// docs/content-style.md's "Adding a setting" checklist).
//
// Rows are lines of text on the section card behind them, not buttons: no
// fill or border at rest, and a selected row inverts to solid accent rather
// than lifting off the page. This is the same `SelectionBar` inverse-video
// vocabulary `BrowseList` uses, so the two lists cannot drift apart — see
// SelectionBar.qml.
//
// The component is purely presentational. The screen owns layout (Column
// stacking + selection index) and value mutation.
Item {
    id: root

    required property string label
    required property string value
    property string control: "picker"
    property bool checked: false
    property bool isFocused: false
    // Set false by SettingsScreen during a page switch so reused delegates
    // do not animate the row flash or toggle position when the new
    // page's model data lands. Restored to true on the next event-loop
    // tick so ordinary user navigation still animates.
    property bool animateChanges: true
    // For `control: "action"` — short live-state string painted on a
    // second centered line below the label ("In progress", "Paused", or
    // "" when idle). The screen owns the binding; the field treats it as
    // a plain caption.
    property string actionStatus: ""
    // Inverse-blink cue, matching the BrowseList vocabulary. The host
    // increments this on accept; the focused row flashes bar/content colors
    // and settles back. Toggle rows are exempt — the knob slide is their
    // feedback — so they ignore it.
    property int activatePulse: 0
    property int _barActivatePulse: 0

    signal hovered
    signal clicked
    signal rightClicked
    // Emitted when the action-control row receives an accept press.
    // The screen wires this to the matching invokable (start/cancel
    // index, start/cancel scrape) and gates by `actionStatus`.
    signal accepted

    // Item.enabled (built-in) gates the MouseArea below; the dimmed
    // opacity here gives a matching visual cue. Setting `enabled: false`
    // on the row makes Accept a no-op (the index/scrape pair use this
    // when one of the two is in flight — Core serialises them).
    opacity: enabled ? 1 : 0.4
    readonly property bool _isAction: root.control === "action"
    // Action rows grow to show a second, centered status line only while
    // there's something to say (a live run state or an idle count) — see
    // the actionStatus Text below and docs/style.md's "Inverse-video
    // rows" note on centered action-row labels.
    readonly property bool _hasActionStatus: root._isAction && root.actionStatus !== ""
    readonly property int _singleLineHeight: Sizing.pctH(8)
    readonly property int _actionStatusBandHeight: Sizing.pctH(3.2)
    implicitHeight: root._hasActionStatus ? root._singleLineHeight + root._actionStatusBandHeight : root._singleLineHeight

    // One-shot inverse blink. Non-toggle rows only; focused row plays it.
    // `animateChanges` is false during a page switch (same gate the field's
    // other animations use), so a reused delegate cannot flash as a new page
    // mounts. Unlike BrowseList (which forwards a host-driven releasePulse),
    // the blink here self-terminates after `Motion.pressMs` — see
    // SelectionBar.qml — so the field just forwards the raw pulse.
    onActivatePulseChanged: {
        if (root.isFocused && root.control !== "toggle" && root.animateChanges)
            root._barActivatePulse++;
    }

    SelectionBar {
        id: bar
        objectName: "selectionBar"
        anchors.fill: parent
        active: root.isFocused
        activatePulse: root._barActivatePulse
    }

    // Fixed at the selected row's weight (Font.Medium — SelectionBar's
    // contentWeight), never the resting Font.Normal, so `labelText.width`
    // below is selection-independent. Round 8's inverse-video weight step
    // made `labelText` render bold when focused with no width of its own;
    // the picker-value Text (below) is anchor-clamped to `labelText.right`,
    // so the label growing bold on selection shrank the value's own
    // available width and elided it, then un-elided it again on blur. The
    // resting weight is narrower or equal, so a row can never overflow
    // this box once selected regardless of `root.label`. Same fixed-slot
    // reasoning as `ContextMenu.qml`'s `panelWidthMetrics`.
    TextMetrics {
        id: labelMetrics
        text: root.label
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        font.weight: Font.Medium
    }

    // Action rows center their label instead of left-aligning it — the
    // one row kind that does, per docs/style.md's "Inverse-video rows"
    // note. Centered via item position (`x`), never
    // `anchors.horizontalCenter` + `AlignHCenter` — see
    // docs/qml-gotchas.md's "Integer-pixel rules". `labelText.width` is
    // fixed to `labelMetrics` above (Math.max(advance, boundingRect) plus
    // `Sizing.stroke(2)` hinting slack, the same idiom
    // `ListPickerModal._measureLabelWidth` uses) rather than its own
    // unconstrained `implicitWidth`, so `Sizing.center()` and the
    // picker-value's left anchor both measure a width that can't change
    // with focus.
    Text {
        id: labelText

        readonly property int _width: Math.ceil(Math.max(labelMetrics.advanceWidth, labelMetrics.boundingRect.width)) + Sizing.stroke(2)

        x: root._isAction ? Sizing.center(parent.width, labelText.width) : Sizing.pctW(2)
        anchors.top: parent.top
        // Pinned toward the top of the taller row when a status line
        // follows, so that line has room below; centered within the
        // single-line band otherwise. Right-side accessories below
        // anchor to `labelText.verticalCenter`, not
        // `parent.verticalCenter`, so they track this same line.
        anchors.topMargin: root._hasActionStatus ? Sizing.pctH(1) : Sizing.center(root._singleLineHeight, labelText.height)
        width: labelText._width
        text: root.label
        // Action rows tint their label accent (unselected) / onAccent
        // (selected) instead of getting a chevron — the "Erase All Content
        // and Settings" pattern. See the chevron Image below.
        color: bar.active ? bar.contentColor : (root._isAction ? Theme.accent : Theme.textPrimary)
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        font.weight: bar.contentWeight
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }

    // Action-row status line — a second centered line under the label,
    // carrying either a transient run state ("In progress" / "Paused") or
    // a persistent idle count ("100,000 indexed"). Same recipe the old
    // per-row description line used (fontCaption / textLabel), just
    // centered under the now-centered label instead of left-aligned full
    // width. See docs/style.md's "Inverse-video rows" note.
    Text {
        id: actionStatusText
        visible: root._hasActionStatus
        x: Sizing.center(parent.width, actionStatusText.width)
        anchors.top: labelText.bottom
        anchors.topMargin: Sizing.pctH(0.3)
        text: root.actionStatus
        color: bar.active ? bar.contentColor : Theme.textLabel
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontCaption
        font.weight: bar.contentWeight
        renderType: Text.NativeRendering
    }

    // Right-side current-value text for `control: "picker"`. Accept on
    // a picker row opens the list-picker modal owned by `Main.qml`;
    // left/right are no-ops (no inline cycling — see `SettingsScreen`).
    //
    // Anchors clamp between the label's right edge and the chevron's left
    // edge so a long localized value (e.g. a translated language name)
    // elides instead of overlapping `labelText` or the chevron.
    Text {
        visible: root.control === "picker"
        anchors.left: labelText.right
        anchors.leftMargin: Sizing.pctW(2)
        anchors.right: navChevron.left
        anchors.rightMargin: Sizing.pctW(2)
        anchors.verticalCenter: labelText.verticalCenter
        text: root.value
        color: bar.active ? bar.contentColor : Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        font.weight: bar.contentWeight
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        renderType: Text.NativeRendering
    }

    Item {
        id: toggle

        visible: root.control === "toggle"
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(2)
        anchors.verticalCenter: labelText.verticalCenter
        // Compact-toggle proportion: width ≈ 1.85 × height keeps the
        // handle's travel close to one diameter on either side without
        // leaving the long rail of empty space the previous
        // pctW(8) (~3.7× height on a 16:9 panel) painted.
        height: Sizing.pctH(3.8)
        width: Sizing.px(height * 1.85)
        readonly property int _knobInset: Sizing.tier === "240" ? Sizing.cardBorderWidth : 2 * Sizing.cardBorderWidth

        Rectangle {
            objectName: "settingsToggleTrack"
            anchors.fill: parent
            radius: Sizing.radiusSm
            // The track alone carries on/off state, at maximum contrast
            // against the row's own current background — `accent`/`borderMid`
            // on the plain card, `onAccent`/`onAccentMuted` once the row's
            // own background is solid accent. See docs/style.md ->
            // "Toggle rows".
            color: bar.active ? (root.checked ? Theme.onAccent : Theme.onAccentMuted) : (root.checked ? Theme.accent : Theme.borderMid)
        }

        Rectangle {
            objectName: "settingsToggleKnob"
            width: toggle.height - 2 * toggle._knobInset
            height: width
            radius: Sizing.radiusSm
            x: root.checked ? toggle.width - width - toggle._knobInset : toggle._knobInset
            y: Sizing.center(toggle.height, height)
            // Always the row's own current background — a hole cut through
            // the track, never a second on/off signal. This alone let the
            // knob disappear into a selected row (round 4's rule made the
            // fill literally equal `Theme.accent`, the same solid color as
            // the row itself). The border below is what round 5 adds: it
            // reuses the track's own "on" color for that register, which
            // the semantic-tier tests already guarantee clears >=4.5:1
            // against this exact fill (test_accent_against_bg_deep_clears_body_text_contrast
            // for the unselected case, test_on_accent_clears_body_text_contrast
            // for the selected one) — so the knob keeps a visible silhouette
            // with no new color derivation needed. See docs/style.md ->
            // "Toggle rows".
            // Must track the bar's own fill, not the raw accent: the point of
            // this fill is to be invisible against the row behind it (see the
            // comment above), and the bar desaturates.
            color: bar.active ? Theme.selectionFill : Theme.surfaceCard
            border.color: bar.active ? Theme.onAccent : Theme.accent
            border.width: Sizing.cardBorderWidth

            Behavior on x {
                enabled: Motion.enabled && root.animateChanges
                NumberAnimation {
                    duration: Motion.dur(Motion.settleMs)
                    easing.type: Easing.OutQuad
                }
            }
        }
    }

    // Right-side chevron. Means "this row opens something else" — a
    // list-picker modal (`control: "picker"`) or another page
    // (`control: "navigate"`, e.g. About / License), per the iOS/Android
    // disclosure-indicator convention. Toggles get nothing (the switch
    // itself is the affordance) and action rows get an accent-tinted label
    // instead (see `labelText` above) — a chevron there would promise
    // navigation that does not happen.
    Image {
        id: navChevron
        visible: root.control === "picker" || root.control === "navigate"
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(2)
        anchors.verticalCenter: labelText.verticalCenter
        source: Resources.iconUrl("NavRight", bar.active ? bar.contentColor : Theme.textPrimary)
        width: Sizing.pctH(3.5)
        height: width
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        fillMode: Image.PreserveAspectFit
        smooth: true
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.PointingHandCursor
        onEntered: root.hovered()
        // Action rows fire `accepted()` (the screen runs start/cancel
        // there); every other control fires `clicked()` (the screen
        // moves focus and toggles a value). Emitting both for action
        // rows used to make `onClicked` and `onAccepted` race over
        // the same press.
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                root.rightClicked();
            else if (root.control === "action" || root.control === "navigate" || root.control === "picker")
                root.accepted();
            else
                root.clicked();
        }
    }
}
