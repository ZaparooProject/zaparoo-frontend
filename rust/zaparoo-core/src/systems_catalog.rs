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
    pub fn systems_by_category(&self, category: &str) -> Vec<SystemInfo> {
        let is_other = category.eq_ignore_ascii_case("Other");
        self.systems
            .iter()
            .filter(|s| {
                if is_other {
                    s.category.is_empty()
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
    fn systems_by_category_other_selects_uncategorised() {
        let data = CatalogData {
            systems: vec![
                sys("a", "A", ""),
                sys("b", "B", "Consoles"),
                sys("c", "C", ""),
            ],
            categories: vec!["Consoles".into(), "Other".into()],
        };
        let other = data.systems_by_category("Other");
        assert_eq!(other.len(), 2);
        assert!(other.iter().all(|s| s.category.is_empty()));
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
