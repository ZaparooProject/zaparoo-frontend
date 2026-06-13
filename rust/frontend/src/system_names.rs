// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Localized system display names sourced from the Names_MiSTer project
// (ThreepwoodLeBrush/Names_MiSTer, CC0 1.0 Universal).
// See src/LICENSES/Names_MiSTer-ATTRIBUTION.txt for attribution details.
//
// Three locale sets (US / EU / JP) are embedded at compile time and parsed
// lazily into process-lifetime HashMaps. Each map key is the MiSTer core name
// after normalization (strip non-alphanumerics, lowercase); the value is the
// cleaned display name (qualifier suffixes stripped).
//
// Callers pass a Zaparoo system id; the same normalization is applied so
// the lookup is fuzzy-by-convention. An explicit alias table covers the
// small set of ids where normalization alone cannot bridge the gap.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::system_region::Region;

const US_NAMES: &str = include_str!("data/names_CHAR28_Common_US.txt");
const EU_NAMES: &str = include_str!("data/names_CHAR28_Common_EU.txt");
const JP_NAMES: &str = include_str!("data/names_CHAR28_Common_JP.txt");

/// Qualifier suffixes appended by `Names_MiSTer` to mark capability variants
/// (cycle-accurate `+`, Sinden lightgun `S`, two-player `2P`, 3D framepacked
/// `3D`, LLAPI input `LLAPI`, wide-aspect `W`). Strip these from display names
/// so `"Mega Drive +"` renders as `"Mega Drive"`.
///
/// Order matters: check longer suffixes first to avoid partial matches.
const QUALIFIERS: &[&str] = &[" LLAPI", " 2P", " 3D", " +", " S", " W"];

/// Explicit alias table for cases where normalization alone cannot map a
/// Zaparoo system id to a `MiSTer` core key. Both sides must already be
/// in normalized form (alphanumeric-only, lowercase). Add entries here
/// as drift is found.
///
/// Format: (`zaparoo_normalized_id`, `mister_normalized_key`)
const ID_ALIASES: &[(&str, &str)] = &[
    // None needed yet -- direct normalization covers all known cases.
];

/// Strip all non-alphanumeric characters and lowercase the result.
/// Applied identically to both `MiSTer` core keys (at parse time) and
/// Zaparoo system ids (at lookup time) so the match is fuzzy-by-convention.
fn normalize_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Remove a single trailing capability marker from a display name.
/// Applies the first match in `QUALIFIERS` (longest suffix wins by ordering).
/// At most one pass -- the value from `Names_MiSTer` uses at most one qualifier
/// on the base-core entries that Zaparoo exposes.
fn strip_qualifiers(name: &str) -> &str {
    let trimmed = name.trim();
    for q in QUALIFIERS {
        if let Some(stripped) = trimmed.strip_suffix(q) {
            return stripped.trim_end();
        }
    }
    trimmed
}

/// Parse a `Names_MiSTer` `.txt` file into a `(normalized_key -> display_name)` map.
/// Separator ruler lines (contain `|` in the key position) and blank lines are
/// silently skipped. The display name is qualifier-stripped before storage.
fn parse_names(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
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
        map.insert(normalize_key(key), display.to_string());
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
    // Alias table: map Zaparoo normalized id to a different MiSTer normalized key.
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

    // --- localized_name ---

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
    fn snes_jp_is_super_famicom() {
        assert_eq!(
            localized_name("SNES", Region::Jp).as_deref(),
            Some("Super Famicom")
        );
    }

    #[test]
    fn nes_jp_is_famicom() {
        assert_eq!(
            localized_name("NES", Region::Jp).as_deref(),
            Some("Famicom")
        );
    }

    #[test]
    fn sms_jp_is_mark_iii() {
        assert_eq!(
            localized_name("SMS", Region::Jp).as_deref(),
            Some("Mark III")
        );
    }

    #[test]
    fn sms_us_is_master_system() {
        assert_eq!(
            localized_name("SMS", Region::Us).as_deref(),
            Some("Master System")
        );
    }

    #[test]
    fn s32x_jp_is_super_32x() {
        assert_eq!(
            localized_name("S32X", Region::Jp).as_deref(),
            Some("Super 32X")
        );
    }

    #[test]
    fn megacd_us_is_sega_cd() {
        assert_eq!(
            localized_name("MegaCD", Region::Us).as_deref(),
            Some("SEGA CD")
        );
    }

    #[test]
    fn megacd_eu_is_mega_cd() {
        // The hyphenated "Mega-CD" is stored without the hyphen after stripping.
        // The file has `MegaCD: Mega-CD` -- hyphen is not a qualifier, so the
        // display name is "Mega-CD" (hyphen preserved in the value, not the key).
        assert_eq!(
            localized_name("MegaCD", Region::Eu).as_deref(),
            Some("Mega-CD")
        );
    }

    #[test]
    fn nonexistent_id_returns_none() {
        assert_eq!(localized_name("NonExistentSystem", Region::Us), None);
    }

    #[test]
    fn normalization_bridges_space_in_mister_key() {
        // "Atari 2600" in the file normalizes to "atari2600".
        // Zaparoo id "Atari2600" normalizes to "atari2600". Match.
        assert_eq!(
            localized_name("Atari2600", Region::Us).as_deref(),
            Some("Atari 2600")
        );
    }

    #[test]
    fn normalization_bridges_hyphen_in_mister_key() {
        // "ZX-Spectrum" normalizes to "zxspectrum".
        assert_eq!(
            localized_name("ZXSpectrum", Region::Eu).as_deref(),
            Some("ZX Spectrum")
        );
    }

    #[test]
    fn gba_us_has_plus_stripped() {
        // File has `GBA: Game Boy Advance +`. After qualifier strip: "Game Boy Advance".
        assert_eq!(
            localized_name("GBA", Region::Us).as_deref(),
            Some("Game Boy Advance")
        );
    }

    #[test]
    fn turbo_grafx_us_is_turbo_duo() {
        assert_eq!(
            localized_name("TurboGrafx16", Region::Us).as_deref(),
            Some("Turbo Duo")
        );
    }

    #[test]
    fn turbo_grafx_eu_is_pc_engine_duo() {
        assert_eq!(
            localized_name("TurboGrafx16", Region::Eu).as_deref(),
            Some("PC Engine Duo")
        );
    }
}
