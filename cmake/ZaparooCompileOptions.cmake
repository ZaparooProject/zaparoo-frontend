# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Centralised warning flags, sanitizer options, and LTO for all targets.

include_guard(GLOBAL)

option(ZAPAROO_ENABLE_ASAN "Enable AddressSanitizer (Debug builds only)" OFF)
option(ZAPAROO_ENABLE_UBSAN "Enable UndefinedBehaviourSanitizer (Debug builds only)" OFF)

# Interface library — link targets to this to inherit the compile options.
add_library(zaparoo_compile_options INTERFACE)
add_library(Zaparoo::CompileOptions ALIAS zaparoo_compile_options)

if(MSVC)
    target_compile_options(
        zaparoo_compile_options
        INTERFACE /W4 /permissive- /w14640 # thread-unsafe static member init
                  /wd4068 # unknown pragma (Qt emits these)
    )
else()
    target_compile_options(
        zaparoo_compile_options
        INTERFACE -Wall
                  -Wextra
                  -Wpedantic
                  -Wshadow
                  -Wnon-virtual-dtor
                  -Wnull-dereference
                  -Woverloaded-virtual
                  -Wcast-align
                  -Wunused
                  # Suppress warnings from Qt-generated MOC code
                  -Wno-redundant-decls
                  # Qt's qCDebug/qCWarning macros use empty __VA_ARGS__ which is a C++20 extension;
                  # suppress the pedantic diagnostic it triggers.
                  -Wno-gnu-zero-variadic-macro-arguments
    )
endif()

# Sanitizers — Debug only, non-MSVC
if(NOT MSVC)
    if(ZAPAROO_ENABLE_ASAN)
        target_compile_options(
            zaparoo_compile_options INTERFACE $<$<CONFIG:Debug>:-fsanitize=address
                                              -fno-omit-frame-pointer>
        )
        target_link_options(zaparoo_compile_options INTERFACE $<$<CONFIG:Debug>:-fsanitize=address>)
    endif()

    if(ZAPAROO_ENABLE_UBSAN)
        target_compile_options(
            zaparoo_compile_options INTERFACE $<$<CONFIG:Debug>:-fsanitize=undefined>
        )
        target_link_options(
            zaparoo_compile_options INTERFACE $<$<CONFIG:Debug>:-fsanitize=undefined>
        )
    endif()
endif()

# LTO for Release builds where supported
include(CheckIPOSupported)
check_ipo_supported(RESULT _ipo_supported OUTPUT _ipo_output)
if(_ipo_supported)
    set_target_properties(
        zaparoo_compile_options PROPERTIES INTERFACE_INTERPROCEDURAL_OPTIMIZATION_RELEASE ON
    )
endif()
