// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `CatalogData` is the shape every consumer of the systems list (the
// AppStatus banner, CategoriesModel, SystemsModel) reads from. The
// fetch + sort + category-derivation pipeline that produces it lives
// behind `crate::endpoints::catalog::CatalogEndpoint`, dispatched by
// `crate::store::Store::subscribe::<CatalogEndpoint>(())`.

use crate::media_types::SystemInfo;

#[derive(Debug, Clone)]
pub struct CatalogData {
    pub systems: Vec<SystemInfo>,
    pub categories: Vec<String>,
}

impl CatalogData {
    /// Count of indexed (non-launchable) systems. A launchable is a
    /// launch-only "virtual" system Core synthesizes without a media-db
    /// index — it carries a non-empty `zap_script`. Since Core's
    /// launchables feature, a device with no `media.db` still returns
    /// these, so "is the catalog empty?" no longer answers "are there
    /// indexed games?". This count does: it ignores launchables and only
    /// tallies real, indexed systems. The first-run scan prompt gates on
    /// it (see `Main.qml` → `_maybeOpenFirstRunIndex`).
    pub fn indexed_count(&self) -> usize {
        self.systems
            .iter()
            .filter(|s| s.zap_script.is_empty())
            .count()
    }

    pub fn systems_by_category(&self, category: &str) -> Vec<SystemInfo> {
        let is_other = category.eq_ignore_ascii_case("Other");
        self.systems
            .iter()
            .filter(|s| {
                if is_other {
                    // "Other" is the catch-all bucket: it collects both
                    // systems with no upstream category (synthesized into
                    // "Other" by `derive_categories`) and systems Core
                    // tags with a literal "Other" category, such as the
                    // MiSTer launchables (`misterLaunchableCategoryOther`).
                    s.category.is_empty() || s.category.eq_ignore_ascii_case("Other")
                } else {
                    s.category.eq_ignore_ascii_case(category)
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(id: &str, name: &str, category: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            ..SystemInfo::default()
        }
    }

    #[test]
    fn systems_by_category_filters_case_insensitively() {
        let data = CatalogData {
            systems: vec![
                sys("a", "A", "Arcade"),
                sys("b", "B", "Consoles"),
                sys("c", "C", "arcade"),
            ],
            categories: vec!["Arcade".into(), "Consoles".into()],
        };
        let arcade = data.systems_by_category("Arcade");
        assert_eq!(arcade.len(), 2);
        assert!(arcade
            .iter()
            .all(|s| s.category.eq_ignore_ascii_case("arcade")));
    }

    #[test]
    fn systems_by_category_other_selects_uncategorised_and_literal_other() {
        let data = CatalogData {
            systems: vec![
                sys("a", "A", ""),
                sys("b", "B", "Consoles"),
                sys("c", "C", ""),
                // A launchable Core tags with a literal "Other" category.
                sys("chess", "Chess", "Other"),
            ],
            categories: vec!["Consoles".into(), "Other".into()],
        };
        let other = data.systems_by_category("Other");
        assert_eq!(other.len(), 3);
        assert!(other
            .iter()
            .all(|s| s.category.is_empty() || s.category.eq_ignore_ascii_case("Other")));
        assert!(other.iter().any(|s| s.id == "chess"));
    }

    fn launchable(id: &str, name: &str, category: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            zap_script: format!("zaparoo://launch/{id}"),
            ..SystemInfo::default()
        }
    }

    #[test]
    fn indexed_count_ignores_launchables() {
        let data = CatalogData {
            systems: vec![
                sys("snes", "Super Nintendo", "Consoles"),
                sys("nes", "Nintendo", "Consoles"),
                launchable("chess", "Chess", "Other"),
                launchable("2048", "2048", "Other"),
            ],
            categories: vec!["Consoles".into(), "Other".into()],
        };
        assert_eq!(data.indexed_count(), 2);
    }

    #[test]
    fn indexed_count_zero_when_only_launchables() {
        let data = CatalogData {
            systems: vec![
                launchable("chess", "Chess", "Other"),
                launchable("2048", "2048", "Other"),
            ],
            categories: vec!["Other".into()],
        };
        assert_eq!(data.indexed_count(), 0);
    }

    #[test]
    fn indexed_count_zero_when_empty() {
        let data = CatalogData {
            systems: Vec::new(),
            categories: Vec::new(),
        };
        assert_eq!(data.indexed_count(), 0);
    }

    #[test]
    fn systems_by_category_missing_returns_empty() {
        let data = CatalogData {
            systems: vec![sys("a", "A", "Arcade")],
            categories: vec!["Arcade".into()],
        };
        assert!(data.systems_by_category("Handhelds").is_empty());
    }
}
