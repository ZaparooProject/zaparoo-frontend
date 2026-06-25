# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_update_host STATIC)

set_source_files_properties(UpdateTheme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)

# Compatibility facade for QML packaged by the zaparoo-update Cargo crate.
# Update QML should import this module for frontend-owned components whose
# implementation may move over time. It intentionally does not wrap
# Zaparoo.Theme.Sizing: Sizing's integer-pixel helpers are expected to remain a
# stable frontend host API, and forwarding every sizing call would add avoidable
# JS work in update detail-list delegates.
qt_add_qml_module(
    zaparoo_update_host
    URI Zaparoo.Update.Host
    VERSION 1.0
    QML_FILES
        Modal.qml
        TopStatusStrip.qml
        UpdateTheme.qml
    IMPORTS
        Zaparoo.Theme
        Zaparoo.Ui
)

target_link_libraries(
    zaparoo_update_host
    PUBLIC Qt6::Quick zaparoo_ui_componentsplugin zaparoo_ui_themeplugin
    PRIVATE Zaparoo::CompileOptions
)
