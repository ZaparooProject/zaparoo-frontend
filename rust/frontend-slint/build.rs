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
    // Translations are bundled from translations/<lang>/LC_MESSAGES/
    // frontend-slint.po (harvested from the Qt catalogs by
    // scripts/convert-ts-catalogs.py) and selected at runtime by
    // `apply_language` in main.rs. No default context: one msgid is one
    // entry, which is what lets the Qt translations carry over.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config)
        .unwrap_or_else(|e| panic!("slint compile failed: {e}"));
}
