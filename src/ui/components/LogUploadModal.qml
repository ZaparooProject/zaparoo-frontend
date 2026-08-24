// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read trips qmllint's
// "Member can be shadowed" check. Until the schema grows the slot,
// suppress the compiler category file-wide.
// qmllint disable compiler

// Modal shown when the user triggers "Upload log" from Settings. Three
// visual phases driven by `Browse.LogUpload.state`:
//
//   uploading (1) — single line of body copy, no buttons. Cancel via
//                   Escape (router pops the modal; the upload finishes
//                   in the background but the user can no longer see
//                   the result).
//   success   (2) — QR of the URL on top, the URL as plain text below,
//                   and a "Done" button that closes the modal.
//   error     (3) — error message + a "Retry" button that re-fires
//                   `Browse.LogUpload.upload()`.
//
// Routing and the modal stack are owned by `Main.qml`. We emit
// `closeRequested` and trust the router to pop. `handleAction` is the
// input hook called from `Main.qml`'s modal-dispatch branch. Chrome
// (scrim, panel, border, radius, title) comes from the shared `Modal`
// shell so every dialog in the app reads as the same surface.
Item {
    id: modal

    property bool open: false
    property bool _pressed: false
    property int _pendingAcceptPhase: -1

    signal closeRequested

    readonly property int _stateIdle: 0
    readonly property int _stateUploading: 1
    readonly property int _stateSuccess: 2
    readonly property int _stateError: 3

    readonly property int phase: Browse.LogUpload.state

    visible: modal.open
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        if (!modal.open) {
            acceptCommit.stop();
            modal._pressed = false;
            modal._pendingAcceptPhase = -1;
            return;
        }
        modal._pressed = false;
        if (modal.phase === modal._stateIdle)
            Browse.LogUpload.upload();
    }

    // Generate the QR matrix as soon as we have a URL. `Browse.QrCode`
    // is shared across the frontend; the next consumer (a future flow)
    // will overwrite it with their own content, so generate eagerly
    // rather than relying on a previously cached matrix.
    Connections {
        target: Browse.LogUpload
        function onStateChanged(): void {
            if (Browse.LogUpload.state === modal._stateSuccess)
                Browse.QrCode.generate(Browse.LogUpload.url);
        }
    }

    function handleAction(action: string): void {
        if (action === "accept" && (modal.phase === modal._stateSuccess || modal.phase === modal._stateError)) {
            modal._pendingAcceptPhase = modal.phase;
            modal._pressed = true;
            acceptCommit.arm();
        } else if (action === "cancel") {
            modal.closeRequested();
        }
    }

    DeferredAction {
        id: acceptCommit
        onDeferred: {
            const acceptedPhase = modal._pendingAcceptPhase;
            modal._pendingAcceptPhase = -1;
            modal._pressed = false;
            if (acceptedPhase === modal._stateSuccess)
                modal.closeRequested();
            else if (acceptedPhase === modal._stateError)
                Browse.LogUpload.upload();
        }
    }

    Modal {
        id: shell

        open: modal.open
        kind: "shell"
        title: qsTr("Upload log file")
        panelMaxWidth: Sizing.pctH(110)

        Column {
            width: parent.width
            spacing: Sizing.pctH(3)

            Text {
                width: parent.width
                visible: modal.phase === modal._stateUploading || modal.phase === modal._stateIdle
                text: qsTr("Uploading log file. This may take a moment.")
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontBody
                color: Theme.textPrimary
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                renderType: Text.NativeRendering
            }

            // Success block — QR matrix above, URL plain-text below.
            // The QR is a fixed pixel size derived from the matrix size
            // so the modal's panel height stays stable across phases.
            Item {
                id: successBlock

                width: parent.width
                visible: modal.phase === modal._stateSuccess
                height: visible ? qrHolder.height + urlText.height + Sizing.pctH(2) : 0

                QrMatrix {
                    id: qrHolder

                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.top: parent.top
                    maxQrPixels: Math.min(Sizing.pctW(38), Sizing.pctH(54))
                }

                Text {
                    id: urlText

                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.top: qrHolder.bottom
                    anchors.topMargin: Sizing.pctH(2)
                    width: parent.width
                    text: Browse.LogUpload.url
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontSmall
                    color: Theme.textPrimary
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WrapAnywhere
                    renderType: Text.NativeRendering
                }
            }

            Text {
                width: parent.width
                visible: modal.phase === modal._stateError
                text: qsTr("Upload failed. Check the network connection and try again.")
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontCaption
                color: Theme.textPrimary
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                renderType: Text.NativeRendering
            }

            Item {
                width: parent.width
                height: Sizing.pctH(7)
                visible: modal.phase === modal._stateSuccess || modal.phase === modal._stateError

                PressableSurface {
                    x: Sizing.center(parent.width, width)
                    y: Sizing.center(parent.height, height)
                    width: Sizing.pctW(28)
                    height: parent.height
                    focused: true
                    pressed: modal._pressed
                    pointerAcceptedButtons: Qt.LeftButton
                    onPointerClicked: modal.handleAction("accept")

                    Text {
                        x: Sizing.center(parent.width, width)
                        y: Sizing.center(parent.height, height)
                        text: modal.phase === modal._stateError ? qsTr("Retry") : qsTr("Done")
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
