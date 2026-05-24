# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_ui_screens STATIC)

set_source_files_properties(
    ScreenManager.qml
    PROPERTIES QT_QML_SINGLETON_TYPE TRUE
)

qt_add_qml_module(
    zaparoo_ui_screens
    URI Zaparoo.Screens
    VERSION 1.0
    QML_FILES
        ScreenManager.qml
        HubScreen.qml
        SystemsScreen.qml
        GamesScreen.qml
        MediaListScreen.qml
        FavoritesScreen.qml
        RecentsScreen.qml
        SettingsScreen.qml
        AboutScreen.qml
    IMPORTS
        Zaparoo.Ui
        Zaparoo.Theme
        Zaparoo.Browse
)

target_link_libraries(
    zaparoo_ui_screens
    PUBLIC
        Qt6::Quick
        zaparoo_ui_componentsplugin
        zaparoo_ui_themeplugin
    PRIVATE Zaparoo::CompileOptions
)
