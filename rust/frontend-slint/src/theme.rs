// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Pushes the palette for the persisted color scheme into the `Theme`
// global. The rules live in `zaparoo_app::palette` (a port of
// `ColorSchemes.qml`, pinned to its golden fixture); this is the adapter
// that turns `Rgb16` values into Slint colors, the way `Theme.qml` binds
// the QML roles to `ColorSchemes.palette(...)`.

use crate::{App, Theme};
use slint::ComponentHandle;
use zaparoo_app::palette::{self, Palette, Rgb16};

/// A (highlight, midtone, shadow) ramp for the logo tinter.
pub type TintRamp = [(u8, u8, u8); 3];

fn color(value: Rgb16) -> slint::Color {
    let (r, g, b) = value.rgb8();
    slint::Color::from_rgb_u8(r, g, b)
}

/// Applies the palette for `scheme` at `intensity` to the app's `Theme`
/// global and returns it, so callers can feed the roles that live outside
/// Slint (the logo tint ramps) from the same values. Unknown ids fall back
/// the way the Qt build does.
pub fn apply_palette(app: &App, scheme: &str, intensity: &str) -> Palette {
    let p = palette::palette(scheme, intensity);
    let theme = app.global::<Theme>();
    theme.set_bg_deep(color(p.bg_deep));
    theme.set_bg_panel(color(p.bg_panel));
    theme.set_bg_bar(color(p.bg_bar));
    theme.set_surface_card(color(p.surface_card));
    theme.set_tile_edge(color(p.tile_edge));
    theme.set_control_edge(color(p.control_edge));
    theme.set_border_subtle(color(p.border_subtle));
    theme.set_border_mid(color(p.border_mid));
    theme.set_text_primary(color(p.text_primary));
    theme.set_text_label(color(p.text_label));
    theme.set_text_variant(color(p.text_variant));
    theme.set_accent(color(p.accent));
    theme.set_selection_fill(color(p.selection_fill));
    theme.set_on_accent(color(p.on_accent));
    theme.set_on_accent_muted(color(p.on_accent_muted));
    theme.set_logo_primary(color(p.logo_primary));
    theme.set_logo_secondary(color(p.logo_secondary));
    theme.set_logo_shadow(color(p.logo_shadow));
    theme.set_logo_focus_primary(color(p.logo_focus_primary));
    theme.set_logo_focus_secondary(color(p.logo_focus_secondary));
    theme.set_logo_focus_shadow(color(p.logo_focus_shadow));
    theme.set_marker(color(p.marker));
    theme.set_marker_outline(color(p.marker_outline));
    theme.set_error(color(p.error));
    theme.set_qr_light(color(p.qr_light));
    theme.set_qr_dark(color(p.qr_dark));
    // The scrim is a constant dark veil in every preset; the .slint default
    // carries it.
    p
}

/// The (highlight, midtone, shadow) ramps the logo tinter wants, resting
/// and focused, straight from the palette's logo roles.
#[allow(
    dead_code,
    reason = "the snapshot tool includes this module but renders no logos"
)]
pub fn logo_tints(p: &Palette) -> (TintRamp, TintRamp) {
    (
        [
            p.logo_primary.rgb8(),
            p.logo_secondary.rgb8(),
            p.logo_shadow.rgb8(),
        ],
        [
            p.logo_focus_primary.rgb8(),
            p.logo_focus_secondary.rgb8(),
            p.logo_focus_shadow.rgb8(),
        ],
    )
}
