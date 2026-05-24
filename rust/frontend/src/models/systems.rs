// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, trace, warn};
use zaparoo_core::endpoints::catalog::CatalogEndpoint;
use zaparoo_core::endpoints::readers_write::ReadersWriteMutation;
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::media_types::{ReadersWriteParams, RunParams};
use zaparoo_core::remote_resource::ResourceStatus;
use zaparoo_core::systems_catalog::CatalogData;

// `coverKey` rather than `id`: bracket-access via `model["id"]` from a
// QML delegate trips over `id`'s reserved-keyword status, leaving the
// role unreachable. Renaming sidesteps the reservation entirely and
// matches the generic Tile cover-key contract used by all models.
const COVER_KEY_ROLE: i32 = 256 + 1;
const NAME_ROLE: i32 = 256 + 2;
const CATEGORY_ROLE: i32 = 256 + 3;
const FAVORITE_ROLE: i32 = 256 + 4;
const FILE_STEM_ROLE: i32 = 256 + 5;

pub struct SystemInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub release_date: Option<String>,
    pub manufacturer: Option<String>,
}

#[derive(Default)]
pub struct SystemsModelRust {
    systems: Vec<SystemInfo>,
    count: i32,
    loading: bool,
    current_category: QString,
    error_message: QString,
    card_write_pending: bool,
    card_write_error: QString,
    card_write_seq: Arc<AtomicU64>,
    // Last-known-good catalog. Updated by `apply_state` on every
    // `Ready`; never cleared on `Loading`/`Errored`. Lets
    // `set_category` keep populating rows during a transient refetch
    // instead of wiping the grid until the catalog returns to
    // `Ready`.
    last_ready: Option<CatalogData>,
    // Cancellation handle for the in-flight `set_category` filter.
    // The filter itself is short (microseconds on ARM); spawning is
    // what enables the loading→ready four-state UI on the systems
    // grid and aborts a queued result when the user has already
    // moved on.
    pending_task: Option<JoinHandle<()>>,
    // Monotonic ticket bumped on each `set_category`, and again by
    // `apply_state` whenever a fresh catalog supersedes whatever
    // the in-flight worker is computing. The worker's queued
    // closure captures the ticket value at spawn time and bails
    // when `seq` has advanced — closing the window where a stale
    // filter result lands on top of newer rows. Distinct from
    // `card_write_seq` — different concerns, must not share a
    // counter.
    seq: Arc<AtomicU64>,
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
        #[qproperty(bool, loading)]
        #[qproperty(QString, current_category)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, card_write_pending)]
        #[qproperty(QString, card_write_error)]
        type SystemsModel = super::SystemsModelRust;

        #[qinvokable]
        fn set_category(self: Pin<&mut SystemsModel>, category: QString);

        #[qinvokable]
        fn system_id_at(self: &SystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn system_name_at(self: &SystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn detail_tags_at(self: &SystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn write_card_at(self: Pin<&mut SystemsModel>, index: i32);

        #[qinvokable]
        fn launch_at(self: Pin<&mut SystemsModel>, index: i32);

        #[qinvokable]
        fn launch_text_at(self: &SystemsModel, index: i32) -> QString;

        #[qinvokable]
        fn cancel_card_write(self: Pin<&mut SystemsModel>);

        #[qinvokable]
        fn index_for_system_id(self: &SystemsModel, id: &QString) -> i32;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut SystemsModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut SystemsModel>);

        #[cxx_name = "rowCount"]
        fn row_count(self: &SystemsModel, parent: &QModelIndex) -> i32;
        fn data(self: &SystemsModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_name = "roleNames"]
        fn role_names(self: &SystemsModel) -> QHash_i32_QByteArray;
    }

    impl cxx_qt::Threading for SystemsModel {}
    impl cxx_qt::Initialize for SystemsModel {}
}

crate::bind_to_endpoint! {
    for ffi::SystemsModel,
    endpoint = CatalogEndpoint,
    args = (),
    select = project,
    apply = apply_state,
}

/// Pull the two pieces this model cares about out of the unified
/// `ResourceStatus`: the catalog payload (only present on `Ready`) and
/// the surfaced error message (empty unless `Errored`).
fn project(status: &ResourceStatus<CatalogData>) -> (Option<CatalogData>, String) {
    match status {
        ResourceStatus::Ready(data) => (Some(data.clone()), String::new()),
        ResourceStatus::Errored { message, .. } => (None, message.clone()),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, String::new()),
    }
}

/// Find `needle` in `systems` with case-sensitive id equality. Returns
/// position as i32, or -1 if not found / empty needle. The
/// case-sensitive contract is deliberate: `SystemsState.system_id` is
/// persisted as the exact ID Core surfaced, and the artwork bundled
/// under `resources/images/systems/<id>.png` matches that exact case
/// (Linux qrc lookups are case-sensitive). A case-insensitive lookup
/// would mask an upstream case drift in Core. Pulled out so the
/// contract is unit-testable.
fn position_of_system_id(systems: &[SystemInfo], needle: &str) -> i32 {
    if needle.is_empty() {
        return -1;
    }
    systems
        .iter()
        .position(|s| s.id == needle)
        .map_or(-1, |i| i as i32)
}

/// Filter `catalog`'s systems to the named category and re-shape them
/// into the local row type. Returns empty when `catalog` is `None` so
/// `set_category` and `apply_state` share one filter+map definition.
fn rows_for_category(catalog: Option<&CatalogData>, cat: &str) -> Vec<SystemInfo> {
    catalog.map_or_else(Vec::new, |c| {
        c.systems_by_category(cat)
            .into_iter()
            .map(|s| SystemInfo {
                id: s.id,
                name: s.name,
                category: s.category,
                release_date: s.release_date,
                manufacturer: s.manufacturer,
            })
            .collect()
    })
}

fn apply_state(mut model: Pin<&mut ffi::SystemsModel>, (data, err): (Option<CatalogData>, String)) {
    if let Some(data) = data {
        let cat = model.rust().current_category.to_string();
        if !cat.is_empty() {
            // Invalidate any in-flight `set_category` worker so its
            // queued result — computed against the pre-update catalog
            // — doesn't land on top of the fresher rows we're about
            // to write. The Qt event loop is single-threaded, so the
            // bump and the worker callback's `seq.load` are serialized:
            // any callback that runs after this point sees a mismatch
            // and bails. Without this bump the worker becomes the
            // authoritative writer for the moment, and stale rows
            // win.
            model.rust().seq.fetch_add(1, Ordering::SeqCst);
            let rows = rows_for_category(Some(&data), &cat);
            let count = rows.len() as i32;
            let ids: Vec<&str> = rows.iter().map(|s| s.id.as_str()).collect();
            debug!(
                category = %cat,
                count,
                ?ids,
                "systems: apply_state filled rows for category",
            );
            model.as_mut().begin_reset_model();
            model.as_mut().rust_mut().systems = rows;
            model.as_mut().rust_mut().count = count;
            model.as_mut().end_reset_model();
            model.as_mut().count_changed();
            // A fresh catalog arrival is the authoritative resolver
            // for `loading`: any worker spawned by an earlier
            // `set_category` has just been invalidated above and its
            // queued callback will bail rather than clear `loading`.
            if model.loading {
                model.as_mut().set_loading(false);
            }
        }
        model.as_mut().rust_mut().last_ready = Some(data);
    }
    let qerr = QString::from(err.as_str());
    if model.error_message != qerr {
        model.as_mut().set_error_message(qerr);
    }
    // An error is a terminal state for the in-flight load — any
    // worker spawned by an earlier `set_category` is invalidated by
    // the seq bump above (or never ran because there was no fresh
    // catalog), so this is the authoritative loading clear for the
    // error path. Without it the spinner sticks on after the catalog
    // surfaces an error mid-flight. Idle/Loading projects to
    // `(None, "")`, so leave `loading` alone there.
    if !err.is_empty() && model.loading {
        model.as_mut().set_loading(false);
    }
}

impl ffi::SystemsModel {
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
            COVER_KEY_ROLE => {
                // Relative path under `resources/images/` (no extension).
                // Tile resolves the PNG via `images/<coverKey>.png`, so
                // category, system and game tiles share one URL builder.
                QVariant::from(&QString::from(format!("systems/{}", s.id).as_str()))
            }
            NAME_ROLE | FILE_STEM_ROLE => QVariant::from(&QString::from(s.name.as_str())),
            CATEGORY_ROLE => QVariant::from(&QString::from(s.category.as_str())),
            FAVORITE_ROLE => QVariant::from(&0_i32),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut h = QHash::<QHashPair_i32_QByteArray>::default();
        h.insert(COVER_KEY_ROLE, QByteArray::from("coverKey"));
        h.insert(NAME_ROLE, QByteArray::from("name"));
        h.insert(CATEGORY_ROLE, QByteArray::from("category"));
        h.insert(FAVORITE_ROLE, QByteArray::from("favorite"));
        h.insert(FILE_STEM_ROLE, QByteArray::from("fileStem"));
        h
    }

    fn set_category(mut self: Pin<&mut Self>, category: QString) {
        // Repeat drill-ins on the same *populated* category (e.g. user
        // mashes Down→Escape→Down) would otherwise rebuild every Tile
        // delegate for no visible change. The `is_empty` guard lets a
        // re-call recover from a stale-but-empty model — e.g. the
        // catalog refetched and the previously-current category now
        // has no systems, and a caller wants to retry the same value.
        // The `error_message.is_empty()` guard is the [OK] RETRY path:
        // when the catalog errored after a successful fill, `systems`
        // is non-empty (apply_state's error branch leaves prior rows
        // alone), so without this clause the retry would short-circuit
        // and the surfaced error would never clear.
        if self.rust().current_category == category
            && !self.rust().systems.is_empty()
            && self.rust().error_message.is_empty()
        {
            return;
        }
        let cat = category.to_string();

        // User-visible reset happens synchronously so QML sees fresh
        // state immediately, before the worker has even scheduled.
        // Drop the prior category's rows here too — otherwise the
        // grid would keep painting (and accepting input on) the
        // outgoing category until the worker resolves.
        self.as_mut().set_current_category(category);
        if !self.loading {
            self.as_mut().set_loading(true);
        }
        if !self.error_message.is_empty() {
            self.as_mut().set_error_message(QString::default());
        }
        if !self.systems.is_empty() {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().systems.clear();
            self.as_mut().rust_mut().count = 0;
            self.as_mut().end_reset_model();
            self.as_mut().count_changed();
        }

        // Abort the prior task so it stops enqueuing further callbacks.
        // Any callback already on the Qt event loop is still pending —
        // the `seq` ticket below is what discards those stale ones.
        if let Some(handle) = self.as_mut().rust_mut().pending_task.take() {
            handle.abort();
        }

        // Bump the ticket *before* spawning. The new worker captures
        // this value; any earlier-worker callback still in the Qt
        // queue captured the previous ticket and will bail when it
        // sees the mismatch. `apply_state` also bumps this ticket
        // when a fresh catalog arrives mid-flight.
        let seq = self.rust().seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;

        // Snapshot the catalog for the worker. Reading from
        // `last_ready` rather than the live `ResourceStatus` means a
        // transient `Loading` (a refetch in flight) doesn't wipe the
        // grid between the user's category change and the refetch
        // completing. The clone is small (~hundreds of `SystemInfo`
        // rows, microseconds on ARM) and unavoidable without
        // Arc-wrapping `last_ready`, which is out of scope.
        let catalog = self.rust().last_ready.clone();

        let qt_thread = self.qt_thread();
        let handle = global_handle().spawn(async move {
            let rows = rows_for_category(catalog.as_ref(), &cat);
            let count = rows.len() as i32;
            let cat_for_log = cat.clone();
            let ids_for_log: Vec<String> = rows.iter().map(|s| s.id.clone()).collect();
            let _ = qt_thread.queue(move |mut model| {
                let current = seq.load(Ordering::SeqCst);
                if current != ticket {
                    trace!(
                        category = %cat_for_log,
                        ticket,
                        current,
                        "discarding stale set_category callback"
                    );
                    return;
                }
                debug!(
                    category = %cat_for_log,
                    count,
                    ids = ?ids_for_log,
                    "systems: set_category worker filled rows",
                );
                model.as_mut().begin_reset_model();
                model.as_mut().rust_mut().systems = rows;
                model.as_mut().rust_mut().count = count;
                model.as_mut().end_reset_model();
                model.as_mut().count_changed();
                if model.loading {
                    model.as_mut().set_loading(false);
                }
            });
        });
        self.as_mut().rust_mut().pending_task = Some(handle);
    }

    fn system_id_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].id.as_str())
    }

    fn system_name_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].name.as_str())
    }

    fn detail_tags_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(detail_tags_for_system(&self.systems[index as usize]).as_str())
    }

    fn launch_text_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        let system = &self.systems[index as usize];
        if system.id.is_empty() {
            return QString::default();
        }
        QString::from(format!("**launch.system:{}", system.id).as_str())
    }

    fn write_card_at(mut self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            self.as_mut()
                .set_card_write_error(QString::from("invalid selection"));
            self.as_mut().set_card_write_pending(false);
            return;
        }
        let system = &self.systems[index as usize];
        if system.id.is_empty() {
            self.as_mut()
                .set_card_write_error(QString::from("missing system id"));
            self.as_mut().set_card_write_pending(false);
            return;
        }
        let text = format!("**launch.system:{}", system.id);
        let name = system.name.clone();
        let store = global_store();
        let seq = self.rust().card_write_seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.as_mut().set_card_write_error(QString::default());
        self.as_mut().set_card_write_pending(true);
        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            let result = store
                .run_mutation::<ReadersWriteMutation>(ReadersWriteParams { text })
                .await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                let error = match result {
                    Ok(()) => QString::default(),
                    Err(e) => {
                        warn!("card write failed for {name}: {}", e.message);
                        QString::from(e.message.as_str())
                    }
                };
                model.as_mut().set_card_write_error(error);
                model.as_mut().set_card_write_pending(false);
            });
        });
    }

    fn launch_at(self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            return;
        }
        let system = &self.systems[index as usize];
        if system.id.is_empty() {
            return;
        }
        let text = format!("**launch.system:{}", system.id);
        let name = system.name.clone();
        let store = global_store();
        global_handle().spawn(async move {
            if let Err(e) = store.run_mutation::<RunMutation>(RunParams { text }).await {
                warn!("run failed for {name}: {}", e.message);
            }
        });
    }

    fn cancel_card_write(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust()
            .card_write_seq
            .fetch_add(1, Ordering::SeqCst);
        if !self.card_write_error.is_empty() {
            self.as_mut().set_card_write_error(QString::default());
        }
        if self.card_write_pending {
            self.as_mut().set_card_write_pending(false);
        }
    }

    fn index_for_system_id(&self, id: &QString) -> i32 {
        position_of_system_id(&self.systems, &id.to_string())
    }
}

fn detail_tags_for_system(system: &SystemInfo) -> String {
    let rows = [
        ("Category", system.category.trim().to_string()),
        (
            "Release date",
            system
                .release_date
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
        ),
        (
            "Manufacturer",
            system
                .manufacturer
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
        ),
    ];
    rows.into_iter()
        .map(|(label, value)| format!("{label}\t{value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        detail_tags_for_system, position_of_system_id, project, rows_for_category, SystemInfo,
    };
    use zaparoo_core::media_types::SystemInfo as MediaSystemInfo;
    use zaparoo_core::remote_resource::ResourceStatus;
    use zaparoo_core::systems_catalog::CatalogData;

    fn sys(id: &str, name: &str, category: &str) -> MediaSystemInfo {
        MediaSystemInfo {
            id: id.into(),
            name: name.into(),
            category: category.into(),
            ..MediaSystemInfo::default()
        }
    }

    fn catalog_with(systems: Vec<MediaSystemInfo>) -> CatalogData {
        CatalogData {
            systems,
            categories: Vec::new(),
        }
    }

    #[test]
    fn idle_projects_to_no_data_no_error() {
        let (data, err) = project(&ResourceStatus::Idle);
        assert!(data.is_none());
        assert!(err.is_empty());
    }

    #[test]
    fn loading_projects_to_no_data_no_error() {
        let (data, err) = project(&ResourceStatus::Loading);
        assert!(data.is_none());
        assert!(err.is_empty());
    }

    #[test]
    fn ready_projects_data_and_no_error() {
        let catalog = catalog_with(vec![sys("smb", "SMB", "Consoles")]);
        let (data, err) = project(&ResourceStatus::Ready(catalog));
        assert!(data.is_some());
        assert!(err.is_empty());
        assert_eq!(data.unwrap().systems.len(), 1);
    }

    #[test]
    fn errored_projects_message_and_no_data() {
        let status: ResourceStatus<CatalogData> = ResourceStatus::Errored {
            message: "boom".into(),
            retrying: false,
        };
        let (data, err) = project(&status);
        assert!(data.is_none());
        assert_eq!(err, "boom");
    }

    #[test]
    fn errored_with_retrying_still_propagates_message() {
        let status: ResourceStatus<CatalogData> = ResourceStatus::Errored {
            message: "reconnecting".into(),
            retrying: true,
        };
        let (data, err) = project(&status);
        assert!(data.is_none());
        assert_eq!(err, "reconnecting");
    }

    #[test]
    fn rows_for_category_none_returns_empty() {
        let rows = rows_for_category(None, "Arcade");
        assert!(rows.is_empty());
    }

    #[test]
    fn rows_for_category_filters_and_reshapes() {
        let catalog = catalog_with(vec![
            sys("smb", "Super Mario Bros", "Consoles"),
            sys("snk", "SNK Heroes", "Arcade"),
            sys("zelda", "Zelda", "Consoles"),
        ]);
        let rows = rows_for_category(Some(&catalog), "Consoles");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "smb");
        assert_eq!(rows[0].name, "Super Mario Bros");
        assert_eq!(rows[0].category, "Consoles");
        assert_eq!(rows[1].id, "zelda");
    }

    #[test]
    fn rows_for_category_preserves_system_metadata() {
        let mut nes = sys("nes", "Nintendo Entertainment System", "Consoles");
        nes.release_date = Some("1983".into());
        nes.manufacturer = Some("Nintendo".into());
        let catalog = catalog_with(vec![nes]);
        let rows = rows_for_category(Some(&catalog), "Consoles");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].release_date.as_deref(), Some("1983"));
        assert_eq!(rows[0].manufacturer.as_deref(), Some("Nintendo"));
    }

    #[test]
    fn detail_tags_for_system_emits_fixed_rows() {
        let mut system = local_sys("NES");
        system.release_date = Some("1983".into());
        system.manufacturer = Some("Nintendo".into());
        assert_eq!(
            detail_tags_for_system(&system),
            "Category\tConsoles\nRelease date\t1983\nManufacturer\tNintendo"
        );
    }

    #[test]
    fn rows_for_category_unknown_returns_empty() {
        let catalog = catalog_with(vec![sys("smb", "SMB", "Consoles")]);
        let rows = rows_for_category(Some(&catalog), "DoesNotExist");
        assert!(rows.is_empty());
    }

    fn local_sys(id: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: id.into(),
            category: "Consoles".into(),
            release_date: None,
            manufacturer: None,
        }
    }

    #[test]
    fn position_of_system_id_returns_index_on_case_exact_match() {
        let systems = vec![local_sys("NES"), local_sys("SNES")];
        assert_eq!(position_of_system_id(&systems, "SNES"), 1);
    }

    #[test]
    fn position_of_system_id_is_case_sensitive() {
        let systems = vec![local_sys("NES"), local_sys("SNES")];
        // SystemsState.system_id is persisted exact and the bundled
        // artwork (`resources/images/systems/<id>.png`) matches that
        // exact case — case-insensitive lookup would silently mask a
        // Core case-drift bug.
        assert_eq!(position_of_system_id(&systems, "snes"), -1);
        assert_eq!(position_of_system_id(&systems, "Snes"), -1);
    }

    #[test]
    fn position_of_system_id_empty_needle_returns_minus_one() {
        let systems = vec![local_sys("NES")];
        assert_eq!(position_of_system_id(&systems, ""), -1);
    }

    #[test]
    fn position_of_system_id_missing_returns_minus_one() {
        let systems = vec![local_sys("NES")];
        assert_eq!(position_of_system_id(&systems, "Missing"), -1);
    }
}
