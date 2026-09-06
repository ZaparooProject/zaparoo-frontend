// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Runtime rasterization of the UI glyphs from the SVG sources under
// resources/images, the same files the Qt build's tinted-svg image
// provider serves. Rendering at the exact display pixel size keeps them
// crisp at every resolution. Keys are the resource path without the
// extension ("icons/ScrollUp", "status/NFC", "buttons/style_a/FaceEast"),
// the shape Resources.qml's URL helpers produce, so a call site that
// names a Qt asset names the same file here. Every source is authored
// white on transparent; the UI tints with `colorize`. The two-tone
// favorite heart is the exception: its white fill and black keyline map
// to the theme's marker roles before rasterization, as the Qt provider
// does with its foreground and background tokens.

use std::cell::RefCell;
use std::collections::HashMap;

const SOURCES: &[(&str, &str)] = &[
    // Hub category art.
    (
        "categories/Arcade",
        include_str!("../../../resources/images/categories/Arcade.svg"),
    ),
    (
        "categories/Computer",
        include_str!("../../../resources/images/categories/Computer.svg"),
    ),
    (
        "categories/Console",
        include_str!("../../../resources/images/categories/Console.svg"),
    ),
    (
        "categories/Favorites",
        include_str!("../../../resources/images/categories/Favorites.svg"),
    ),
    (
        "categories/Handheld",
        include_str!("../../../resources/images/categories/Handheld.svg"),
    ),
    (
        "categories/Media",
        include_str!("../../../resources/images/categories/Media.svg"),
    ),
    (
        "categories/Other",
        include_str!("../../../resources/images/categories/Other.svg"),
    ),
    // General UI glyphs.
    (
        "icons/Appearance",
        include_str!("../../../resources/images/icons/Appearance.svg"),
    ),
    (
        "icons/Browsing",
        include_str!("../../../resources/images/icons/Browsing.svg"),
    ),
    (
        "icons/Controls",
        include_str!("../../../resources/images/icons/Controls.svg"),
    ),
    (
        "icons/Display",
        include_str!("../../../resources/images/icons/Display.svg"),
    ),
    (
        "icons/File",
        include_str!("../../../resources/images/icons/File.svg"),
    ),
    (
        "icons/Folder",
        include_str!("../../../resources/images/icons/Folder.svg"),
    ),
    (
        "icons/Heart",
        include_str!("../../../resources/images/icons/Heart.svg"),
    ),
    (
        "icons/HeartOutline",
        include_str!("../../../resources/images/icons/HeartOutline.svg"),
    ),
    (
        "icons/History",
        include_str!("../../../resources/images/icons/History.svg"),
    ),
    (
        "icons/Language",
        include_str!("../../../resources/images/icons/Language.svg"),
    ),
    (
        "icons/Library",
        include_str!("../../../resources/images/icons/Library.svg"),
    ),
    (
        "icons/Loading",
        include_str!("../../../resources/images/icons/Loading.svg"),
    ),
    (
        "icons/NavDown",
        include_str!("../../../resources/images/icons/NavDown.svg"),
    ),
    (
        "icons/NavLeft",
        include_str!("../../../resources/images/icons/NavLeft.svg"),
    ),
    (
        "icons/NavRight",
        include_str!("../../../resources/images/icons/NavRight.svg"),
    ),
    (
        "icons/NavUp",
        include_str!("../../../resources/images/icons/NavUp.svg"),
    ),
    (
        "icons/PlayOutline",
        include_str!("../../../resources/images/icons/PlayOutline.svg"),
    ),
    (
        "icons/RefreshCw",
        include_str!("../../../resources/images/icons/RefreshCw.svg"),
    ),
    (
        "icons/ScrollDown",
        include_str!("../../../resources/images/icons/ScrollDown.svg"),
    ),
    (
        "icons/ScrollUp",
        include_str!("../../../resources/images/icons/ScrollUp.svg"),
    ),
    (
        "icons/Settings",
        include_str!("../../../resources/images/icons/Settings.svg"),
    ),
    (
        "icons/Support",
        include_str!("../../../resources/images/icons/Support.svg"),
    ),
    (
        "icons/Tools",
        include_str!("../../../resources/images/icons/Tools.svg"),
    ),
    // Header HUD status icons.
    (
        "status/BatteryEmpty",
        include_str!("../../../resources/images/status/BatteryEmpty.svg"),
    ),
    (
        "status/BatteryFull",
        include_str!("../../../resources/images/status/BatteryFull.svg"),
    ),
    (
        "status/BatteryHalf",
        include_str!("../../../resources/images/status/BatteryHalf.svg"),
    ),
    (
        "status/BatteryLow",
        include_str!("../../../resources/images/status/BatteryLow.svg"),
    ),
    (
        "status/Bluetooth",
        include_str!("../../../resources/images/status/Bluetooth.svg"),
    ),
    (
        "status/NFC",
        include_str!("../../../resources/images/status/NFC.svg"),
    ),
    (
        "status/WiFi",
        include_str!("../../../resources/images/status/WiFi.svg"),
    ),
    (
        "status/WiredNetwork",
        include_str!("../../../resources/images/status/WiredNetwork.svg"),
    ),
    // Help-bar controller glyphs, one set per style (Kenney CC0 "Input
    // Prompts", see src/LICENSES/Kenney-ATTRIBUTION.txt).
    (
        "buttons/style_a/Dpad",
        include_str!("../../../resources/images/buttons/style_a/Dpad.svg"),
    ),
    (
        "buttons/style_a/DpadUp",
        include_str!("../../../resources/images/buttons/style_a/DpadUp.svg"),
    ),
    (
        "buttons/style_a/DpadDown",
        include_str!("../../../resources/images/buttons/style_a/DpadDown.svg"),
    ),
    (
        "buttons/style_a/DpadLeft",
        include_str!("../../../resources/images/buttons/style_a/DpadLeft.svg"),
    ),
    (
        "buttons/style_a/DpadRight",
        include_str!("../../../resources/images/buttons/style_a/DpadRight.svg"),
    ),
    (
        "buttons/style_a/FaceEast",
        include_str!("../../../resources/images/buttons/style_a/FaceEast.svg"),
    ),
    (
        "buttons/style_a/FaceNorth",
        include_str!("../../../resources/images/buttons/style_a/FaceNorth.svg"),
    ),
    (
        "buttons/style_a/FaceSouth",
        include_str!("../../../resources/images/buttons/style_a/FaceSouth.svg"),
    ),
    (
        "buttons/style_a/FaceWest",
        include_str!("../../../resources/images/buttons/style_a/FaceWest.svg"),
    ),
    (
        "buttons/style_a/ShoulderL",
        include_str!("../../../resources/images/buttons/style_a/ShoulderL.svg"),
    ),
    (
        "buttons/style_a/ShoulderR",
        include_str!("../../../resources/images/buttons/style_a/ShoulderR.svg"),
    ),
    (
        "buttons/style_b/Dpad",
        include_str!("../../../resources/images/buttons/style_b/Dpad.svg"),
    ),
    (
        "buttons/style_b/DpadUp",
        include_str!("../../../resources/images/buttons/style_b/DpadUp.svg"),
    ),
    (
        "buttons/style_b/DpadDown",
        include_str!("../../../resources/images/buttons/style_b/DpadDown.svg"),
    ),
    (
        "buttons/style_b/DpadLeft",
        include_str!("../../../resources/images/buttons/style_b/DpadLeft.svg"),
    ),
    (
        "buttons/style_b/DpadRight",
        include_str!("../../../resources/images/buttons/style_b/DpadRight.svg"),
    ),
    (
        "buttons/style_b/FaceEast",
        include_str!("../../../resources/images/buttons/style_b/FaceEast.svg"),
    ),
    (
        "buttons/style_b/FaceNorth",
        include_str!("../../../resources/images/buttons/style_b/FaceNorth.svg"),
    ),
    (
        "buttons/style_b/FaceSouth",
        include_str!("../../../resources/images/buttons/style_b/FaceSouth.svg"),
    ),
    (
        "buttons/style_b/FaceWest",
        include_str!("../../../resources/images/buttons/style_b/FaceWest.svg"),
    ),
    (
        "buttons/style_b/ShoulderL",
        include_str!("../../../resources/images/buttons/style_b/ShoulderL.svg"),
    ),
    (
        "buttons/style_b/ShoulderR",
        include_str!("../../../resources/images/buttons/style_b/ShoulderR.svg"),
    ),
    (
        "buttons/style_c/Dpad",
        include_str!("../../../resources/images/buttons/style_c/Dpad.svg"),
    ),
    (
        "buttons/style_c/DpadUp",
        include_str!("../../../resources/images/buttons/style_c/DpadUp.svg"),
    ),
    (
        "buttons/style_c/DpadDown",
        include_str!("../../../resources/images/buttons/style_c/DpadDown.svg"),
    ),
    (
        "buttons/style_c/DpadLeft",
        include_str!("../../../resources/images/buttons/style_c/DpadLeft.svg"),
    ),
    (
        "buttons/style_c/DpadRight",
        include_str!("../../../resources/images/buttons/style_c/DpadRight.svg"),
    ),
    (
        "buttons/style_c/FaceEast",
        include_str!("../../../resources/images/buttons/style_c/FaceEast.svg"),
    ),
    (
        "buttons/style_c/FaceNorth",
        include_str!("../../../resources/images/buttons/style_c/FaceNorth.svg"),
    ),
    (
        "buttons/style_c/FaceSouth",
        include_str!("../../../resources/images/buttons/style_c/FaceSouth.svg"),
    ),
    (
        "buttons/style_c/FaceWest",
        include_str!("../../../resources/images/buttons/style_c/FaceWest.svg"),
    ),
    (
        "buttons/style_c/ShoulderL",
        include_str!("../../../resources/images/buttons/style_c/ShoulderL.svg"),
    ),
    (
        "buttons/style_c/ShoulderR",
        include_str!("../../../resources/images/buttons/style_c/ShoulderR.svg"),
    ),
    (
        "buttons/style_d/Dpad",
        include_str!("../../../resources/images/buttons/style_d/Dpad.svg"),
    ),
    (
        "buttons/style_d/DpadUp",
        include_str!("../../../resources/images/buttons/style_d/DpadUp.svg"),
    ),
    (
        "buttons/style_d/DpadDown",
        include_str!("../../../resources/images/buttons/style_d/DpadDown.svg"),
    ),
    (
        "buttons/style_d/DpadLeft",
        include_str!("../../../resources/images/buttons/style_d/DpadLeft.svg"),
    ),
    (
        "buttons/style_d/DpadRight",
        include_str!("../../../resources/images/buttons/style_d/DpadRight.svg"),
    ),
    (
        "buttons/style_d/FaceEast",
        include_str!("../../../resources/images/buttons/style_d/FaceEast.svg"),
    ),
    (
        "buttons/style_d/FaceNorth",
        include_str!("../../../resources/images/buttons/style_d/FaceNorth.svg"),
    ),
    (
        "buttons/style_d/FaceSouth",
        include_str!("../../../resources/images/buttons/style_d/FaceSouth.svg"),
    ),
    (
        "buttons/style_d/FaceWest",
        include_str!("../../../resources/images/buttons/style_d/FaceWest.svg"),
    ),
    (
        "buttons/style_d/ShoulderL",
        include_str!("../../../resources/images/buttons/style_d/ShoulderL.svg"),
    ),
    (
        "buttons/style_d/ShoulderR",
        include_str!("../../../resources/images/buttons/style_d/ShoulderR.svg"),
    ),
    (
        "buttons/style_e/Dpad",
        include_str!("../../../resources/images/buttons/style_e/Dpad.svg"),
    ),
    (
        "buttons/style_e/DpadUp",
        include_str!("../../../resources/images/buttons/style_e/DpadUp.svg"),
    ),
    (
        "buttons/style_e/DpadDown",
        include_str!("../../../resources/images/buttons/style_e/DpadDown.svg"),
    ),
    (
        "buttons/style_e/DpadLeft",
        include_str!("../../../resources/images/buttons/style_e/DpadLeft.svg"),
    ),
    (
        "buttons/style_e/DpadRight",
        include_str!("../../../resources/images/buttons/style_e/DpadRight.svg"),
    ),
    (
        "buttons/style_e/FaceEast",
        include_str!("../../../resources/images/buttons/style_e/FaceEast.svg"),
    ),
    (
        "buttons/style_e/FaceNorth",
        include_str!("../../../resources/images/buttons/style_e/FaceNorth.svg"),
    ),
    (
        "buttons/style_e/FaceSouth",
        include_str!("../../../resources/images/buttons/style_e/FaceSouth.svg"),
    ),
    (
        "buttons/style_e/FaceWest",
        include_str!("../../../resources/images/buttons/style_e/FaceWest.svg"),
    ),
    (
        "buttons/style_e/ShoulderL",
        include_str!("../../../resources/images/buttons/style_e/ShoulderL.svg"),
    ),
    (
        "buttons/style_e/ShoulderR",
        include_str!("../../../resources/images/buttons/style_e/ShoulderR.svg"),
    ),
];

/// Short keys the prototype screens still use, mapped onto the resource
/// paths. The Hub action tiles follow HubScreen.qml's own choices.
const ALIASES: &[(&str, &str)] = &[
    ("Arcade", "categories/Arcade"),
    ("Computer", "categories/Computer"),
    ("Console", "categories/Console"),
    ("Handheld", "categories/Handheld"),
    ("Resume", "icons/PlayOutline"),
    ("Favorites", "icons/HeartOutline"),
    ("Recents", "icons/History"),
    ("Update", "icons/RefreshCw"),
    ("Settings", "icons/Tools"),
    ("NFC", "status/NFC"),
    ("WiFi", "status/WiFi"),
    ("WiredNetwork", "status/WiredNetwork"),
    ("Bluetooth", "status/Bluetooth"),
    ("ScrollUp", "icons/ScrollUp"),
    ("ScrollDown", "icons/ScrollDown"),
    ("Display", "icons/Display"),
    ("Browsing", "icons/Browsing"),
    ("Language", "icons/Language"),
    ("Controls", "icons/Controls"),
    ("Library", "icons/Library"),
    ("Support", "icons/Support"),
    ("Heart", "icons/Heart"),
];

/// Theme tints for the two-tone heart: (white fill, black keyline) as
/// hex, set from the palette's marker and marker-outline roles.
type HeartTints = (String, String);

thread_local! {
    /// Raster cache keyed by (key, px): res switches alternate
    /// between two sizes, so both stay warm. Thread-local because
    /// slint::Image is not Send and the GlyphSource callback only
    /// runs on the UI thread.
    static CACHE: RefCell<HashMap<(String, u32), slint::Image>> = RefCell::new(HashMap::new());
    static HEART: RefCell<HeartTints> = RefCell::new(("#ec5545".to_string(), "#f6e4e0".to_string()));
}

/// Re-tint the favorite heart for a new palette and drop its rasters.
pub fn set_heart_tints(fill: (u8, u8, u8), outline: (u8, u8, u8)) {
    let hex = |(r, g, b): (u8, u8, u8)| format!("#{r:02x}{g:02x}{b:02x}");
    HEART.with(|h| *h.borrow_mut() = (hex(fill), hex(outline)));
    CACHE.with(|c| c.borrow_mut().retain(|(key, _), _| key != "icons/Heart"));
}

fn resolve(key: &str) -> &str {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map_or(key, |(_, path)| *path)
}

#[cfg(test)]
fn has_glyph(key: &str) -> bool {
    let key = resolve(key);
    SOURCES.iter().any(|(k, _)| *k == key)
}

/// Rasterize an SVG document into a square `px` image. Shared with the
/// user's own override artwork, which arrives as a file rather than an
/// embedded source.
pub fn rasterize_svg(svg: &str, px: u32) -> Option<slint::Image> {
    if px == 0 {
        return None;
    }
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let scale = (px as f32 / size.width()).min(px as f32 / size.height());
    // Center the artwork when its viewbox is not square, so a wide
    // glyph does not sit flush left in its cell.
    let dx = (px as f32 - size.width() * scale) / 2.0;
    let dy = (px as f32 - size.height() * scale) / 2.0;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    // tiny-skia's output is premultiplied RGBA, exactly what Slint's
    // premultiplied constructor expects.
    let buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixmap.data(), px, px);
    Some(slint::Image::from_rgba8_premultiplied(buffer))
}

/// Rasterize `key` at `px` x `px` (every source has a square viewbox).
pub fn render(key: &str, px: u32) -> Option<slint::Image> {
    if px == 0 {
        return None;
    }
    let key = resolve(key);
    let cached = CACHE.with(|c| c.borrow().get(&(key.to_string(), px)).cloned());
    if let Some(image) = cached {
        return Some(image);
    }

    let (_, svg) = SOURCES.iter().find(|(k, _)| *k == key)?;
    let tinted;
    let svg: &str = if key == "icons/Heart" {
        let (fill, outline) = HEART.with(|h| h.borrow().clone());
        tinted = svg.replace("#fff", &fill).replace("#000", &outline);
        tinted.as_str()
    } else {
        svg
    };
    let image = rasterize_svg(svg, px)?;

    CACHE.with(|c| {
        c.borrow_mut().insert((key.to_string(), px), image.clone());
    });
    Some(image)
}

#[cfg(test)]
#[allow(
    clippy::panic,
    clippy::expect_used,
    reason = "tests should fail fast on a glyph that does not render"
)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_rasterizes_with_visible_pixels() {
        for (key, _) in SOURCES {
            let image = render(key, 48).unwrap_or_else(|| panic!("{key} did not render"));
            assert_eq!(image.size().width, 48, "{key}");
            let buffer = image
                .to_rgba8_premultiplied()
                .unwrap_or_else(|| panic!("{key} has no pixel buffer"));
            assert!(
                buffer.as_slice().iter().any(|p| p.a > 0),
                "{key} rendered fully transparent"
            );
        }
    }

    #[test]
    fn aliases_resolve_and_unknown_keys_are_none() {
        assert!(render("NotAGlyph", 96).is_none());
        assert!(!has_glyph("NotAGlyph"));
        assert!(has_glyph("Arcade"));
        assert!(has_glyph("buttons/style_e/FaceEast"));
        for (alias, path) in ALIASES {
            assert_eq!(resolve(alias), *path);
            assert!(has_glyph(path), "{path}");
        }
    }

    #[test]
    fn heart_tints_follow_the_palette() {
        set_heart_tints((1, 2, 3), (4, 5, 6));
        let image = render("icons/Heart", 32).expect("heart renders");
        let buffer = image.to_rgba8_premultiplied().expect("pixels");
        assert!(buffer.as_slice().iter().any(|p| p.a > 0));
    }
}
