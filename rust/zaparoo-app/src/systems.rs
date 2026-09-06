// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Systems screen's rules: which region drives names and art, the
// localized display name for a Core system id, the regional logo stem,
// the per-category row projection (hidden filter, display names, sort),
// and the ZapScript text a system launches with. Ported from the Qt
// crate's `system_region.rs`, `system_names.rs`, `system_logos.rs` and
// `models/systems.rs`.
//
// Localized system display names come from the Names_MiSTer project
// (ThreepwoodLeBrush/Names_MiSTer, CC0 1.0 Universal), re-keyed to Zaparoo
// Core canonical system ids and restyled for this frontend. See
// src/LICENSES/Names_MiSTer-ATTRIBUTION.txt.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Display region. Values map to the three `Names_MiSTer` locale sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Us,
    Eu,
    Jp,
}

/// Resolve a `Region` from the persisted setting (`auto`, `us`, `eu`,
/// `jp`) and the effective locale: `auto` maps an English locale to US,
/// Japanese to JP, anything else to EU.
pub fn resolve_region(setting: &str, effective_locale: &str) -> Region {
    match setting.trim() {
        "us" => Region::Us,
        "eu" => Region::Eu,
        "jp" => Region::Jp,
        _ => {
            let lang = effective_locale
                .split_once(['_', '-'])
                .map_or(effective_locale, |(lang, _)| lang)
                .to_ascii_lowercase();
            match lang.as_str() {
                "en" => Region::Us,
                "ja" => Region::Jp,
                _ => Region::Eu,
            }
        }
    }
}

const US_NAMES: &str = include_str!("data/names_us.txt");
const EU_NAMES: &str = include_str!("data/names_eu.txt");
const JP_NAMES: &str = include_str!("data/names_jp.txt");

/// Noise qualifier suffixes that carry no user-visible meaning. Longer
/// suffixes first so a partial match cannot win.
const NOISE_QUALIFIERS: &[&str] = &[" LLAPI", " 3D", " +", " S", " W"];

/// Explicit aliases for ids normalization alone cannot bridge. Both sides
/// in normalized form.
const ID_ALIASES: &[(&str, &str)] = &[];

/// Strip every non-alphanumeric character and lowercase, applied to both
/// file keys and Core ids so the match is fuzzy by convention.
pub fn normalize_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Remove trailing noise qualifiers, preserving a `2P` variant marker.
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

/// The localized display name for `system_id` in `region`, or `None` so
/// the caller falls back to the Core catalog name.
pub fn localized_name(system_id: &str, region: Region) -> Option<String> {
    let normalized = normalize_key(system_id);
    let map = map_for(region);
    if let Some(name) = map.get(&normalized) {
        return Some(name.clone());
    }
    ID_ALIASES
        .iter()
        .find_map(|(id, key)| (*id == normalized.as_str()).then_some(*key))
        .and_then(|alias_key| map.get(alias_key).cloned())
}

/// Regional logo variants: `(base id, region, artwork stem)`. Everything
/// else falls back to `resources/images/systems/{id}.svg`.
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
    ("Sega32X", Region::Jp, "Sega32X.jp"),
];

/// The artwork stem for a system in a region (`systems/{stem}`).
pub fn logo_artwork_stem(system_id: &str, region: Region) -> &str {
    REGIONAL_LOGOS
        .iter()
        .find_map(|(id, r, stem)| (*id == system_id && *r == region).then_some(*stem))
        .unwrap_or(system_id)
}

/// The catalog fields the projection reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogSystem {
    pub id: String,
    pub name: String,
    pub category: String,
    /// `zaparoo://...` launch URI for launch-only virtual systems.
    pub zap_script: String,
    pub release_date: String,
    pub manufacturer: String,
    /// Matching media in this system, when Core scoped the catalog (the
    /// favorites list); `None` when it did not count.
    pub media_count: Option<u32>,
}

/// One Systems grid row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemRow {
    pub id: String,
    pub name: String,
    /// `systems/{stem}`, the tinted bundled art.
    pub cover_key: String,
    pub category: String,
    /// User-hidden and shown only because Show hidden items is on.
    pub hidden: bool,
    pub zap_script: String,
    pub release_date: String,
    pub manufacturer: String,
    /// Favorites in this system, when the catalog was scoped to them.
    pub media_count: Option<u32>,
}

impl SystemRow {
    /// The detail pane's rows (`detail_tags_for_system`): category,
    /// release date and manufacturer, blanks dropped so a launch-only
    /// system shows only the fields it has.
    pub fn detail_rows(&self) -> Vec<(&'static str, String)> {
        [
            ("category", self.category.trim()),
            ("release_date", self.release_date.trim()),
            ("manufacturer", self.manufacturer.trim()),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key, value.to_string()))
        .collect()
    }

    /// A launch-only system carries a launch URI instead of browsable
    /// media; whitespace never counts.
    pub fn is_launchable(&self) -> bool {
        !self.zap_script.trim().is_empty()
    }

    /// The `ZapScript` a system launches (or writes to a card) with.
    pub fn launch_text(&self) -> String {
        if self.is_launchable() {
            self.zap_script.clone()
        } else {
            format!("**launch.system:{}", self.id)
        }
    }
}

/// Display name priority: a user override, the localized name, then the
/// Core catalog name.
pub fn display_name(
    id: &str,
    fallback: &str,
    region: Region,
    user_override: Option<&str>,
) -> String {
    user_override
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| localized_name(id, region))
        .unwrap_or_else(|| fallback.to_string())
}

/// Case-insensitive by display name, then exact name, then id.
pub fn sort_by_display_name(rows: &mut [SystemRow]) {
    rows.sort_by_cached_key(|row| (row.name.to_lowercase(), row.name.clone(), row.id.clone()));
}

/// `systems_by_category`: a system with no category belongs to `Other`.
/// Both spellings of a category name are the same category (the Hub's
/// own `canonical_category` rule), so a plural id from an older layout
/// or Core still finds its systems.
fn in_category(system: &CatalogSystem, category: &str) -> bool {
    if system.category.is_empty() {
        crate::hub::canonical_category(category) == "Other"
    } else {
        crate::hub::canonical_category(&system.category) == crate::hub::canonical_category(category)
    }
}

/// The rows of one category: hidden ids dropped (or kept and flagged when
/// `show_hidden`), names localized for `region`, sorted by display name.
pub fn rows_for_category(
    systems: &[CatalogSystem],
    category: &str,
    hidden_ids: &[String],
    show_hidden: bool,
    region: Region,
    user_name: &dyn Fn(&str) -> Option<String>,
) -> Vec<SystemRow> {
    project_rows(systems, hidden_ids, show_hidden, region, user_name, &|s| {
        in_category(s, category)
    })
}

/// Every system in a scoped catalog (the favorites list Core answers with
/// its own `media_count` per system), under the same hide and name rules.
pub fn rows_for_favorites(
    systems: &[CatalogSystem],
    hidden_ids: &[String],
    show_hidden: bool,
    region: Region,
    user_name: &dyn Fn(&str) -> Option<String>,
) -> Vec<SystemRow> {
    project_rows(systems, hidden_ids, show_hidden, region, user_name, &|_| {
        true
    })
}

/// The favorites total across the rows; `None` when Core left any system
/// uncounted, so the caller shows no total rather than a wrong one.
pub fn favorites_total(rows: &[SystemRow]) -> Option<u32> {
    rows.iter()
        .map(|row| row.media_count)
        .try_fold(0_u32, |sum, count| Some(sum.saturating_add(count?)))
}

/// A random favorite from one system, or from every system when the id is
/// blank (`random_favorite_script`).
pub fn random_favorite_launch_text(system_id: &str) -> String {
    let scope = system_id.trim();
    if scope.is_empty() {
        return "**launch.random:all?tags=user:favorite".to_string();
    }
    let mut escaped = String::with_capacity(scope.len());
    for ch in scope.chars() {
        if matches!(ch, '^' | '?' | ',' | '&' | '|') {
            escaped.push('^');
        }
        escaped.push(ch);
    }
    format!("**launch.random:{escaped}?tags=user:favorite")
}

fn project_rows(
    systems: &[CatalogSystem],
    hidden_ids: &[String],
    show_hidden: bool,
    region: Region,
    user_name: &dyn Fn(&str) -> Option<String>,
    keep: &dyn Fn(&CatalogSystem) -> bool,
) -> Vec<SystemRow> {
    let mut rows: Vec<SystemRow> = systems
        .iter()
        .filter(|s| keep(s))
        .filter_map(|s| {
            let hidden = hidden_ids.iter().any(|h| h == &s.id);
            if hidden && !show_hidden {
                return None;
            }
            Some(SystemRow {
                id: s.id.clone(),
                name: display_name(&s.id, &s.name, region, user_name(&s.id).as_deref()),
                cover_key: format!("systems/{}", logo_artwork_stem(&s.id, region)),
                category: s.category.clone(),
                hidden,
                zap_script: s.zap_script.clone(),
                release_date: s.release_date.clone(),
                manufacturer: s.manufacturer.clone(),
                media_count: s.media_count,
            })
        })
        .collect();
    sort_by_display_name(&mut rows);
    rows
}

/// Ids of the indexable (non-launchable) systems in a category, in
/// catalog order.
pub fn indexable_ids(systems: &[CatalogSystem], category: &str) -> Vec<String> {
    systems
        .iter()
        .filter(|s| in_category(s, category) && s.zap_script.trim().is_empty())
        .map(|s| s.id.clone())
        .collect()
}

/// `**launch.random:` over the given systems, with the directive's own
/// escaping (`^` and `,`); `None` when there is nothing to pick from.
pub fn random_launch_text(system_ids: &[String]) -> Option<String> {
    let ids: Vec<String> = system_ids
        .iter()
        .filter(|id| !id.is_empty())
        .map(|id| id.replace('^', "^^").replace(',', "^,"))
        .collect();
    (!ids.is_empty()).then(|| format!("**launch.random:{}", ids.join(",")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_category_matches_its_other_spelling() {
        let systems = vec![
            CatalogSystem {
                id: "SNES".into(),
                name: "Super Nintendo".into(),
                category: "Consoles".into(),
                ..CatalogSystem::default()
            },
            CatalogSystem {
                id: "Loose".into(),
                name: "Loose".into(),
                category: String::new(),
                ..CatalogSystem::default()
            },
        ];
        // The layout's id and Core's own spelling need not agree.
        assert_eq!(
            rows_for_category(&systems, "Console", &[], false, Region::Us, &|_| None).len(),
            1
        );
        assert_eq!(
            rows_for_category(&systems, "Consoles", &[], false, Region::Us, &|_| None).len(),
            1
        );
        // A system with no category still belongs to Other, alone.
        assert_eq!(
            rows_for_category(&systems, "Other", &[], false, Region::Us, &|_| None).len(),
            1
        );
    }

    #[test]
    fn region_resolution_matches_the_qt_rules() {
        assert_eq!(resolve_region("us", "de_DE"), Region::Us);
        assert_eq!(resolve_region("eu", "en_US"), Region::Eu);
        assert_eq!(resolve_region("jp", "en_US"), Region::Jp);
        for locale in ["en_US", "en_GB", "en"] {
            assert_eq!(resolve_region("auto", locale), Region::Us, "{locale}");
        }
        for locale in ["ja_JP", "ja"] {
            assert_eq!(resolve_region("auto", locale), Region::Jp, "{locale}");
        }
        for locale in ["de_DE", "fr_FR", "es_ES", "it_IT", "ko_KR", "zh_CN", ""] {
            assert_eq!(resolve_region("auto", locale), Region::Eu, "{locale}");
        }
        assert_eq!(resolve_region("", "en_US"), Region::Us);
        assert_eq!(resolve_region("unknown", "de_DE"), Region::Eu);
    }

    #[test]
    fn normalize_key_strips_separators_and_case() {
        assert_eq!(normalize_key("Game Gear"), "gamegear");
        assert_eq!(normalize_key("Atari 2600"), "atari2600");
        assert_eq!(normalize_key("ZX-Spectrum"), "zxspectrum");
        assert_eq!(normalize_key("Casio_PV-1000"), "casiopv1000");
        assert_eq!(normalize_key("AY-3-8500"), "ay38500");
    }

    #[test]
    fn strip_qualifiers_keeps_the_2p_marker() {
        assert_eq!(strip_qualifiers("Genesis +"), "Genesis");
        assert_eq!(strip_qualifiers("Super NES LLAPI"), "Super NES");
        assert_eq!(strip_qualifiers("Mega Drive"), "Mega Drive");
        assert_eq!(strip_qualifiers("Game Boy 2P"), "Game Boy 2P");
        assert_eq!(
            strip_qualifiers("Game Boy Advance + 2P"),
            "Game Boy Advance 2P"
        );
        assert_eq!(strip_qualifiers("Foo + S"), "Foo");
    }

    #[test]
    fn localized_names_follow_the_region() {
        let cases = [
            ("Genesis", Region::Us, "Genesis"),
            ("Genesis", Region::Eu, "Mega Drive"),
            ("Genesis", Region::Jp, "Mega Drive"),
            ("NES", Region::Us, "NES"),
            ("NES", Region::Jp, "Famicom"),
            ("SNES", Region::Us, "SNES"),
            ("SNES", Region::Jp, "Super Famicom"),
            ("MasterSystem", Region::Us, "Master System"),
            ("MasterSystem", Region::Jp, "Mark III"),
            ("MegaCD", Region::Us, "Sega CD"),
            ("MegaCD", Region::Eu, "Mega-CD"),
            ("Sega32X", Region::Us, "Genesis 32X"),
            ("Sega32X", Region::Eu, "Mega Drive 32X"),
            ("Sega32X", Region::Jp, "Super 32X"),
            ("TurboGrafx16", Region::Us, "TurboGrafx-16"),
            ("TurboGrafx16", Region::Eu, "PC Engine"),
            ("TurboGrafx16", Region::Jp, "PC Engine"),
            ("Nintendo64", Region::Us, "Nintendo 64"),
            ("Amiga", Region::Us, "Amiga"),
            ("DOS", Region::Us, "MS-DOS"),
            ("GameNWatch", Region::Us, "Game & Watch"),
            ("NeoGeoPocket", Region::Us, "Neo Geo Pocket"),
            ("NeoGeoPocketColor", Region::Us, "Neo Geo Pocket Color"),
            ("PokemonMini", Region::Us, "Pokémon Mini"),
            ("Atari2600", Region::Us, "Atari 2600"),
            ("ZXSpectrum", Region::Eu, "ZX Spectrum"),
            ("GBA", Region::Us, "Game Boy Advance"),
            ("GBA2P", Region::Us, "Game Boy Advance (2P)"),
        ];
        for (id, region, expected) in cases {
            assert_eq!(
                localized_name(id, region).as_deref(),
                Some(expected),
                "{id} {region:?}"
            );
        }
        assert_eq!(localized_name("NonExistentSystem", Region::Us), None);
    }

    #[test]
    fn logo_stems_pick_regional_variants() {
        assert_eq!(logo_artwork_stem("Genesis", Region::Eu), "Genesis.eu");
        assert_eq!(logo_artwork_stem("Genesis", Region::Jp), "Genesis.jp");
        assert_eq!(logo_artwork_stem("SNES", Region::Jp), "SNES.jp");
        assert_eq!(logo_artwork_stem("MegaCD", Region::Us), "MegaCD.us");
        assert_eq!(
            logo_artwork_stem("TurboGrafx16CD", Region::Jp),
            "TurboGrafx16CD.jp"
        );
        assert_eq!(logo_artwork_stem("Genesis", Region::Us), "Genesis");
        assert_eq!(logo_artwork_stem("SNES", Region::Eu), "SNES");
        assert_eq!(logo_artwork_stem("SMS", Region::Eu), "SMS");
    }

    fn sys(id: &str, name: &str, category: &str) -> CatalogSystem {
        CatalogSystem {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            zap_script: String::new(),
            release_date: String::new(),
            manufacturer: String::new(),
            media_count: None,
        }
    }

    fn row(id: &str) -> SystemRow {
        SystemRow {
            id: id.into(),
            name: id.into(),
            cover_key: format!("systems/{id}"),
            category: "Consoles".into(),
            ..Default::default()
        }
    }

    #[test]
    fn favorite_rows_keep_every_system_and_total_their_counts() {
        let mut nes = sys("NES", "NES", "Consoles");
        nes.media_count = Some(3);
        let mut arcade = sys("Arcade", "Arcade", "Arcade");
        arcade.media_count = Some(4);
        let rows = rows_for_favorites(
            &[nes.clone(), arcade.clone()],
            &[],
            false,
            Region::Us,
            &|_| None,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "Arcade");
        assert_eq!(favorites_total(&rows), Some(7));
        // One uncounted system means no trustworthy total.
        let mut unknown = sys("SNES", "SNES", "Consoles");
        unknown.media_count = None;
        let rows = rows_for_favorites(&[nes, unknown], &[], false, Region::Us, &|_| None);
        assert_eq!(favorites_total(&rows), None);
    }

    #[test]
    fn favorite_rows_still_hide_hidden_systems() {
        let rows = rows_for_favorites(
            &[
                sys("NES", "NES", "Consoles"),
                sys("SNES", "SNES", "Consoles"),
            ],
            &["NES".to_string()],
            false,
            Region::Us,
            &|_| None,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "SNES");
        let shown = rows_for_favorites(
            &[sys("NES", "NES", "Consoles")],
            &["NES".to_string()],
            true,
            Region::Us,
            &|_| None,
        );
        assert!(shown[0].hidden);
    }

    #[test]
    fn random_favorite_scopes_to_a_system_or_everything() {
        assert_eq!(
            random_favorite_launch_text(""),
            "**launch.random:all?tags=user:favorite"
        );
        assert_eq!(
            random_favorite_launch_text(" NES "),
            "**launch.random:NES?tags=user:favorite"
        );
        assert_eq!(
            random_favorite_launch_text("a,b"),
            "**launch.random:a^,b?tags=user:favorite"
        );
    }

    #[test]
    fn detail_rows_emit_fixed_rows() {
        let mut system = row("NES");
        system.release_date = "1983".into();
        system.manufacturer = "Nintendo".into();
        assert_eq!(
            system.detail_rows(),
            vec![
                ("category", "Consoles".to_string()),
                ("release_date", "1983".to_string()),
                ("manufacturer", "Nintendo".to_string())
            ]
        );
    }

    #[test]
    fn detail_rows_omit_empty_metadata_rows() {
        assert_eq!(
            row("Chess").detail_rows(),
            vec![("category", "Consoles".to_string())]
        );
    }

    #[test]
    fn detail_rows_keep_populated_rows_only() {
        let mut system = row("NES");
        system.manufacturer = " Nintendo ".into();
        assert_eq!(
            system.detail_rows(),
            vec![
                ("category", "Consoles".to_string()),
                ("manufacturer", "Nintendo".to_string())
            ]
        );
    }

    #[test]
    fn rows_filter_localize_and_sort() {
        let catalog = vec![
            sys("smb", "Super Mario Bros", "Consoles"),
            sys("snk", "SNK Heroes", "Arcade"),
            sys("zelda", "Zelda", "Consoles"),
            sys("Genesis", "Sega Genesis", "Consoles"),
            sys("NoCat", "Loose", ""),
        ];
        let rows = rows_for_category(&catalog, "Consoles", &[], false, Region::Eu, &|_| None);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Mega Drive", "Super Mario Bros", "Zelda"]);
        assert_eq!(rows[0].cover_key, "systems/Genesis.eu");
        assert!(!rows[0].hidden);

        let hidden = vec!["zelda".to_string()];
        assert_eq!(
            rows_for_category(&catalog, "Consoles", &hidden, false, Region::Us, &|_| None).len(),
            2
        );
        let shown = rows_for_category(&catalog, "Consoles", &hidden, true, Region::Us, &|_| None);
        assert_eq!(shown.len(), 3);
        assert!(shown
            .iter()
            .find(|r| r.id == "zelda")
            .is_some_and(|r| r.hidden));

        let other = rows_for_category(&catalog, "Other", &[], false, Region::Us, &|_| None);
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].id, "NoCat");

        let overridden = rows_for_category(&catalog, "Consoles", &[], false, Region::Us, &|id| {
            (id == "Genesis").then(|| "My Genesis".to_string())
        });
        assert_eq!(overridden[0].name, "My Genesis");
    }

    #[test]
    fn sort_is_case_insensitive_then_exact() {
        let mut rows = vec![
            SystemRow {
                id: "b".into(),
                name: "alpha".into(),
                ..SystemRow::default()
            },
            SystemRow {
                id: "a".into(),
                name: "Alpha".into(),
                ..SystemRow::default()
            },
            SystemRow {
                id: "c".into(),
                name: "Zulu".into(),
                ..SystemRow::default()
            },
        ];
        sort_by_display_name(&mut rows);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn launch_texts_and_random_directive() {
        let normal = SystemRow {
            id: "SNES".into(),
            ..SystemRow::default()
        };
        assert!(!normal.is_launchable());
        assert_eq!(normal.launch_text(), "**launch.system:SNES");
        let virtual_system = SystemRow {
            id: "Menu".into(),
            zap_script: "zaparoo://launch/Menu".into(),
            ..SystemRow::default()
        };
        assert!(virtual_system.is_launchable());
        assert_eq!(virtual_system.launch_text(), "zaparoo://launch/Menu");
        assert_eq!(
            random_launch_text(&["SNES".into(), "a,b".into(), "c^d".into(), String::new()]),
            Some("**launch.random:SNES,a^,b,c^^d".into())
        );
        assert_eq!(random_launch_text(&[String::new()]), None);
        assert_eq!(
            indexable_ids(
                &[
                    sys("SNES", "SNES", "Consoles"),
                    CatalogSystem {
                        zap_script: "x".into(),
                        ..sys("Menu", "Menu", "Consoles")
                    }
                ],
                "Consoles"
            ),
            vec!["SNES".to_string()]
        );
    }
}
