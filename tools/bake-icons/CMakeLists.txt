# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Standalone project, deliberately never added by the root CMakeLists.txt. The
# ARM32 Docker context does not even COPY tools/, so this cannot affect
# `just build`, `just arm32`, `just arm64`, or CI. Configure it with
# `just bake-icons`, which drives it inside the pinned lint image so the
# rasterization matches what the strict parity test expects.

cmake_minimum_required(VERSION 3.21)
project(zaparoo_bake_icons LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Gui Qml Quick Svg)

get_filename_component(_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)

add_executable(bake-icons main.cpp "${_REPO_ROOT}/src/app/tint_lut.cpp")
target_include_directories(bake-icons PRIVATE "${_REPO_ROOT}/src/app")
target_link_libraries(bake-icons PRIVATE Qt6::Gui Qt6::Qml Qt6::Quick Qt6::Svg)
