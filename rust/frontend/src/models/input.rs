// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.Input` — translates raw Qt key codes into `ZaparooAction`
// names (see `zaparoo_core::input_actions`). QML screens react to
// actions, not keys, so gamepad / NFC sources can slot in beside the
// keyboard without touching the UI. Bindings are seeded from the map
// `config::Config::key_to_action` built during init.

use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::collections::HashMap;
use std::pin::Pin;

#[derive(Default)]
pub struct InputRust {
    key_to_action: HashMap<i32, String>,
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
        type Input = super::InputRust;

        #[qinvokable]
        fn action_for_key(self: &Input, key: i32) -> QString;
    }

    impl cxx_qt::Initialize for Input {}
}

impl Initialize for ffi::Input {
    fn initialize(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().key_to_action = crate::models::input_bindings();
    }
}

impl ffi::Input {
    fn action_for_key(&self, key: i32) -> QString {
        self.key_to_action
            .get(&key)
            .map_or_else(QString::default, |s| QString::from(s.as_str()))
    }
}
