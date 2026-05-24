// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.HubState` — persisted state owned by the hub screen. Holds
// the row the user last had focus on (top categories vs bottom action
// tiles), the category they were on inside the top row, and the bottom
// row action they were on. Schema version is checked independently from
// other screens on load (see `zaparoo_core::persist`).

use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_core::persist::{self, HubState};

#[derive(Default)]
pub struct HubStateRust {
    category: QString,
    selected_row: u32,
    selected_action: QString,
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
        #[qproperty(QString, category, READ, WRITE = set_category, NOTIFY)]
        #[qproperty(u32, selected_row, READ, WRITE = set_selected_row, NOTIFY)]
        #[qproperty(QString, selected_action, READ, WRITE = set_selected_action, NOTIFY)]
        type HubState = super::HubStateRust;

        #[qinvokable]
        fn set_category(self: Pin<&mut HubState>, value: QString);

        #[qinvokable]
        fn set_selected_row(self: Pin<&mut HubState>, value: u32);

        #[qinvokable]
        fn set_selected_action(self: Pin<&mut HubState>, value: QString);
    }

    impl cxx_qt::Initialize for HubState {}
}

impl Initialize for ffi::HubState {
    fn initialize(mut self: Pin<&mut Self>) {
        let snapshot: HubState = with_persist_read(|s| s.hub.clone());
        self.as_mut().rust_mut().category = QString::from(snapshot.category.as_str());
        self.as_mut().rust_mut().selected_row = snapshot.selected_row;
        self.as_mut().rust_mut().selected_action = QString::from(snapshot.selected_action.as_str());
    }
}

impl ffi::HubState {
    fn set_category(mut self: Pin<&mut Self>, value: QString) {
        if self.category == value {
            return;
        }
        let value_str = value.to_string();
        self.as_mut().rust_mut().category = value;
        self.as_mut().category_changed();
        persist_hub(|h| h.category = value_str);
    }

    fn set_selected_row(mut self: Pin<&mut Self>, value: u32) {
        if self.selected_row == value {
            return;
        }
        self.as_mut().rust_mut().selected_row = value;
        self.as_mut().selected_row_changed();
        persist_hub(|h| h.selected_row = value);
    }

    fn set_selected_action(mut self: Pin<&mut Self>, value: QString) {
        if self.selected_action == value {
            return;
        }
        let value_str = value.to_string();
        self.as_mut().rust_mut().selected_action = value;
        self.as_mut().selected_action_changed();
        persist_hub(|h| h.selected_action = value_str);
    }
}

fn persist_hub<F: FnOnce(&mut HubState)>(mutator: F) {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.hub);
        s.clone()
    });
    persist::save(&snapshot);
}
