// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.Notice` — first-run notices the user has acknowledged.
//
// Persisted in `frontend.toml` (under `[notice]`), not `state.toml`,
// because `MiSTer`'s `/tmp` state is wiped on every reboot — using
// state would re-show the notice on every cold boot, which is exactly
// the "nag legitimate users" failure mode the plan calls out.
//
// Currently exposes a single `commercial_ack` flag for the
// non-commercial-use notice. The flag is read once at construction
// from disk, so the modal can decide on first paint whether to show.
// The QML side calls `acknowledge_commercial()` from the modal's
// "I understand" button; that flips the flag and writes it back
// atomically.

use cxx_qt::{CxxQtType, Initialize};
use std::pin::Pin;
use tracing::warn;
use zaparoo_core::config::{load_config, save_notice_ack};
use zaparoo_core::platform_paths::config_file_path;

#[derive(Default)]
pub struct NoticeRust {
    commercial_ack: bool,
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
        // READ + NOTIFY — flips exactly once per install (the user
        // pressing "I understand" on the first-run notice). The QML
        // modal binds its `open` gate on this so it self-closes the
        // moment the slot persists.
        #[qproperty(bool, commercial_ack, READ, NOTIFY)]
        type Notice = super::NoticeRust;

        #[qinvokable]
        fn acknowledge_commercial(self: Pin<&mut Notice>);
    }

    impl cxx_qt::Initialize for Notice {}
}

impl Initialize for ffi::Notice {
    fn initialize(mut self: Pin<&mut Self>) {
        let cfg = load_config(&config_file_path());
        self.as_mut().rust_mut().commercial_ack = cfg.notice.commercial_ack;
    }
}

impl ffi::Notice {
    fn acknowledge_commercial(mut self: Pin<&mut Self>) {
        if self.commercial_ack {
            return;
        }
        let path = config_file_path();
        if let Err(e) = save_notice_ack(&path, true) {
            // Log and surface the in-memory flip anyway: the user
            // pressed "I understand" once and shouldn't be re-prompted
            // for the rest of this session even if the disk write
            // failed (read-only FS, full tmpfs, etc.). The next launch
            // will re-show the notice, which is the right fallback.
            warn!(
                "could not persist commercial notice ack to {}: {e}",
                path.display()
            );
        }
        self.as_mut().rust_mut().commercial_ack = true;
        self.as_mut().commercial_ack_changed();
    }
}
