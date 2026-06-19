// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::models::{global_handle, global_store, with_persist_read};
use crate::system_region::Region;
use crate::{image_overrides, system_logos, system_name_overrides, system_names, system_region};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QStringList, QVariant,
};
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
const HIDDEN_ROLE: i32 = 256 + 6;
// Systems have no disambiguating tags; the role exists only so the shared
// grid/list delegates (which require it for media rows) bind cleanly here.
const DISAMBIGUATING_TAGS_ROLE: i32 = 256 + 7;

pub struct SystemInfo {
    pub id: String,
    pub name: String,
    /// Cover key emitted to QML. One of:
    /// - `"systems/{stem}"` — tinted SVG via the `tinted-svg` provider
    ///   (stem comes from `system_logos::logo_artwork_stem` for regional variants).
    /// - `"custom-image/{path}"` — user-supplied override via the `custom-image`
    ///   provider; no tint applied.
    pub cover_key: String,
    pub category: String,
    pub release_date: Option<String>,
    pub manufacturer: Option<String>,
    /// True when the user has hidden this system and `show_hidden` is on.
    /// The tile renders dimmed with a "Hidden" badge.
    pub hidden: bool,
    /// `zaparoo://...` launch URI for launch-only "virtual" systems
    /// (Core's launchables). Empty for normal systems. When present the
    /// system is launched by running this script directly instead of
    /// being browsed via `media.browse`.
    pub zap_script: String,
}

/// A launch-only system carries a `zaparoo://...` launch URI instead of
/// browsable media. Trimmed so whitespace-only never reads as launchable.
fn is_launchable(system: &SystemInfo) -> bool {
    !system.zap_script.trim().is_empty()
}

/// `ZapScript` to run (or write to a card) for a system. Launchables run
/// their own `zaparoo://...` URI; normal systems use the `**launch.system`
/// directive that launches the system's default core/launcher.
fn launch_text_for(system: &SystemInfo) -> String {
    if is_launchable(system) {
        system.zap_script.clone()
    } else {
        format!("**launch.system:{}", system.id)
    }
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
        type QStringList = cxx_qt_lib::QStringList;
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

        /// Returns the cover key for the system at `index`, including any
        /// regional artwork stem (e.g. `"systems/Genesis.eu"`). Use this in
        /// prefetch loops instead of building `"systems/" + system_id_at(i)`,
        /// which would miss regional variants.
        #[qinvokable]
        fn cover_key_at(self: &SystemsModel, index: i32) -> QString;

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

        /// True when the system with `system_id` is a launch-only (virtual)
        /// system. The router launches it directly rather than browsing.
        #[qinvokable]
        fn is_launchable_system(self: &SystemsModel, system_id: &QString) -> bool;

        /// Run the launch script for the launch-only system `system_id`.
        #[qinvokable]
        fn launch_system_id(self: Pin<&mut SystemsModel>, system_id: &QString);

        #[qinvokable]
        fn cancel_card_write(self: Pin<&mut SystemsModel>);

        #[qinvokable]
        fn index_for_system_id(self: &SystemsModel, id: &QString) -> i32;

        /// Returns true when the system at `index` is user-hidden and
        /// `show_hidden` is on (i.e. visible but dimmed). Always false when
        /// the system is fully filtered out (`show_hidden = false`).
        #[qinvokable]
        fn is_hidden_at(self: &SystemsModel, index: i32) -> bool;

        /// Re-run the current category filter using the persisted hidden set
        /// and `show_hidden`. Call after any hide/unhide/toggle action so the
        /// grid reflects the new visibility without waiting for a catalog
        /// refetch. Bumps `seq` to invalidate any in-flight `set_category`
        /// workers.
        #[qinvokable]
        fn reproject(self: Pin<&mut SystemsModel>);

        /// Return all system IDs in `category` from the last-known-good
        /// catalog, ignoring the current hide filter. Used by `Main.qml` to
        /// build a system list for category-level index/scrape operations.
        #[qinvokable]
        fn system_ids_for_category(self: &SystemsModel, category: &QString) -> QStringList;

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
/// under `resources/images/systems/<id>.svg` matches that exact case
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
///
/// `hidden_ids` is the current persisted set of user-hidden system IDs.
/// When `show_hidden` is false, systems in that set are dropped entirely.
/// When true, they are included with `hidden = true` so the tile renders
/// dimmed with a "Hidden" badge.
///
/// `region` drives both the localized display name (via `system_names`) and
/// the logo artwork stem (via `system_logos`). Resolve it once before calling
/// this function and pass it in so the caller controls the snapshot.
fn rows_for_category(
    catalog: Option<&CatalogData>,
    cat: &str,
    hidden_ids: &[String],
    show_hidden: bool,
    region: Region,
) -> Vec<SystemInfo> {
    catalog.map_or_else(Vec::new, |c| {
        c.systems_by_category(cat)
            .into_iter()
            .filter_map(|s| {
                let is_hidden = hidden_ids.contains(&s.id);
                if is_hidden && !show_hidden {
                    return None;
                }
                // Display name priority: user `[system_names]` override, then
                // Names_MiSTer localized data, then the Core catalog name so
                // unknown systems still show.
                let name = system_name_overrides::lookup(&s.id)
                    .or_else(|| system_names::localized_name(&s.id, region))
                    .unwrap_or(s.name);
                // Cover key: user override takes priority over bundled art.
                let cover_key = image_overrides::override_path("systems", &s.id).map_or_else(
                    || format!("systems/{}", system_logos::logo_artwork_stem(&s.id, region)),
                    |p| format!("custom-image/{}", p.display()),
                );
                Some(SystemInfo {
                    id: s.id,
                    name,
                    cover_key,
                    category: s.category,
                    release_date: s.release_date,
                    manufacturer: s.manufacturer,
                    hidden: is_hidden,
                    zap_script: s.zap_script,
                })
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
            let (hidden_ids, show_hidden) = with_persist_read(|s| {
                (s.systems.hidden_system_ids.clone(), s.settings.show_hidden)
            });
            let region = system_region::current_region();
            let rows = rows_for_category(Some(&data), &cat, &hidden_ids, show_hidden, region);
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
                // `cover_key` is set at row-build time in `rows_for_category`:
                // either `"custom-image/{path}"` for a user override or
                // `"systems/{stem}"` for bundled artwork. `Resources.qml`
                // resolves this to the appropriate image:// URL.
                QVariant::from(&QString::from(s.cover_key.as_str()))
            }
            NAME_ROLE | FILE_STEM_ROLE => QVariant::from(&QString::from(s.name.as_str())),
            CATEGORY_ROLE => QVariant::from(&QString::from(s.category.as_str())),
            FAVORITE_ROLE => QVariant::from(&0_i32),
            HIDDEN_ROLE => QVariant::from(&s.hidden),
            DISAMBIGUATING_TAGS_ROLE => QVariant::from(&QString::default()),
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
        h.insert(HIDDEN_ROLE, QByteArray::from("hidden"));
        h.insert(
            DISAMBIGUATING_TAGS_ROLE,
            QByteArray::from("disambiguatingTags"),
        );
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

        // Snapshot the catalog and current hidden state for the worker.
        // Reading from `last_ready` rather than the live `ResourceStatus`
        // means a transient `Loading` (a refetch in flight) doesn't wipe
        // the grid between the user's category change and the refetch
        // completing. The clone is small (~hundreds of `SystemInfo` rows,
        // microseconds on ARM) and unavoidable without Arc-wrapping
        // `last_ready`, which is out of scope.
        let catalog = self.rust().last_ready.clone();
        let (hidden_ids, show_hidden) =
            with_persist_read(|s| (s.systems.hidden_system_ids.clone(), s.settings.show_hidden));
        // Resolve region on the Qt thread so the async worker captures a
        // snapshot rather than reading global state from a tokio thread.
        let region = system_region::current_region();

        let qt_thread = self.qt_thread();
        let handle = global_handle().spawn(async move {
            let rows = rows_for_category(catalog.as_ref(), &cat, &hidden_ids, show_hidden, region);
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

    fn cover_key_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.systems[index as usize].cover_key.as_str())
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
        QString::from(launch_text_for(system).as_str())
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
        let text = launch_text_for(system);
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
        let text = launch_text_for(system);
        let name = system.name.clone();
        let store = global_store();
        global_handle().spawn(async move {
            if let Err(e) = store.run_mutation::<RunMutation>(RunParams { text }).await {
                warn!("run failed for {name}: {}", e.message);
            }
        });
    }

    /// True when the system with `system_id` is a launch-only (virtual)
    /// system carrying a `zaparoo://...` script. The router uses this to
    /// launch the system directly instead of routing into a games browse.
    fn is_launchable_system(&self, system_id: &QString) -> bool {
        let id = system_id.to_string();
        if id.is_empty() {
            return false;
        }
        self.systems
            .iter()
            .find(|s| s.id == id)
            .is_some_and(is_launchable)
    }

    /// Run the launch script for the system with `system_id`. Used by the
    /// router for launchable systems, where selection is an action rather
    /// than navigation; lookup is by id since the router has no index.
    fn launch_system_id(self: Pin<&mut Self>, system_id: &QString) {
        let id = system_id.to_string();
        let Some(system) = self.systems.iter().find(|s| s.id == id) else {
            return;
        };
        let text = launch_text_for(system);
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

    fn is_hidden_at(&self, index: i32) -> bool {
        if index < 0 || index >= self.count {
            return false;
        }
        self.systems[index as usize].hidden
    }

    fn reproject(mut self: Pin<&mut Self>) {
        let cat = self.rust().current_category.to_string();
        if cat.is_empty() {
            return;
        }
        let (hidden_ids, show_hidden) =
            with_persist_read(|s| (s.systems.hidden_system_ids.clone(), s.settings.show_hidden));
        let region = system_region::current_region();
        // Bump seq to invalidate any in-flight set_category workers; their
        // stale result would undo the reproject if they ran after us.
        let seq = self.rust().seq.clone();
        seq.fetch_add(1, Ordering::SeqCst);
        let catalog = self.rust().last_ready.clone();
        let rows = rows_for_category(catalog.as_ref(), &cat, &hidden_ids, show_hidden, region);
        let count = rows.len() as i32;
        debug!(category = %cat, count, "systems: reproject");
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().systems = rows;
        self.as_mut().rust_mut().count = count;
        self.as_mut().end_reset_model();
        self.as_mut().count_changed();
        if self.loading {
            self.as_mut().set_loading(false);
        }
    }

    fn system_ids_for_category(&self, category: &QString) -> QStringList {
        let cat = category.to_string();
        let mut list = QStringList::default();
        if let Some(ref c) = self.rust().last_ready {
            for id in indexable_system_ids(c, &cat) {
                list.append(QString::from(id.as_str()));
            }
        }
        list
    }
}

/// Ids of the indexable systems in `category` (the ones a category-level
/// index/scrape can act on). Launch-only systems carry a `zap_script` and
/// have no indexed media, so they are skipped. A category whose members are
/// all launch-only yields an empty list, which is how the systems context
/// menu decides to omit the dead index/scrape actions.
fn indexable_system_ids(catalog: &CatalogData, category: &str) -> Vec<String> {
    catalog
        .systems_by_category(category)
        .into_iter()
        .filter(|s| s.zap_script.trim().is_empty())
        .map(|s| s.id)
        .collect()
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
    // Drop rows with no value so a system missing metadata (common for
    // launch-only systems, which carry no releaseDate/manufacturer) shows
    // only the fields it actually has rather than blank detail rows.
    rows.into_iter()
        .filter(|(_, value)| !value.is_empty())
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
        detail_tags_for_system, indexable_system_ids, is_launchable, launch_text_for,
        position_of_system_id, project, rows_for_category, SystemInfo,
    };
    use crate::system_region::Region;
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
        let rows = rows_for_category(None, "Arcade", &[], false, Region::Us);
        assert!(rows.is_empty());
    }

    #[test]
    fn rows_for_category_filters_and_reshapes() {
        let catalog = catalog_with(vec![
            sys("smb", "Super Mario Bros", "Consoles"),
            sys("snk", "SNK Heroes", "Arcade"),
            sys("zelda", "Zelda", "Consoles"),
        ]);
        let rows = rows_for_category(Some(&catalog), "Consoles", &[], false, Region::Us);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "smb");
        assert_eq!(rows[0].name, "Super Mario Bros");
        assert_eq!(rows[0].category, "Consoles");
        assert_eq!(rows[1].id, "zelda");
        assert!(!rows[0].hidden);
    }

    #[test]
    fn rows_for_category_preserves_system_metadata() {
        let mut nes = sys("nes", "Nintendo Entertainment System", "Consoles");
        nes.release_date = Some("1983".into());
        nes.manufacturer = Some("Nintendo".into());
        let catalog = catalog_with(vec![nes]);
        let rows = rows_for_category(Some(&catalog), "Consoles", &[], false, Region::Us);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].release_date.as_deref(), Some("1983"));
        assert_eq!(rows[0].manufacturer.as_deref(), Some("Nintendo"));
    }

    #[test]
    fn rows_for_category_hidden_system_excluded_when_show_hidden_false() {
        let catalog = catalog_with(vec![
            sys("nes", "NES", "Consoles"),
            sys("snes", "SNES", "Consoles"),
        ]);
        let hidden = vec!["snes".to_string()];
        let rows = rows_for_category(Some(&catalog), "Consoles", &hidden, false, Region::Us);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "nes");
    }

    #[test]
    fn rows_for_category_hidden_system_shown_dimmed_when_show_hidden_true() {
        let catalog = catalog_with(vec![
            sys("nes", "NES", "Consoles"),
            sys("snes", "SNES", "Consoles"),
        ]);
        let hidden = vec!["snes".to_string()];
        let rows = rows_for_category(Some(&catalog), "Consoles", &hidden, true, Region::Us);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].hidden);
        assert!(rows[1].hidden);
        assert_eq!(rows[1].id, "snes");
    }

    #[test]
    fn rows_for_category_empty_hidden_ids_no_change() {
        let catalog = catalog_with(vec![sys("nes", "NES", "Consoles")]);
        let rows_off = rows_for_category(Some(&catalog), "Consoles", &[], false, Region::Us);
        let rows_on = rows_for_category(Some(&catalog), "Consoles", &[], true, Region::Us);
        assert_eq!(rows_off.len(), 1);
        assert_eq!(rows_on.len(), 1);
        assert!(!rows_off[0].hidden);
        assert!(!rows_on[0].hidden);
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
        let rows = rows_for_category(Some(&catalog), "DoesNotExist", &[], false, Region::Us);
        assert!(rows.is_empty());
    }

    fn local_sys(id: &str) -> SystemInfo {
        SystemInfo {
            id: id.into(),
            name: id.into(),
            cover_key: format!("systems/{id}"),
            category: "Consoles".into(),
            release_date: None,
            manufacturer: None,
            hidden: false,
            zap_script: String::new(),
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
        // artwork (`resources/images/systems/<id>.svg`) matches that
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

    #[test]
    fn detail_tags_for_system_omits_empty_metadata_rows() {
        // A launch-only system typically has no release date or
        // manufacturer; those rows must be dropped, not rendered blank.
        let system = local_sys("Chess");
        assert_eq!(detail_tags_for_system(&system), "Category\tConsoles");
    }

    #[test]
    fn detail_tags_for_system_keeps_populated_rows_only() {
        let mut system = local_sys("NES");
        system.manufacturer = Some("Nintendo".into());
        // Release date stays None -> its row is dropped, Manufacturer kept.
        assert_eq!(
            detail_tags_for_system(&system),
            "Category\tConsoles\nManufacturer\tNintendo"
        );
    }

    #[test]
    fn launchable_detection_ignores_whitespace_only_script() {
        let mut system = local_sys("NES");
        assert!(!is_launchable(&system));
        system.zap_script = "   ".into();
        assert!(!is_launchable(&system));
        system.zap_script = "zaparoo://abc/Chess".into();
        assert!(is_launchable(&system));
    }

    #[test]
    fn launch_text_uses_zap_script_for_launchables() {
        let mut system = local_sys("Chess");
        system.zap_script = "zaparoo://abc/Chess".into();
        assert_eq!(launch_text_for(&system), "zaparoo://abc/Chess");
    }

    #[test]
    fn launch_text_uses_launch_system_directive_for_normal_systems() {
        let system = local_sys("NES");
        assert_eq!(launch_text_for(&system), "**launch.system:NES");
    }

    #[test]
    fn rows_for_category_propagates_zap_script() {
        // Launchables land in the synthesized "Other" bucket, which matches
        // systems with an empty upstream category.
        let mut chess = sys("chess", "Chess", "");
        chess.zap_script = "zaparoo://abc/Chess".into();
        let catalog = catalog_with(vec![chess]);
        let rows = rows_for_category(Some(&catalog), "Other", &[], false, Region::Us);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].zap_script, "zaparoo://abc/Chess");
        assert!(is_launchable(&rows[0]));
    }

    #[test]
    fn indexable_system_ids_excludes_launch_only_in_mixed_category() {
        // A category can mix indexable systems (NESMusic) with launch-only
        // ones (a launchable in the same bucket). The category-level
        // index/scrape must target only the indexable members, so the
        // launch-only system's id must not appear.
        let mut launchable = sys("chess", "Chess", "Other");
        launchable.zap_script = "zaparoo://abc/Chess".into();
        let catalog = catalog_with(vec![
            sys("NESMusic", "NES Music", "Other"),
            launchable,
            sys("SNESMusic", "SNES Music", "Other"),
        ]);
        let ids = indexable_system_ids(&catalog, "Other");
        assert_eq!(ids, vec!["NESMusic".to_string(), "SNESMusic".to_string()]);
    }

    #[test]
    fn indexable_system_ids_empty_when_category_all_launch_only() {
        // A category whose members are all launch-only yields no indexable
        // ids; the context menu uses this to omit the dead index/scrape
        // actions entirely.
        let mut a = sys("a", "A", "Other");
        a.zap_script = "zaparoo://abc/A".into();
        let mut b = sys("b", "B", "Other");
        b.zap_script = "zaparoo://abc/B".into();
        let catalog = catalog_with(vec![a, b]);
        assert!(indexable_system_ids(&catalog, "Other").is_empty());
    }
}
