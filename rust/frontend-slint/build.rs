// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#[allow(
    clippy::panic,
    reason = "build scripts communicate failure by panicking; there is no richer channel to cargo"
)]
fn main() {
    // One configuration for every target: fonts imported by the .slint
    // files are embedded as files and registered with the runtime font
    // stack on startup (see `src/fonts.rs`), on MiSTer as on the desktop.
    // The MiSTer build used to pre-render glyphs here at a fixed size
    // ladder (`EmbedForSoftwareRenderer`); that path has no text shaping.
    slint_build::compile("ui/app.slint").unwrap_or_else(|e| panic!("slint compile failed: {e}"));
}
