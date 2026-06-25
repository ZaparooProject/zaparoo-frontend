// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use cxx_qt_build::{CxxQtBuilder, QmlModule};

const MODEL_FILES: &[&str] = &[
    "src/models/alternate_versions.rs",
    "src/models/categories.rs",
    "src/models/crt_video.rs",
    "src/models/systems.rs",
    "src/models/game_info.rs",
    "src/models/games.rs",
    "src/models/favorites.rs",
    "src/models/browse.rs",
    "src/models/app_state.rs",
    "src/models/build_info.rs",
    "src/models/app_status.rs",
    "src/models/hub_state.rs",
    "src/models/image_overrides.rs",
    "src/models/systems_state.rs",
    "src/models/games_state.rs",
    "src/models/favorites_state.rs",
    "src/models/input.rs",
    "src/models/log_upload.rs",
    "src/models/media_status.rs",
    "src/models/notice.rs",
    "src/models/platform.rs",
    "src/models/qr_code.rs",
    "src/models/recents.rs",
    "src/models/recents_state.rs",
    "src/models/runtime.rs",
    "src/models/settings.rs",
    "src/models/system_launchers.rs",
    "src/models/system_status.rs",
];

fn main() {
    println!("cargo:rerun-if-env-changed=ZAPAROO_CARGO_CHEF");
    if std::env::var_os("ZAPAROO_CARGO_CHEF").is_some() {
        // cargo-chef builds a synthetic crate graph to cache dependencies.
        // That graph does not contain the real CXX-Qt bridge source files,
        // so skip bridge generation for the dependency layer only.
        return;
    }

    // cxx_qt_build compiles the CXX-Qt bridge code and registers the
    // Zaparoo.Browse QML module. Qt is located via the QMAKE env var
    // (set by ZaparooRust.cmake for ARM32 cross) or PATH qmake6 on desktop.
    //
    // 0.8 builder shape: new_qml_module() takes the QmlModule up front and
    // auto-links Qt Core + Qml; .files([...]) replaces the 0.7 rust_files
    // field on QmlModule (removed in 0.8).
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("Zaparoo.Browse")
            .version(1, 0)
            // QAbstractListModel-derived singletons (CategoriesModel, SystemsModel,
            // GamesModel, BrowseModel) need this for qmllint to follow the
            // prototype chain back to QObject.
            .depend("QtQml.Models"),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .files(MODEL_FILES);

    // SAFETY: cc_builder is unsafe in 0.8 because cxx-qt makes no stability
    // guarantees about the cc::Build instance. We only adjust the include
    // path so the generated bridge code can find model_includes.h and add
    // a diagnostic-suppression flag; we do not mutate flags or sources
    // cxx-qt depends on for correctness.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("src/models");
            // GCC 16's -Wsfinae-incomplete fires on Qt 6's own headers
            // (qchar.h via QHash) when they are included with -I instead
            // of -isystem, flooding every cargo build log through the
            // cc warning replay. Qt-internal noise, nothing we can fix
            // here; no-op on compilers without the flag.
            cc.flag_if_supported("-Wno-sfinae-incomplete");
        })
    };
    builder.build();

    // Build provenance (commit / date / channel) deliberately does NOT
    // live here: it is baked by the `zaparoo-build-info` leaf crate so
    // that its `.git/` rerun triggers never re-run this build script —
    // a rerun here means re-running the entire cxx-qt codegen and
    // recompiling its generated C++.
    println!("cargo:rerun-if-env-changed=ZAPAROO_RUNTIME");
    println!("cargo:rerun-if-env-changed=ZAPAROO_DEV_BUILD");

    println!("cargo:rustc-check-cfg=cfg(zaparoo_runtime, values(\"mister\"))");
    println!("cargo:rustc-check-cfg=cfg(dev_build)");
    if let Ok(rt) = std::env::var("ZAPAROO_RUNTIME") {
        if rt.trim().eq_ignore_ascii_case("mister") {
            println!("cargo:rustc-cfg=zaparoo_runtime=\"mister\"");
        } else {
            println!(
                "cargo:warning=ignoring unknown ZAPAROO_RUNTIME value: {rt:?} (expected \"mister\")"
            );
        }
    }
    if std::env::var("ZAPAROO_DEV_BUILD").is_ok() {
        println!("cargo:rustc-cfg=dev_build");
    }
}
