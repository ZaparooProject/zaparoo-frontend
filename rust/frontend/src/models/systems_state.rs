// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.SystemsState` — persisted state owned by the systems screen.
// Records the system the user last highlighted in the systems grid.
// Schema version is checked independently from other screens on load
// (see `zaparoo_core::persist`).

use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_core::persist::{self, SystemsState};

#[derive(Default)]
pub struct SystemsStateRust {
    system_id: QString,
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
        #[qproperty(QString, system_id, READ, WRITE = set_system_id, NOTIFY)]
        type SystemsState = super::SystemsStateRust;

        #[qinvokable]
        fn set_system_id(self: Pin<&mut SystemsState>, value: QString);
    }

    impl cxx_qt::Initialize for SystemsState {}
}

impl Initialize for ffi::SystemsState {
    fn initialize(mut self: Pin<&mut Self>) {
        let snapshot: SystemsState = with_persist_read(|s| s.systems.clone());
        self.as_mut().rust_mut().system_id = QString::from(snapshot.system_id.as_str());
    }
}

impl ffi::SystemsState {
    fn set_system_id(mut self: Pin<&mut Self>, value: QString) {
        if self.system_id == value {
            return;
        }
        let value_str = value.to_string();
        self.as_mut().rust_mut().system_id = value;
        self.as_mut().system_id_changed();
        persist_systems(|s| s.system_id = value_str);
    }
}

fn persist_systems<F: FnOnce(&mut SystemsState)>(mutator: F) {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.systems);
        s.clone()
    });
    persist::save(&snapshot);
}
