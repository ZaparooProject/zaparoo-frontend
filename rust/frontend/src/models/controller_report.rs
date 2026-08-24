// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.ControllerReport` -- the QML projection of the controller-input
// report `Main_MiSTer` writes (see `zaparoo_core::controller_report`). When
// the button-layout setting is `auto`, `MainLayout.qml` drives the help-bar
// glyph style from `glyph_layout`; `accept_button`/`cancel_button` drive the
// confirm/cancel face glyphs unconditionally -- see `MainLayout.qml`'s
// binding comment for why that stays true even with a manually-pinned style.
//
// Properties:
//   * `available` -- a report has been read. False on desktop / before the
//     first report, or when the polling watcher was never started (see
//     `zaparoo_core::controller_report::spawn_watcher`).
//   * `glyph_layout` -- the neutral style id the detected controller maps to
//     (`style_a`/`style_b`/`style_c`/`style_d`, or `style_e` when the active
//     input source is the keyboard) -- same ids as
//     `Browse.Settings.current_button_layout`. `style_d` (the neutral
//     style) when no report is available.
//   * `accept_button` / `cancel_button` -- the positionally-named glyph file
//     (`FaceEast`=east, `FaceSouth`=south, ...) for confirm and cancel.
//     Default to `FaceEast` / `FaceSouth` (today's layout) with no report.

use cxx_qt::{Initialize, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_core::controller_report::{self, ControllerGlyphs};

pub struct ControllerReportRust {
    available: bool,
    glyph_layout: QString,
    accept_button: QString,
    cancel_button: QString,
}

impl Default for ControllerReportRust {
    fn default() -> Self {
        let (available, glyph_layout, accept_button, cancel_button) = project(None);
        Self {
            available,
            glyph_layout: QString::from(glyph_layout),
            accept_button: QString::from(accept_button),
            cancel_button: QString::from(cancel_button),
        }
    }
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
        // Bare qproperty (READ + WRITE + NOTIFY): the setters are how
        // `apply_state` pushes watcher updates in. QML only ever reads them.
        #[qproperty(bool, available)]
        #[qproperty(QString, glyph_layout)]
        #[qproperty(QString, accept_button)]
        #[qproperty(QString, cancel_button)]
        type ControllerReport = super::ControllerReportRust;
    }

    impl cxx_qt::Threading for ControllerReport {}
    impl cxx_qt::Initialize for ControllerReport {}
}

impl Initialize for ffi::ControllerReport {
    fn initialize(mut self: Pin<&mut Self>) {
        let started = std::time::Instant::now();
        crate::startup_trace("rust:model ControllerReport init start");
        let mut rx = controller_report::subscribe();
        apply_state(self.as_mut(), project(rx.borrow_and_update().as_ref()));

        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            while rx.changed().await.is_ok() {
                let next = project(rx.borrow_and_update().as_ref());
                let _ = qt_thread.queue(move |m| apply_state(m, next));
            }
        });
        crate::startup_trace(format!(
            "rust:model ControllerReport init end dur_ms={}",
            started.elapsed().as_millis()
        ));
    }
}

/// Project the channel value into the four owned strings the model exposes.
/// No report -> not available, neutral style, today's east/south layout.
fn project(value: Option<&ControllerGlyphs>) -> (bool, &'static str, &'static str, &'static str) {
    match value {
        Some(g) => (true, g.layout, g.accept_button, g.cancel_button),
        None => (false, "style_d", "FaceEast", "FaceSouth"),
    }
}

fn apply_state(
    mut model: Pin<&mut ffi::ControllerReport>,
    (available, layout, accept, cancel): (bool, &str, &str, &str),
) {
    if model.available != available {
        model.as_mut().set_available(available);
    }
    if model.glyph_layout.to_string() != layout {
        model.as_mut().set_glyph_layout(QString::from(layout));
    }
    if model.accept_button.to_string() != accept {
        model.as_mut().set_accept_button(QString::from(accept));
    }
    if model.cancel_button.to_string() != cancel {
        model.as_mut().set_cancel_button(QString::from(cancel));
    }
}

#[cfg(test)]
mod tests {
    use super::project;

    #[test]
    fn no_report_projects_to_neutral_defaults() {
        assert_eq!(project(None), (false, "style_d", "FaceEast", "FaceSouth"));
    }

    #[test]
    fn report_with_default_orientation_projects_available() {
        let glyphs = zaparoo_core::controller_report::ControllerGlyphs {
            layout: "style_b",
            accept_button: "FaceEast",
            cancel_button: "FaceSouth",
        };
        assert_eq!(
            project(Some(&glyphs)),
            (true, "style_b", "FaceEast", "FaceSouth")
        );
    }

    #[test]
    fn report_with_swapped_positions_projects_swapped() {
        let glyphs = zaparoo_core::controller_report::ControllerGlyphs {
            layout: "style_a",
            accept_button: "FaceSouth",
            cancel_button: "FaceEast",
        };
        assert_eq!(
            project(Some(&glyphs)),
            (true, "style_a", "FaceSouth", "FaceEast")
        );
    }
}
