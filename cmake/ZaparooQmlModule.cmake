# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# zaparoo_add_qml_module(target URI <uri> QML_FILES ... [RESOURCES ...] [IMPORTS ...])
#
# Thin wrapper around qt_add_qml_module that enforces project-wide defaults:
# - VERSION 1.0 always
# - Library modules are always STATIC
# - Inherits Zaparoo::CompileOptions
#
# The caller must separately call target_link_libraries to wire up
# the module's plugin (e.g. target PRIVATE zaparoo_ui_appplugin).

include_guard(GLOBAL)

function(zaparoo_add_qml_module target)
    cmake_parse_arguments(_ARG "" "URI" "QML_FILES;RESOURCES;IMPORTS;SOURCES" ${ARGN})

    if(NOT _ARG_URI)
        message(FATAL_ERROR "zaparoo_add_qml_module: URI is required")
    endif()

    qt_add_qml_module(
        ${target}
        URI
        ${_ARG_URI}
        VERSION
        1.0
        STATIC
        QML_FILES
        ${_ARG_QML_FILES}
        RESOURCES
        ${_ARG_RESOURCES}
        IMPORTS
        ${_ARG_IMPORTS}
        SOURCES
        ${_ARG_SOURCES}
    )

    target_link_libraries(${target} PRIVATE Zaparoo::CompileOptions)
endfunction()
