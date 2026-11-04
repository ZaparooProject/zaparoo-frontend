// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! What the rename screen saves. A tile's name is typed on the full
//! keyboard (`keyboard::Layers::Full`), so case and punctuation are kept.

/// The longest name a tile takes; the caption elides past what fits.
pub const TILE_NAME_MAX_CHARS: usize = 40;

/// The value to store for a typed name: trimmed, and empty (the tile's own
/// name again) when it is blank or spells the default exactly. A default
/// kept as empty still follows the language and the system's own name.
pub fn tile_name(typed: &str, default_name: &str) -> String {
    let typed = typed.trim();
    if typed == default_name.trim() {
        String::new()
    } else {
        typed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_the_default_or_a_blank_resets() {
        assert_eq!(tile_name("  Coin-op ", "Arcade"), "Coin-op");
        assert_eq!(tile_name("Arcade", "Arcade"), "");
        assert_eq!(tile_name("   ", "Arcade"), "");
        assert_eq!(
            tile_name("arcade", "Arcade"),
            "arcade",
            "case is the user's"
        );
    }
}
