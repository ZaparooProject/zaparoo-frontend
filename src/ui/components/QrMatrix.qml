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

// The QR matrix render shared by QrCodeModal.qml (write-to-token) and
// LogUploadModal.qml (log upload success) — both used to carry a
// near-verbatim copy of this nested-Repeater matrix with hardcoded
// "white"/"black" fills. Extracted (round 6, item 4) when those fills
// became theme roles — see docs/style.md -> "Themed QR codes".
//
// Renders the single shared `Browse.QrCode` matrix at whatever pixel
// budget the caller passes via `maxQrPixels`. Callers must call
// `Browse.QrCode.generate(...)` themselves before opening; this component
// only reads `Browse.QrCode.size` / `Browse.QrCode.rows`.
//
// `rows` must stay a notifying property, never an invokable. Binding a
// delegate's bits to a `row_at(row)` call made the delegate's own index
// the binding's only dependency, so a regenerate that landed on the same
// QR version (same `size`, no `Repeater` rebuild) painted the previous
// payload's matrix forever -- the documentation code leaking into the
// log-upload modal, and one game's "Write with App" code showing for
// another. See qr_code.rs's `rows` doc comment.
//
// `qrLight` (quiet zone + background) and `qrDark` (modules) never invert
// regardless of whether the active preset itself is light or dark —
// inverted QR is out of spec and scans unreliably on a phone camera, which
// is the primary use of this component. Theming comes from both rungs
// riding the accent's own OKLCh hue instead. See `ColorSchemes.qml` ->
// `palette()`'s `qrLight`/`qrDark` comment.
Item {
    id: root

    // Caller's pixel budget for the whole square, quiet zone included.
    required property int maxQrPixels
    property int quietZone: 4

    readonly property int matrixSize: Browse.QrCode.size
    readonly property int moduleSize: root.matrixSize > 0 ? Math.max(1, Math.floor(root.maxQrPixels / (root.matrixSize + root.quietZone * 2))) : 1
    readonly property int qrPixels: root.moduleSize * (root.matrixSize + root.quietZone * 2)

    width: root.qrPixels
    height: root.qrPixels

    Rectangle {
        anchors.fill: parent
        color: Theme.qrLight
        border.width: Sizing.stroke(root.moduleSize * 0.18)
        border.color: Theme.borderSubtle

        Item {
            id: matrix

            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            width: root.moduleSize * root.matrixSize
            height: root.moduleSize * root.matrixSize
            visible: root.matrixSize > 0

            Repeater {
                model: root.matrixSize

                delegate: Item {
                    id: rowDelegate

                    required property int index

                    readonly property int row: index
                    readonly property string bits: Browse.QrCode.rows[rowDelegate.row] ?? ""

                    x: 0
                    y: row * root.moduleSize
                    width: matrix.width
                    height: root.moduleSize

                    Repeater {
                        model: root.matrixSize

                        delegate: Rectangle {
                            required property int index

                            x: index * root.moduleSize
                            y: 0
                            width: root.moduleSize
                            height: root.moduleSize
                            color: Theme.qrDark
                            visible: rowDelegate.bits.charAt(index) === "1"
                        }
                    }
                }
            }
        }
    }
}
