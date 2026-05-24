// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.Runtime` — process-level constant: where is the frontend
// binary running? Reads `zaparoo_core::runtime::current()` once at
// construction and exposes `is_mister` for QML branches that need to
// gate behavior on the host (e.g. the Arcade-bypass shortcut on
// MiSTer, where the Arcade category contains exactly one system and
// the second navigate would be redundant).
//
// Distinct from `Browse.Platform` (where Zaparoo Core is running).
// Do not collapse the two — see docs/architecture.md.

use cxx_qt::CxxQtType;
use cxx_qt::Initialize;
use std::pin::Pin;
use zaparoo_core::runtime;

#[derive(Default)]
pub struct RuntimeRust {
    is_mister: bool,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        // READ + CONSTANT — the value is decided once during
        // Initialize::initialize and never changes; no setter is
        // exposed to QML, no NOTIFY signal is generated.
        // FINAL silences qmllint's "member can be shadowed" warning
        // and is correct: Runtime is a singleton with no inheritance
        // story, so the property cannot meaningfully be overridden.
        #[qproperty(bool, is_mister, READ, CONSTANT, FINAL)]
        type Runtime = super::RuntimeRust;
    }

    impl cxx_qt::Initialize for Runtime {}
}

impl Initialize for ffi::Runtime {
    fn initialize(mut self: Pin<&mut Self>) {
        // No `*_changed` emit here — QML bindings don't attach during
        // Initialize and the value never mutates after this point.
        self.as_mut().rust_mut().is_mister = runtime::current().is_mister();
    }
}
