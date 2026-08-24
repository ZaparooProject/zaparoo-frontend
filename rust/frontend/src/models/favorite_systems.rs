// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.FavoriteSystemsModel` — a flat systems list driven by the
// filtered systems-favorites resource.

use crate::image_overrides;
use crate::models::{global_store, with_hidden_browse_prefs_read, with_persist_read};
use crate::system_region::Region;
use crate::{system_logos, system_name_overrides, system_names, system_region};
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use std::collections::HashMap;
use std::pin::Pin;
use zaparoo_core::endpoints::systems_favorites::SystemsFavoritesEndpoint;
use zaparoo_core::media_types::SystemsResult;
use zaparoo_core::remote_resource::ResourceStatus;

use crate::models::systems::{sort_systems_by_display_name, SystemInfo};

const COVER_KEY_ROLE: i32 = 256 + 1;
const NAME_ROLE: i32 = 256 + 2;
const FAVORITE_ROLE: i32 = 256 + 3;
const FILE_STEM_ROLE: i32 = 256 + 4;
const HIDDEN_ROLE: i32 = 256 + 5;
const DISAMBIGUATING_TAGS_ROLE: i32 = 256 + 6;
// Every real row is a real system, never a structural placeholder; the role
// exists only so PagedGrid's `isEmpty` delegate contract (round 6 follow-up
// — see PagedGrid.qml) is satisfied by direct QAbstractListModel callers.
const IS_EMPTY_ROLE: i32 = 256 + 7;
// Round 11: `entryType`/`fileCount` exist so this model satisfies the same
// shared BrowseList/PagedGrid delegate contract GamesModel's folder-count
// suffix uses (see games.rs's identically-named roles). A system row is
// never a folder, so these are constant.
const ENTRY_TYPE_ROLE: i32 = 256 + 8;
const FILE_COUNT_ROLE: i32 = 256 + 9;
// No Favorite Systems row is ever "disabled" (Hub-only concept). The role
// exists only so PagedGrid's `cellItem.disabled` delegate contract (round
// 11 follow-up — see PagedGrid.qml) is satisfied by direct
// QAbstractListModel callers.
const DISABLED_ROLE: i32 = 256 + 10;

#[derive(Default)]
pub struct FavoriteSystemsModelRust {
    systems: Vec<SystemInfo>,
    media_counts: HashMap<String, u32>,
    count: i32,
    total_items: i32,
    loading: bool,
    cover_requests_paused: bool,
    error_message: QString,
    current_detail_image_key: QString,
    current_detail_tags: QString,
    current_detail_loading: bool,
    detail_prefetch_key_next: QString,
    detail_prefetch_key_prev: QString,
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
        #[qproperty(i32, total_items)]
        #[qproperty(bool, loading)]
        #[qproperty(bool, cover_requests_paused)]
        #[qproperty(QString, error_message)]
        #[qproperty(QString, current_detail_image_key)]
        #[qproperty(QString, current_detail_tags)]
        #[qproperty(bool, current_detail_loading)]
        #[qproperty(QString, detail_prefetch_key_next)]
        #[qproperty(QString, detail_prefetch_key_prev)]
        type FavoriteSystemsModel = super::FavoriteSystemsModelRust;

        #[qinvokable]
        fn fetch_more(self: Pin<&mut FavoriteSystemsModel>);

        #[qinvokable]
        fn retry(self: Pin<&mut FavoriteSystemsModel>);

        #[qinvokable]
        fn name_at(self: &FavoriteSystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn path_at(self: &FavoriteSystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn system_id_at(self: &FavoriteSystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn media_count_for_system(self: &FavoriteSystemsModel, system_id: &QString) -> i32;

        #[qinvokable]
        fn index_for_path(self: &FavoriteSystemsModel, path: &QString) -> i32;

        #[qinvokable]
        fn disambiguating_tags_at(self: &FavoriteSystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn clear_current_detail(self: Pin<&mut FavoriteSystemsModel>);

        #[qinvokable]
        fn peek_detail_at(self: Pin<&mut FavoriteSystemsModel>, index: i32);

        #[qinvokable]
        fn load_detail_at(self: Pin<&mut FavoriteSystemsModel>, index: i32);

        #[qinvokable]
        fn refresh_cover_keys(self: Pin<&mut FavoriteSystemsModel>, first_row: i32, count: i32);

        #[qinvokable]
        fn clear_pending_cover_requests(self: Pin<&mut FavoriteSystemsModel>);

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut FavoriteSystemsModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut FavoriteSystemsModel>);

        #[cxx_name = "rowCount"]
        fn row_count(self: &FavoriteSystemsModel, parent: &QModelIndex) -> i32;
        fn data(self: &FavoriteSystemsModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_name = "roleNames"]
        fn role_names(self: &FavoriteSystemsModel) -> QHash_i32_QByteArray;
    }

    impl cxx_qt::Threading for FavoriteSystemsModel {}
    impl cxx_qt::Initialize for FavoriteSystemsModel {}
}

crate::bind_to_endpoint! {
    for ffi::FavoriteSystemsModel,
    endpoint = SystemsFavoritesEndpoint,
    args = (),
    select = project,
    apply = apply_state,
}

fn project(status: &ResourceStatus<SystemsResult>) -> (Option<SystemsResult>, String, bool) {
    match status {
        ResourceStatus::Ready(data) => (Some(data.clone()), String::new(), false),
        ResourceStatus::Errored { message, .. } => (None, message.clone(), false),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, String::new(), true),
    }
}

fn rows_for_catalog(
    catalog: Option<&SystemsResult>,
    hidden_ids: &[String],
    show_hidden: bool,
    region: Region,
) -> Vec<SystemInfo> {
    let mut rows = catalog.map_or_else(Vec::new, |c| {
        c.systems
            .iter()
            .filter_map(|s| {
                let is_hidden = hidden_ids.contains(&s.id);
                if is_hidden && !show_hidden {
                    return None;
                }
                let name = system_name_overrides::lookup(&s.id)
                    .or_else(|| system_names::localized_name(&s.id, region))
                    .unwrap_or_else(|| s.name.clone());
                let cover_key = image_overrides::override_path("systems", &s.id).map_or_else(
                    || format!("systems/{}", system_logos::logo_artwork_stem(&s.id, region)),
                    |p| format!("custom-image/{}", p.display()),
                );
                Some(SystemInfo {
                    id: s.id.clone(),
                    name,
                    cover_key,
                    category: s.category.clone(),
                    release_date: s.release_date.clone(),
                    manufacturer: s.manufacturer.clone(),
                    hidden: is_hidden,
                    zap_script: s.zap_script.clone(),
                })
            })
            .collect()
    });
    sort_systems_by_display_name(&mut rows);
    rows
}

fn favorite_media_counts(catalog: &SystemsResult) -> (HashMap<String, u32>, i32) {
    let counts = catalog
        .systems
        .iter()
        .filter_map(|system| system.media_count.map(|count| (system.id.clone(), count)))
        .collect::<HashMap<_, _>>();
    let total = if counts.len() == catalog.systems.len() {
        counts
            .values()
            .fold(0_u64, |sum, count| sum.saturating_add(u64::from(*count)))
            .min(i32::MAX as u64) as i32
    } else {
        -1
    };
    (counts, total)
}

fn apply_state(
    mut model: Pin<&mut ffi::FavoriteSystemsModel>,
    (data, err, is_loading): (Option<SystemsResult>, String, bool),
) {
    if let Some(data) = data {
        let hidden_ids = with_hidden_browse_prefs_read(|p| p.hidden_system_ids.clone());
        let show_hidden = with_persist_read(|s| s.settings.show_hidden);
        let region = system_region::current_region();
        let rows = rows_for_catalog(Some(&data), &hidden_ids, show_hidden, region);
        let (media_counts, total_items) = favorite_media_counts(&data);
        if model.rust().systems == rows {
            model.as_mut().rust_mut().media_counts = media_counts;
        } else {
            let count = i32::try_from(rows.len()).unwrap_or(i32::MAX);
            model.as_mut().begin_reset_model();
            {
                let mut rust = model.as_mut().rust_mut();
                rust.systems = rows;
                rust.media_counts = media_counts;
                rust.count = count;
            }
            model.as_mut().end_reset_model();
            model.as_mut().count_changed();
        }
        if model.total_items != total_items {
            model.as_mut().set_total_items(total_items);
        }
        if model.loading {
            model.as_mut().set_loading(false);
        }
    } else if model.total_items != -1 {
        model.as_mut().set_total_items(-1);
    }

    let qerr = QString::from(err.as_str());
    if model.error_message != qerr {
        model.as_mut().set_error_message(qerr);
    }

    if !err.is_empty() && model.loading {
        model.as_mut().set_loading(false);
    } else if model.loading != is_loading {
        model.as_mut().set_loading(is_loading);
    }
}

impl ffi::FavoriteSystemsModel {
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
        let s = &self.systems[index.row() as usize];
        match role {
            COVER_KEY_ROLE => QVariant::from(&QString::from(s.cover_key.as_str())),
            NAME_ROLE | FILE_STEM_ROLE => QVariant::from(&QString::from(s.name.as_str())),
            FAVORITE_ROLE | FILE_COUNT_ROLE => QVariant::from(&0_i32),
            HIDDEN_ROLE => QVariant::from(&s.hidden),
            DISAMBIGUATING_TAGS_ROLE => QVariant::from(&QString::default()),
            IS_EMPTY_ROLE | DISABLED_ROLE => QVariant::from(&false),
            ENTRY_TYPE_ROLE => QVariant::from(&QString::from("media")),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut h = QHash::<QHashPair_i32_QByteArray>::default();
        h.insert(COVER_KEY_ROLE, QByteArray::from("coverKey"));
        h.insert(NAME_ROLE, QByteArray::from("name"));
        h.insert(FAVORITE_ROLE, QByteArray::from("favorite"));
        h.insert(FILE_STEM_ROLE, QByteArray::from("fileStem"));
        h.insert(HIDDEN_ROLE, QByteArray::from("hidden"));
        h.insert(
            DISAMBIGUATING_TAGS_ROLE,
            QByteArray::from("disambiguatingTags"),
        );
        h.insert(IS_EMPTY_ROLE, QByteArray::from("isEmpty"));
        h.insert(ENTRY_TYPE_ROLE, QByteArray::from("entryType"));
        h.insert(FILE_COUNT_ROLE, QByteArray::from("fileCount"));
        h.insert(DISABLED_ROLE, QByteArray::from("disabled"));
        h
    }

    fn fetch_more(self: Pin<&mut Self>) {
        // All systems are loaded in one pass from Core. No paging.
    }

    fn retry(self: Pin<&mut Self>) {
        global_store()
            .subscribe::<SystemsFavoritesEndpoint>(())
            .refetch();
    }

    fn clear_current_detail(mut self: Pin<&mut Self>) {
        self.as_mut().set_current_detail_loading(false);
        self.as_mut()
            .set_current_detail_image_key(QString::default());
        self.as_mut().set_current_detail_tags(QString::default());
        self.as_mut()
            .set_detail_prefetch_key_next(QString::default());
        self.as_mut()
            .set_detail_prefetch_key_prev(QString::default());
    }

    fn peek_detail_at(mut self: Pin<&mut Self>, index: i32) {
        self.as_mut().load_detail_at(index);
    }

    fn load_detail_at(mut self: Pin<&mut Self>, _index: i32) {
        self.as_mut().set_current_detail_loading(false);
        self.as_mut()
            .set_current_detail_image_key(QString::default());
        self.as_mut().set_current_detail_tags(QString::default());
    }

    fn refresh_cover_keys(self: Pin<&mut Self>, _first_row: i32, _count: i32) {}

    fn clear_pending_cover_requests(self: Pin<&mut Self>) {}

    fn name_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].name.as_str())
    }

    fn path_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].id.as_str())
    }

    fn system_id_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].id.as_str())
    }

    fn media_count_for_system(&self, system_id: &QString) -> i32 {
        self.media_counts
            .get(&system_id.to_string())
            .and_then(|count| i32::try_from(*count).ok())
            .unwrap_or(-1)
    }

    fn index_for_path(&self, path: &QString) -> i32 {
        let needle = path.to_string();
        if needle.is_empty() {
            return -1;
        }
        self.systems
            .iter()
            .position(|s| s.id == needle)
            .map_or(-1, |i| i as i32)
    }

    fn disambiguating_tags_at(&self, _index: i32) -> QString {
        QString::default()
    }
}

#[cfg(test)]
mod tests {
    use super::favorite_media_counts;
    use zaparoo_core::media_types::{SystemInfo, SystemsResult};

    #[test]
    fn favorite_counts_sum_exact_system_counts() {
        let catalog = SystemsResult {
            systems: vec![
                SystemInfo {
                    id: "NES".into(),
                    media_count: Some(4),
                    ..SystemInfo::default()
                },
                SystemInfo {
                    id: "SNES".into(),
                    media_count: Some(7),
                    ..SystemInfo::default()
                },
            ],
        };
        let (counts, total) = favorite_media_counts(&catalog);
        assert_eq!(counts.get("NES"), Some(&4));
        assert_eq!(counts.get("SNES"), Some(&7));
        assert_eq!(total, 11);
    }

    #[test]
    fn favorite_total_is_unknown_when_a_count_is_missing() {
        let catalog = SystemsResult {
            systems: vec![SystemInfo {
                id: "NES".into(),
                ..SystemInfo::default()
            }],
        };
        let (_, total) = favorite_media_counts(&catalog);
        assert_eq!(total, -1);
    }
}
