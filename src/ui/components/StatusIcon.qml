// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property alias source: icon.source
    property string name: ""

    objectName: root.name
    width: root.visible ? Sizing.fontSize(2.4) : 0
    height: width

    Image {
        id: icon

        anchors.fill: parent
        fillMode: Image.PreserveAspectFit
        sourceSize.width: root.width
        sourceSize.height: root.height
        smooth: true
        cache: true
    }
}
