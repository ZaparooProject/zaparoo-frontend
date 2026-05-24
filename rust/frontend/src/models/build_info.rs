// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.BuildInfo` — provenance baked in at build time. Surfaces the
// short git commit, the UTC build date, and the build channel
// ("official" for binaries produced via `just release`, "dev"
// otherwise). Read-only constants seeded from `cargo:rustc-env`
// values emitted by `build.rs`.
//
// Goal is provenance, not enforcement. A fork can rebuild without
// `ZAPAROO_OFFICIAL_BUILD` and the channel falls back to "dev"; that
// is the desired behavior — it makes unofficial builds visibly
// unofficial without sabotaging anyone.

use cxx_qt::CxxQtType;
use cxx_qt::Initialize;
use cxx_qt_lib::QString;
use std::pin::Pin;

const BUILD_COMMIT: &str = env!("ZAPAROO_BUILD_COMMIT");
const BUILD_DATE: &str = env!("ZAPAROO_BUILD_DATE");
const BUILD_CHANNEL: &str = env!("ZAPAROO_BUILD_CHANNEL");

#[derive(Default)]
pub struct BuildInfoRust {
    commit: QString,
    build_date: QString,
    channel: QString,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        // READ + CONSTANT + FINAL — values are baked at compile time
        // from build.rs env vars and never change after Initialize.
        // Same shape as `Browse.Runtime`.
        #[qproperty(QString, commit, READ, CONSTANT, FINAL)]
        #[qproperty(QString, build_date, READ, CONSTANT, FINAL)]
        #[qproperty(QString, channel, READ, CONSTANT, FINAL)]
        type BuildInfo = super::BuildInfoRust;
    }

    impl cxx_qt::Initialize for BuildInfo {}
}

impl Initialize for ffi::BuildInfo {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        rust.commit = QString::from(BUILD_COMMIT);
        rust.build_date = QString::from(BUILD_DATE);
        rust.channel = QString::from(BUILD_CHANNEL);
    }
}

/// Plain-Rust accessor for the startup log. Avoids constructing the
/// `QObject` just to read the constants.
pub fn provenance_string(version: &str) -> String {
    format!("version={version} commit={BUILD_COMMIT} date={BUILD_DATE} channel={BUILD_CHANNEL}")
}
