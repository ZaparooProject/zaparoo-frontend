// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Regional logo variant selection.
//
// `logo_artwork_stem` maps a (system_id, region) pair to the filename stem
// of the SVG that should be used for that system's tile logo. When no
// regional variant exists yet, the base system id is returned unchanged and
// the tinted SVG provider falls back to `images/systems/{id}.svg`.
//
// Art naming convention for future variants:
//   Base logo:        resources/images/systems/{id}.svg
//   Regional variant: resources/images/systems/{id}.{region}.svg
//     where {region} is "us", "eu", or "jp".
//
// Regional logo art status:
//   - Genesis / Mega Drive     (EU/JP): Genesis.eu.svg, Genesis.jp.svg
//   - SNES / Super Famicom     (JP):    SNES.jp.svg
//   - NES / Famicom            (JP):    NES.jp.svg
//   - MasterSystem / Mark III  (JP):    MasterSystem.jp.svg
//   - MegaCD / Sega CD         (US):    MegaCD.us.svg
//   - TurboGrafx16 / PC Engine (EU/JP): TurboGrafx16.eu.svg, TurboGrafx16.jp.svg
//   - TurboGrafx-CD / PCE CD   (EU/JP): TurboGrafx16CD.eu.svg, TurboGrafx16CD.jp.svg
//   - Sega32X / Super 32X      (JP):    Sega32X.jp.svg  [Wikimedia placeholder]

use crate::system_region::Region;

/// Regional logo variant table.
///
/// Each entry is `(base_system_id, region, variant_stem)`. When a
/// (`system_id`, region) pair matches, `logo_artwork_stem` returns
/// `variant_stem`; otherwise the base `system_id` is returned.
const REGIONAL_LOGOS: &[(&str, Region, &str)] = &[
    ("Genesis", Region::Eu, "Genesis.eu"),
    ("Genesis", Region::Jp, "Genesis.jp"),
    ("SNES", Region::Jp, "SNES.jp"),
    ("NES", Region::Jp, "NES.jp"),
    ("MasterSystem", Region::Jp, "MasterSystem.jp"),
    ("MegaCD", Region::Us, "MegaCD.us"),
    ("TurboGrafx16", Region::Eu, "TurboGrafx16.eu"),
    ("TurboGrafx16", Region::Jp, "TurboGrafx16.jp"),
    ("TurboGrafx16CD", Region::Eu, "TurboGrafx16CD.eu"),
    ("TurboGrafx16CD", Region::Jp, "TurboGrafx16CD.jp"),
    ("Sega32X", Region::Jp, "Sega32X.jp"), // Wikimedia placeholder pending real Super 32X JP art
];

/// Return the artwork stem for a system in a given region.
/// The returned value is used to build the cover key `systems/{stem}`.
/// Falls back to `system_id` when no regional variant is registered.
pub fn logo_artwork_stem(system_id: &str, region: Region) -> &str {
    REGIONAL_LOGOS
        .iter()
        .find_map(|(id, r, stem)| {
            if *id == system_id && *r == region {
                Some(*stem)
            } else {
                None
            }
        })
        .unwrap_or(system_id)
}

#[cfg(test)]
mod tests {
    use super::logo_artwork_stem;
    use crate::system_region::Region;

    #[test]
    fn registered_variants_return_variant_stem() {
        assert_eq!(logo_artwork_stem("Genesis", Region::Eu), "Genesis.eu");
        assert_eq!(logo_artwork_stem("Genesis", Region::Jp), "Genesis.jp");
        assert_eq!(logo_artwork_stem("SNES", Region::Jp), "SNES.jp");
        assert_eq!(logo_artwork_stem("NES", Region::Jp), "NES.jp");
        assert_eq!(
            logo_artwork_stem("MasterSystem", Region::Jp),
            "MasterSystem.jp"
        );
        assert_eq!(logo_artwork_stem("MegaCD", Region::Us), "MegaCD.us");
        assert_eq!(
            logo_artwork_stem("TurboGrafx16", Region::Eu),
            "TurboGrafx16.eu"
        );
        assert_eq!(
            logo_artwork_stem("TurboGrafx16", Region::Jp),
            "TurboGrafx16.jp"
        );
        assert_eq!(
            logo_artwork_stem("TurboGrafx16CD", Region::Eu),
            "TurboGrafx16CD.eu"
        );
        assert_eq!(
            logo_artwork_stem("TurboGrafx16CD", Region::Jp),
            "TurboGrafx16CD.jp"
        );
        assert_eq!(logo_artwork_stem("Sega32X", Region::Jp), "Sega32X.jp");
    }

    #[test]
    fn unregistered_region_returns_base_id() {
        // Genesis US has no variant — falls through to base id.
        assert_eq!(logo_artwork_stem("Genesis", Region::Us), "Genesis");
        // SNES EU/US have no variants.
        assert_eq!(logo_artwork_stem("SNES", Region::Us), "SNES");
        assert_eq!(logo_artwork_stem("SNES", Region::Eu), "SNES");
        // MegaCD EU/JP have no variants.
        assert_eq!(logo_artwork_stem("MegaCD", Region::Eu), "MegaCD");
        assert_eq!(logo_artwork_stem("MegaCD", Region::Jp), "MegaCD");
    }

    #[test]
    fn unknown_system_returns_base_id() {
        assert_eq!(logo_artwork_stem("SMS", Region::Eu), "SMS");
        assert_eq!(logo_artwork_stem("Atari2600", Region::Jp), "Atari2600");
    }
}
