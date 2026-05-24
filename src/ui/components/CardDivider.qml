// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property color lineColor: Theme.borderMid

    width: Sizing.stroke(1)

    Rectangle {
        anchors.fill: parent
        color: root.lineColor
    }
}
