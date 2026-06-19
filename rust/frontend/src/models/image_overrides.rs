// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.ImageOverrides` — deferred customization image discovery. Scans run
// after first paint on Tokio's blocking pool, never on the GUI thread.
//
// System artwork resolves entirely in Rust (`models::systems`) and does not
// go through this singleton.

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;

#[derive(Default, PartialEq, Eq)]
enum ScanState {
    #[default]
    Idle,
    Loading,
}

#[derive(Default)]
pub struct ImageOverridesRust {
    hub_loaded: bool,
    hub_scan_state: ScanState,
    systems_loaded: bool,
    systems_scan_state: ScanState,
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
        #[qproperty(bool, hub_loaded)]
        #[qproperty(bool, systems_loaded)]
        type ImageOverrides = super::ImageOverridesRust;

        #[qinvokable]
        fn load_hub_overrides(self: Pin<&mut ImageOverrides>);

        #[qinvokable]
        fn load_system_overrides(self: Pin<&mut ImageOverrides>);

        /// Return the `"custom-image/{path}"` cover key for an override file
        /// in namespace `ns` (e.g. `"hub"`) matching `id`, or an empty string
        /// when no override is present. QML callers fall back to the bundled
        /// cover key on empty. The returned key is served as-is (no tint) by
        /// the `custom-image` image provider. (`ns`, not `namespace`, because
        /// the latter is a C++ keyword in the generated wrapper.)
        #[qinvokable]
        fn override_cover_key(self: &ImageOverrides, ns: &QString, id: &QString) -> QString;
    }

    impl cxx_qt::Threading for ImageOverrides {}
}

impl ffi::ImageOverrides {
    fn load_hub_overrides(mut self: Pin<&mut Self>) {
        if self.hub_loaded || self.rust().hub_scan_state != ScanState::Idle {
            return;
        }
        self.as_mut().rust_mut().hub_scan_state = ScanState::Loading;
        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            let result =
                tokio::task::spawn_blocking(|| crate::image_overrides::scan_namespace("hub")).await;
            if let Err(e) = result {
                tracing::warn!("hub image override scan failed: {e}");
            }
            let _ = qt_thread.queue(|mut model| {
                model.as_mut().rust_mut().hub_scan_state = ScanState::Idle;
                model.as_mut().set_hub_loaded(true);
            });
        });
    }

    fn load_system_overrides(mut self: Pin<&mut Self>) {
        if self.systems_loaded || self.rust().systems_scan_state != ScanState::Idle {
            return;
        }
        self.as_mut().rust_mut().systems_scan_state = ScanState::Loading;
        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            let result =
                tokio::task::spawn_blocking(|| crate::image_overrides::scan_namespace("systems"))
                    .await;
            if let Err(e) = result {
                tracing::warn!("system image override scan failed: {e}");
            }
            let _ = qt_thread.queue(|mut model| {
                model.as_mut().rust_mut().systems_scan_state = ScanState::Idle;
                model.as_mut().set_systems_loaded(true);
            });
        });
    }

    fn override_cover_key(&self, ns: &QString, id: &QString) -> QString {
        crate::image_overrides::override_path(&ns.to_string(), &id.to_string())
            .map_or_else(QString::default, |path| {
                QString::from(format!("custom-image/{}", path.display()).as_str())
            })
    }
}
