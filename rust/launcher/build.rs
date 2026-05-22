// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use cxx_qt_build::{CxxQtBuilder, QmlModule};

const MODEL_FILES: &[&str] = &[
    "src/models/alternate_versions.rs",
    "src/models/categories.rs",
    "src/models/systems.rs",
    "src/models/game_info.rs",
    "src/models/games.rs",
    "src/models/favorites.rs",
    "src/models/browse.rs",
    "src/models/app_state.rs",
    "src/models/build_info.rs",
    "src/models/app_status.rs",
    "src/models/hub_state.rs",
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
    // path so the generated bridge code can find model_includes.h; we do
    // not mutate flags or sources cxx-qt depends on.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("src/models");
        })
    };
    builder.build();

    println!("cargo:rerun-if-env-changed=ZAPAROO_RUNTIME");
    println!("cargo:rerun-if-env-changed=ZAPAROO_DEV_BUILD");
    println!("cargo:rerun-if-env-changed=ZAPAROO_OFFICIAL_BUILD");
    println!("cargo:rerun-if-env-changed=ZAPAROO_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=ZAPAROO_BUILD_DATE");

    // Rerun when HEAD or any branch ref moves so ZAPAROO_BUILD_COMMIT /
    // ZAPAROO_BUILD_DATE refresh after rebases, branch switches, and
    // commits that don't otherwise touch this crate. Emitting any
    // rerun-if-* directive disables Cargo's "rerun on any package
    // file change" default, which is why these are needed alongside
    // the env-changed lines above.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");

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

    // Build provenance — baked into the binary and surfaced through the
    // `Browse.BuildInfo` singleton plus the startup log. Goal is
    // "this binary is from this source tree at this date, and it is /
    // is not an official package", not DRM. Failures fall back to
    // "unknown" / "dev"; the build still succeeds.
    //
    // Prefer values supplied via env so cross-builds that don't have
    // `.git/` in their build context (e.g. the ARM32 Docker build,
    // which COPYs only source dirs) can be told the commit and date by
    // the host. Fall back to running `git` / `date` when the env vars
    // are absent or empty, which is the common path for host builds.
    let commit = std::env::var("ZAPAROO_BUILD_COMMIT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ZAPAROO_BUILD_COMMIT={commit}");

    let build_date = std::env::var("ZAPAROO_BUILD_DATE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("date")
                .args(["-u", "+%Y-%m-%d"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ZAPAROO_BUILD_DATE={build_date}");

    let channel = if std::env::var("ZAPAROO_OFFICIAL_BUILD").is_ok() {
        "official"
    } else {
        "dev"
    };
    println!("cargo:rustc-env=ZAPAROO_BUILD_CHANNEL={channel}");
}
