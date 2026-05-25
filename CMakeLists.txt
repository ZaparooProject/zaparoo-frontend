# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

cmake_minimum_required(VERSION 3.22)

project(
    zaparoo-frontend
    VERSION 1.0.3
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

if(ZAPAROO_DEV)
    add_compile_definitions(ZAPAROO_DEV_BUILD)
endif()

if(ZAPAROO_EMBEDDED)
    add_compile_definitions(ZAPAROO_EMBEDDED_BUILD)
endif()

list(APPEND CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake")
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
