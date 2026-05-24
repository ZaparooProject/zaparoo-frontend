// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.Platform` — where Zaparoo Core is running. Populated by the
// `version` RPC after each successful connect (see
// `zaparoo_core::platform::spawn_fetcher`). Distinct from
// `Browse.Runtime` (where this frontend binary is running).
//
// `ready` distinguishes "not yet known" from "known not MiSTer" so QML
// branches that gate behavior on Platform can choose between waiting and
// falling through. Any QML check that *requires* MiSTer should AND with
// `ready` so a fast cold-start path can't mistake a pre-RPC `None` for a
// confirmed non-MiSTer host.

use cxx_qt::{Initialize, Threading};
use std::pin::Pin;
use zaparoo_core::platform::{self, Platform};

#[derive(Default)]
pub struct PlatformRust {
    is_mister: bool,
    ready: bool,
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
        #[qproperty(bool, is_mister)]
        #[qproperty(bool, ready)]
        type Platform = super::PlatformRust;
    }

    impl cxx_qt::Threading for Platform {}
    impl cxx_qt::Initialize for Platform {}
}

impl Initialize for ffi::Platform {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut rx = platform::subscribe();
        apply_state(self.as_mut(), project(rx.borrow_and_update().as_ref()));

        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            while rx.changed().await.is_ok() {
                let next = project(rx.borrow_and_update().as_ref());
                let _ = qt_thread.queue(move |m| apply_state(m, next));
            }
        });
    }
}

fn project(value: Option<&Platform>) -> (bool, bool) {
    match value {
        Some(Platform::Mister) => (true, true),
        Some(_) => (false, true),
        None => (false, false),
    }
}

fn apply_state(mut model: Pin<&mut ffi::Platform>, (is_mister, ready): (bool, bool)) {
    if model.is_mister != is_mister {
        model.as_mut().set_is_mister(is_mister);
    }
    if model.ready != ready {
        model.as_mut().set_ready(ready);
    }
}

#[cfg(test)]
mod tests {
    use super::{project, Platform};

    #[test]
    fn unresolved_is_not_ready_and_not_mister() {
        assert_eq!(project(None), (false, false));
    }

    #[test]
    fn mister_is_ready_and_is_mister() {
        assert_eq!(project(Some(&Platform::Mister)), (true, true));
    }

    #[test]
    fn linux_is_ready_but_not_mister() {
        assert_eq!(project(Some(&Platform::Linux)), (false, true));
    }

    #[test]
    fn unknown_is_ready_but_not_mister() {
        assert_eq!(
            project(Some(&Platform::Unknown("playdate".into()))),
            (false, true),
        );
    }
}
