// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

/// Font sizes pre-rendered into the `MiSTer` binary. Must be a superset
/// of everything `Sizing.font-size` can produce on that build: the
/// CRT path quantizes to 8/16 and the HDMI path snaps to this ladder
/// (see `ui/theme.slint`).
const FONT_SIZE_LADDER: &str = "8,12,16,20,24,32,40,48";

#[allow(
    clippy::panic,
    reason = "build scripts communicate failure by panicking; there is no richer channel to cargo"
)]
fn main() {
    let mister = std::env::var_os("CARGO_FEATURE_MISTER").is_some();
    let config = if mister {
        // The software renderer without fontconfig needs glyphs baked
        // in at build time. SLINT_FONT_SIZES is read by the slint
        // compiler from the environment.
        std::env::set_var("SLINT_FONT_SIZES", FONT_SIZE_LADDER);
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer)
    } else {
        slint_build::CompilerConfiguration::new()
    };
    slint_build::compile_with_config("ui/app.slint", config)
        .unwrap_or_else(|e| panic!("slint compile failed: {e}"));
}
