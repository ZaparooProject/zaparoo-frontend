# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

# Homebrew Qt ships a Qt-specific FindRust.cmake that only finds rustc. Corrosion requires its
# bundled module because it also defines Rust::Cargo and target-triple metadata. Keep this
# project-level shim ahead of Qt's module path so Corrosion always gets the API shape it expects.
include("${CMAKE_BINARY_DIR}/_deps/corrosion-src/cmake/FindRust.cmake")
