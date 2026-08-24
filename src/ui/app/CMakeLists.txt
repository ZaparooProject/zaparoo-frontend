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
)

# System logos are black/white SVGs (curated from PRO100BYTE/console-logos — see
# src/LICENSES/console-logos-ATTRIBUTION.txt) tinted at load time by the
# tinted-svg image provider by default. Optional full-color PNG system logos live
# under resources/images/systems-color/ and are selected by Settings. Categories
# and status icons accept either PNG or SVG
# so SVGs can be dropped straight in without a pre-render step. Bundled system,
# category, and icon SVGs are pre-rasterized into resources/baked/icons.zbin by
# `just bake-icons`; anything not in that atlas falls back to rasterizing through
# QtSvg on first load. Top-level icons
# (folder, file, loading, settings, navigation arrows, ...) live under
# resources/images/icons/. Gamepad button and D-pad glyphs ship as
# per-style SVG under resources/images/buttons/<id>/ (the same neutral
# style_a/b/c/d ids as the "Style A/B/C/D" picker, plus style_e for
# keyboard), named by physical position (FaceEast/South/North/West,
# ShoulderL/R, Dpad/Up/Down/Left/Right) -- every glyph in the help bar
# comes from the same style, so nothing looks mismatched.
# Globs use CONFIGURE_DEPENDS so adding/removing a file triggers a re-glob
# on the next build.
file(GLOB _HEADER_LOGO CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/logo/*.png")
file(GLOB _SYSTEM_LOGOS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/systems/*.svg")
file(GLOB _SYSTEM_COLOR_LOGOS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/systems-color/*.png")
file(GLOB _CATEGORY_LOGOS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/categories/*.png"
    "${_RESOURCE_ROOT}/images/categories/*.svg")
file(GLOB _STATUS_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/status/*.svg")
file(GLOB _UI_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/icons/*.png"
    "${_RESOURCE_ROOT}/images/icons/*.svg")
file(GLOB _BUTTON_ICONS CONFIGURE_DEPENDS
    "${_RESOURCE_ROOT}/images/buttons/*/*.svg")
list(APPEND _APP_RESOURCES
    ${_HEADER_LOGO} ${_SYSTEM_LOGOS} ${_SYSTEM_COLOR_LOGOS} ${_CATEGORY_LOGOS} ${_STATUS_ICONS}
    ${_UI_ICONS} ${_BUTTON_ICONS})

foreach(_res IN LISTS _APP_RESOURCES)
    file(RELATIVE_PATH _alias "${_RESOURCE_ROOT}/.." "${_res}")
    set_source_files_properties("${_res}" PROPERTIES QT_RESOURCE_ALIAS "${_alias}")
endforeach()

set(_APP_IMPORTS Zaparoo.Ui Zaparoo.Theme Zaparoo.Screens Zaparoo.Browse)
set(_APP_QML_FILES)
set(
    _APP_LINK_LIBRARIES
    Qt6::Quick
    # Pulls in libQt6Svg so `Image { source: "...svg" }` resolves
    # via the QSvgPlugin image format. For shared Qt the plugin is
    # auto-discovered at runtime; the static MiSTer build wires the
    # plugin import explicitly in cmake/ZaparooRust.cmake.
    Qt6::Svg
    zaparoo_ui_componentsplugin
    zaparoo_ui_screensplugin
    zaparoo_ui_themeplugin
)
if(ZAPAROO_WITH_UPDATE)
    list(APPEND _APP_LINK_LIBRARIES zaparoo_update_qmlplugin)
    list(APPEND _APP_QML_FILES UpdateEntry.qml)
endif()

qt_add_qml_module(
    zaparoo_ui_app
    URI Zaparoo.App
    VERSION 1.0
    QML_FILES
        Main.qml
        MainLayout.qml
        ${_APP_QML_FILES}
    RESOURCES ${_APP_RESOURCES}
    IMPORTS ${_APP_IMPORTS}
)

# The baked icon atlas is registered separately from _APP_RESOURCES purely so
# it can carry -no-compress. rcc would otherwise zstd it (alpha masks compress
# well), and QResource::data() would then heap-inflate all 6 MB on first touch
# -- exactly the allocation the atlas exists to delete. Uncompressed, rcc emits
# it verbatim into .rodata, which is a MAP_PRIVATE file-backed range of the
# executable: the runtime reads mask bytes straight out of mapped memory and
# the kernel can drop those clean pages for free under pressure.
# BakedIconAtlas asserts NoCompression at startup because a silent regression
# here has no visible symptom.
#
# BIG_RESOURCES makes rcc emit an object file instead of a multi-megabyte
# generated C hex array, which the ARM32 cross compiler would grind on.
#
# Attached to zaparoo_ui_app rather than to frontend so tst_ui gets the atlas
# too -- it links zaparoo_ui_appplugin.
set(_BAKED_ATLAS "${CMAKE_SOURCE_DIR}/resources/baked/icons.zbin")
set_source_files_properties(
    "${_BAKED_ATLAS}" PROPERTIES QT_RESOURCE_ALIAS "resources/baked/icons.zbin"
)
qt_add_resources(
    zaparoo_ui_app
    "zaparoo_baked_icons"
    PREFIX
    "/qt/qml/Zaparoo/App"
    FILES
    "${_BAKED_ATLAS}"
    OPTIONS
    -no-compress
    BIG_RESOURCES
)

target_link_libraries(
    zaparoo_ui_app
    PUBLIC ${_APP_LINK_LIBRARIES}
    PRIVATE
        Zaparoo::CompileOptions
)
