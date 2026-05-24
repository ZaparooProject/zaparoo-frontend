# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_executable(tst_ui tst_main.cpp)

qt_add_qml_module(
    tst_ui
    URI ZaparooTest.Ui
    VERSION 1.0
    QML_FILES
        tst_browse_layout_profiles.qml
        tst_smoke.qml
        tst_navigation.qml
        tst_sizing.qml
        tst_persistence.qml
        tst_paged_grid.qml
        tst_focused_media_detail_controller.qml
        tst_list_picker_modal.qml
    IMPORTS
        Zaparoo.App
        Zaparoo.Browse
        Zaparoo.Ui
        Zaparoo.Screens
        Zaparoo.Theme
)

qt_import_qml_plugins(tst_ui)

target_compile_definitions(
    tst_ui
    PRIVATE QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}"
)

target_link_libraries(
    tst_ui
    PRIVATE
        Zaparoo::CompileOptions
        zaparoo_frontend_rs
        zaparoo_ui_appplugin
        zaparoo_ui_componentsplugin
        zaparoo_ui_screensplugin
        zaparoo_ui_themeplugin
        Qt6::QuickTest
        Qt6::Quick
        Qt6::QuickControls2
)

add_test(NAME ui COMMAND tst_ui -input "${CMAKE_CURRENT_SOURCE_DIR}" -platform offscreen)
set_tests_properties(ui PROPERTIES TIMEOUT 30)
