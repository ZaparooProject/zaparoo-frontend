// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Localized system display names sourced from the Names_MiSTer project
// (ThreepwoodLeBrush/Names_MiSTer, CC0 1.0 Universal), re-keyed to
// Zaparoo Core canonical system IDs and restyled for this frontend.
// See src/LICENSES/Names_MiSTer-ATTRIBUTION.txt for attribution details.
//
// Three locale sets (US / EU / JP) are embedded at compile time and parsed
// lazily into process-lifetime HashMaps. Each map key is the Core system ID
// after normalization (strip non-alphanumerics, lowercase); the value is the
// cleaned display name (qualifier suffixes stripped as a defensive measure).
//
// Keys in the .txt files are Core canonical system IDs, so the lookup is
// direct after normalization. The ID_ALIASES table covers any remaining
// cases where normalization alone cannot bridge the gap.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::system_region::Region;

const US_NAMES: &str = include_str!("data/names_us.txt");
const EU_NAMES: &str = include_str!("data/names_eu.txt");
const JP_NAMES: &str = include_str!("data/names_jp.txt");

/// Noise qualifier suffixes that are purely implementation markers and carry no
/// user-visible meaning (cycle-accurate `+`, Sinden lightgun `S`, LLAPI input
/// `LLAPI`, wide-aspect `W`, 3D framepacked `3D`). These are stripped entirely.
///
/// Order matters: check longer suffixes first to avoid partial matches.
const NOISE_QUALIFIERS: &[&str] = &[" LLAPI", " 3D", " +", " S", " W"];

/// Explicit alias table for cases where normalization alone cannot map a
/// Zaparoo system id to a key in the names files. Both sides must already be
/// in normalized form (alphanumeric-only, lowercase). Add entries here
/// as drift is found.
///
/// Format: (`zaparoo_normalized_id`, `names_file_normalized_key`)
const ID_ALIASES: &[(&str, &str)] = &[
    // None needed yet -- direct normalization covers all known cases.
];

/// Strip all non-alphanumeric characters and lowercase the result.
/// Applied identically to both file keys (at parse time) and
/// Zaparoo system ids (at lookup time) so the match is fuzzy-by-convention.
/// Shared with `system_name_overrides` so user-config keys are matched with
/// the exact same forgiving rule as the bundled names.
pub(crate) fn normalize_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Remove trailing noise qualifiers from a display name, preserving the `2P`
/// variant marker when present.
///
/// Names like `"Game Boy Advance + 2P"` carry two qualifiers: a noise suffix
/// (`+`) and a variant marker (`2P`). Stripping left to right in a single pass
/// would remove `2P` first, leaving the stray `+`. Instead:
///
/// 1. Check whether the name ends with ` 2P`; if so, peel it off temporarily.
/// 2. Strip all noise qualifiers from the remainder in a loop.
/// 3. Re-append ` 2P` if it was present.
///
/// The curated names files are clean and do not carry these suffixes, so this
/// function is a no-op for all current entries. It is kept as a defensive
/// measure in case raw upstream data is ever mixed in.
fn strip_qualifiers(name: &str) -> String {
    let trimmed = name.trim();
    let (base, has_2p) = match trimmed.strip_suffix(" 2P") {
        Some(b) => (b.trim_end(), true),
        None => (trimmed, false),
    };
    let mut s = base;
    loop {
        let mut changed = false;
        for q in NOISE_QUALIFIERS {
            if let Some(stripped) = s.strip_suffix(q) {
                s = stripped.trim_end();
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    if has_2p {
        format!("{s} 2P")
    } else {
        s.to_owned()
    }
}

/// Parse a localized names `.txt` file into a `(normalized_key -> display_name)` map.
/// Lines beginning with `#` and blank lines are silently skipped.
/// The display name is qualifier-stripped before storage.
fn parse_names(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let Some((key_raw, value_raw)) = line.split_once(':') else {
            continue;
        };
        let key = key_raw.trim();
        if key.is_empty() || key.contains('|') {
            continue;
        }
        let display = strip_qualifiers(value_raw);
        if display.is_empty() {
            continue;
        }
        map.insert(normalize_key(key), display);
    }
    map
}

static US_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
static EU_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
static JP_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

fn map_for(region: Region) -> &'static HashMap<String, String> {
    match region {
        Region::Us => US_MAP.get_or_init(|| parse_names(US_NAMES)),
        Region::Eu => EU_MAP.get_or_init(|| parse_names(EU_NAMES)),
        Region::Jp => JP_MAP.get_or_init(|| parse_names(JP_NAMES)),
    }
}

/// Return a localized display name for `system_id` in `region`, or `None` if
/// no match exists. Callers should fall back to the Core catalog name on `None`.
pub fn localized_name(system_id: &str, region: Region) -> Option<String> {
    let normalized = normalize_key(system_id);
    let map = map_for(region);
    if let Some(name) = map.get(&normalized) {
        return Some(name.clone());
    }
    // Alias table: map Zaparoo normalized id to a different normalized key.
    if let Some(alias_key) = ID_ALIASES
        .iter()
        .find_map(|(id, key)| (*id == normalized.as_str()).then_some(*key))
    {
        return map.get(alias_key).cloned();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{localized_name, normalize_key, strip_qualifiers};
    use crate::system_region::Region;

    // --- normalize_key ---

    #[test]
    fn normalize_strips_spaces_and_lowercases() {
        assert_eq!(normalize_key("Game Gear"), "gamegear");
        assert_eq!(normalize_key("Atari 2600"), "atari2600");
        assert_eq!(normalize_key("WonderSwan Color"), "wonderswancolor");
    }

    #[test]
    fn normalize_strips_hyphens_and_underscores() {
        assert_eq!(normalize_key("ZX-Spectrum"), "zxspectrum");
        assert_eq!(normalize_key("Apple-II"), "appleii");
        assert_eq!(normalize_key("Casio_PV-1000"), "casiopv1000");
        assert_eq!(normalize_key("AY-3-8500"), "ay38500");
    }

    // --- strip_qualifiers ---

    #[test]
    fn strips_plus_qualifier() {
        assert_eq!(strip_qualifiers("Genesis +"), "Genesis");
        assert_eq!(strip_qualifiers("Mega Drive +"), "Mega Drive");
        assert_eq!(strip_qualifiers("Game Boy Advance +"), "Game Boy Advance");
    }

    #[test]
    fn strips_llapi_qualifier() {
        assert_eq!(strip_qualifiers("Super NES LLAPI"), "Super NES");
    }

    #[test]
    fn no_qualifier_unchanged() {
        assert_eq!(strip_qualifiers("Mega Drive"), "Mega Drive");
        assert_eq!(strip_qualifiers("Super Famicom"), "Super Famicom");
        assert_eq!(strip_qualifiers("Master System"), "Master System");
    }

    #[test]
    fn preserves_2p_variant_marker() {
        // "2P" is a variant marker, not noise -- keep it.
        assert_eq!(strip_qualifiers("Game Boy 2P"), "Game Boy 2P");
        assert_eq!(
            strip_qualifiers("Game Boy Advance 2P"),
            "Game Boy Advance 2P"
        );
    }

    #[test]
    fn strips_noise_before_2p() {
        // Defensive: "+" is noise; "2P" must survive.
        assert_eq!(
            strip_qualifiers("Game Boy Advance + 2P"),
            "Game Boy Advance 2P"
        );
    }

    #[test]
    fn strips_multiple_noise_qualifiers() {
        // Pathological: multiple noise suffixes on the same name.
        assert_eq!(strip_qualifiers("Foo + S"), "Foo");
    }

    // --- localized_name - regional split systems ---

    #[test]
    fn genesis_us_is_genesis() {
        assert_eq!(
            localized_name("Genesis", Region::Us).as_deref(),
            Some("Genesis")
        );
    }

    #[test]
    fn genesis_eu_is_mega_drive() {
        assert_eq!(
            localized_name("Genesis", Region::Eu).as_deref(),
            Some("Mega Drive")
        );
    }

    #[test]
    fn genesis_jp_is_mega_drive() {
        assert_eq!(
            localized_name("Genesis", Region::Jp).as_deref(),
            Some("Mega Drive")
        );
    }

    #[test]
    fn nes_us_is_nes() {
        assert_eq!(localized_name("NES", Region::Us).as_deref(), Some("NES"));
    }

    #[test]
    fn nes_jp_is_famicom() {
        assert_eq!(
            localized_name("NES", Region::Jp).as_deref(),
            Some("Famicom")
        );
    }

    #[test]
    fn snes_us_is_snes() {
        assert_eq!(localized_name("SNES", Region::Us).as_deref(), Some("SNES"));
    }

    #[test]
    fn snes_jp_is_super_famicom() {
        assert_eq!(
            localized_name("SNES", Region::Jp).as_deref(),
            Some("Super Famicom")
        );
    }

    #[test]
    fn mastersystem_us_is_master_system() {
        assert_eq!(
            localized_name("MasterSystem", Region::Us).as_deref(),
            Some("Master System")
        );
    }

    #[test]
    fn mastersystem_jp_is_mark_iii() {
        // Previously broken: MiSTer key "SMS" != Core ID "MasterSystem".
        // Re-keying to Core IDs fixes this.
        assert_eq!(
            localized_name("MasterSystem", Region::Jp).as_deref(),
            Some("Mark III")
        );
    }

    #[test]
    fn megacd_us_is_sega_cd() {
        assert_eq!(
            localized_name("MegaCD", Region::Us).as_deref(),
            Some("Sega CD")
        );
    }

    #[test]
    fn megacd_eu_is_mega_cd() {
        assert_eq!(
            localized_name("MegaCD", Region::Eu).as_deref(),
            Some("Mega-CD")
        );
    }

    #[test]
    fn sega32x_us_is_genesis_32x() {
        assert_eq!(
            localized_name("Sega32X", Region::Us).as_deref(),
            Some("Genesis 32X")
        );
    }

    #[test]
    fn sega32x_eu_is_mega_drive_32x() {
        assert_eq!(
            localized_name("Sega32X", Region::Eu).as_deref(),
            Some("Mega Drive 32X")
        );
    }

    #[test]
    fn sega32x_jp_is_super_32x() {
        // Previously broken: MiSTer key "S32X" != Core ID "Sega32X".
        // Re-keying to Core IDs fixes this.
        assert_eq!(
            localized_name("Sega32X", Region::Jp).as_deref(),
            Some("Super 32X")
        );
    }

    #[test]
    fn turbografx16_us_is_turbografx_16() {
        assert_eq!(
            localized_name("TurboGrafx16", Region::Us).as_deref(),
            Some("TurboGrafx-16")
        );
    }

    #[test]
    fn turbografx16_eu_is_pc_engine() {
        assert_eq!(
            localized_name("TurboGrafx16", Region::Eu).as_deref(),
            Some("PC Engine")
        );
    }

    #[test]
    fn turbografx16_jp_is_pc_engine() {
        assert_eq!(
            localized_name("TurboGrafx16", Region::Jp).as_deref(),
            Some("PC Engine")
        );
    }

    // --- localized_name - previously dead keys now resolved ---

    #[test]
    fn nintendo64_resolves() {
        // Previously broken: MiSTer key "N64" != Core ID "Nintendo64".
        assert_eq!(
            localized_name("Nintendo64", Region::Us).as_deref(),
            Some("Nintendo 64")
        );
    }

    #[test]
    fn amiga_resolves() {
        // Previously broken: MiSTer key "Minimig" != Core ID "Amiga".
        assert_eq!(
            localized_name("Amiga", Region::Us).as_deref(),
            Some("Amiga")
        );
    }

    #[test]
    fn dos_resolves() {
        // Previously broken: MiSTer key "ao486" != Core ID "DOS".
        assert_eq!(localized_name("DOS", Region::Us).as_deref(), Some("MS-DOS"));
    }

    #[test]
    fn gamenwatch_resolves() {
        // Previously broken: MiSTer key "GameAndWatch" != Core ID "GameNWatch".
        assert_eq!(
            localized_name("GameNWatch", Region::Us).as_deref(),
            Some("Game & Watch")
        );
    }

    // --- localized_name - fixed values ---

    #[test]
    fn neogeopocket_is_neo_geo_pocket_not_color() {
        // Previously the MiSTer file had "Neo Geo Pocket Color" for the base handheld.
        // Core has separate NeoGeoPocket and NeoGeoPocketColor systems.
        assert_eq!(
            localized_name("NeoGeoPocket", Region::Us).as_deref(),
            Some("Neo Geo Pocket")
        );
        assert_eq!(
            localized_name("NeoGeoPocketColor", Region::Us).as_deref(),
            Some("Neo Geo Pocket Color")
        );
    }

    #[test]
    fn pokemonmini_has_accent_and_capital_m() {
        assert_eq!(
            localized_name("PokemonMini", Region::Us).as_deref(),
            Some("Pokémon Mini")
        );
    }

    // --- localized_name - normalization bridge ---

    #[test]
    fn normalization_bridges_case_and_digits() {
        // Core ID "Atari2600" normalizes to "atari2600".
        // File key "Atari2600" also normalizes to "atari2600". Match.
        assert_eq!(
            localized_name("Atari2600", Region::Us).as_deref(),
            Some("Atari 2600")
        );
    }

    #[test]
    fn normalization_bridges_hyphens_in_system_id() {
        // Core ID "ZXSpectrum" normalizes to "zxspectrum".
        // File key "ZXSpectrum" also normalizes to "zxspectrum". Match.
        assert_eq!(
            localized_name("ZXSpectrum", Region::Eu).as_deref(),
            Some("ZX Spectrum")
        );
    }

    // --- localized_name - qualifier stripping is a no-op on clean values ---

    #[test]
    fn gba_us_is_game_boy_advance() {
        // File has clean "Game Boy Advance"; strip_qualifiers is a no-op.
        assert_eq!(
            localized_name("GBA", Region::Us).as_deref(),
            Some("Game Boy Advance")
        );
    }

    #[test]
    fn gba2p_us_is_game_boy_advance_2p() {
        // File has clean "Game Boy Advance (2P)"; strip_qualifiers is a no-op.
        assert_eq!(
            localized_name("GBA2P", Region::Us).as_deref(),
            Some("Game Boy Advance (2P)")
        );
    }

    // --- localized_name - missing entries return None ---

    #[test]
    fn nonexistent_id_returns_none() {
        assert_eq!(localized_name("NonExistentSystem", Region::Us), None);
    }
}
