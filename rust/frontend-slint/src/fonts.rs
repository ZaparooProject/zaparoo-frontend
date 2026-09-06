// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Runtime font directory. The base faces (Noto Sans and the CRT bitmap face)
// are imported by `ui/app.slint` and travel inside the binary; the six
// script faces (Arabic, Devanagari, Hebrew, JP, KR, TC) are loaded at
// startup from a directory so the binary stays small and a face can be
// swapped without a rebuild. They are static Regular instances
// (`resources/fonts/runtime/`, see its README) rather than the variable
// files the Qt build embeds: Slint's runtime text path renders a variable
// font's default instance, and four of the six default to Thin.
// Slint's font collection reads
// `SLINT_FONT_PATH` when the platform is created and appends every face it
// finds to the generic sans-serif fallback chain, which is what lets parley
// pick Noto Sans Arabic for an Arabic run inside a "Noto Sans" label. This
// must therefore run before `slint::platform::set_platform` and before the
// first component is created.
//
// Search order for the directory:
//   1. $ZAPAROO_FONTS_DIR        (explicit override)
//   2. <exe dir>/fonts           (MiSTer deploy layout, see deploy-mister-slint.sh)
//   3. <repo>/resources/fonts/runtime  (desktop dev runs from the checkout)
// An already-set `SLINT_FONT_PATH` is left alone.

use std::path::PathBuf;

/// Points Slint at the runtime font directory. Returns the directory that
/// was chosen, or `None` when nothing was found (Latin text still renders
/// from the embedded faces; other scripts fall back to whatever the host
/// provides).
pub fn install_font_path() -> Option<PathBuf> {
    if std::env::var_os("SLINT_FONT_PATH").is_some() {
        return None;
    }
    let dir = candidates().into_iter().find(|p| p.is_dir())?;
    std::env::set_var("SLINT_FONT_PATH", &dir);
    Some(dir)
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(3);
    if let Some(explicit) = std::env::var_os("ZAPAROO_FONTS_DIR") {
        out.push(PathBuf::from(explicit));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("fonts"));
        }
    }
    out.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("resources")
            .join("fonts")
            .join("runtime"),
    );
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "tests should fail fast on unexpected errors"
    )]

    use super::candidates;

    #[test]
    fn repo_fonts_dir_is_the_final_candidate() {
        let last = candidates().pop().expect("at least one candidate");
        assert!(
            last.ends_with("resources/fonts/runtime"),
            "{}",
            last.display()
        );
        assert!(last.join("NotoSansArabic-Regular.ttf").is_file());
    }
}
