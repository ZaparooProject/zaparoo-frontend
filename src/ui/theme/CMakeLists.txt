# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_ui_theme STATIC)

set_source_files_properties(
    BrowseLayouts.qml
    Resources.qml
    Sizing.qml
    Theme.qml
    PROPERTIES QT_QML_SINGLETON_TYPE TRUE
)

qt_add_qml_module(
    zaparoo_ui_theme
    URI Zaparoo.Theme
    VERSION 1.0
    QML_FILES
        BrowseLayouts.qml
        Resources.qml
        Sizing.qml
        Theme.qml
)

target_link_libraries(
    zaparoo_ui_theme
    PUBLIC Qt6::Quick
    PRIVATE Zaparoo::CompileOptions
)
