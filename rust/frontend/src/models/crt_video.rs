// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.CrtVideo` - native CRT video state for the MiSTer `--crt`
// path. Owns the settings the Analog video section and the calibration
// screen read and write:
//
//   * `crt_enabled` - CONSTANT. Whether this process is running the
//     native CRT path (`--crt`). The durable truth lives in
//     `config/zaparoo_launcher_crt.bin` on the MiSTer SD card, which
//     Main_MiSTer reads to decide how to spawn us; toggling goes
//     through `write_crt_enable_file` + exit code 42 so Main respawns
//     the frontend under the new mode.
//   * `available_video_standards` - CONSTANT. `ntsc` / `pal` picker
//     keys. `480i` is a valid persisted value (hand-set in
//     frontend.toml for hardware smoke tests) but is deliberately not
//     offered until the 480i flicker-discipline UI pass lands.
//   * `current_video_standard` - READ + NOTIFY, persisted. Restart-
//     applied: the next `--crt` boot sizes the framebuffer from it
//     (352x240 NTSC, 352x288 PAL) and the Menu fork core derives its
//     mode from that geometry.
//   * `h_offset` / `v_offset` - READ + NOTIFY. Centering trims within
//     the core's honored ranges. `set_offsets` updates them live (the
//     calibration screen pokes the DDR control word per keypress) but
//     does NOT persist - arrow hold-repeat must not hammer the SD card
//     through the frontend.toml mirror. `commit_offsets` persists the
//     current values; the calibration screen calls it on exit.

use crate::models::settings::{mirror_settings_to_config, persist_settings};
use crate::models::with_persist_read;
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use zaparoo_core::config::{
    clamp_crt_offsets, load_config, normalize_crt_video_standard, CRT_H_OFFSET_MAX,
    CRT_H_OFFSET_MIN, CRT_V_OFFSET_MAX, CRT_V_OFFSET_MIN,
};
use zaparoo_core::platform_paths::config_file_path;

/// Picker order for the Analog video section's standard picker.
const VIDEO_STANDARDS: &[&str] = &["ntsc", "pal"];

/// The CRT state file Main_MiSTer reads when the menu core loads
/// (`zaparoo_alt_launcher_init_for_menu`) and on every CRT spawn:
/// byte 0 = enabled, byte 1 = DDR mode id (`crt_mode_id`). Main needs
/// the mode byte because it programs the framebuffer geometry before
/// the spawn AND re-asserts it ~1 s after - without it, a PAL fb would
/// be stomped back to 352x240. The frontend writes the file and exits
/// with code 42; Main re-reads it and respawns us with or without
/// `--crt`. Legacy 1-byte files read as mode 0 (NTSC) on Main's side.
#[cfg(zaparoo_runtime = "mister")]
const CRT_ENABLE_FILE: &str = "/media/fat/config/zaparoo_launcher_crt.bin";

// Live word1 poke in the C++ writer (`native_video_writer.cpp`). Only
// referenced on MiSTer builds: desktop `cargo test` and the QML test
// harness link this crate without the writer object, so an ungated
// extern would fail to link there.
#[cfg(zaparoo_runtime = "mister")]
extern "C" {
    fn zaparoo_native_video_set_offsets(h_offset: i32, v_offset: i32);
}

#[derive(Default)]
pub struct CrtVideoRust {
    crt_enabled: bool,
    available_video_standards: QStringList,
    current_video_standard: QString,
    h_offset: i32,
    v_offset: i32,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, crt_enabled, READ, CONSTANT)]
        #[qproperty(QStringList, available_video_standards, READ, CONSTANT)]
        #[qproperty(QString, current_video_standard, READ, WRITE = set_video_standard, NOTIFY)]
        #[qproperty(i32, h_offset, READ, NOTIFY)]
        #[qproperty(i32, v_offset, READ, NOTIFY)]
        type CrtVideo = super::CrtVideoRust;

        #[qinvokable]
        fn set_video_standard(self: Pin<&mut CrtVideo>, value: QString);

        /// Clamp and apply centering trims live (updates the DDR
        /// control word on MiSTer); does not persist.
        #[qinvokable]
        fn set_offsets(self: Pin<&mut CrtVideo>, h_offset: i32, v_offset: i32);

        /// Persist the current trims to state.toml + frontend.toml.
        #[qinvokable]
        fn commit_offsets(self: Pin<&mut CrtVideo>);

        /// Write the CRT state file Main_MiSTer reads on respawn:
        /// [enabled, mode id of the current video standard]. Returns
        /// false if the write failed (caller should not exit-42 in
        /// that case). No-op true off MiSTer.
        #[qinvokable]
        fn write_crt_enable_file(self: Pin<&mut CrtVideo>, enabled: bool) -> bool;
    }

    impl cxx_qt::Initialize for CrtVideo {}
}

impl Initialize for ffi::CrtVideo {
    fn initialize(mut self: Pin<&mut Self>) {
        // Self-contained merge: config wins over the persisted snapshot
        // (same precedence as `settings::merge_settings`) so this model
        // does not depend on Browse.Settings having initialised first.
        let snapshot = with_persist_read(|s| s.settings.clone());
        let config = load_config(&config_file_path());
        let standard = normalize_crt_video_standard(
            config
                .settings
                .crt_video_standard
                .as_deref()
                .unwrap_or(snapshot.crt_video_standard.as_str()),
        );
        let (h_offset, v_offset) = clamp_crt_offsets(
            config
                .settings
                .crt_h_offset
                .unwrap_or(snapshot.crt_h_offset),
            config
                .settings
                .crt_v_offset
                .unwrap_or(snapshot.crt_v_offset),
        );
        let mut standards = QStringList::default();
        for s in VIDEO_STANDARDS {
            standards.append(QString::from(*s));
        }
        self.as_mut().rust_mut().crt_enabled = crate::zaparoo_rust_crt_native_path_enabled();
        self.as_mut().rust_mut().available_video_standards = standards;
        self.as_mut().rust_mut().current_video_standard = QString::from(standard);
        self.as_mut().rust_mut().h_offset = h_offset;
        self.as_mut().rust_mut().v_offset = v_offset;
    }
}

impl ffi::CrtVideo {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_video_standard(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_crt_video_standard(&value.to_string()).to_string();
        if self.current_video_standard.to_string() == value_str {
            return;
        }
        // Restart-applied: the next `--crt` boot reads it from
        // frontend.toml when sizing the framebuffer.
        let snapshot = persist_settings(|s| s.crt_video_standard.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_video_standard = QString::from(value_str.as_str());
        self.as_mut().current_video_standard_changed();
    }

    fn set_offsets(mut self: Pin<&mut Self>, h_offset: i32, v_offset: i32) {
        let (h_offset, v_offset) = clamp_crt_offsets(h_offset, v_offset);
        let changed_h = self.h_offset != h_offset;
        let changed_v = self.v_offset != v_offset;
        if !changed_h && !changed_v {
            return;
        }
        self.as_mut().rust_mut().h_offset = h_offset;
        self.as_mut().rust_mut().v_offset = v_offset;
        if changed_h {
            self.as_mut().h_offset_changed();
        }
        if changed_v {
            self.as_mut().v_offset_changed();
        }
        #[cfg(zaparoo_runtime = "mister")]
        // SAFETY: plain int arguments into a C function that only
        // rewrites a mapped control word (and no-ops when the writer
        // is not initialised).
        unsafe {
            zaparoo_native_video_set_offsets(h_offset, v_offset);
        }
    }

    fn commit_offsets(self: Pin<&mut Self>) {
        let h_offset = self.h_offset.clamp(CRT_H_OFFSET_MIN, CRT_H_OFFSET_MAX);
        let v_offset = self.v_offset.clamp(CRT_V_OFFSET_MIN, CRT_V_OFFSET_MAX);
        let snapshot = persist_settings(|s| {
            s.crt_h_offset = h_offset;
            s.crt_v_offset = v_offset;
        });
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
    }

    #[cfg(zaparoo_runtime = "mister")]
    fn write_crt_enable_file(self: Pin<&mut Self>, enabled: bool) -> bool {
        let mode = zaparoo_core::config::crt_mode_id(&self.current_video_standard.to_string());
        match std::fs::write(CRT_ENABLE_FILE, [u8::from(enabled), mode]) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("could not write {CRT_ENABLE_FILE}: {e}");
                false
            }
        }
    }

    #[cfg(not(zaparoo_runtime = "mister"))]
    fn write_crt_enable_file(self: Pin<&mut Self>, enabled: bool) -> bool {
        let _ = enabled;
        true
    }
}
