# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Developer lint targets. Include this AFTER all add_subdirectory() calls so
# that the Qt-generated all_qmllint target already exists.
#
# Targets:
#   format-check  — clang-format dry-run on all first-party C++
#   tidy          — clang-tidy static analysis (requires compile_commands.json)
#   lint          — aggregate: format-check + tidy + all_qmllint

include_guard(GLOBAL)

file(
    GLOB_RECURSE
    _ZAPAROO_CXX_SOURCES
    CONFIGURE_DEPENDS
    "${CMAKE_SOURCE_DIR}/src/*.cpp"
    "${CMAKE_SOURCE_DIR}/src/*.h"
    "${CMAKE_SOURCE_DIR}/tests/*.cpp"
    "${CMAKE_SOURCE_DIR}/tests/*.h"
)

# Translation units only. run-clang-tidy treats its positional args as regex filters against
# compile_commands.json, which never contains headers — so passing headers here produces unmatched
# filter strings. Headers still get analysed, just indirectly via the TUs that #include them.
file(GLOB_RECURSE _ZAPAROO_CXX_TIDY_SOURCES CONFIGURE_DEPENDS "${CMAKE_SOURCE_DIR}/src/*.cpp"
     "${CMAKE_SOURCE_DIR}/tests/*.cpp"
)

# ── clang-format check ────────────────────────────────────────────────────────

find_program(CLANG_FORMAT_EXE NAMES clang-format)

if(CLANG_FORMAT_EXE)
    add_custom_target(
        format-check
        COMMAND ${CLANG_FORMAT_EXE} --dry-run -Werror ${_ZAPAROO_CXX_SOURCES}
        WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
        COMMENT "clang-format: checking C++ style"
        VERBATIM
    )
else()
    message(STATUS "clang-format not found — format-check target is a no-op")
    add_custom_target(
        format-check COMMAND ${CMAKE_COMMAND} -E echo "clang-format not found; skipping"
    )
endif()

# ── clang-tidy ──────────────────────────────────────────────────────────────── Prefers
# run-clang-tidy (parallel, reads compile_commands.json automatically). Falls back to plain
# clang-tidy with -p flag.

find_program(RUN_CLANG_TIDY_EXE NAMES run-clang-tidy run-clang-tidy.py)
find_program(CLANG_TIDY_EXE NAMES clang-tidy)

if(RUN_CLANG_TIDY_EXE)
    # Pass first-party sources as positional args rather than `-source-filter` so we work with the
    # older run-clang-tidy shipped in Ubuntu (noble has clang-tidy 18 but its run-clang-tidy rejects
    # `-source-filter`, which only stabilized across distros in clang 19+).
    add_custom_target(
        tidy
        COMMAND ${RUN_CLANG_TIDY_EXE} -p "${CMAKE_BINARY_DIR}" ${_ZAPAROO_CXX_TIDY_SOURCES}
        WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
        COMMENT "clang-tidy: static analysis (parallel via run-clang-tidy)"
        VERBATIM
    )
elseif(CLANG_TIDY_EXE)
    add_custom_target(
        tidy
        COMMAND ${CLANG_TIDY_EXE} -p "${CMAKE_BINARY_DIR}" ${_ZAPAROO_CXX_TIDY_SOURCES}
        WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
        COMMENT "clang-tidy: static analysis"
        VERBATIM
    )
else()
    message(STATUS "clang-tidy not found — tidy target is a no-op")
    add_custom_target(tidy COMMAND ${CMAKE_COMMAND} -E echo "clang-tidy not found; skipping")
endif()

# ── lint aggregate ────────────────────────────────────────────────────────────

add_custom_target(lint COMMENT "Running all linters (format-check + tidy + qmllint)")
add_dependencies(lint format-check tidy)

if(TARGET all_qmllint)
    add_dependencies(lint all_qmllint)
    # qmllint must see cxx-qt-generated qmldir + plugin.qmltypes under the Qt QML output root;
    # otherwise Rust-backed singletons resolve as [unresolved-type]. The aggregate all_qmllint
    # target lists per-module qmllint targets as siblings — ninja treats the dep list as unordered
    # and will race qmllint against the sync unless each per-module target depends on the sync
    # individually.
    if(TARGET zaparoo_cxxqt_qml_sync)
        add_dependencies(all_qmllint zaparoo_cxxqt_qml_sync)
        foreach(_qmllint_target IN
                ITEMS zaparoo_ui_app_qmllint zaparoo_ui_components_qmllint
                      zaparoo_ui_screens_qmllint zaparoo_ui_theme_qmllint
                      zaparoo_update_qml_qmllint tst_ui_qmllint
        )
            if(TARGET ${_qmllint_target})
                add_dependencies(${_qmllint_target} zaparoo_cxxqt_qml_sync)
            endif()
        endforeach()
    endif()
endif()
