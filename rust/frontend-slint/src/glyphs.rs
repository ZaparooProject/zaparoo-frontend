// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Runtime rasterization of the category/action glyphs from the
// ORIGINAL SVG sources (copied verbatim from the Qt repo's
// resources/images). Rendering at the exact display pixel size makes
// them crisp at every resolution - fixed-size PNG sets minified
// badly, and Slint's `Path` element is unavailable in the no-std
// MiSTer build. Re-rendered whenever the layout size changes (DRS
// res switches included); nine small icons cost ~a millisecond each.

use std::cell::RefCell;
use std::collections::HashMap;

const SOURCES: &[(&str, &str)] = &[
    ("Arcade", include_str!("../ui/glyphs/Arcade.svg")),
    ("Computer", include_str!("../ui/glyphs/Computer.svg")),
    ("Console", include_str!("../ui/glyphs/Console.svg")),
    ("Handheld", include_str!("../ui/glyphs/Handheld.svg")),
    ("Resume", include_str!("../ui/glyphs/Resume.svg")),
    ("Favorites", include_str!("../ui/glyphs/Favorites.svg")),
    ("Recents", include_str!("../ui/glyphs/Recents.svg")),
    ("Update", include_str!("../ui/glyphs/Update.svg")),
    ("Settings", include_str!("../ui/glyphs/Settings.svg")),
    // Header HUD status icons (host status + Core NFC).
    ("NFC", include_str!("../ui/glyphs/NFC.svg")),
    ("WiFi", include_str!("../ui/glyphs/WiFi.svg")),
    (
        "WiredNetwork",
        include_str!("../ui/glyphs/WiredNetwork.svg"),
    ),
    ("Bluetooth", include_str!("../ui/glyphs/Bluetooth.svg")),
    // PagedGrid scroll-arrow chrome.
    ("ScrollUp", include_str!("../ui/glyphs/ScrollUp.svg")),
    ("ScrollDown", include_str!("../ui/glyphs/ScrollDown.svg")),
    // Settings root-grid category icons (SettingsScreen.qml coverKeys).
    ("Display", include_str!("../ui/glyphs/Display.svg")),
    ("Browsing", include_str!("../ui/glyphs/Browsing.svg")),
    ("Language", include_str!("../ui/glyphs/Language.svg")),
    ("Controls", include_str!("../ui/glyphs/Controls.svg")),
    ("Library", include_str!("../ui/glyphs/Library.svg")),
    ("Support", include_str!("../ui/glyphs/Support.svg")),
    // Favorite marker. Authored neutral grayscale (black keyline,
    // white fill); `render` substitutes the theme's two tones, the
    // same mapping the Qt tinted-svg provider applies.
    ("Heart", include_str!("../ui/glyphs/Heart.svg")),
];

/// Theme tints for the two-tone `Heart` glyph: white fill ->
/// stateMarker lavender, black keyline -> bgBar dark outline
/// (Theme.qml tokens; the demo theme is static so they live here).
const HEART_TINTS: [(&str, &str); 2] = [("#fff", "#9898CC"), ("#000", "#0a0a15")];

thread_local! {
    /// Raster cache keyed by (key, px): res switches alternate
    /// between two sizes, so both stay warm. Thread-local because
    /// slint::Image is not Send and the GlyphSource callback only
    /// runs on the UI thread.
    static CACHE: RefCell<HashMap<(String, u32), slint::Image>> = RefCell::new(HashMap::new());
}

#[cfg(test)]
fn has_glyph(key: &str) -> bool {
    SOURCES.iter().any(|(k, _)| *k == key)
}

/// Rasterize `key` at `px` x `px` (glyphs are square viewboxes). The
/// SVGs paint white strokes; the UI tints via `colorize`.
pub fn render(key: &str, px: u32) -> Option<slint::Image> {
    if px == 0 {
        return None;
    }
    let cached = CACHE.with(|c| c.borrow().get(&(key.to_string(), px)).cloned());
    if let Some(image) = cached {
        return Some(image);
    }

    let (_, svg) = SOURCES.iter().find(|(k, _)| *k == key)?;
    let tinted;
    let svg: &str = if key == "Heart" {
        tinted = HEART_TINTS
            .iter()
            .fold((*svg).to_string(), |s, (from, to)| s.replace(from, to));
        tinted.as_str()
    } else {
        svg
    };
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let size = tree.size();
    let scale = (px as f32 / size.width()).min(px as f32 / size.height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // tiny-skia's output is premultiplied RGBA, exactly what Slint's
    // premultiplied constructor expects.
    let buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixmap.data(), px, px);
    let image = slint::Image::from_rgba8_premultiplied(buffer);

    CACHE.with(|c| {
        c.borrow_mut().insert((key.to_string(), px), image.clone());
    });
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_rasterizes_with_visible_pixels() {
        for (key, _) in SOURCES {
            assert!(
                render(key, 96).is_some_and(|image| image.size().width == 96),
                "{key}"
            );
        }
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(render("NotAGlyph", 96).is_none());
        assert!(!has_glyph("NotAGlyph"));
        assert!(has_glyph("Arcade"));
    }
}
