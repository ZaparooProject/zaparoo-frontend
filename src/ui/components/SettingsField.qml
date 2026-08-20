// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Single row in a `SettingsScreen.qml` form. Label on the left, an
// accessory cluster on the right whose shape is selected by `control`:
//   "picker"   — current value text. Accept opens a list-picker modal.
//   "toggle"   — compact square-cornered on/off control.
//   "action"   — trigger row. Status caption when an operation is in
//                flight; nothing while idle.
//   "navigate" — `›` chevron. Reserved for rows that open another
//                screen (subpages, About / License).
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
    // For `control: "action"` — short live-state string painted on the
    // right ("In progress", "Paused", or "" when idle). The screen
    // owns the binding; the field treats it as a plain caption.
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
    implicitHeight: Sizing.pctH(8)

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

    Text {
        id: labelText

        anchors.left: parent.left
        anchors.leftMargin: Sizing.pctW(2)
        anchors.verticalCenter: parent.verticalCenter
        text: root.label
        // Action rows tint their label accent (unselected) / onAccent
        // (selected) instead of getting a chevron — the "Erase All Content
        // and Settings" pattern. See the chevron Image below.
        color: bar.active ? bar.contentColor : (root.control === "action" ? Theme.accent : Theme.textPrimary)
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
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
        anchors.verticalCenter: parent.verticalCenter
        text: root.value
        color: bar.active ? bar.contentColor : Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        renderType: Text.NativeRendering
    }

    Item {
        id: toggle

        visible: root.control === "toggle"
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(2)
        anchors.verticalCenter: parent.verticalCenter
        // Compact-toggle proportion: width ≈ 1.85 × height keeps the
        // handle's travel close to one diameter on either side without
        // leaving the long rail of empty space the previous
        // pctW(8) (~3.7× height on a 16:9 panel) painted.
        height: Sizing.pctH(3.8)
        width: Sizing.px(height * 1.85)
        readonly property int _knobInset: Sizing.tier === "crt" || Sizing.tier === "240" ? Sizing.cardBorderWidth : 2 * Sizing.cardBorderWidth

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
            color: bar.active ? Theme.accent : Theme.surfaceCard
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

    // Right-side value for `control: "action"`. Carries either a
    // transient run state ("In progress" / "Paused" / "Optimizing")
    // or a persistent idle count ("100,000 indexed"). Styled to match
    // the picker right-text recipe so idle counts read as values, not
    // dimmed chrome. No chevron — chevron is reserved for navigation.
    Text {
        visible: root.control === "action" && root.actionStatus !== ""
        anchors.left: labelText.right
        anchors.leftMargin: Sizing.pctW(2)
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(2)
        anchors.verticalCenter: parent.verticalCenter
        text: root.actionStatus
        color: bar.active ? bar.contentColor : Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        renderType: Text.NativeRendering
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
        anchors.verticalCenter: parent.verticalCenter
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
