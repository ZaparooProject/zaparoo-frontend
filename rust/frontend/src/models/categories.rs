// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::models::with_persist_read;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use std::pin::Pin;
use tracing::debug;
use zaparoo_core::endpoints::catalog::CatalogEndpoint;
use zaparoo_core::remote_resource::ResourceStatus;
use zaparoo_core::systems_catalog::CatalogData;

const NAME_ROLE: i32 = 256 + 1; // Qt::UserRole + 1
const COVER_KEY_ROLE: i32 = 256 + 2;
const HIDDEN_ROLE: i32 = 256 + 3;

// Categories Core surfaces but the frontend doesn't expose. `Media` is
// reserved for non-game content the frontend doesn't have a screen for
// yet (tracked in #21). `Other` — the synthesized bucket for systems with
// no upstream category — is now surfaced: Core's launchables (launch-only
// virtual systems) land there, so it carries real, launchable content.
const HIDDEN_CATEGORIES: &[&str] = &["Media"];

#[derive(Default)]
pub struct CategoriesModelRust {
    /// Raw category list received from Core. Stored so `reproject()` can
    /// re-filter without waiting for a catalog refetch.
    raw: Vec<String>,
    /// Filtered+visible category names in display order.
    categories: Vec<String>,
    /// Parallel to `categories`: true when the category is user-hidden but
    /// visible because `show_hidden` is on. Always false for unhidden items.
    hidden_flags: Vec<bool>,
    count: i32,
    raw_count: i32,
    /// Count of indexed (non-launchable) systems in the catalog. Distinct
    /// from `count` (visible categories) and `raw_count` (all categories):
    /// Core's launchables surface as systems under the `Other` category
    /// even with no media-db index, so `count`/`raw_count` are non-zero on
    /// a fresh device. `indexed_count` ignores launchables, so the first-
    /// run scan prompt in `Main.qml` can tell "no games indexed yet" apart
    /// from "only launchables present".
    indexed_count: i32,
    // Sticky-true flag: flips to true the first time the catalog
    // resolves Ready, never resets. The first-run modal in
    // `Main.qml` gates on `loaded && count === 0` so it only fires
    // after we've seen an authoritative empty catalog — without
    // this we'd misread the initial Default state (count=0,
    // pre-fetch) as "no systems" and fire the modal on every cold
    // launch before Core has answered.
    loaded: bool,
    error_message: QString,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        #[allow(non_snake_case, reason = "Qt class names are PascalCase")]
        type QAbstractListModel;

        type QModelIndex = cxx_qt_lib::QModelIndex;
        type QVariant = cxx_qt_lib::QVariant;
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        type QByteArray = cxx_qt_lib::QByteArray;
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, count)]
        #[qproperty(i32, raw_count)]
        #[qproperty(i32, indexed_count)]
        #[qproperty(bool, loaded)]
        #[qproperty(QString, error_message)]
        type CategoriesModel = super::CategoriesModelRust;

        #[qinvokable]
        fn category_at(self: &CategoriesModel, index: i32) -> QString;

        #[qinvokable]
        fn index_for_category(self: &CategoriesModel, name: &QString) -> i32;

        /// Returns true when the category at `index` is user-hidden and
        /// `show_hidden` is on.
        #[qinvokable]
        fn is_hidden_at(self: &CategoriesModel, index: i32) -> bool;

        /// Re-filter the categories list using the current persisted hidden set
        /// and `show_hidden`. Call after any hide/unhide/toggle so the hub grid
        /// reflects new visibility without waiting for a catalog refetch.
        #[qinvokable]
        fn reproject(self: Pin<&mut CategoriesModel>);

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut CategoriesModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut CategoriesModel>);

        // QAbstractListModel virtual overrides
        #[cxx_name = "rowCount"]
        fn row_count(self: &CategoriesModel, parent: &QModelIndex) -> i32;
        fn data(self: &CategoriesModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_name = "roleNames"]
        fn role_names(self: &CategoriesModel) -> QHash_i32_QByteArray;
    }

    impl cxx_qt::Threading for CategoriesModel {}
    impl cxx_qt::Initialize for CategoriesModel {}
}

crate::bind_to_endpoint! {
    for ffi::CategoriesModel,
    endpoint = CatalogEndpoint,
    args = (),
    select = project,
    apply = apply_state,
}

/// Pull the two pieces this model cares about out of the unified
/// `ResourceStatus`: the raw category list (only present on `Ready`) and the
/// surfaced error message (empty unless `Errored`). Filtering is deferred to
/// `apply_state` / `reproject_inner` so `reproject()` can re-filter in-place
/// without waiting for a catalog refetch.
fn project(status: &ResourceStatus<CatalogData>) -> (Option<(Vec<String>, i32)>, String) {
    match status {
        ResourceStatus::Ready(data) => (
            Some((data.categories.clone(), data.indexed_count() as i32)),
            String::new(),
        ),
        ResourceStatus::Errored { message, .. } => (None, message.clone()),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, String::new()),
    }
}

/// Find `needle` in `haystack` with case-sensitive equality. Returns
/// the position as i32, or -1 if not found / empty needle. The
/// case-sensitive contract is deliberate: `HubState.category` is
/// persisted to disk and the frontend re-derives the row index from
/// that string. A case-insensitive lookup would silently coerce
/// "consoles" into "Consoles" if Core ever returned mixed case,
/// hiding a real upstream bug. Pulled out of `index_for_category`
/// so the contract is unit-testable without a `QObject` instance.
fn position_of(haystack: &[String], needle: &str) -> i32 {
    if needle.is_empty() {
        return -1;
    }
    haystack
        .iter()
        .position(|c| c == needle)
        .map_or(-1, |i| i as i32)
}

/// Apply the frontend-side category presentation rules to the raw list from
/// Core, returning the filtered names and a parallel hidden-flag vector.
///
/// Always drops built-in `HIDDEN_CATEGORIES` (case-insensitive). For
/// `user_hidden` (case-sensitive equality — matches the persisted category
/// string exactly): drops the entry when `show_hidden` is false, includes it
/// with `hidden = true` when true. Pulled out of `apply_state` for test
/// coverage.
fn visible_categories(
    raw: &[String],
    user_hidden: &[String],
    show_hidden: bool,
) -> (Vec<String>, Vec<bool>) {
    let mut names = Vec::with_capacity(raw.len());
    let mut flags = Vec::with_capacity(raw.len());
    for c in raw {
        if HIDDEN_CATEGORIES
            .iter()
            .any(|hidden| c.eq_ignore_ascii_case(hidden))
        {
            continue;
        }
        let is_user_hidden = user_hidden.iter().any(|h| h == c);
        if is_user_hidden && !show_hidden {
            continue;
        }
        names.push(c.clone());
        flags.push(is_user_hidden);
    }
    (names, flags)
}

/// Re-run the visibility filter in-place using the current persisted state.
/// Wraps `begin/endResetModel` + `count_changed` + the `loaded` sticky flag.
fn reproject_inner(mut model: Pin<&mut ffi::CategoriesModel>) {
    let (user_hidden, show_hidden) =
        with_persist_read(|s| (s.hub.hidden_categories.clone(), s.settings.show_hidden));
    let raw = model.rust().raw.clone();
    let (names, flags) = visible_categories(&raw, &user_hidden, show_hidden);
    let count = names.len() as i32;
    debug!(count, categories = ?names, "categories: reproject_inner");
    model.as_mut().begin_reset_model();
    model.as_mut().rust_mut().categories = names;
    model.as_mut().rust_mut().hidden_flags = flags;
    model.as_mut().rust_mut().count = count;
    model.as_mut().end_reset_model();
    model.as_mut().count_changed();
    if !model.loaded {
        model.as_mut().set_loaded(true);
    }
}

fn apply_state(
    mut model: Pin<&mut ffi::CategoriesModel>,
    (ready, err): (Option<(Vec<String>, i32)>, String),
) {
    if let Some((raw, indexed_count)) = ready {
        let raw_count = raw.len() as i32;
        model.as_mut().rust_mut().raw = raw;
        if model.raw_count != raw_count {
            model.as_mut().rust_mut().raw_count = raw_count;
            model.as_mut().raw_count_changed();
        }
        if model.indexed_count != indexed_count {
            model.as_mut().rust_mut().indexed_count = indexed_count;
            model.as_mut().indexed_count_changed();
        }
        reproject_inner(model.as_mut());
    }
    let qerr = QString::from(err.as_str());
    if model.error_message != qerr {
        model.as_mut().set_error_message(qerr);
    }
}

impl ffi::CategoriesModel {
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            0
        } else {
            self.count
        }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() || index.row() < 0 || index.row() >= self.count {
            return QVariant::default();
        }
        let row = index.row() as usize;
        match role {
            NAME_ROLE => {
                let s = &self.categories[row];
                QVariant::from(&QString::from(s.as_str()))
            }
            COVER_KEY_ROLE => {
                // Relative path under `resources/images/` (no extension).
                // Categories without a curated PNG (anything we haven't
                // bundled yet) still emit a key — Tile's Image fails to
                // resolve and the procedural fallback takes over.
                let s = &self.categories[row];
                QVariant::from(&QString::from(format!("categories/{s}").as_str()))
            }
            HIDDEN_ROLE => QVariant::from(&self.hidden_flags[row]),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut hash = QHash::<QHashPair_i32_QByteArray>::default();
        hash.insert(NAME_ROLE, QByteArray::from("name"));
        hash.insert(COVER_KEY_ROLE, QByteArray::from("coverKey"));
        hash.insert(HIDDEN_ROLE, QByteArray::from("hidden"));
        hash
    }

    fn category_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.categories[index as usize].as_str())
    }

    fn index_for_category(&self, name: &QString) -> i32 {
        position_of(&self.categories, &name.to_string())
    }

    fn is_hidden_at(&self, index: i32) -> bool {
        if index < 0 || index >= self.count {
            return false;
        }
        self.hidden_flags[index as usize]
    }

    fn reproject(self: Pin<&mut Self>) {
        reproject_inner(self);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{position_of, visible_categories};

    #[test]
    fn position_of_returns_index_on_case_exact_match() {
        let items = vec!["Consoles".to_string(), "Arcade".to_string()];
        assert_eq!(position_of(&items, "Arcade"), 1);
    }

    #[test]
    fn position_of_is_case_sensitive_and_returns_minus_one_on_mismatch() {
        let items = vec!["Consoles".to_string(), "Arcade".to_string()];
        // Mixed case must NOT match — HubState.category is persisted as
        // an exact string and the lookup is case-sensitive on purpose.
        assert_eq!(position_of(&items, "arcade"), -1);
        assert_eq!(position_of(&items, "ARCADE"), -1);
    }

    #[test]
    fn position_of_empty_needle_returns_minus_one() {
        let items = vec!["Consoles".to_string()];
        assert_eq!(position_of(&items, ""), -1);
    }

    #[test]
    fn position_of_missing_returns_minus_one() {
        let items = vec!["Consoles".to_string()];
        assert_eq!(position_of(&items, "Missing"), -1);
    }

    #[test]
    fn raw_categories_pass_through_in_order() {
        let raw = vec!["Consoles".to_string(), "Arcade".to_string()];
        let (names, flags) = visible_categories(&raw, &[], false);
        assert_eq!(names, vec!["Consoles", "Arcade"]);
        assert_eq!(flags, vec![false, false]);
    }

    #[test]
    fn media_is_filtered_case_insensitively_but_other_is_surfaced() {
        let raw = vec![
            "Arcade".to_string(),
            "Other".to_string(),
            "media".to_string(),
            "Consoles".to_string(),
        ];
        let (names, flags) = visible_categories(&raw, &[], false);
        // `media` is dropped; `Other` now passes through (it holds launchables).
        assert_eq!(names, vec!["Arcade", "Other", "Consoles"]);
        assert_eq!(flags, vec![false, false, false]);
    }

    #[test]
    fn empty_raw_yields_empty_visible_list() {
        let (names, flags) = visible_categories(&[], &[], false);
        assert!(names.is_empty());
        assert!(flags.is_empty());
    }

    #[test]
    fn original_casing_is_preserved_for_visible_entries() {
        let raw = vec!["arcade".to_string(), "CONSOLES".to_string()];
        let (names, _) = visible_categories(&raw, &[], false);
        assert_eq!(names, vec!["arcade", "CONSOLES"]);
    }

    #[test]
    fn user_hidden_category_excluded_when_show_hidden_false() {
        let raw = vec!["Arcade".to_string(), "Consoles".to_string()];
        let user_hidden = vec!["Consoles".to_string()];
        let (names, _) = visible_categories(&raw, &user_hidden, false);
        assert_eq!(names, vec!["Arcade"]);
    }

    #[test]
    fn user_hidden_category_shown_with_flag_when_show_hidden_true() {
        let raw = vec!["Arcade".to_string(), "Consoles".to_string()];
        let user_hidden = vec!["Consoles".to_string()];
        let (names, flags) = visible_categories(&raw, &user_hidden, true);
        assert_eq!(names, vec!["Arcade", "Consoles"]);
        assert_eq!(flags, vec![false, true]);
    }

    #[test]
    fn user_hidden_is_case_sensitive() {
        // The category string in user_hidden must match the raw string exactly.
        // A case mismatch does not hide the category.
        let raw = vec!["Consoles".to_string()];
        let user_hidden = vec!["consoles".to_string()];
        let (names, flags) = visible_categories(&raw, &user_hidden, false);
        assert_eq!(names, vec!["Consoles"]);
        assert_eq!(flags, vec![false]);
    }

    #[test]
    fn builtin_hidden_categories_never_user_unhideable() {
        // Even if "Media" somehow ends up in user_hidden (which should never
        // happen since it's never surfaced as a tile), the builtin filter
        // still drops it before we check user_hidden.
        let raw = vec!["Arcade".to_string(), "Media".to_string()];
        let user_hidden = vec!["Media".to_string()];
        let (names_off, _) = visible_categories(&raw, &user_hidden, false);
        let (names_on, flags_on) = visible_categories(&raw, &user_hidden, true);
        // Media is always gone, regardless of show_hidden.
        assert_eq!(names_off, vec!["Arcade"]);
        assert_eq!(names_on, vec!["Arcade"]);
        assert_eq!(flags_on, vec![false]);
    }
}
