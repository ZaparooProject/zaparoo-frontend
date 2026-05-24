// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for plain reads either, so any
// `Browse.<Singleton>.<prop>` access trips the "Member can be shadowed"
// check. Suppress at file level the same way every other QML file in
// the tree does.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.App
import Zaparoo.Browse as Browse

TestCase {
    function test_window_loads() {
        verify(mainWindow.visible, "Main window should be visible");
        compare(mainWindow.title, "Zaparoo Frontend");
    }

    function test_initial_state() {
        compare(mainWindow.activeScreen, "hub");
    }

    function test_system_status_properties_exist() {
        compare(typeof Browse.SystemStatus.has_nfc, "boolean");
        compare(typeof Browse.SystemStatus.has_wifi_internet, "boolean");
        compare(typeof Browse.SystemStatus.has_lan_internet, "boolean");
        compare(typeof Browse.SystemStatus.has_bluetooth, "boolean");
    }

    name: "UiWindow"
    when: windowShown

    Main {
        id: mainWindow

        fullScreen: false
        width: 1280
        height: 720
    }
}
