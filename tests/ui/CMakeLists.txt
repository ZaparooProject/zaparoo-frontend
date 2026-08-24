# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

# The tinted-svg provider (and the baked atlas + LUT behind it) is compiled in
# so the QML suite resolves bundled artwork exactly the way the app does. Without
# it every icon request fails and the cover-load tests would be vacuous.
qt_add_executable(
    tst_ui
    tst_main.cpp
    "${CMAKE_SOURCE_DIR}/src/app/baked_icon_atlas.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tint_lut.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tinted_svg_image_provider.cpp"
)
target_include_directories(tst_ui PRIVATE "${CMAKE_SOURCE_DIR}/src/app")

set(_TST_UI_IMPORTS Zaparoo.App Zaparoo.Browse Zaparoo.Ui Zaparoo.Screens Zaparoo.Theme)
set(_TST_UI_QML_FILES
    tst_browse_layout_profiles.qml
    tst_color_schemes.qml
    tst_smoke.qml
    tst_navigation.qml
    tst_resources.qml
    tst_sizing.qml
    tst_persistence.qml
    tst_paged_grid.qml
    tst_footer_chrome.qml
    tst_status_line.qml
    tst_format.qml
    tst_pressable_surface.qml
    tst_context_menu.qml
    tst_modal.qml
    tst_press_cues.qml
    tst_screen_state_overlay.qml
    tst_focused_media_detail_controller.qml
    tst_browse_detail_pane.qml
    tst_media_list_titles.qml
    tst_media_list_paging.qml
    tst_list_picker_modal.qml
    tst_letter_jump_modal.qml
    tst_settings_field_control.qml
    tst_settings_section_header.qml
    tst_motion.qml
    tst_deferred_action.qml
    tst_tile_cover_sync.qml
    tst_game_info_modal.qml
    tst_scrape_setup_modal.qml
    tst_index_setup_modal.qml
)
set(_TST_UI_LINK_LIBRARIES
    Zaparoo::CompileOptions
    zaparoo_frontend_rs
    zaparoo_ui_appplugin
    zaparoo_ui_componentsplugin
    zaparoo_ui_screensplugin
    zaparoo_ui_themeplugin
    Qt6::QuickTest
    Qt6::Quick
    Qt6::QuickControls2
    Qt6::Svg
)
if(ZAPAROO_WITH_UPDATE)
    set(_TST_UPDATE_QML_TEST "${ZAPAROO_UPDATE_PACKAGE_ROOT}/tests/ui/tst_update_screen.qml")
    if(EXISTS "${_TST_UPDATE_QML_TEST}")
        zaparoo_update_configure_qml_test_target(
            _TST_UI_QML_FILES _TST_UI_IMPORTS _TST_UI_LINK_LIBRARIES
        )
        set(_TST_UPDATE_HAS_QML_TESTS ON)
    endif()
    zaparoo_update_runtime_qml_import_path(_tst_update_runtime_qml_import_path)
endif()

qt_add_qml_module(
    tst_ui
    URI
    ZaparooTest.Ui
    VERSION
    1.0
    QML_FILES
    ${_TST_UI_QML_FILES}
    IMPORTS
    ${_TST_UI_IMPORTS}
)

qt_import_qml_plugins(tst_ui)

target_compile_definitions(tst_ui PRIVATE QUICK_TEST_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
if(_tst_update_runtime_qml_import_path)
    target_compile_definitions(
        tst_ui
        PRIVATE ZAPAROO_UPDATE_RUNTIME_QML_IMPORT_PATH=\"${_tst_update_runtime_qml_import_path}\"
    )
endif()

target_link_libraries(
    tst_ui
    PRIVATE ${_TST_UI_LINK_LIBRARIES}
)

add_test(NAME ui COMMAND tst_ui -input "${CMAKE_CURRENT_SOURCE_DIR}" -platform offscreen)
set_tests_properties(ui PROPERTIES TIMEOUT 30)
if(_TST_UPDATE_HAS_QML_TESTS)
    zaparoo_update_add_qml_tests(tst_ui)
endif()
