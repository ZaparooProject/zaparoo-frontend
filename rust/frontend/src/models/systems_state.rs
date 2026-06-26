// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.SystemsState` — persisted state owned by the systems screen.
// Records the system the user last highlighted in the systems grid.
// Schema version is checked independently from other screens on load
// (see `zaparoo_core::persist`).

use crate::models::{
    with_hidden_browse_prefs_mut, with_hidden_browse_prefs_read, with_persist_mut,
    with_persist_read,
};
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

        /// Add `id` to the persisted hidden-system set. No-op if already there.
        #[qinvokable]
        fn hide_system(self: Pin<&mut SystemsState>, id: &QString);

        /// Remove `id` from the persisted hidden-system set.
        #[qinvokable]
        fn unhide_system(self: Pin<&mut SystemsState>, id: &QString);

        /// Returns true when `id` is in the persisted hidden-system set.
        #[qinvokable]
        fn is_system_hidden(self: &SystemsState, id: &QString) -> bool;
    }

    impl cxx_qt::Initialize for SystemsState {}
}

impl Initialize for ffi::SystemsState {
    fn initialize(mut self: Pin<&mut Self>) {
        let started = std::time::Instant::now();
        crate::startup_trace("rust:model SystemsState init start");
        let snapshot: SystemsState = with_persist_read(|s| s.systems.clone());
        self.as_mut().rust_mut().system_id = QString::from(snapshot.system_id.as_str());
        crate::startup_trace(format!(
            "rust:model SystemsState init end dur_ms={}",
            started.elapsed().as_millis()
        ));
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

    fn hide_system(self: Pin<&mut Self>, id: &QString) {
        let id_str = id.to_string();
        if id_str.is_empty() {
            return;
        }
        with_hidden_browse_prefs_mut(|p| {
            if !p.hidden_system_ids.contains(&id_str) {
                p.hidden_system_ids.push(id_str);
            }
        });
    }

    fn unhide_system(self: Pin<&mut Self>, id: &QString) {
        let id_str = id.to_string();
        with_hidden_browse_prefs_mut(|p| p.hidden_system_ids.retain(|x| x != &id_str));
    }

    fn is_system_hidden(&self, id: &QString) -> bool {
        let id_str = id.to_string();
        with_hidden_browse_prefs_read(|p| p.hidden_system_ids.contains(&id_str))
    }
}

fn persist_systems<F: FnOnce(&mut SystemsState)>(mutator: F) {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.systems);
        s.clone()
    });
    persist::save(&snapshot);
}
