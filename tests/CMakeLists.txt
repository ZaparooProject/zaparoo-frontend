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
