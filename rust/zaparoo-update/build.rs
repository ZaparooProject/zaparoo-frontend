// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Chooses between this crate's stub and the private module, and tells the
//! frontend's Slint build where the screen and translation catalogs live.
//!
//! Official builds have the private module checked out at
//! `rust/private/zaparoo-update` (gitignored). When it is there, its sources
//! compile into this crate as the `imp` module and its `ui/` and
//! `translations/` directories are the ones published; otherwise the stub
//! above does nothing. One crate identity either way keeps `Cargo.lock`,
//! `cargo metadata`, `cargo deny` and every workspace command unchanged.

#![allow(
    clippy::expect_used,
    reason = "build scripts communicate failure by panicking; there is no richer channel to cargo"
)]

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let private = manifest.join("../private/zaparoo-update");
    let real =
        private.join("src/imp/mod.rs").is_file() && private.join("ui/update.slint").is_file();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rustc-check-cfg=cfg(zaparoo_update_private)");

    if real {
        let private = private
            .canonicalize()
            .expect("the private module path resolves");
        // Watched only while it exists: cargo treats a missing path as
        // always changed, which would rebuild this crate and everything
        // above it on every build of a public checkout. The price is that a
        // checkout that appears later needs `touch build.rs` (or
        // `just update-module`) to be noticed.
        println!("cargo:rerun-if-changed=../private/zaparoo-update");
        println!("cargo:rustc-cfg=zaparoo_update_private");
        std::fs::write(
            out.join("private.rs"),
            format!(
                "#[path = {:?}]\nmod imp;\n",
                private.join("src/imp/mod.rs").display().to_string()
            ),
        )
        .expect("write private.rs");
        println!("cargo:ui_dir={}", private.join("ui").display());
        println!(
            "cargo:translations_dir={}",
            private.join("translations").display()
        );
    } else {
        // The stub draws nothing and carries no catalogs.
        std::fs::write(out.join("private.rs"), "").expect("write private.rs");
        println!("cargo:ui_dir={}", manifest.join("ui").display());
        println!("cargo:translations_dir=");
    }
}
