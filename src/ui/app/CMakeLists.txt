# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

qt_add_library(zaparoo_ui_app STATIC)

# Fonts and images live in resources/ at the repo root. Qt requires that
# absolute-path resource files have a QT_RESOURCE_ALIAS set so the generated
# resource system can give them a stable, relative-looking path.
set(_RESOURCE_ROOT "${CMAKE_SOURCE_DIR}/resources")
set(_APP_RESOURCES
    "${_RESOURCE_ROOT}/fonts/MxPlus_HP_100LX_6x8.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSans.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansArabic.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansDevanagari.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansHebrew.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansJP.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansKR.ttf"
    "${_RESOURCE_ROOT}/fonts/NotoSansTC.ttf"
    "${_RESOURCE_ROOT}/images/logo.png"
    # Tiled circuit-trace background. The .png is pre-rendered from the
    # .svg at the pattern's native 304×304 (rsvg-convert / Inkscape /
    # ImageMagick); shipping both keeps the source available for
    # re-rendering. The .png is what `Image.Tile` consumes — tiled SVG
    # would re-rasterize the source on every paint of the dirty rect.
    "${_RESOURCE_ROOT}/images/bg-circuit.png"
    "${_RESOURCE_ROOT}/images/bg-circuit.svg"
)

# System logos stay PNG (curated from PRO100BYTE/console-logos — see
# src/LICENSES/console-logos-ATTRIBUTION.txt). Categories and status
# icons accept either PNG or SVG so SVGs can be dropped straight in
# without a pre-render step; QtSvg rasterizes once on first load and
# the result is cached in QPixmapCache. Top-level icons (folder, file,
# loading, settings, navigation arrows, D-pad, ...) live under
# resources/images/icons/. Gamepad button glyphs (ButtonA/B/X/Y/L/R) ship
# as PNG under resources/images/buttons/<layout>/.
# Globs use CONFIGURE_DEPENDS so adding/removing a file triggers a re-glob
# on the next build.
file(GLOB _SYSTEM_LOGOS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/systems/*.png")
file(GLOB _CATEGORY_LOGOS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/categories/*.png"
    "${_RESOURCE_ROOT}/images/categories/*.svg")
file(GLOB _STATUS_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/status/*.svg")
file(GLOB _UI_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/icons/*.png"
    "${_RESOURCE_ROOT}/images/icons/*.svg")
file(GLOB _BUTTON_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/buttons/*/*.png")
list(APPEND _APP_RESOURCES
    ${_SYSTEM_LOGOS} ${_CATEGORY_LOGOS} ${_STATUS_ICONS} ${_UI_ICONS}
    ${_BUTTON_ICONS})

foreach(_res IN LISTS _APP_RESOURCES)
    file(RELATIVE_PATH _alias "${_RESOURCE_ROOT}/.." "${_res}")
    set_source_files_properties("${_res}" PROPERTIES QT_RESOURCE_ALIAS "${_alias}")
endforeach()

qt_add_qml_module(
    zaparoo_ui_app
    URI Zaparoo.App
    VERSION 1.0
    QML_FILES
        Main.qml
        MainLayout.qml
    RESOURCES ${_APP_RESOURCES}
    IMPORTS
        Zaparoo.Ui
        Zaparoo.Theme
        Zaparoo.Screens
        Zaparoo.Browse
)

target_link_libraries(
    zaparoo_ui_app
    PUBLIC
        Qt6::Quick
        # Pulls in libQt6Svg so `Image { source: "...svg" }` resolves
        # via the QSvgPlugin image format. For shared Qt the plugin is
        # auto-discovered at runtime; the static MiSTer build wires the
        # plugin import explicitly in cmake/ZaparooRust.cmake.
        Qt6::Svg
        zaparoo_ui_componentsplugin
        zaparoo_ui_screensplugin
        zaparoo_ui_themeplugin
    PRIVATE
        Zaparoo::CompileOptions
)
