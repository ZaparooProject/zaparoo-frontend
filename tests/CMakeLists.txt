# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

add_subdirectory(ui)

add_executable(
    frontend_arguments_test
    tst_frontend_arguments.cpp
    "${CMAKE_SOURCE_DIR}/src/app/frontend_arguments.cpp"
)
target_include_directories(frontend_arguments_test PRIVATE "${CMAKE_SOURCE_DIR}/src/app")
target_link_libraries(frontend_arguments_test PRIVATE Zaparoo::CompileOptions)
add_test(NAME frontend_arguments COMMAND frontend_arguments_test)

# Byte-exact lock on the tritone tint. Needs QtGui for QImage/QColor but not
# QtSvg, so it runs anywhere and is deterministic across Qt versions.
add_executable(tint_lut_test tst_tint_lut.cpp "${CMAKE_SOURCE_DIR}/src/app/tint_lut.cpp")
target_include_directories(tint_lut_test PRIVATE "${CMAKE_SOURCE_DIR}/src/app")
target_link_libraries(tint_lut_test PRIVATE Zaparoo::CompileOptions Qt6::Gui)
add_test(NAME tint_lut COMMAND tint_lut_test)
set_tests_properties(tint_lut PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

# Parity lock between the committed atlas and the SVG path it replaced. Links
# zaparoo_ui_appplugin for the app's own qrc registration -- registering a
# private copy here would prove nothing about how the shipped binary stores it.
add_executable(
    baked_icon_atlas_test
    tst_baked_icon_atlas.cpp
    "${CMAKE_SOURCE_DIR}/src/app/baked_icon_atlas.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tint_lut.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tinted_svg_image_provider.cpp"
)
target_include_directories(baked_icon_atlas_test PRIVATE "${CMAKE_SOURCE_DIR}/src/app")
target_link_libraries(
    baked_icon_atlas_test
    PRIVATE
        Zaparoo::CompileOptions
        zaparoo_frontend_rs
        zaparoo_ui_appplugin
        Qt6::Quick
        Qt6::Svg
)
add_test(NAME baked_icon_atlas COMMAND baked_icon_atlas_test)
set_tests_properties(baked_icon_atlas PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

# Coverage and byte-exact lock for ContextMenu's baked corner masks (Part 5).
# Needs Qt6::Qml directly (QQmlEngine/QQmlComponent) to render a fresh
# rounded-rect comparison rather than going through the app's own engine.
add_executable(
    corner_masks_test
    tst_corner_masks.cpp
    "${CMAKE_SOURCE_DIR}/src/app/baked_icon_atlas.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tint_lut.cpp"
    "${CMAKE_SOURCE_DIR}/src/app/tinted_svg_image_provider.cpp"
)
target_include_directories(corner_masks_test PRIVATE "${CMAKE_SOURCE_DIR}/src/app")
target_link_libraries(
    corner_masks_test
    PRIVATE
        Zaparoo::CompileOptions
        zaparoo_frontend_rs
        zaparoo_ui_appplugin
        Qt6::Quick
        Qt6::Qml
        Qt6::Svg
)
add_test(NAME corner_masks COMMAND corner_masks_test)
set_tests_properties(corner_masks PROPERTIES ENVIRONMENT "QT_QPA_PLATFORM=offscreen")

if(NOT CMAKE_CROSSCOMPILING)
    set(_VERSION_CHECK_SCRIPT "${CMAKE_CURRENT_SOURCE_DIR}/check_frontend_version.cmake")
    add_test(
        NAME frontend_version
        COMMAND
            "${CMAKE_COMMAND}" -DFRONTEND=$<TARGET_FILE:frontend>
            -DEXPECTED=${CMAKE_PROJECT_VERSION} -DARG1=--version -P "${_VERSION_CHECK_SCRIPT}"
    )
    add_test(
        NAME frontend_version_short
        COMMAND
            "${CMAKE_COMMAND}" -DFRONTEND=$<TARGET_FILE:frontend>
            -DEXPECTED=${CMAKE_PROJECT_VERSION} -DARG1=-v -P "${_VERSION_CHECK_SCRIPT}"
    )
    add_test(
        NAME frontend_version_with_crt
        COMMAND
            "${CMAKE_COMMAND}" -DFRONTEND=$<TARGET_FILE:frontend>
            -DEXPECTED=${CMAKE_PROJECT_VERSION} -DARG1=--crt -DARG2=--version -P
            "${_VERSION_CHECK_SCRIPT}"
    )
endif()
