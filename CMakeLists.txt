# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

cmake_minimum_required(VERSION 3.22)

project(
    zaparoo-frontend
    VERSION 1.1.1
    DESCRIPTION "Zaparoo Core frontend"
    HOMEPAGE_URL "https://zaparoo.org"
    LANGUAGES CXX
)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Dynamic Qt linking required for LGPL compliance on desktop. The ARM32 Docker build overrides this
# via -DBUILD_SHARED_LIBS=OFF in the toolchain.
option(BUILD_SHARED_LIBS "Build shared libraries (required for LGPL desktop builds)" ON)
option(ZAPAROO_BUILD_TESTS "Build unit/smoke tests" ON)
option(ZAPAROO_DEV "Development mode: windowed window and dev core endpoint" OFF)
option(ZAPAROO_EMBEDDED "Embedded device build (MiSTer): default fullscreen, no window chrome" OFF)
option(ZAPAROO_WITH_UPDATE "Build with zaparoo-update integration" ON)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake")

if(ZAPAROO_DEV)
    add_compile_definitions(ZAPAROO_DEV_BUILD)
endif()

if(ZAPAROO_EMBEDDED)
    add_compile_definitions(ZAPAROO_EMBEDDED_BUILD)
endif()

# Compiler cache for all C++ TUs (first-party, moc, qmlcachegen). Found at configure time rather
# than pinned in presets so contributors without ccache still build; an explicit
# -DCMAKE_CXX_COMPILER_LAUNCHER wins.
find_program(CCACHE_PROGRAM ccache)
if(CCACHE_PROGRAM AND NOT CMAKE_CXX_COMPILER_LAUNCHER)
    set(CMAKE_CXX_COMPILER_LAUNCHER "${CCACHE_PROGRAM}")
endif()

message(STATUS "Update integration: ${ZAPAROO_WITH_UPDATE}")

if(ZAPAROO_WITH_UPDATE)
    add_compile_definitions(ZAPAROO_UPDATE_ENABLED)
endif()

include(ZaparooCompileOptions)
include(ZaparooQmlModule)

# Always emit compile_commands.json so clangd and clang-tidy work without extra flags.
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Qt6 6.10 REQUIRED COMPONENTS Quick QuickControls2 Qml Svg)
find_package(Qt6LinguistTools 6.10 REQUIRED)

if(ZAPAROO_BUILD_TESTS)
    find_package(Qt6 6.10 REQUIRED COMPONENTS QuickTest Test)
endif()

qt_standard_project_setup(REQUIRES 6.10)

# Place all QML module outputs in a predictable location so qmllint and Qt Creator's code model can
# find them. See: https://www.basyskom.de/en/how-to-use-modern-qml-tooling-in-practice/
set(QT_QML_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/qml")
set(QML_IMPORT_PATH
    "${QT_QML_OUTPUT_DIRECTORY}"
    CACHE PATH "Extra QML import paths for IDE"
)

add_subdirectory(src/ui/theme)
add_subdirectory(src/ui/components)
if(ZAPAROO_WITH_UPDATE)
    add_subdirectory(src/ui/update_host)
    set(
        ZAPAROO_UPDATE_PACKAGE_ROOT
        ""
        CACHE PATH "Optional path to the zaparoo-update Cargo package root. Empty resolves via cargo metadata."
    )
    if(NOT ZAPAROO_UPDATE_PACKAGE_ROOT)
        include(ZaparooCargoPackage)
        zaparoo_find_cargo_package_root("zaparoo-update" ZAPAROO_UPDATE_PACKAGE_ROOT)
    endif()
    if(NOT EXISTS "${ZAPAROO_UPDATE_PACKAGE_ROOT}/cmake/ZaparooUpdate.cmake")
        message(
            FATAL_ERROR
                "zaparoo-update package at '${ZAPAROO_UPDATE_PACKAGE_ROOT}' does not provide cmake/ZaparooUpdate.cmake"
        )
    endif()
    include("${ZAPAROO_UPDATE_PACKAGE_ROOT}/cmake/ZaparooUpdate.cmake")
    zaparoo_update_add_qml_module()
endif()
add_subdirectory(src/ui/screens)
add_subdirectory(src/ui/app)

# All Zaparoo QML CMake targets must exist before ZaparooRust is included.
include(ZaparooRust)

if(ZAPAROO_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

# Lint targets — included last so all_qmllint already exists.
include(ZaparooLint)
