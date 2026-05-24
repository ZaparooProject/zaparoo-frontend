// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.RecentsState` — persisted state owned by the recents screen.
// Holds the path of the entry that last had focus, so a kill-resume
// puts the highlight back on the same row. The recents list itself
// lives in `RecentsModel` (Core's `media.history`); this singleton
// just remembers the cursor.

use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_core::persist::{self, RecentsState};

#[derive(Default)]
pub struct RecentsStateRust {
    selected_path: QString,
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
        #[qproperty(QString, selected_path, READ, WRITE = set_selected_path, NOTIFY)]
        type RecentsState = super::RecentsStateRust;

        #[qinvokable]
        fn set_selected_path(self: Pin<&mut RecentsState>, value: QString);
    }

    impl cxx_qt::Initialize for RecentsState {}
}

impl Initialize for ffi::RecentsState {
    fn initialize(mut self: Pin<&mut Self>) {
        let snapshot: RecentsState = with_persist_read(|s| s.recents.clone());
        self.as_mut().rust_mut().selected_path = QString::from(snapshot.selected_path.as_str());
    }
}

impl ffi::RecentsState {
    fn set_selected_path(mut self: Pin<&mut Self>, value: QString) {
        if self.selected_path == value {
            return;
        }
        let value_str = value.to_string();
        self.as_mut().rust_mut().selected_path = value;
        self.as_mut().selected_path_changed();
        persist_recents(|r| r.selected_path = value_str);
    }
}

fn persist_recents<F: FnOnce(&mut RecentsState)>(mutator: F) {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.recents);
        s.clone()
    });
    persist::save(&snapshot);
}
