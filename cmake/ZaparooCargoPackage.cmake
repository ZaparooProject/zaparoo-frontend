# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

include_guard(GLOBAL)

# Resolve a Cargo package's source root from the workspace lockfile. This lets
# CMake consume non-Rust package assets (QML, icons, CMake helpers) from either a
# workspace path dependency or a crates.io dependency without hardcoding
# rust/<crate-name>/ paths.
function(zaparoo_find_cargo_package_root package_name out_var)
    find_program(_zaparoo_cargo_executable NAMES cargo REQUIRED)

    execute_process(
        COMMAND "${_zaparoo_cargo_executable}" metadata --format-version 1 --locked
                --manifest-path "${CMAKE_SOURCE_DIR}/rust/Cargo.toml"
        WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}/rust"
        OUTPUT_VARIABLE _zaparoo_cargo_metadata
        ERROR_VARIABLE _zaparoo_cargo_metadata_error
        RESULT_VARIABLE _zaparoo_cargo_metadata_result
    )
    if(NOT _zaparoo_cargo_metadata_result EQUAL 0)
        message(
            FATAL_ERROR
                "cargo metadata failed while locating ${package_name}:\n${_zaparoo_cargo_metadata_error}"
        )
    endif()

    string(JSON _zaparoo_package_count LENGTH "${_zaparoo_cargo_metadata}" packages)
    math(EXPR _zaparoo_last_package "${_zaparoo_package_count} - 1")

    set(_zaparoo_manifest_path "")
    foreach(_zaparoo_index RANGE 0 ${_zaparoo_last_package})
        string(JSON _zaparoo_current_name GET "${_zaparoo_cargo_metadata}" packages
               ${_zaparoo_index} name
        )
        if(_zaparoo_current_name STREQUAL package_name)
            string(JSON _zaparoo_manifest_path GET "${_zaparoo_cargo_metadata}" packages
                   ${_zaparoo_index} manifest_path
            )
            break()
        endif()
    endforeach()

    if(_zaparoo_manifest_path STREQUAL "")
        message(
            FATAL_ERROR
                "Cargo package '${package_name}' was not found in cargo metadata. Check rust/Cargo.toml and Cargo.lock."
        )
    endif()

    get_filename_component(_zaparoo_package_root "${_zaparoo_manifest_path}" DIRECTORY)
    set(${out_var} "${_zaparoo_package_root}" PARENT_SCOPE)
endfunction()
