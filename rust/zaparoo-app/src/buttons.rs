// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Help-bar glyph resolution, ported from MainLayout.qml's Resources
// bindings: which button style directory the bar draws from and which
// positional glyph the semantic confirm / cancel / options / view tokens
// resolve to. "auto" defers the style to the connected controller
// (Main_MiSTer's input report); a manual pick pins the family. Confirm
// and cancel always follow the live report, because which physical
// button accepts is a fact about the controller, not a look. The swap
// settings compensate for controllers whose own mapping is backwards and
// never apply while the keyboard is the live input source.

pub const STYLES: [&str; 5] = ["style_a", "style_b", "style_c", "style_d", "style_e"];
pub const DEFAULT_STYLE: &str = "style_d";
/// The style the controller report resolves for a plain keyboard.
pub const KEYBOARD_STYLE: &str = "style_e";
pub const DEFAULT_ACCEPT: &str = "FaceEast";
pub const DEFAULT_CANCEL: &str = "FaceSouth";

/// What the controller report resolved: style directory and the two
/// positional face glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report<'a> {
    pub layout: &'a str,
    pub accept_button: &'a str,
    pub cancel_button: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved<'a> {
    pub style: &'a str,
    pub confirm: &'a str,
    pub cancel: &'a str,
    pub options: &'static str,
    pub view: &'static str,
}

/// The keyboard is the live input source. The swap settings
/// compensate for a controller whose own mapping is backwards, so they
/// never apply to it; the action seam reads this too.
pub fn keyboard_active(glyph_layout: &str) -> bool {
    glyph_layout == KEYBOARD_STYLE
}

/// Resolve the bar's glyphs for the `button_layout` setting (`auto` or a
/// style id) and the two swap settings, given the latest report if any.
pub fn resolve<'a>(
    report: Option<Report<'a>>,
    button_layout: &'a str,
    swap_confirm_cancel: bool,
    swap_options_view: bool,
) -> Resolved<'a> {
    let glyph_layout = report.map_or(DEFAULT_STYLE, |r| r.layout);
    let accept = report.map_or(DEFAULT_ACCEPT, |r| r.accept_button);
    let cancel = report.map_or(DEFAULT_CANCEL, |r| r.cancel_button);
    let style = if STYLES.contains(&button_layout) {
        button_layout
    } else {
        glyph_layout
    };
    // Enter/Escape and Tab/Space are fixed keys: no swap applies while
    // the keyboard drives the UI.
    let keyboard_active = keyboard_active(glyph_layout);
    let swap_cc = swap_confirm_cancel && !keyboard_active;
    let swap_ov = swap_options_view && !keyboard_active;
    Resolved {
        style,
        confirm: if swap_cc { cancel } else { accept },
        cancel: if swap_cc { accept } else { cancel },
        options: if swap_ov { "FaceWest" } else { "FaceNorth" },
        view: if swap_ov { "FaceNorth" } else { "FaceWest" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_report_is_the_neutral_style_with_the_default_faces() {
        let r = resolve(None, "auto", false, false);
        assert_eq!(
            (r.style, r.confirm, r.cancel, r.options, r.view),
            ("style_d", "FaceEast", "FaceSouth", "FaceNorth", "FaceWest")
        );
    }

    #[test]
    fn auto_follows_the_report_and_a_pin_keeps_the_report_faces() {
        let report = Report {
            layout: "style_a",
            accept_button: "FaceSouth",
            cancel_button: "FaceEast",
        };
        let auto = resolve(Some(report), "auto", false, false);
        assert_eq!(
            (auto.style, auto.confirm, auto.cancel),
            ("style_a", "FaceSouth", "FaceEast")
        );
        let pinned = resolve(Some(report), "style_c", false, false);
        assert_eq!(
            (pinned.style, pinned.confirm, pinned.cancel),
            ("style_c", "FaceSouth", "FaceEast")
        );
        let stale = resolve(Some(report), "nintendo", false, false);
        assert_eq!(stale.style, "style_a");
    }

    #[test]
    fn swaps_apply_only_off_the_keyboard() {
        let pad = Report {
            layout: "style_b",
            accept_button: "FaceEast",
            cancel_button: "FaceSouth",
        };
        let swapped = resolve(Some(pad), "auto", true, true);
        assert_eq!((swapped.confirm, swapped.cancel), ("FaceSouth", "FaceEast"));
        assert_eq!((swapped.options, swapped.view), ("FaceWest", "FaceNorth"));
        let keyboard = Report {
            layout: "style_e",
            accept_button: "FaceEast",
            cancel_button: "FaceSouth",
        };
        let fixed = resolve(Some(keyboard), "style_a", true, true);
        assert_eq!(
            (fixed.style, fixed.confirm, fixed.cancel),
            ("style_a", "FaceEast", "FaceSouth")
        );
        assert_eq!((fixed.options, fixed.view), ("FaceNorth", "FaceWest"));
    }
}
