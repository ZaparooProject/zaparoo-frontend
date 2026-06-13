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
// Systems pending regional logo art (add entries to REGIONAL_LOGOS when art lands):
//   - Genesis / Mega Drive        (US: Genesis, EU/JP: MegaDrive variant)
//   - SNES / Super Famicom        (JP: SuperFamicom variant)
//   - TurboGrafx16 / PC Engine    (EU/JP: PCEngine variant)
//   - SMS / Mark III              (JP: MarkIII variant)
//   - MegaCD / SEGA CD            (US: SegaCD variant, EU/JP: MegaCD)
//   - S32X / Super 32X            (JP: Super32X variant)
//   - NES / Famicom               (JP: Famicom variant)

use crate::system_region::Region;

/// Regional logo variant table.
///
/// Each entry is `(base_system_id, region, variant_stem)`. When a
/// (`system_id`, region) pair matches, `logo_artwork_stem` returns
/// `variant_stem`; otherwise the base `system_id` is returned.
///
/// The table is intentionally empty until regional artwork is added.
/// `logo_artwork_stem` is a pure pass-through in the meantime.
const REGIONAL_LOGOS: &[(&str, Region, &str)] = &[
    // Add entries here as regional SVG artwork lands under
    // resources/images/systems/. Example (not yet live):
    // ("Genesis", Region::Eu, "Genesis.eu"),
    // ("Genesis", Region::Jp, "Genesis.jp"),
    // ("SNES",    Region::Jp, "SNES.jp"),
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
    fn unknown_system_returns_base_id() {
        assert_eq!(logo_artwork_stem("Genesis", Region::Us), "Genesis");
        assert_eq!(logo_artwork_stem("SNES", Region::Jp), "SNES");
        assert_eq!(logo_artwork_stem("SMS", Region::Eu), "SMS");
    }

    #[test]
    fn all_regions_fall_through_with_empty_table() {
        for region in [Region::Us, Region::Eu, Region::Jp] {
            assert_eq!(logo_artwork_stem("NES", region), "NES");
        }
    }
}
