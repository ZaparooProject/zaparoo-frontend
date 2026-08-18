// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.FavoriteSystemsState` — persisted selection state for the
// favorite-systems screen. Mirrors `FavoritesState`: we only remember
// the last focused row's system id/path.

use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_core::persist::{self, FavoriteSystemsState as PersistedFavoriteSystemsState};

#[derive(Default)]
pub struct FavoriteSystemsStateRust {
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
        type FavoriteSystemsState = super::FavoriteSystemsStateRust;

        #[qinvokable]
        fn set_selected_path(self: Pin<&mut FavoriteSystemsState>, value: QString);
    }

    impl cxx_qt::Initialize for FavoriteSystemsState {}
}

impl Initialize for ffi::FavoriteSystemsState {
    fn initialize(mut self: Pin<&mut Self>) {
        let snapshot: PersistedFavoriteSystemsState =
            with_persist_read(|s| s.favorite_systems.clone());
        self.as_mut().rust_mut().selected_path = QString::from(snapshot.selected_path.as_str());
    }
}

impl ffi::FavoriteSystemsState {
    fn set_selected_path(mut self: Pin<&mut Self>, value: QString) {
        if self.selected_path == value {
            return;
        }
        let value_str = value.to_string();
        self.as_mut().rust_mut().selected_path = value;
        self.as_mut().selected_path_changed();
        persist_favorite_systems(|r| r.selected_path = value_str);
    }
}

fn persist_favorite_systems<F: FnOnce(&mut PersistedFavoriteSystemsState)>(mutator: F) {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.favorite_systems);
        s.clone()
    });
    persist::save(&snapshot);
}
