// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Reusable modal panel. Four flavors selected by `kind`:
//   "action_error" — title (+ optional body) + one OK button. Caller
//                    wires `accepted` to its dismiss handler.
//   "transient"    — title (+ optional body) + optional Cancel pill, no
//                    accept button. Auto-dismisses via the caller's
//                    failure timer or external signal. The Cancel pill
//                    hides once `failed` flips so the failure flash is
//                    non-interactive.
//   "confirm"      — title + body + two pills (No / Yes). Default focus
//                    is "No", so a stray accept can't trigger the
//                    destructive path. The router calls `handleAction`
//                    to toggle focus and dispatch confirm/cancel.
//   "shell"        — title + caller-provided content slot, no built-in
//                    body or buttons. Used by the first-run, commercial-
//                    notice, and log-upload modals so they share this
//                    chrome instead of hand-rolling their own scrim,
//                    panel, and Column. The consumer places its content
//                    (and any phase-specific buttons) in the default
//                    property slot and owns its own `handleAction` and
//                    dismissal.
//
// All four kinds share the same chrome — `Sizing.radiusMd` panel corners,
// `Theme.bgPanel` fill, dark scrim — so every
// modal in the app reads as the same surface. See `docs/style.md` →
// "Modal chrome".
//
// Pure presentation: input routing for the prebaked kinds lives in
// Main.qml, persistence in Browse.AppState. The component renders,
// swallows clicks on its scrim, and emits `cancelRequested` (transient
// Cancel pill, confirm No / Back), `accepted` (action_error button), or
// `confirmed` (confirm Yes).
//
// Software-rendering safe — only Item, Rectangle, Text, Column, Row,
// MouseArea, and small PressableSurface translations.
Item {
    id: modal

    property bool open: false
    property string kind: "action_error"
    property string title: ""
    property string body: ""                 // optional secondary line
    property string buttonLabel: qsTr("OK")  // action_error only
    property string confirmYesLabel: qsTr("Yes")  // confirm only
    property string confirmNoLabel: qsTr("No")    // confirm only
    property bool failed: false              // transient only
    // Override the panel's max width on a per-callsite basis. The
    // content-heavier shell modals (legal notice, log upload with QR)
    // bump this up; shell consumers with narrower content (e.g.
    // ListPickerModal) bring it down to their own measured content width.
    property int panelMaxWidth: Sizing.pctH(90)
    // Shell content is opaque to Modal by default, so the shell branch below
    // applies its own fixed 78%-of-viewport breathing-room ceiling on top of
    // `panelMaxWidth` (QR/legal notice content can't be measured, so that
    // ceiling is the real cap for them). A shell consumer that measures its
    // own content precisely and hands the exact target through
    // `panelMaxWidth` (ListPickerModal) needs that number honored like the
    // four prebaked kinds do, not clamped a second time — round 6 follow-up:
    // the color-scheme picker's widest rows still truncated post-measurement
    // fix because 78% of the viewport was tighter than the picker's own
    // computed width.
    property bool contentSized: false

    // Content-driven width for the four prebaked kinds (title/body/buttons —
    // content Modal owns and can measure), mirroring ContextMenu.qml's
    // `_widestEntryLabelWidth` pattern. `kind: "shell"` content is opaque to
    // Modal (an arbitrary caller-supplied Item), so it keeps the old
    // percentage-of-viewport sizing instead; a shell caller with narrow,
    // measurable content overrides `panelMaxWidth` itself.
    readonly property int _contentHorizontalMargin: Sizing.pctW(4)
    readonly property int _buttonHorizontalPadding: Sizing.pctW(8)
    readonly property string _cancelLabel: qsTr("Cancel")
    readonly property int _titleWidth: modal.title !== "" ? Math.ceil(_titleMetrics.advanceWidth(modal.title)) : 0
    // 45 characters is the low end of the 45-75 standard readable measure —
    // a long body paragraph wraps onto more lines instead of forcing a
    // full-width panel.
    readonly property int _bodyMaxLineWidth: Math.ceil(_bodyMetrics.averageCharacterWidth * 45)
    readonly property int _bodyWidth: (modal.body !== "" && modal.kind !== "shell") ? Math.min(Math.ceil(_bodyMetrics.advanceWidth(modal.body)), modal._bodyMaxLineWidth) : 0
    readonly property int _buttonsWidth: {
        if (modal.kind === "action_error")
            return Math.ceil(_bodyMetrics.advanceWidth(modal.buttonLabel)) + modal._buttonHorizontalPadding;
        if (modal.kind === "transient")
            return Math.ceil(_bodyMetrics.advanceWidth(modal._cancelLabel)) + modal._buttonHorizontalPadding;
        if (modal.kind === "confirm")
            return Math.ceil(_bodyMetrics.advanceWidth(modal.confirmNoLabel)) + modal._buttonHorizontalPadding + Math.ceil(_bodyMetrics.advanceWidth(modal.confirmYesLabel)) + modal._buttonHorizontalPadding + Sizing.pctW(2);
        return 0;
    }
    readonly property int _desiredPanelWidth: Math.max(modal._titleWidth, modal._bodyWidth, modal._buttonsWidth) + 2 * modal._contentHorizontalMargin
    // Degenerate-case floor only — real prompts size around
    // `_desiredPanelWidth`, which tracks the longer of title/body/buttons.
    readonly property int _minPanelWidth: Sizing.pctW(30)

    // confirm-only focus. False = No focused (safe default), true = Yes
    // focused. Reset on every open so a previous Yes-focus doesn't leak
    // into the next prompt.
    property bool _focusYes: false

    // Physical press for button activation, matching grid tiles.
    // _pressTarget identifies which button is currently lowered; the others
    // stay raised so only the pressed button cues the user's intention.
    property bool _pressed: false
    property string _pressTarget: ""
    property string _pendingSignal: ""

    // Shell-mode content slot. Children declared inside a Modal are
    // routed here; only rendered when kind === "shell" so a stray child
    // on a prebaked-kind modal can't leak into the panel.
    default property alias contentData: contentSlot.data

    signal accepted         // action_error: button click
    signal cancelRequested  // transient Cancel; confirm No / Back
    signal confirmed        // confirm: Yes selected

    visible: modal.open
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        if (!modal.open) {
            // Disarm a pending deferred signal so a press-then-close inside the
            // deferred window cannot emit confirmed/accepted after dismissal.
            actionCommit.stop();
            return;
        }
        if (modal.kind === "confirm")
            modal._focusYes = false;
        modal._pressed = false;
        modal._pressTarget = "";
        modal._pendingSignal = "";
    }

    // Input dispatch. Main.qml routes key/controller actions here while
    // this modal is on top of the stack.
    function handleAction(action: string): void {
        // action_error has a single OK button — accept plays the push cue
        // then emits `accepted`, matching the mouse path so key/controller
        // dismissal animates identically.
        if (modal.kind === "action_error") {
            if (action === "accept")
                modal._commit("ok", "accepted");
            return;
        }
        if (modal.kind !== "confirm")
            return;
        if (action === "left") {
            modal._focusYes = false;
        } else if (action === "right") {
            modal._focusYes = true;
        } else if (action === "accept") {
            if (modal._focusYes)
                modal._commit("yes", "confirmed");
            else
                modal._commit("no", "cancelRequested");
        } else if (action === "cancel") {
            // Back-key dismissal — not an on-screen button, no push-in.
            modal.cancelRequested();
        }
    }

    // Play the push-in cue on the named button, then emit the pending
    // signal deferred so the animation completes before the caller acts.
    function _commit(target: string, sig: string): void {
        modal._pressTarget = target;
        modal._pendingSignal = sig;
        modal._pressed = true;
        actionCommit.arm();
    }

    FontMetrics {
        id: _titleMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontTitle
    }

    FontMetrics {
        id: _bodyMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
    }

    DeferredAction {
        id: actionCommit
        onDeferred: {
            const sig = modal._pendingSignal;
            modal._pendingSignal = "";
            modal._pressed = false;
            if (sig === "accepted")
                modal.accepted();
            else if (sig === "confirmed")
                modal.confirmed();
            else if (sig === "cancelRequested")
                modal.cancelRequested();
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.scrim

        // Eat clicks AND hover on the scrim so they don't reach the
        // screens underneath. Without `hoverEnabled`, mouse-mode hover
        // events fall through to the screen, and the screen's
        // `onHovered` handlers keep moving its `currentIndex` while
        // a modal is on top — focus tracks the cursor under the scrim.
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.AllButtons
        }

        Rectangle {
            objectName: "modalPanel"
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            // `panelMaxWidth` (and the viewport itself) is an unconditional
            // ceiling -- clamped first -- with the desired/floor width
            // applied beneath it, so a caller-supplied cap always wins even
            // when it is tighter than `_minPanelWidth`'s own default floor.
            width: modal.kind === "shell" ? Sizing.px(Math.min(parent.width * (modal.contentSized ? 0.92 : 0.78), modal.panelMaxWidth)) : Sizing.px(Math.min(Math.min(parent.width * 0.92, modal.panelMaxWidth), Math.max(modal._minPanelWidth, modal._desiredPanelWidth)))
            height: contentColumn.height + Sizing.pctH(8)
            color: Theme.bgPanel
            radius: Sizing.radiusMd

            Column {
                id: contentColumn

                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.topMargin: Sizing.pctH(4)
                anchors.leftMargin: Sizing.pctW(4)
                anchors.rightMargin: Sizing.pctW(4)
                spacing: Sizing.pctH(3)

                Text {
                    width: parent.width
                    visible: modal.title !== ""
                    text: modal.title
                    textFormat: Text.PlainText
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontTitle
                    color: Theme.textPrimary
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                    renderType: Text.NativeRendering
                }

                Text {
                    width: parent.width
                    visible: modal.body !== "" && modal.kind !== "shell"
                    text: modal.body
                    textFormat: Text.PlainText
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontBody
                    color: Theme.textPrimary
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                    renderType: Text.NativeRendering
                }

                // Caller content — only rendered in shell mode. Column
                // skips invisible children, so the slot consumes no
                // vertical space outside shell mode.
                Item {
                    id: contentSlot

                    width: parent.width
                    height: childrenRect.height
                    visible: modal.kind === "shell"
                }

                // Cancel pill — transient flavor, hidden once `failed`
                // flips. Failure is a terminal display that auto-dismisses,
                // not interactive.
                Item {
                    id: cancelSlot
                    width: parent.width
                    height: Sizing.pctH(7)
                    visible: modal.kind === "transient" && !modal.failed

                    PressableSurface {
                        x: Sizing.center(parent.width, width)
                        y: Sizing.center(parent.height, height)
                        // Cap at pctW(28) for the typical case but never
                        // exceed the slot width — the modal panel is
                        // height-bound on widescreen, so a screen-width
                        // pill can otherwise overflow the panel.
                        width: Math.min(Sizing.pctW(28), cancelSlot.width)
                        height: parent.height
                        focused: true
                        pressed: modal._pressed && modal._pressTarget === "cancel"
                        pointerAcceptedButtons: Qt.LeftButton
                        onPointerClicked: modal._commit("cancel", "cancelRequested")

                        Text {
                            x: Sizing.center(parent.width, width)
                            y: Sizing.center(parent.height, height)
                            text: modal._cancelLabel
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontBody
                            color: Theme.textPrimary
                            renderType: Text.NativeRendering
                        }
                    }
                }

                // Accept button — action_error flavor.
                Item {
                    id: acceptSlot
                    width: parent.width
                    height: Sizing.pctH(7)
                    visible: modal.kind === "action_error"

                    PressableSurface {
                        x: Sizing.center(parent.width, width)
                        y: Sizing.center(parent.height, height)
                        width: Math.min(Sizing.pctW(28), acceptSlot.width)
                        height: parent.height
                        focused: true
                        pressed: modal._pressed && modal._pressTarget === "ok"
                        pointerAcceptedButtons: Qt.LeftButton
                        onPointerClicked: modal._commit("ok", "accepted")

                        Text {
                            x: Sizing.center(parent.width, width)
                            y: Sizing.center(parent.height, height)
                            text: modal.buttonLabel
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontBody
                            color: Theme.textPrimary
                            renderType: Text.NativeRendering
                        }
                    }
                }

                // No / Yes pair — confirm flavor. Focused pill draws an
                // accent border; mouse clicks bypass focus and dispatch
                // straight to the matching signal.
                Item {
                    id: confirmSlot

                    width: parent.width
                    height: Sizing.pctH(7)
                    visible: modal.kind === "confirm"

                    // Pill width caps at pctW(28) but shrinks to half
                    // the slot (minus the gap) when the panel is too
                    // narrow for two preferred pills. Computed off the
                    // slot, not the Row, so the Row can stay implicitly
                    // sized by its children and centered.
                    readonly property int _gap: Sizing.pctW(2)
                    readonly property int _pillWidth: Math.min(Sizing.pctW(28), Math.max(0, Sizing.px((width - _gap) / 2)))

                    Row {
                        x: Sizing.center(parent.width, width)
                        y: Sizing.center(parent.height, height)
                        spacing: confirmSlot._gap

                        PressableSurface {
                            width: confirmSlot._pillWidth
                            height: Sizing.pctH(7)
                            focused: !modal._focusYes
                            pressed: modal._pressed && modal._pressTarget === "no"
                            pointerAcceptedButtons: Qt.LeftButton
                            onPointerClicked: {
                                modal._focusYes = false;
                                modal._commit("no", "cancelRequested");
                            }

                            Text {
                                x: Sizing.center(parent.width, width)
                                y: Sizing.center(parent.height, height)
                                text: modal.confirmNoLabel
                                font.family: Theme.fontUi
                                font.pixelSize: Sizing.fontBody
                                color: Theme.textPrimary
                                renderType: Text.NativeRendering
                            }
                        }

                        PressableSurface {
                            width: confirmSlot._pillWidth
                            height: Sizing.pctH(7)
                            focused: modal._focusYes
                            pressed: modal._pressed && modal._pressTarget === "yes"
                            pointerAcceptedButtons: Qt.LeftButton
                            onPointerClicked: {
                                modal._focusYes = true;
                                modal._commit("yes", "confirmed");
                            }

                            Text {
                                x: Sizing.center(parent.width, width)
                                y: Sizing.center(parent.height, height)
                                text: modal.confirmYesLabel
                                font.family: Theme.fontUi
                                font.pixelSize: Sizing.fontBody
                                color: Theme.textPrimary
                                renderType: Text.NativeRendering
                            }
                        }
                    }
                }
            }
        }
    }
}
