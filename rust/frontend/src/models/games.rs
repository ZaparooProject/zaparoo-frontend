// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.GamesModel` — directory-aware, cumulative paged browse over
// Zaparoo Core's `media.browse` endpoint.
//
// Two paths into the model:
//
//   * `set_system(id)` — entry from SystemsScreen accept. Issues
//     `media.browse({systems: [id]})` (system-scoped roots). When the
//     scoped roots collapse to a single folder entry, we auto-navigate
//     into that folder so the user never sees a 1-item list — most
//     systems have one root and showing it standalone would be empty
//     ceremony.
//
//   * `set_path(path)` — entry from a folder accept inside the games
//     screen. Issues `media.browse({path, systems: [current_system]})`.
//     No auto-navigation; explicit folder navigation is treated as
//     intent.
//
// `fetch_more()` advances pagination without resetting the model:
// `begin_insert_rows` appends entries from the next cursor onto the
// existing vec. Cursor lives in the model, not in the endpoint cache
// key, because cursor-paged follow-ups bypass the cache (one entry per
// cursor would defeat the cache).
//
// `seq` discipline: bumped on every `set_system` / `set_path`, read
// (not bumped) by `fetch_more`. Late callbacks whose ticket doesn't
// match `seq` are dropped — the standard cure for Qt-thread races
// when the user spams direction-arrow + Accept across a model swap.

use crate::media_image_cache::{global_media_image_cache, MediaImageCache, MediaKey};
use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant,
};
use std::collections::{BTreeSet, HashSet};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use zaparoo_core::client::ClientError;
use zaparoo_core::endpoints::media_browse::{BrowseArgs, MediaBrowseEndpoint};
use zaparoo_core::endpoints::media_tags_update::MediaTagsUpdateMutation;
use zaparoo_core::endpoints::readers_write::ReadersWriteMutation;
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::media_types::{
    BrowseEntry, MediaBrowseParams, MediaBrowseResult, MediaMeta, MediaMetaParams,
    MediaTagsUpdateParams, ReadersWriteParams, RunParams, TagInfo,
};
use zaparoo_core::platform::{self, Platform};
use zaparoo_core::remote_resource::ResourceStatus;

const NAME_ROLE: i32 = 256 + 1;
const PATH_ROLE: i32 = 256 + 2;
const ZAP_SCRIPT_ROLE: i32 = 256 + 3;
const SYSTEM_ID_ROLE: i32 = 256 + 4;
// Folder tiles always use `icons/Folder`. Media tiles default to
// `icons/File` and switch to `media-image/<encoded>` once the in-memory
// media image cache has bytes for `(systemId, path)`;
// Resources.coverUrl() rewrites the `media-image/` prefix to
// `image://media-image/<encoded>` so the QQuickImageProvider serves the
// bytes from RAM.
const COVER_KEY_ROLE: i32 = 256 + 5;
const ENTRY_TYPE_ROLE: i32 = 256 + 6;
const FILE_COUNT_ROLE: i32 = 256 + 7;
const FAVORITE_ROLE: i32 = 256 + 8;
const DESCRIPTION_ROLE: i32 = 256 + 9;
const FILE_STEM_ROLE: i32 = 256 + 10;

// Default API page size before QML binds the model's `page_size` to the
// grid's `pageSize`. 15 = 5 columns × 3 rows, the desktop default. The
// test harness sees this until it overrides explicitly. Server cap is
// 1000; grid page sizes top out at ~30 so we stay well inside bounds.
const DEFAULT_PAGE_SIZE: i32 = 15;
// `media.browse` `max_results` for cursor follow-ups. Held separate
// from `page_size` (which dictates grid layout, scroll-thumb sizing,
// and the initial-page cover gate) so wire chunks can be larger than a
// single visual page without changing the grid math. Sized for the
// MiSTer main-thread cost of `apply_append_page`: each chunk runs
// `transform_entries` and a `begin_insert_rows`/`end_insert_rows`
// pair on the Qt thread, which stalls input until it returns. 500 was Core's wire-cost optimum but
// produced a multi-second stall on ARM32 with the indicator showing
// the whole time; 100 keeps the per-chunk stall short enough to be
// invisible while still cutting an 805-entry Arcade to ~8 round-trips.
// Server caps `max_results` at 1000; 100 stays well inside that. The
// initial browse keeps the smaller `page_size` so the cover gate
// doesn't have to wait on a big first decode. Cover prefetch is no
// longer driven by metadata page size — `prefetch_around` warms only
// the visible and next pages, so a large `FETCH_MORE_CHUNK_SIZE` no
// longer floods the cover queue.
const FETCH_MORE_CHUNK_SIZE: i32 = 100;
const FETCH_MORE_RAPID_CHUNK_SIZE: i32 = 300;

// `apply_append_page` sub-batches the model insert into chunks of this
// many rows so the Repeater's per-delegate `createObject` cost (the
// dominant Qt-thread stall on MiSTer at ~7-8 ms per Tile) is spread
// across frames instead of one ~750 ms - 1.6 s block. The first batch
// runs synchronously inside the original Qt-thread call so PagedGrid's
// `_commitPendingTarget` fires within ~200 ms of the user's press; the
// rest are posted from `global_handle()` with one frame of breathing
// room (`SUB_BATCH_FRAME_GAP_MS`) between batches so input + paint can
// drain in between. 12 is intentionally below a full dense-grid page:
// more batches trickle in, but each batch has less chance to monopolize
// the Qt thread during a held Down/Up scroll.
const APPEND_SUB_BATCH_SIZE: usize = 12;

// One frame at 30 Hz, the MiSTer software renderer's effective frame
// budget. Gives Qt one full frame to drain input + paint between
// posted sub-batches. Smaller values risk the next batch landing
// before the paint completes; larger values make the trailing rows
// trickle in too slowly.
const SUB_BATCH_FRAME_GAP_MS: u64 = 33;

#[allow(
    clippy::struct_excessive_bools,
    reason = "the bools are independent qproperties surfaced to QML; collapsing them \
              into an enum would force the QML side to read a single state property \
              and re-derive each flag locally, which is exactly the work the bridge \
              avoids"
)]
pub struct GamesModelRust {
    entries: Vec<BrowseEntry>,
    count: i32,
    loading: bool,
    loading_more: bool,
    error_message: QString,
    has_next_page: bool,
    // Files-only count from Core (`BrowseFileCount`). Combined with
    // `dir_count` to compute total entries for the page denominator —
    // dirs are returned only on page 1 and always before files, so
    // `dir_count + total_files` is exact, not an estimate.
    total_files: i32,
    // Count of leading directory entries seen on page 1. Set once when
    // page 1 lands and never touched on cursor follow-ups (Core never
    // returns directories on follow-up pages).
    dir_count: i32,
    // API page size, bound to the QML grid's `pageSize` so the API page
    // matches the visual page exactly — no mid-page "Loading more…"
    // pause on the user's first scroll past row 0.
    page_size: i32,
    current_system_id: QString,
    current_path: QString,
    next_cursor: Option<String>,
    card_write_pending: bool,
    card_write_error: QString,
    current_description: QString,
    current_detail_tags: QString,
    current_detail_loading: bool,
    current_detail_image_key: QString,
    current_detail_image_index: i32,
    current_detail_image_count: i32,
    current_detail_image_can_prev: bool,
    current_detail_image_can_next: bool,
    cover_key_roles_enabled: bool,
    cover_requests_paused: bool,
    detail_image_keys: Vec<MediaKey>,
    // Watcher for the current initial-page subscription. Aborted on
    // every path swap so the prior watcher stops enqueuing callbacks
    // for the old `BrowseArgs`. Callbacks already in the Qt queue are
    // disarmed by `seq` rather than by the abort.
    watcher: Option<JoinHandle<()>>,
    // Bumped on every `set_system` / `set_path`. Read but NOT bumped
    // by `fetch_more` — cursor-driven follow-ups are part of the same
    // path's load and should keep the same ticket.
    seq: Arc<AtomicU64>,
    // True only for `set_system`-driven initial loads. The single-root
    // auto-nav case is the only place we want to skip the rendered
    // intermediate state, so the flag is consumed by the apply
    // function and reset once the decision has been made.
    auto_nav_eligible: bool,
    // Set once `apply_initial_page` has installed a result for the
    // current browse target; cleared on every `start_initial_browse`.
    // Subsequent Ready transitions on the same target (cache
    // invalidation refetches, e.g. after `MediaTagsUpdateMutation`)
    // skip the full reset so the user's pagination position and
    // appended pages aren't clobbered. Local mutations
    // (`apply_favorite_tags`) keep the visible model in sync without
    // needing the refetch.
    is_seeded: bool,
    // Invalidates stale card-write completions after the user cancels
    // or starts another write before the previous RPC returns.
    card_write_seq: Arc<AtomicU64>,
    // Long-lived bridge from `media_image_cache` broadcast updates onto Qt
    // `dataChanged(coverKey)` emits. Spun up lazily on the first
    // `start_initial_browse` so the model singleton owns exactly one
    // subscriber for the whole process lifetime.
    cover_subscription: Option<JoinHandle<()>>,
    // Keys whose first-paint we're still waiting on. While non-empty we
    // hold `loading = true` so the screen-flip overlay covers the gap
    // between "page rendered with glyphs" and "covers cached". Drained
    // by `notify_cover_update` as each cover lands; force-cleared by
    // the gate timer or a subsequent `start_initial_browse`.
    pending_first_paint_keys: HashSet<MediaKey>,
    // Safety timer that force-releases the cover gate after a bounded
    // delay, so a stalled bulk RPC can't park the user on `Loading…`
    // forever.
    cover_gate_timer: Option<JoinHandle<()>>,
    // Bumped on every cover-gate arm and on every `start_initial_browse`.
    // The timer's queued closure compares against the current value and
    // bails on a mismatch — necessary because aborting the JoinHandle
    // doesn't cancel a callback that was already queued onto the Qt
    // thread between sleep-completion and abort.
    cover_gate_seq: Arc<AtomicU64>,
    description_seq: Arc<AtomicU64>,
    // Tagging ticket for in-flight append sub-batches scheduled by
    // `apply_append_page`. Bumped on every `start_initial_browse` so a
    // deferred batch from a stale dataset detects that the model has
    // moved on and bails before splicing rows from the old chain onto
    // the new one. Same race shape as `cover_gate_seq`, just for the
    // sub-batch fan-out.
    append_seq: Arc<AtomicU64>,
    // True from the moment `apply_initial_page` queues its metadata
    // look-ahead `fetch_more` until the matching `apply_append_page`
    // lands. This is informational only: background look-ahead must
    // not hold the full-screen loading gate after the visible page is
    // ready.
    pending_initial_lookahead: bool,
    // First visible row in the grid. Bound from QML to
    // `gamesGrid.currentPage * gamesGrid.pageSize` so the model knows
    // which entries are on screen and can warm the next page's covers
    // explicitly. Read by `prefetch_around` and by `apply_append_page`
    // so a freshly-landed metadata chunk can re-issue the prefetch
    // window for whatever row the user is currently looking at.
    visible_first_row: i32,
}

impl Default for GamesModelRust {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            count: 0,
            loading: false,
            loading_more: false,
            error_message: QString::default(),
            has_next_page: false,
            total_files: 0,
            dir_count: 0,
            page_size: DEFAULT_PAGE_SIZE,
            current_system_id: QString::default(),
            current_path: QString::default(),
            next_cursor: None,
            card_write_pending: false,
            card_write_error: QString::default(),
            current_description: QString::default(),
            current_detail_tags: QString::default(),
            current_detail_loading: false,
            current_detail_image_key: QString::default(),
            current_detail_image_index: 0,
            current_detail_image_count: 0,
            current_detail_image_can_prev: false,
            current_detail_image_can_next: false,
            cover_key_roles_enabled: true,
            cover_requests_paused: false,
            detail_image_keys: Vec::new(),
            watcher: None,
            seq: Arc::new(AtomicU64::new(0)),
            auto_nav_eligible: false,
            is_seeded: false,
            card_write_seq: Arc::new(AtomicU64::new(0)),
            cover_subscription: None,
            pending_first_paint_keys: HashSet::new(),
            cover_gate_timer: None,
            cover_gate_seq: Arc::new(AtomicU64::new(0)),
            description_seq: Arc::new(AtomicU64::new(0)),
            append_seq: Arc::new(AtomicU64::new(0)),
            pending_initial_lookahead: false,
            visible_first_row: 0,
        }
    }
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
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, count)]
        #[qproperty(bool, loading)]
        #[qproperty(bool, loading_more)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, has_next_page)]
        #[qproperty(i32, total_files)]
        #[qproperty(i32, dir_count)]
        #[qproperty(i32, page_size)]
        #[qproperty(QString, current_system_id)]
        #[qproperty(QString, current_path)]
        #[qproperty(bool, card_write_pending)]
        #[qproperty(QString, card_write_error)]
        #[qproperty(QString, current_description)]
        #[qproperty(QString, current_detail_tags)]
        #[qproperty(bool, current_detail_loading)]
        #[qproperty(QString, current_detail_image_key)]
        #[qproperty(i32, current_detail_image_index)]
        #[qproperty(i32, current_detail_image_count)]
        #[qproperty(bool, current_detail_image_can_prev)]
        #[qproperty(bool, current_detail_image_can_next)]
        #[qproperty(bool, cover_key_roles_enabled)]
        #[qproperty(bool, cover_requests_paused)]
        #[qproperty(i32, visible_first_row)]
        type GamesModel = super::GamesModelRust;

        #[qinvokable]
        fn set_system(self: Pin<&mut GamesModel>, system_id: QString);

        #[qinvokable]
        fn set_path(self: Pin<&mut GamesModel>, path: &QString);

        #[qinvokable]
        fn fetch_more(self: Pin<&mut GamesModel>);

        #[qinvokable]
        fn fetch_more_rapid(self: Pin<&mut GamesModel>);

        #[qinvokable]
        fn prefetch_around(self: Pin<&mut GamesModel>, first_visible_row: i32);

        #[qinvokable]
        fn refresh_cover_keys(self: Pin<&mut GamesModel>, first_row: i32, count: i32);

        #[qinvokable]
        fn clear_pending_cover_requests(self: Pin<&mut GamesModel>);

        #[qinvokable]
        fn launch_at(self: Pin<&mut GamesModel>, index: i32);

        #[qinvokable]
        fn launch_text_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn write_card_at(self: Pin<&mut GamesModel>, index: i32);

        #[qinvokable]
        fn toggle_favorite_at(self: Pin<&mut GamesModel>, index: i32);

        #[qinvokable]
        fn is_favorite_at(self: &GamesModel, index: i32) -> bool;

        #[qinvokable]
        fn cancel_card_write(self: Pin<&mut GamesModel>);

        #[qinvokable]
        fn name_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn description_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn load_description_at(self: Pin<&mut GamesModel>, index: i32);

        #[qinvokable]
        fn clear_current_detail(self: Pin<&mut GamesModel>);

        #[qinvokable]
        fn cycle_detail_image(self: Pin<&mut GamesModel>, delta: i32);

        #[qinvokable]
        fn path_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn system_id_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn entry_type_at(self: &GamesModel, index: i32) -> QString;

        #[qinvokable]
        fn is_media_capable_at(self: &GamesModel, index: i32) -> bool;

        #[qinvokable]
        fn index_for_game_path(self: &GamesModel, path: &QString) -> i32;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut GamesModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut GamesModel>);

        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(
            self: Pin<&mut GamesModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut GamesModel>);

        // Qt signal bound as a callable so the cover-cache bridge can
        // invoke it directly from the Qt thread when an async cover
        // fetch completes for a row that is already on screen.
        #[inherit]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut GamesModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );

        #[cxx_name = "rowCount"]
        fn row_count(self: &GamesModel, parent: &QModelIndex) -> i32;
        fn data(self: &GamesModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_name = "roleNames"]
        fn role_names(self: &GamesModel) -> QHash_i32_QByteArray;

        // Materialise a `QModelIndex` for `(row, column)` so the cover-
        // cache bridge can target individual rows in `dataChanged`.
        // Forwarded to the QAbstractListModel implementation.
        #[inherit]
        fn index(self: &GamesModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
    }

    impl cxx_qt::Threading for GamesModel {}
}

impl ffi::GamesModel {
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
        let entry = &self.entries[index.row() as usize];
        match role {
            NAME_ROLE => QVariant::from(&QString::from(entry.name.as_str())),
            PATH_ROLE => QVariant::from(&QString::from(entry.path.as_str())),
            ZAP_SCRIPT_ROLE => QVariant::from(&QString::from(entry.zap_script.as_str())),
            SYSTEM_ID_ROLE => QVariant::from(&QString::from(entry_system_id(entry).as_str())),
            COVER_KEY_ROLE => QVariant::from(&QString::from(
                if self.cover_key_roles_enabled {
                    cover_key_for(
                        entry,
                        u32::try_from(self.page_size.max(1)).unwrap_or(1),
                        !self.cover_requests_paused,
                    )
                } else {
                    cover_placeholder_for(entry)
                }
                .as_str(),
            )),
            ENTRY_TYPE_ROLE => QVariant::from(&QString::from(entry.entry_type.as_str())),
            FILE_COUNT_ROLE => QVariant::from(&i32::try_from(entry.file_count).unwrap_or(i32::MAX)),
            FAVORITE_ROLE => QVariant::from(&favorite_role_value(&entry.tags)),
            DESCRIPTION_ROLE => QVariant::from(&QString::from(entry.description.as_str())),
            FILE_STEM_ROLE => {
                QVariant::from(&QString::from(file_stem_or_name(&entry.path, &entry.name)))
            }
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut h = QHash::<QHashPair_i32_QByteArray>::default();
        h.insert(NAME_ROLE, QByteArray::from("name"));
        h.insert(PATH_ROLE, QByteArray::from("path"));
        h.insert(ZAP_SCRIPT_ROLE, QByteArray::from("zapScript"));
        h.insert(SYSTEM_ID_ROLE, QByteArray::from("systemId"));
        h.insert(COVER_KEY_ROLE, QByteArray::from("coverKey"));
        h.insert(ENTRY_TYPE_ROLE, QByteArray::from("entryType"));
        h.insert(FILE_COUNT_ROLE, QByteArray::from("fileCount"));
        h.insert(FAVORITE_ROLE, QByteArray::from("favorite"));
        h.insert(DESCRIPTION_ROLE, QByteArray::from("description"));
        h.insert(FILE_STEM_ROLE, QByteArray::from("fileStem"));
        h
    }

    fn set_system(mut self: Pin<&mut Self>, system_id: QString) {
        let sid = system_id.to_string();
        self.as_mut().set_current_system_id(system_id);
        // No-op short-circuit removed deliberately: SystemsScreen
        // accept always wants a "show me this system again" round
        // trip after Esc-back, even when the same system is current.
        // The Endpoint cache makes the repeat cheap (cached
        // `BrowseArgs` returns its existing `Ready` value
        // synchronously through the watcher's seed).
        let systems = if sid.is_empty() {
            Vec::new()
        } else {
            vec![sid]
        };
        self.start_initial_browse(String::new(), systems, true);
    }

    fn set_path(self: Pin<&mut Self>, path: &QString) {
        let p = path.to_string();
        let sid = self.current_system_id.to_string();
        let systems = if sid.is_empty() {
            Vec::new()
        } else {
            vec![sid]
        };
        self.start_initial_browse(p, systems, false);
    }

    fn fetch_more(self: Pin<&mut Self>) {
        self.fetch_more_with_limit(FETCH_MORE_CHUNK_SIZE);
    }

    fn fetch_more_rapid(self: Pin<&mut Self>) {
        self.fetch_more_with_limit(FETCH_MORE_RAPID_CHUNK_SIZE);
    }

    fn fetch_more_with_limit(mut self: Pin<&mut Self>, limit: i32) {
        // Debounce: PagedGrid fires `loadMoreRequested` once per page
        // turn, but a fast spinner on the last loaded page can fire
        // twice before the first follow-up returns. All three guards
        // matter: `loading_more` covers in-flight, `has_next_page`
        // covers terminal pages, and `next_cursor.is_none()` covers
        // the in-between window in `apply_append_page` where the final
        // chunk has cleared the cursor before flipping
        // `has_next_page` (the flip is intentionally deferred so the
        // pending-target watchdog reads a fresh `itemCount`). Without
        // this third guard, a `_commitPendingTarget` re-fire from
        // `onItemCountChanged` would dispatch `media.browse` with
        // `cursor=None`, re-fetching from page 1 and splicing
        // duplicates onto the loaded slice.
        if self.loading_more || !self.has_next_page || self.next_cursor.is_none() {
            return;
        }
        let cursor = self.next_cursor.clone();
        let path = self.current_path.to_string();
        let sid = self.current_system_id.to_string();
        let systems = if sid.is_empty() {
            Vec::new()
        } else {
            vec![sid]
        };
        // Read the active ticket WITHOUT bumping it. `fetch_more`
        // continues the same path's load — only `set_system` /
        // `set_path` invalidate the prior cursor sequence.
        let seq = self.seq.clone();
        let ticket = seq.load(Ordering::SeqCst);
        let max_results = u32::try_from(limit.max(1)).unwrap_or(u32::from(u16::MAX));
        self.as_mut().set_loading_more(true);
        let qt_thread = self.qt_thread();
        let store = global_store();
        // Capture the cursor we're advancing from. If `next_cursor`
        // differs by the time the response arrives (e.g. a watcher
        // refetch reset the chain via `apply_initial_page`), this
        // append no longer belongs to the current page chain and
        // would corrupt the freshly-reset entries.
        let expected_prev_cursor = cursor.clone();
        global_handle().spawn(async move {
            let result = store
                .client()
                .media_browse(MediaBrowseParams {
                    path,
                    systems,
                    max_results: Some(max_results),
                    cursor,
                    letter: None,
                    sort: None,
                })
                .await;
            let _ = qt_thread.queue(move |model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                apply_append_page(model, result, expected_prev_cursor.as_deref());
            });
        });
    }

    /// Rebuild the cover queue around the current visual page.
    ///
    /// `first_visible_row` is the row index of the topmost visible
    /// tile (`gamesGrid.currentPage * gamesGrid.pageSize` from QML).
    /// The replacement queue drains current page first, then next
    /// page, then previous page. Queued stale requests are dropped so
    /// a page change immediately makes the new visible page win.
    fn prefetch_around(self: Pin<&mut Self>, first_visible_row: i32) {
        let cache = global_media_image_cache();
        if self.cover_requests_paused {
            cache.clear_pending_requests();
            return;
        }
        let plan =
            prefetch_around_plan(&self.entries, self.count, self.page_size, first_visible_row);
        let ps = u32::try_from(self.page_size.max(1)).unwrap_or(1);
        cache.replace_pending_requests_ordered(plan, ps);
    }

    fn refresh_cover_keys(mut self: Pin<&mut Self>, first_row: i32, count: i32) {
        emit_cover_key_range(self.as_mut(), first_row, count);
    }

    fn clear_pending_cover_requests(self: Pin<&mut Self>) {
        global_media_image_cache().clear_pending_requests();
    }

    fn launch_at(self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            return;
        }
        let entry = &self.entries[index as usize];
        let fallback_text = run_text_for_entry(entry);
        if fallback_text.is_empty() {
            return;
        }
        let params = singleton_directory_needs_launch_resolution(entry)
            .then(|| meta_params_for_entry(entry))
            .flatten();
        let name = entry.name.clone();
        let store = global_store();
        global_handle().spawn(async move {
            let text = if let Some(params) = params {
                match store.client().media_meta(params).await {
                    Ok(result) if !result.media.path.trim().is_empty() => result.media.path,
                    Ok(_) => fallback_text.clone(),
                    Err(e) => {
                        warn!(
                            "singleton launch path resolve failed for {name}: {}",
                            e.message
                        );
                        fallback_text.clone()
                    }
                }
            } else {
                fallback_text.clone()
            };
            if let Err(e) = store.run_mutation::<RunMutation>(RunParams { text }).await {
                warn!("run failed for {name}: {}", e.message);
            }
        });
    }

    fn launch_text_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(portable_text_for_entry(&self.entries[index as usize]).as_str())
    }

    fn write_card_at(mut self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            self.as_mut()
                .set_card_write_error(QString::from("invalid selection"));
            self.as_mut().set_card_write_pending(false);
            return;
        }
        let entry = &self.entries[index as usize];
        let text = portable_text_for_entry(entry);
        if text.is_empty() {
            self.as_mut()
                .set_card_write_error(QString::from("missing launch payload"));
            self.as_mut().set_card_write_pending(false);
            return;
        }
        let name = entry.name.clone();
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

    fn toggle_favorite_at(self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            return;
        }
        let entry = &self.entries[index as usize];
        if !is_media_capable_entry(entry) {
            return;
        }
        let Some(params) = favorite_params_for_entry(entry, !has_favorite_tag(&entry.tags)) else {
            warn!(
                "favorite update skipped: missing media identity for {}",
                entry.name
            );
            return;
        };
        let name = entry.name.clone();
        let media_id = entry.media_id;
        let system_id = entry_system_id(entry);
        let path = entry.path.clone();
        let store = global_store();
        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            match store.run_mutation::<MediaTagsUpdateMutation>(params).await {
                Ok(result) => {
                    let _ = qt_thread.queue(move |mut model| {
                        apply_favorite_tags(
                            model.as_mut(),
                            index,
                            media_id,
                            &system_id,
                            &path,
                            result.tags,
                        );
                    });
                }
                Err(e) => warn!("favorite update failed for {name}: {}", e.message),
            }
        });
    }

    fn is_favorite_at(&self, index: i32) -> bool {
        if index < 0 || index >= self.count {
            return false;
        }
        has_favorite_tag(&self.entries[index as usize].tags)
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

    fn name_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].name.as_str())
    }

    fn description_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].description.as_str())
    }

    fn load_description_at(mut self: Pin<&mut Self>, index: i32) {
        let ticket = self
            .as_mut()
            .rust_mut()
            .description_seq
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        if index < 0 || index >= self.count {
            self.as_mut().set_current_detail_loading(false);
            self.as_mut().set_current_description(QString::default());
            self.as_mut().set_current_detail_tags(QString::default());
            clear_detail_images(self.as_mut());
            return;
        }

        let entry = &self.entries[index as usize];
        if !is_media_capable_entry(entry) {
            self.as_mut().set_current_detail_loading(false);
            self.as_mut().set_current_description(QString::default());
            self.as_mut().set_current_detail_tags(QString::default());
            clear_detail_images(self.as_mut());
            return;
        }

        let description = entry.description.clone();
        let detail_tags = detail_tags_from_entry(entry);
        let detail_image_key = media_key_for(entry);
        let Some(params) = meta_params_for_entry(entry) else {
            self.as_mut().set_current_detail_loading(false);
            self.as_mut()
                .set_current_description(QString::from(description.as_str()));
            self.as_mut()
                .set_current_detail_tags(QString::from(detail_tags.as_str()));
            set_single_detail_image_key(self.as_mut(), detail_image_key);
            return;
        };

        self.as_mut().set_current_detail_loading(true);
        self.as_mut()
            .set_current_description(QString::from(description.as_str()));
        self.as_mut()
            .set_current_detail_tags(QString::from(detail_tags.as_str()));
        set_single_detail_image_key(self.as_mut(), detail_image_key);

        let seq = self.rust().description_seq.clone();
        let qt_thread = self.qt_thread();
        let store = global_store();
        global_handle().spawn(async move {
            let result = store.client().media_meta(params).await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                match result {
                    Ok(result) => {
                        let meta = result.media;
                        let description = description_from_meta(&meta);
                        if !description.is_empty() {
                            model
                                .as_mut()
                                .set_current_description(QString::from(description.as_str()));
                        }
                        model.as_mut().set_current_detail_tags(QString::from(
                            detail_tags_from_meta(&meta).as_str(),
                        ));
                        let mut detail_keys = detail_image_keys_from_meta(
                            &meta,
                            meta.title.system.id.as_str(),
                            meta.path.as_str(),
                        );
                        if detail_keys.is_empty() {
                            if let Some(key) = media_key_for(&model.entries[index as usize]) {
                                detail_keys.push(key);
                            }
                        }
                        set_detail_image_keys(model.as_mut(), detail_keys);
                    }
                    Err(e) => warn!("games detail fetch failed: {}", e.message),
                }
                model.as_mut().set_current_detail_loading(false);
            });
        });
    }

    fn clear_current_detail(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .description_seq
            .fetch_add(1, Ordering::SeqCst);
        self.as_mut().set_current_detail_loading(false);
        self.as_mut().set_current_description(QString::default());
        self.as_mut().set_current_detail_tags(QString::default());
        clear_detail_images(self);
    }

    fn cycle_detail_image(self: Pin<&mut Self>, delta: i32) {
        if delta == 0 || self.current_detail_image_count <= 1 {
            return;
        }
        let current = self.current_detail_image_index;
        let next = (current + delta).clamp(0, self.current_detail_image_count - 1);
        if next == current {
            return;
        }
        set_detail_image_index(self, next);
    }

    fn path_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].path.as_str())
    }

    fn system_id_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(entry_system_id(&self.entries[index as usize]).as_str())
    }

    fn entry_type_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].entry_type.as_str())
    }

    fn is_media_capable_at(&self, index: i32) -> bool {
        if index < 0 || index >= self.count {
            return false;
        }
        is_media_capable_entry(&self.entries[index as usize])
    }

    fn index_for_game_path(&self, path: &QString) -> i32 {
        position_of_game_path(&self.entries, &path.to_string())
    }

    /// Spin up the long-lived cover-cache subscriber on first use.
    /// Subsequent calls are no-ops — the model singleton owns exactly
    /// one subscriber for the whole process lifetime, decoupled from
    /// `seq` because cover updates are not tied to a particular browse
    /// path. Lagged broadcast frames are dropped silently; the
    /// `dataChanged` we'd otherwise emit is recoverable on the next
    /// scroll because `data()` re-reads the cache for every visible
    /// row's `coverKey`.
    fn ensure_cover_subscription(mut self: Pin<&mut Self>) {
        if self.cover_subscription.is_some() {
            return;
        }
        let cache = global_media_image_cache();
        let mut rx = cache.subscribe();
        let qt_thread = self.qt_thread();
        let handle = global_handle().spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        let _ = qt_thread.queue(move |model| {
                            notify_cover_update(model, &update.key);
                        });
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
        self.as_mut().rust_mut().cover_subscription = Some(handle);
    }

    /// Issue a fresh `media.browse` for `(path, systems)`. Bumps `seq`,
    /// aborts the prior watcher, clears entries via `beginResetModel`,
    /// subscribes to `MediaBrowseEndpoint`, and spawns a watcher whose
    /// queued callbacks bail unless the ticket still matches.
    ///
    /// `eligible_for_auto_nav` is set when this load came from
    /// `set_system`. The single-root auto-nav case in
    /// `apply_initial_page` consumes the flag to decide whether to skip
    /// rendering the 1-item roots list and dive straight into that
    /// root.
    fn start_initial_browse(
        mut self: Pin<&mut Self>,
        path: String,
        systems: Vec<String>,
        eligible_for_auto_nav: bool,
    ) {
        info!(
            ?path,
            ?systems,
            eligible_for_auto_nav,
            page_size = self.page_size,
            "games: start_initial_browse",
        );
        self.as_mut().ensure_cover_subscription();
        self.as_mut().set_current_path(QString::from(path.as_str()));
        self.as_mut().set_loading(true);
        self.as_mut().set_error_message(QString::default());
        self.as_mut().set_current_detail_loading(false);
        self.as_mut().set_current_description(QString::default());
        self.as_mut().set_current_detail_tags(QString::default());
        self.as_mut()
            .rust()
            .description_seq
            .fetch_add(1, Ordering::SeqCst);
        self.as_mut().set_has_next_page(false);
        self.as_mut().set_loading_more(false);
        self.as_mut().rust_mut().auto_nav_eligible = eligible_for_auto_nav;
        // Cleared on every browse target swap: a new path needs the
        // full `apply_initial_page` reset to install fresh entries.
        // The flag is set back to true at the end of that function
        // so subsequent refetches (cache invalidations) skip the
        // reset and preserve appended pages + selection.
        self.as_mut().rust_mut().is_seeded = false;
        self.as_mut().rust_mut().next_cursor = None;
        // Drop any held initial-look-ahead gate from the prior browse —
        // its append (if it lands at all) will be ticket-rejected and
        // can't re-arm the gate for this new target.
        self.as_mut().rust_mut().pending_initial_lookahead = false;
        // Invalidate any in-flight sub-batch posts from the prior
        // browse: each posted closure compares against the snapshotted
        // ticket and bails if the model has moved on. Otherwise a
        // batch from the old chain could splice rows onto the new
        // dataset.
        self.as_mut()
            .rust_mut()
            .append_seq
            .fetch_add(1, Ordering::SeqCst);
        if !self.entries.is_empty() {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().entries.clear();
            self.as_mut().rust_mut().count = 0;
            self.as_mut().end_reset_model();
            self.as_mut().count_changed();
        }
        // Total-files counter resets too — the previous path's
        // denominator would be misleading until the new fetch lands.
        self.as_mut().set_total_files(0);
        // Same reasoning for dir_count: a stale value from the previous
        // browse target would misclassify which leading entries are
        // folders if the new fetch fails or stalls.
        self.as_mut().set_dir_count(0);

        if let Some(handle) = self.as_mut().rust_mut().watcher.take() {
            handle.abort();
        }
        // Tear down any cover gate left from the prior path. See
        // `reset_cover_gate` for the rationale; without this teardown
        // a stale timer callback could fire after the new path's
        // `set_loading(true)` and prematurely release its gate.
        reset_cover_gate(self.as_mut());

        let seq = self.rust().seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;

        let max_results = u32::try_from(self.page_size.max(1)).unwrap_or(u32::from(u16::MAX));
        let resource = global_store().subscribe::<MediaBrowseEndpoint>(BrowseArgs::new(
            path,
            systems,
            max_results,
        ));
        let mut status_rx = resource.subscribe();

        // Spawn the watcher BEFORE applying the sync seed. If
        // `apply_status` recurses into `start_initial_browse` (auto-nav
        // cascade on a single-folder result), the inner call's
        // `watcher.take().abort()` cleans up this handle correctly —
        // doing this in the other order leaks the inner watcher when
        // the outer call later overwrites `self.watcher`.
        let qt_thread = self.qt_thread();
        let snapshot = status_rx.borrow_and_update().clone();
        let seq_for_loop = seq.clone();
        let handle = global_handle().spawn(async move {
            while status_rx.changed().await.is_ok() {
                let snapshot = status_rx.borrow_and_update().clone();
                let seq_for_closure = seq_for_loop.clone();
                let _ = qt_thread.queue(move |model| {
                    if seq_for_closure.load(Ordering::SeqCst) != ticket {
                        return;
                    }
                    apply_status(model, snapshot);
                });
            }
        });
        self.as_mut().rust_mut().watcher = Some(handle);

        // Sync seed runs inline on the Qt thread, so it cannot race a
        // queued callback (Qt won't pump events until set_system /
        // set_path returns). No ticket check needed; the value we read
        // is whichever the resource has right now.
        apply_status(self.as_mut(), snapshot);
    }
}

/// Resolve the system id role exposed to QML. Single-system entries
/// (media items, single-system roots/directories) populate `system_id`;
/// multi-system roots fall back to the first entry of `system_ids` so
/// the tile still has *a* logo to draw rather than a blank.
fn entry_system_id(entry: &BrowseEntry) -> String {
    if !entry.system_id.is_empty() {
        return entry.system_id.clone();
    }
    entry.system_ids.first().cloned().unwrap_or_default()
}

fn is_media_capable_entry(entry: &BrowseEntry) -> bool {
    entry.entry_type == "media"
        || (entry.entry_type == "directory"
            && (entry.media_id.is_some() || !entry.zap_script.is_empty()))
}

fn run_text_for_entry(entry: &BrowseEntry) -> String {
    if !entry.path.trim().is_empty() {
        return entry.path.clone();
    }
    entry.zap_script.clone()
}

fn meta_params_for_entry(entry: &BrowseEntry) -> Option<MediaMetaParams> {
    if let Some(media_id) = entry.media_id {
        return Some(MediaMetaParams::for_media_id(media_id));
    }
    let system_id = entry_system_id(entry);
    if system_id.trim().is_empty() || entry.path.trim().is_empty() {
        return None;
    }
    Some(MediaMetaParams::for_media(system_id, entry.path.clone()))
}

fn singleton_directory_needs_launch_resolution(entry: &BrowseEntry) -> bool {
    entry.entry_type == "directory" && entry.media_id.is_some()
}

fn description_from_meta(meta: &MediaMeta) -> String {
    meta.title
        .properties
        .get("property:description")
        .or_else(|| meta.properties.get("property:description"))
        .map(|property| property.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn detail_tags_from_meta(meta: &MediaMeta) -> String {
    let source = if meta.title.tags.is_empty() {
        meta.tags.as_slice()
    } else {
        meta.title.tags.as_slice()
    };
    detail_tags_from_tags(source)
}

fn image_type_from_property_key(key: &str) -> Option<String> {
    let suffix = key.strip_prefix("property:image")?;
    if suffix.is_empty() {
        return Some("image".to_string());
    }
    Some(suffix.trim_start_matches('-').to_string()).filter(|image_type| !image_type.is_empty())
}

fn detail_image_keys_from_meta(meta: &MediaMeta, system: &str, path: &str) -> Vec<MediaKey> {
    let mut seen = BTreeSet::<String>::new();
    let mut ordered = Vec::<String>::new();
    for image_type in meta
        .available_image_types
        .iter()
        .chain(meta.title.available_image_types.iter())
    {
        if !image_type.trim().is_empty() && seen.insert(image_type.clone()) {
            ordered.push(image_type.clone());
        }
    }
    if ordered.is_empty() {
        for key in meta
            .title
            .properties
            .keys()
            .chain(meta.properties.keys())
            .filter_map(|key| image_type_from_property_key(key))
        {
            seen.insert(key);
        }
        if seen.remove("image") {
            ordered.push("image".to_string());
        }
        ordered.extend(seen);
    }
    ordered
        .into_iter()
        .map(|image_type| MediaKey::with_image_type(system, path, image_type))
        .collect()
}

fn portable_text_for_entry(entry: &BrowseEntry) -> String {
    if !entry.zap_script.trim().is_empty() {
        return entry.zap_script.clone();
    }
    entry.path.clone()
}

fn detail_tags_from_entry(entry: &BrowseEntry) -> String {
    detail_tags_from_tags(entry.tags.as_slice())
}

fn detail_tags_from_tags(source: &[TagInfo]) -> String {
    let rows = [
        (
            "Year",
            detail_value_for_aliases(source, &["year", "release date", "release_date"]),
        ),
        (
            "Genre",
            detail_value_for_aliases(source, &["genre", "gamegenre"]),
        ),
        ("Players", detail_value_for_aliases(source, &["players"])),
        (
            "Developer",
            detail_value_for_aliases(source, &["developer"]),
        ),
        (
            "Publisher",
            detail_value_for_aliases(source, &["publisher"]),
        ),
        ("Rating", detail_value_for_aliases(source, &["rating"])),
    ];
    rows.into_iter()
        .map(|(label, value)| format!("{label}\t{value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn detail_value_for_aliases(source: &[TagInfo], aliases: &[&str]) -> String {
    source
        .iter()
        .filter(|tag| {
            aliases
                .iter()
                .any(|alias| tag.tag_type.eq_ignore_ascii_case(alias))
                && !tag.tag.trim().is_empty()
        })
        .map(|tag| tag.tag.trim().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn file_stem_or_name(path: &str, name: &str) -> String {
    let file = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem).trim();
    if stem.is_empty() {
        name.to_string()
    } else {
        stem.to_string()
    }
}

fn clear_detail_images(mut model: Pin<&mut ffi::GamesModel>) {
    model.as_mut().rust_mut().detail_image_keys.clear();
    model
        .as_mut()
        .set_current_detail_image_key(QString::default());
    model.as_mut().set_current_detail_image_index(0);
    model.as_mut().set_current_detail_image_count(0);
    model.as_mut().set_current_detail_image_can_prev(false);
    model.as_mut().set_current_detail_image_can_next(false);
}

fn set_detail_image_keys(mut model: Pin<&mut ffi::GamesModel>, keys: Vec<MediaKey>) {
    model.as_mut().rust_mut().detail_image_keys = keys;
    model.as_mut().set_current_detail_image_index(0);
    let count = i32::try_from(model.detail_image_keys.len()).unwrap_or(i32::MAX);
    model.as_mut().set_current_detail_image_count(count);
    model.as_mut().set_current_detail_image_can_prev(false);
    model.as_mut().set_current_detail_image_can_next(count > 1);
    sync_current_detail_image_key_with_page_size(model, 1);
}

fn set_single_detail_image_key(model: Pin<&mut ffi::GamesModel>, key: Option<MediaKey>) {
    set_detail_image_keys(model, key.into_iter().collect());
}

fn set_detail_image_index(mut model: Pin<&mut ffi::GamesModel>, index: i32) {
    let count = i32::try_from(model.detail_image_keys.len()).unwrap_or(i32::MAX);
    let clamped = if count <= 0 {
        0
    } else {
        index.clamp(0, count - 1)
    };
    model.as_mut().set_current_detail_image_index(clamped);
    model.as_mut().set_current_detail_image_count(count);
    model
        .as_mut()
        .set_current_detail_image_can_prev(clamped > 0);
    model
        .as_mut()
        .set_current_detail_image_can_next(count > 0 && clamped < count - 1);
    sync_current_detail_image_key(model);
}

fn sync_current_detail_image_key(model: Pin<&mut ffi::GamesModel>) {
    let page_size = u32::try_from(model.page_size.max(1)).unwrap_or(1);
    sync_current_detail_image_key_with_page_size(model, page_size);
}

fn sync_current_detail_image_key_with_page_size(
    mut model: Pin<&mut ffi::GamesModel>,
    page_size: u32,
) {
    let index = model.current_detail_image_index;
    if index < 0 {
        model
            .as_mut()
            .set_current_detail_image_key(QString::default());
        return;
    }
    let Some(key) = model.detail_image_keys.get(index as usize).cloned() else {
        model
            .as_mut()
            .set_current_detail_image_key(QString::default());
        return;
    };
    let cache = global_media_image_cache();
    if cache.is_cached(&key) {
        model.as_mut().set_current_detail_image_key(QString::from(
            MediaImageCache::image_key_for(&key).as_str(),
        ));
    } else if cache.is_negative(&key) || cache.is_soft_no_image(&key) {
        model
            .as_mut()
            .set_current_detail_image_key(QString::default());
    } else {
        cache.enqueue_with_media_id(key, None, page_size.max(1));
        model
            .as_mut()
            .set_current_detail_image_key(QString::from("icons/Loading"));
    }
}

fn cover_placeholder_for(entry: &BrowseEntry) -> String {
    if !is_media_capable_entry(entry) && entry.is_folder() {
        "icons/Folder".to_string()
    } else {
        "icons/File".to_string()
    }
}

fn cover_key_for(entry: &BrowseEntry, page_size: u32, requests_enabled: bool) -> String {
    if !is_media_capable_entry(entry) && entry.is_folder() {
        return "icons/Folder".to_string();
    }
    let media_key = media_key_for(entry).map(MediaKey::with_current_cover_preference);
    let cache = global_media_image_cache();
    let cached = media_key.as_ref().is_some_and(|k| cache.is_cached(k));
    let negative = media_key.as_ref().is_some_and(|k| cache.is_negative(k));
    let soft_no_image = media_key
        .as_ref()
        .is_some_and(|k| cache.is_soft_no_image(k));
    if requests_enabled && !cached && !negative && !soft_no_image {
        // Miss-driven re-enqueue: when QML asks for the cover URL of
        // a media entry whose bytes aren't in the cache, kick a fetch
        // right here. This is the only implicit path covers reach the
        // queue — `apply_initial_page` / `apply_append_page` no longer
        // bulk-enqueue, so a tile is enqueued exactly when the grid
        // materialises its delegate. Combined with `prefetch_around`
        // for the next-page warm, the queue contents always reflect
        // the user's current view rather than whichever metadata
        // chunk Core most recently sent back.
        if let Some(k) = media_key.as_ref() {
            cache.enqueue_with_media_id(k.clone(), entry.media_id, page_size);
        }
    }
    cover_key_for_with(
        entry,
        media_key.as_ref(),
        cached,
        negative || soft_no_image || !requests_enabled,
    )
}

fn emit_cover_key_range(mut model: Pin<&mut ffi::GamesModel>, first_row: i32, count: i32) {
    if model.count <= 0 || count <= 0 {
        return;
    }
    let first = first_row.clamp(0, model.count - 1);
    let last = first.saturating_add(count - 1).min(model.count - 1);
    if last < first {
        return;
    }
    let mut roles = QList::<i32>::default();
    roles.append(COVER_KEY_ROLE);
    let parent = QModelIndex::default();
    let top_left = model.index(first, 0, &parent);
    let bottom_right = model.index(last, 0, &parent);
    model
        .as_mut()
        .data_changed(&top_left, &bottom_right, &roles);
}

/// Build the canonical `(systemId, path)` identifier for a media
/// entry. Returns `None` for entries the cache cannot key on (folder
/// roots, unattributed entries, browse roots without a path).
fn media_key_for(entry: &BrowseEntry) -> Option<MediaKey> {
    if !is_media_capable_entry(entry) || entry.path.is_empty() {
        return None;
    }
    let system_id = entry_system_id(entry);
    if system_id.is_empty() {
        return None;
    }
    match entry.media_id {
        Some(media_id) => Some(MediaKey::with_media_id(
            system_id,
            entry.path.clone(),
            media_id,
        )),
        None => Some(MediaKey::new(system_id, entry.path.clone())),
    }
}

/// Pure ordering helper for `prefetch_around`. Returns
/// (`MediaKey`, `media_id`) pairs in desired fetch order: current page
/// top-to-bottom, then next page, then previous page. Folders and
/// entries without a `media_key` are skipped.
fn prefetch_around_plan(
    entries: &[BrowseEntry],
    count: i32,
    page_size: i32,
    first_visible_row: i32,
) -> Vec<(MediaKey, Option<i64>)> {
    if count <= 0 {
        return Vec::new();
    }
    let page_size = page_size.max(1);
    let first = first_visible_row.clamp(0, count.saturating_sub(1));
    let current_end = first.saturating_add(page_size).min(count);
    let next_end = current_end.saturating_add(page_size).min(count);
    let previous_start = first.saturating_sub(page_size);
    let mut plan: Vec<(MediaKey, Option<i64>)> =
        Vec::with_capacity(((next_end - previous_start) as usize).min(entries.len()));
    let push = |row: i32, plan: &mut Vec<(MediaKey, Option<i64>)>| {
        let idx = row as usize;
        if idx >= entries.len() {
            return;
        }
        let entry = &entries[idx];
        if let Some(key) = media_key_for(entry) {
            plan.push((key.with_current_cover_preference(), entry.media_id));
        }
    };
    for row in first..current_end {
        push(row, &mut plan);
    }
    for row in current_end..next_end {
        push(row, &mut plan);
    }
    for row in previous_start..first {
        push(row, &mut plan);
    }
    plan
}

fn has_favorite_tag(tags: &[TagInfo]) -> bool {
    tags.iter()
        .any(|tag| tag.tag_type == "user" && tag.tag == "favorite")
}

fn favorite_role_value(tags: &[TagInfo]) -> i32 {
    i32::from(has_favorite_tag(tags))
}

fn favorite_params_for_entry(entry: &BrowseEntry, add: bool) -> Option<MediaTagsUpdateParams> {
    let mut params = MediaTagsUpdateParams::default();
    if add {
        params.add.push("user:favorite".to_string());
    } else {
        params.remove.push("user:favorite".to_string());
    }
    if let Some(media_id) = entry.media_id {
        params.media_id = Some(media_id);
        return Some(params);
    }

    let system = entry_system_id(entry);
    if system.is_empty() || entry.path.is_empty() {
        return None;
    }
    params.system = system;
    params.path.clone_from(&entry.path);
    Some(params)
}

fn apply_favorite_tags(
    mut model: Pin<&mut ffi::GamesModel>,
    index: i32,
    media_id: Option<i64>,
    system_id: &str,
    path: &str,
    tags: Vec<TagInfo>,
) {
    if index < 0 || index >= model.count {
        return;
    }
    let entry = &model.entries[index as usize];
    let same_entry = if media_id.is_some() {
        entry.media_id == media_id
    } else {
        entry_system_id(entry) == system_id && entry.path == path
    };
    if !same_entry {
        return;
    }
    model.as_mut().rust_mut().entries[index as usize].tags = tags;
    let mut roles = QList::<i32>::default();
    roles.append(FAVORITE_ROLE);
    let parent = QModelIndex::default();
    let idx = model.index(index, 0, &parent);
    model.as_mut().data_changed(&idx, &idx, &roles);
}

/// Pure helper for `cover_key_for`. Split out so tests can drive the
/// branches (folder, cached, uncached, negative-memoed, unattributed)
/// without spinning up the global cover cache and its tokio runtime.
///
/// `icons/Loading` is the in-flight cue: an entry that has a media key
/// but no cached bytes and no negative memo is one we're actively
/// fetching (or about to). Tile.qml's cover Image renders that
/// hourglass at full size until the cache update broadcast lands and
/// `dataChanged(COVER_KEY_ROLE)` flips this to either `media-image/...`
/// (success) or `icons/File` (negative memo).
fn cover_key_for_with(
    entry: &BrowseEntry,
    key: Option<&MediaKey>,
    cached: bool,
    unavailable: bool,
) -> String {
    if !is_media_capable_entry(entry) && entry.is_folder() {
        return "icons/Folder".to_string();
    }
    match key {
        Some(k) if cached => MediaImageCache::image_key_for(k),
        Some(_) if !unavailable => "icons/Loading".to_string(),
        _ => "icons/File".to_string(),
    }
}

/// Emit `dataChanged(coverKey)` for every row whose entry's
/// `(systemId, path)` matches `key`. Cheap walk of the current
/// `entries` vec — pages top out at a few hundred rows after look-
/// ahead, and the bridge runs only when the cover-cache fetch driver
/// delivers a result.
///
/// Also drains `pending_first_paint_keys`: each cover landing during
/// the gate's hold ticks the set down, and emptying the set releases
/// the gate so the screen-flip overlay clears.
fn notify_cover_update(mut model: Pin<&mut ffi::GamesModel>, key: &MediaKey) {
    let rows: Vec<i32> = model
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            key.is_cover_key()
                && is_media_capable_entry(e)
                && match (key.media_id, e.media_id) {
                    (Some(a), Some(b)) => a == b,
                    _ => e.path == *key.path && entry_system_id(e) == *key.system_id,
                }
        })
        .filter_map(|(i, _)| i32::try_from(i).ok())
        .collect();
    if !rows.is_empty() {
        let mut roles = QList::<i32>::default();
        roles.append(COVER_KEY_ROLE);
        let parent = QModelIndex::default();
        for row in rows {
            let idx = model.index(row, 0, &parent);
            model.as_mut().data_changed(&idx, &idx, &roles);
        }
    }
    if model
        .detail_image_keys
        .get(model.current_detail_image_index.max(0) as usize)
        .is_some_and(|current| current == key)
    {
        sync_current_detail_image_key(model.as_mut());
    }
    // Tick the gate's pending set down. `remove` returns false if the
    // key wasn't gated (broadcast events fire for every cache update,
    // including miss-recovery enqueues from `cover_key_for`); we only
    // try to release when a gated key was actually drained.
    let was_pending = model
        .as_mut()
        .rust_mut()
        .pending_first_paint_keys
        .remove(key);
    if was_pending && model.pending_first_paint_keys.is_empty() && model.loading {
        if let Some(handle) = model.as_mut().rust_mut().cover_gate_timer.take() {
            handle.abort();
        }
        // Bytes are cached, but QML's `MediaImageProvider` still has to
        // decode them. The hidden cover pre-warmer in `GamesScreen.qml`
        // dispatches all N requests at once and the provider's 4-worker
        // pool decodes them in ~75–150 ms; without this settle window
        // the gate flips `loading=false` ~80 ms after the last byte
        // lands and `gamesGrid` materialises before the last few
        // decodes complete, painting the procedural fallback over those
        // tiles for a frame or two. The settle uses the same seq-ticket
        // guard as the safety timer so a folder change cancels the
        // pending release.
        info!("games: cover gate bytes settled — entering decode-settle window");
        let seq = model.rust().cover_gate_seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
        let qt_thread = model.qt_thread();
        let handle = global_handle().spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = qt_thread.queue(move |mut model: Pin<&mut ffi::GamesModel>| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                model.as_mut().rust_mut().cover_gate_timer = None;
                if model.loading {
                    info!("games: cover gate released after decode-settle window");
                    model.as_mut().set_loading(false);
                }
            });
        });
        model.as_mut().rust_mut().cover_gate_timer = Some(handle);
    }
}

/// Compute the set of media keys on the current page whose covers we
/// must wait on before releasing the cover gate. Folders, unattributed
/// entries, already-cached keys, and negatively-memoised keys are all
/// excluded. Pure helper so the gate's binning logic is unit-testable
/// without spinning up the global cache + tokio runtime.
fn compute_unresolved_keys<F, G>(
    entries: &[BrowseEntry],
    is_cached: F,
    is_negative: G,
) -> HashSet<MediaKey>
where
    F: Fn(&MediaKey) -> bool,
    G: Fn(&MediaKey) -> bool,
{
    entries
        .iter()
        .filter_map(|entry| media_key_for(entry).map(MediaKey::with_current_cover_preference))
        .filter(|k| !is_cached(k) && !is_negative(k))
        .collect()
}

/// Abort any in-flight cover-gate timer, drop the waiting-keys set,
/// and bump `cover_gate_seq` so a callback that already queued onto
/// the Qt thread before the abort took effect sees a stale ticket and
/// bails. Used on every browse status edge that doesn't go on to call
/// `arm_cover_gate` itself (Pending, Errored, and the
/// `start_initial_browse` reset).
fn reset_cover_gate(mut model: Pin<&mut ffi::GamesModel>) {
    if let Some(handle) = model.as_mut().rust_mut().cover_gate_timer.take() {
        handle.abort();
    }
    model.as_mut().rust_mut().pending_first_paint_keys.clear();
    model.rust().cover_gate_seq.fetch_add(1, Ordering::SeqCst);
}

/// Decide whether to hold `loading=true` until the page's covers are
/// cached, or release immediately. Called once per `apply_initial_page`.
///
/// - If every media entry is already cached or negatively-memoised
///   (folder-only page, or revisit), set loading=false right now —
///   there's nothing to wait on, the screen-flip overlay clears.
/// - Otherwise, store the unresolved set on the model, arm a 3 s safety
///   timer, and leave loading=true. `notify_cover_update` will drain
///   the set as covers land; whichever happens first (set empties or
///   timer fires) releases the gate.
///
/// The 3 s timeout is the fall-through: if the bulk RPC stalls, the
/// user sees `Loading…` for at most 3 s before the existing
/// "list with placeholders → covers pop in" behaviour resumes.
fn arm_cover_gate(mut model: Pin<&mut ffi::GamesModel>) {
    if let Some(handle) = model.as_mut().rust_mut().cover_gate_timer.take() {
        handle.abort();
    }
    let cache = global_media_image_cache();
    let unresolved = compute_unresolved_keys(
        &model.entries,
        |k| cache.is_cached(k),
        |k| cache.is_negative(k),
    );
    if unresolved.is_empty() {
        model.as_mut().rust_mut().pending_first_paint_keys.clear();
        if model.loading {
            model.as_mut().set_loading(false);
        }
        return;
    }
    info!(
        pending = unresolved.len(),
        "games: arm cover gate (holding loading until covers cached)"
    );
    model.as_mut().rust_mut().pending_first_paint_keys = unresolved;
    let seq = model.rust().cover_gate_seq.clone();
    let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
    let qt_thread = model.qt_thread();
    let handle = global_handle().spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let _ = qt_thread.queue(move |model| {
            if seq.load(Ordering::SeqCst) != ticket {
                return;
            }
            release_cover_gate_after_timeout(model);
        });
    });
    model.as_mut().rust_mut().cover_gate_timer = Some(handle);
}

/// Clear `pending_initial_lookahead` after the background prefetch
/// lands. Look-ahead no longer participates in full-screen loading:
/// visible page readiness and cover first-paint own that gate.
fn release_initial_lookahead_gate(mut model: Pin<&mut ffi::GamesModel>) {
    model.as_mut().rust_mut().pending_initial_lookahead = false;
}

/// Force-release the cover gate from the safety timer. Called only via
/// the timer's queued callback after a seq-match check; the
/// notify-driven release path lives inline in `notify_cover_update`.
fn release_cover_gate_after_timeout(mut model: Pin<&mut ffi::GamesModel>) {
    let pending = model.pending_first_paint_keys.len();
    info!(pending, "games: cover gate timed out, releasing");
    model.as_mut().rust_mut().pending_first_paint_keys.clear();
    model.as_mut().rust_mut().cover_gate_timer = None;
    // Safety timer is the hard upper bound — release regardless of
    // whether the look-ahead prefetch has landed. Clear the flag too
    // so a late `apply_append_page` doesn't try to flip loading off a
    // second time.
    model.as_mut().rust_mut().pending_initial_lookahead = false;
    if model.loading {
        model.as_mut().set_loading(false);
    }
}

/// Find `needle` in `entries` with case-sensitive path equality.
fn position_of_game_path(entries: &[BrowseEntry], needle: &str) -> i32 {
    if needle.is_empty() {
        return -1;
    }
    entries
        .iter()
        .position(|e| e.path == needle)
        .map_or(-1, |i| i as i32)
}

/// Pure projection of a `ResourceStatus<MediaBrowseResult>` onto the
/// shape `apply_status` writes into the model. Pulled out so the
/// four arms can be unit-tested without a Qt event loop.
#[derive(Debug)]
enum Projection {
    /// `Idle`/`Loading` collapse to the same view: spinner on, no
    /// error. Items are not touched.
    Pending,
    /// Successful fetch; carries the raw result so the apply layer
    /// can decide whether to auto-nav into a single root.
    Ready(MediaBrowseResult),
    /// Both `retrying` and terminal errors map here; the UI treats
    /// them the same (banner + clear loading state).
    Errored { message: String },
}

fn project_status(status: ResourceStatus<MediaBrowseResult>) -> Projection {
    match status {
        ResourceStatus::Idle | ResourceStatus::Loading => Projection::Pending,
        ResourceStatus::Ready(result) => Projection::Ready(result),
        ResourceStatus::Errored { message, .. } => Projection::Errored { message },
    }
}

/// Auto-nav decision for the initial browse result. Pure so the
/// edge cases (single folder, single media, multi-root, empty) are
/// testable without a Qt environment.
#[derive(Debug, PartialEq, Eq)]
enum InitialAction {
    /// The result has exactly one folder entry and the load was
    /// `set_system`-eligible. Skip rendering and recurse into the
    /// single root's path.
    AutoNavigate { path: String },
    /// Render the result as the visible state.
    Apply,
}

fn decide_initial(
    result: &MediaBrowseResult,
    eligible_after_set_system: bool,
    platform: Option<&Platform>,
    current_path: &str,
) -> InitialAction {
    // On MiSTer, single-folder loads also flatten on `set_path`. MiSTer
    // collections often unzip into nested single-child folders; the
    // recursion in `apply_status` calls `start_initial_browse(... false)`
    // on the auto-nav target, so a chain of single-child folders
    // collapses all the way down on each navigation step.
    let mister_set_path_flatten = matches!(platform, Some(Platform::Mister));
    let single_entry_flatten = eligible_after_set_system || mister_set_path_flatten;
    if !single_entry_flatten || result.entries.len() != 1 {
        return InitialAction::Apply;
    }
    let entry = &result.entries[0];
    // Cycle guard: refuse to auto-nav into the path we're already on.
    // Prevents stack overflow if Core erroneously returns a folder
    // whose path equals the parent.
    if entry.is_folder() && !entry.path.is_empty() && entry.path != current_path {
        return InitialAction::AutoNavigate {
            path: entry.path.clone(),
        };
    }
    InitialAction::Apply
}

/// Pre-process a freshly-decoded `media.browse` page before storing it
/// in the model. Currently only used for the `MiSTer` leading-underscore
/// strip: `MiSTer` paths use `_PrefixedFolder` to control sort order in
/// the `MiSTer` menu, which is a layout artifact not user-facing intent.
/// Path stays untouched — launching/navigation still depend on the
/// original.
fn transform_entries(
    mut entries: Vec<BrowseEntry>,
    platform: Option<&Platform>,
) -> Vec<BrowseEntry> {
    if matches!(platform, Some(Platform::Mister)) {
        for entry in &mut entries {
            entry.name = display_name(&entry.name, platform).into_owned();
        }
    }
    entries
}

fn display_name<'a>(raw: &'a str, platform: Option<&Platform>) -> std::borrow::Cow<'a, str> {
    if matches!(platform, Some(Platform::Mister)) {
        if let Some(stripped) = raw.strip_prefix('_') {
            return std::borrow::Cow::Owned(stripped.to_string());
        }
    }
    std::borrow::Cow::Borrowed(raw)
}

fn leading_dir_count(entries: &[BrowseEntry]) -> i32 {
    let n = entries.iter().take_while(|e| e.is_folder()).count();
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// Drop any root-type entry whose path is a strict ancestor of another
/// root entry's path. Defends against Core occasionally surfacing the
/// shared parent dir (e.g. `/media/fat/games`) as a system root alongside
/// the actual per-system roots beneath it; the parent appears as a
/// phantom option in the frontend's roots screen, and selecting it
/// browses into a directory that contains every other system.
///
/// Only `root`-type entries participate. `directory` and `media` entries
/// pass through unchanged — a normal directory listing where a folder
/// and its subfolder appear as siblings is legitimate.
fn dedup_roots_drop_ancestors(entries: Vec<BrowseEntry>) -> Vec<BrowseEntry> {
    let root_paths: Vec<String> = entries
        .iter()
        .filter(|e| e.entry_type == "root" && !e.path.is_empty())
        .map(|e| e.path.trim_end_matches('/').to_string())
        .collect();
    entries
        .into_iter()
        .filter(|e| {
            if e.entry_type != "root" || e.path.is_empty() {
                return true;
            }
            let candidate = e.path.trim_end_matches('/');
            !root_paths
                .iter()
                .any(|other| is_strict_ancestor_path(candidate, other.as_str()))
        })
        .collect()
}

fn is_strict_ancestor_path(parent: &str, child: &str) -> bool {
    if parent.is_empty() || child.is_empty() || parent == child {
        return false;
    }
    child
        .strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn apply_status(mut model: Pin<&mut ffi::GamesModel>, status: ResourceStatus<MediaBrowseResult>) {
    match project_status(status) {
        Projection::Pending => {
            // A new browse round started (or a Ready→Pending refetch
            // is in flight). Abort any cover gate left from the
            // previous Ready so its safety-timer callback can't race
            // with the loading=true we're about to set and clear it
            // mid-load.
            reset_cover_gate(model.as_mut());
            if !model.loading {
                model.as_mut().set_loading(true);
            }
            if !model.error_message.is_empty() {
                model.as_mut().set_error_message(QString::default());
            }
            // Pagination state is only stale on a fresh browse target;
            // a transient Pending (connection blip, post-error retry)
            // on a seeded model must preserve `has_next_page` so the
            // user's paginated view stays usable once Ready arrives —
            // the early-return in `apply_initial_page` won't restore
            // it.
            if !model.is_seeded && model.has_next_page {
                model.as_mut().set_has_next_page(false);
            }
        }
        Projection::Ready(mut result) => {
            let eligible = model.rust().auto_nav_eligible;
            // Eligibility is consumed by the first Ready that follows
            // an eligible load. A subsequent refetch (mutation
            // invalidation) on the same `BrowseArgs` should NOT
            // re-auto-nav — the user is already inside the auto-nav
            // target's path stack and wouldn't expect to be teleported
            // again.
            model.as_mut().rust_mut().auto_nav_eligible = false;
            let platform = platform::current();
            let current_path = model.current_path.to_string();
            let current_system_id = model.current_system_id.to_string();
            info!(
                entries_len = result.entries.len(),
                total_files = result.total_files,
                eligible,
                ?current_path,
                "media.browse Ready",
            );
            if result.entries.is_empty() {
                warn!(
                    ?current_path,
                    ?current_system_id,
                    total_files = result.total_files,
                    "media.browse returned 0 entries",
                );
            } else {
                let mut type_counts: std::collections::BTreeMap<&str, usize> =
                    std::collections::BTreeMap::new();
                for e in &result.entries {
                    *type_counts.entry(e.entry_type.as_str()).or_insert(0) += 1;
                }
                let sample: Vec<_> = result
                    .entries
                    .iter()
                    .take(3)
                    .map(|e| {
                        format!(
                            "{}|sid={}|sids={:?}|type={}|path={}",
                            e.name, e.system_id, e.system_ids, e.entry_type, e.path
                        )
                    })
                    .collect();
                debug!(?type_counts, ?sample, "media.browse Ready entry shape",);
            }
            let pre_dedup = result.entries.len();
            result.entries = dedup_roots_drop_ancestors(result.entries);
            let removed = pre_dedup.saturating_sub(result.entries.len());
            if removed > 0 {
                warn!(
                    removed,
                    ?current_path,
                    ?current_system_id,
                    "media.browse: dropped ancestor-of-sibling root entries",
                );
            }
            match decide_initial(&result, eligible, platform.as_ref(), &current_path) {
                InitialAction::AutoNavigate { path } => {
                    info!(?path, "media.browse: auto-navigating into single folder");
                    let sid = model.current_system_id.to_string();
                    let systems = if sid.is_empty() {
                        Vec::new()
                    } else {
                        vec![sid]
                    };
                    // Recurse *without* eligibility — the auto-nav
                    // target's contents are the user's view, even if
                    // by happenstance they themselves contain a
                    // single folder.
                    model.as_mut().start_initial_browse(path, systems, false);
                }
                InitialAction::Apply => {
                    apply_initial_page(model, result);
                }
            }
        }
        Projection::Errored { message } => {
            warn!("media.browse errored: {message}");
            // Errored takes us out of Ready without going through
            // `arm_cover_gate`, so any timer left armed by the prior
            // Ready needs to be torn down explicitly.
            reset_cover_gate(model.as_mut());
            let qstr = QString::from(message.as_str());
            if model.error_message != qstr {
                model.as_mut().set_error_message(qstr);
            }
            if model.loading {
                model.as_mut().set_loading(false);
            }
            if model.has_next_page {
                model.as_mut().set_has_next_page(false);
            }
        }
    }
}

fn apply_initial_page(mut model: Pin<&mut ffi::GamesModel>, result: MediaBrowseResult) {
    // Already seeded: this Ready is a cache invalidation refetch on
    // the same browse target (e.g. `MediaTagsUpdateMutation`'s
    // `MediaBrowseEndpoint` invalidation after a favorite toggle, or
    // a Loading→Ready cycle from a connection blip / post-error
    // retry). The full reset below would clobber any pages the user
    // paginated to and reset the grid to row 0; the local mutation
    // paths (`apply_favorite_tags`) keep the visible model in sync.
    // Loading was set true by the preceding Pending — clear it so a
    // transient blip doesn't leave the spinner stuck on after the
    // refetch lands.
    if model.is_seeded {
        if model.loading {
            model.as_mut().set_loading(false);
        }
        return;
    }
    let has_next_page = result.has_next_page();
    let next_cursor = result.next_cursor();
    let total = i32::try_from(result.total_files).unwrap_or(i32::MAX);
    let platform = platform::current();
    let entries = transform_entries(result.entries, platform.as_ref());
    let dir_count = leading_dir_count(&entries);
    let count = i32::try_from(entries.len()).unwrap_or(i32::MAX);
    info!(
        count,
        dir_count, total, has_next_page, "games: apply_initial_page"
    );
    model.as_mut().begin_reset_model();
    model.as_mut().rust_mut().entries = entries;
    model.as_mut().rust_mut().count = count;
    model.as_mut().rust_mut().next_cursor = next_cursor;
    // Property setters run BEFORE `end_reset_model` so Main.qml's
    // `onModelReset` handler observes the post-load state. In
    // particular, the deep-page restore branch reads `has_next_page`
    // to decide whether to chase the saved entry across pages; if
    // we set it after `end_reset_model`, that handler sees the
    // stale `false` left by `start_initial_browse` and abandons the
    // restore (currentIndex snaps to 0). The order in
    // `apply_append_page` is the opposite (set has_next_page AFTER
    // the last insert) for a different reason: that path needs
    // PagedGrid's pending-target watchdog to read a fresh itemCount
    // when the flag flips false on the terminal chunk. A fresh
    // model reset has no pending-target watchdog to mislead, so
    // setting the flag early here is safe.
    model.as_mut().set_dir_count(dir_count);
    model.as_mut().set_total_files(total);
    model.as_mut().set_has_next_page(has_next_page);
    model.as_mut().end_reset_model();
    model.as_mut().count_changed();
    // Seed the cover queue from the visible row outwards instead of
    // bulk-enqueuing every entry. The grid resets to row 0 on a fresh
    // browse, so anchor the first prefetch there. Any later page turn
    // re-issues this through `onCurrentPageChanged` in QML.
    model.as_mut().rust_mut().visible_first_row = 0;
    model.as_mut().prefetch_around(0);
    // Metadata look-ahead runs in the background. Track it only so
    // stale follow-up completions can clear their own bookkeeping; do
    // not let it hold the first visible page behind the full-screen
    // loading overlay.
    let will_lookahead = has_next_page && !model.loading_more;
    if will_lookahead {
        model.as_mut().rust_mut().pending_initial_lookahead = true;
    }
    // Decide whether to release `loading` immediately or hold it until
    // visible-page covers are cached. Background metadata look-ahead
    // does not participate in this gate.
    arm_cover_gate(model.as_mut());
    if !model.error_message.is_empty() {
        model.as_mut().set_error_message(QString::default());
    }
    // Metadata look-ahead: keep rows one chunk ahead of the highlight
    // so a page advance does not pause on "Loading more…". Cover
    // prefetch is separate: `prefetch_around` rebuilds the queue in
    // current → next → previous order whenever rows land or the page
    // changes.
    if will_lookahead {
        model.as_mut().fetch_more();
    }
    // Mark seeded last so any early-return from this function leaves
    // the flag in its previous state. Subsequent Ready transitions
    // on the same browse target now skip the reset above.
    model.as_mut().rust_mut().is_seeded = true;
}

/// Split a freshly-fetched chunk of `BrowseEntry` rows into sub-batches
/// of at most `size` rows each, preserving order. The first batch is
/// applied synchronously by `apply_append_page`; the rest are posted
/// one frame apart so the Repeater's per-delegate materialisation
/// cost is spread across frames instead of one big main-thread stall.
///
/// Pure helper so the partitioning is unit-testable without a Qt
/// event loop. Returns an empty Vec for an empty input or `size == 0`.
fn chunk_for_subbatching(entries: Vec<BrowseEntry>, size: usize) -> Vec<Vec<BrowseEntry>> {
    if entries.is_empty() || size == 0 {
        return Vec::new();
    }
    let mut out: Vec<Vec<BrowseEntry>> = Vec::with_capacity(entries.len().div_ceil(size));
    let mut current: Vec<BrowseEntry> = Vec::with_capacity(size);
    for entry in entries {
        if current.len() == size {
            out.push(std::mem::replace(&mut current, Vec::with_capacity(size)));
        }
        current.push(entry);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Single `begin_insert_rows`/`end_insert_rows` pair for one sub-batch
/// of an append. Shared between the synchronous-first-batch path and
/// the deferred-tail path so the row-count math + signal ordering live
/// in exactly one place. No-ops on an empty batch.
fn insert_sub_batch(mut model: Pin<&mut ffi::GamesModel>, batch: Vec<BrowseEntry>) {
    let new_count = i32::try_from(batch.len()).unwrap_or(i32::MAX - model.count);
    if new_count <= 0 {
        return;
    }
    let first = model.count;
    let last = first.saturating_add(new_count).saturating_sub(1);
    info!(
        "insert_sub_batch first={} last={} new_count={} count_after={}",
        first,
        last,
        new_count,
        first.saturating_add(new_count),
    );
    let parent = QModelIndex::default();
    model.as_mut().begin_insert_rows(&parent, first, last);
    model.as_mut().rust_mut().entries.extend(batch);
    model.as_mut().rust_mut().count = first.saturating_add(new_count);
    model.as_mut().end_insert_rows();
    model.as_mut().count_changed();
}

fn apply_append_page(
    mut model: Pin<&mut ffi::GamesModel>,
    result: Result<MediaBrowseResult, ClientError>,
    expected_prev_cursor: Option<&str>,
) {
    // Cursor-chain check: if the model's `next_cursor` no longer
    // matches the cursor this append was advancing from, an
    // intervening `apply_initial_page` reset the chain. Appending
    // here would splice rows from the old chain onto the new one.
    if model.next_cursor.as_deref() != expected_prev_cursor {
        // Clear the in-flight cue so the UI doesn't get stuck.
        if model.loading_more {
            model.as_mut().set_loading_more(false);
        }
        return;
    }
    match result {
        Ok(result) => {
            let has_next_page = result.has_next_page();
            let next_cursor = result.next_cursor();
            let platform = platform::current();
            let entries = transform_entries(result.entries, platform.as_ref());
            let total = i32::try_from(result.total_files).unwrap_or(i32::MAX);
            // Order matters for the pending-target chain in PagedGrid:
            //
            // 1. `next_cursor` and `loading_more=false` MUST happen
            //    before the FIRST sub-batch's `end_insert_rows`. The
            //    Repeater reacts synchronously to `rowsInserted`;
            //    PagedGrid's `onItemCountChanged` runs
            //    `_commitPendingTarget`, which can re-fire
            //    `loadMoreRequested` -> `fetch_more`. If `loading_more`
            //    is still true at that moment the `fetch_more` guard
            //    early-returns and the chain stalls after one append.
            //
            // 2. `has_next_page` MUST happen after the LAST sub-batch's
            //    `end_insert_rows`. On the final chunk the value flips
            //    true -> false; if we set it first, PagedGrid's
            //    `onHasMorePagesChanged` fires `_commitPendingTarget`
            //    while `itemCount` is still stale (pre-insert), the
            //    watchdog sees a too-low itemCount, and settles the
            //    pending target on whatever was loaded BEFORE this
            //    chunk landed -- the wrap appears to land ~1 chunk
            //    short of the real last page. Setting it after the
            //    insert lets the watchdog read the fresh itemCount and
            //    clamp to the actual last item.
            //
            // Sub-batching note: the rest of the chunk is posted from
            // `global_handle()` one frame apart so the Repeater's
            // per-delegate `createObject` cost (the dominant Qt-thread
            // stall on MiSTer) is spread across frames. Stale batches
            // self-disarm via the `append_seq` ticket if a new
            // `start_initial_browse` lands during the trickle window.
            model.as_mut().rust_mut().next_cursor = next_cursor;
            model.as_mut().set_loading_more(false);
            let mut batches = chunk_for_subbatching(entries, APPEND_SUB_BATCH_SIZE);
            if batches.is_empty() {
                // No new rows landed (empty page). Finalise immediately
                // — there's nothing to defer.
                model.as_mut().set_has_next_page(has_next_page);
                if model.total_files != total {
                    model.as_mut().set_total_files(total);
                }
                release_initial_lookahead_gate(model.as_mut());
                return;
            }
            // First batch runs synchronously inside the existing
            // Qt-thread call so PagedGrid's `_commitPendingTarget`
            // resolves within ~200 ms of the user's press.
            let first_batch = batches.remove(0);
            insert_sub_batch(model.as_mut(), first_batch);
            // Re-arm prefetch around the user's current visible row.
            // The freshly-appended rows may now occupy the
            // "current" or "next" page window; this is the only
            // hook that gets covers warmed for them without a bulk
            // enqueue.
            let visible = model.visible_first_row;
            model.as_mut().prefetch_around(visible);
            if batches.is_empty() {
                // Single-batch chunk (<= APPEND_SUB_BATCH_SIZE rows).
                // Finalise now without scheduling deferred work.
                model.as_mut().set_has_next_page(has_next_page);
                if model.total_files != total {
                    model.as_mut().set_total_files(total);
                }
                release_initial_lookahead_gate(model.as_mut());
                return;
            }
            // Remaining batches: post one frame apart on the Qt
            // thread, with the LAST one carrying the finaliser
            // (has_next_page, total_files, look-ahead gate release).
            // dir_count intentionally not touched: Core only returns
            // directory entries on page 1 (cursor == nil).
            //
            // No auto-prefetch here. apply_initial_page pre-warms
            // chunk 2 once; subsequent chunks are driven by the
            // grid's onLoadMoreRequested as the user scrolls.
            // Chaining a fetch_more here turned the look-ahead into a
            // self-driving cascade that downloaded every page
            // back-to-back, tripping Core's WebSocket rate limit on
            // huge folders (Arcade).
            let seq = model.rust().append_seq.clone();
            let ticket = seq.load(Ordering::SeqCst);
            let qt_thread = model.qt_thread();
            global_handle().spawn(async move {
                let last_idx = batches.len().saturating_sub(1);
                for (i, batch) in batches.into_iter().enumerate() {
                    tokio::time::sleep(Duration::from_millis(SUB_BATCH_FRAME_GAP_MS)).await;
                    let is_last = i == last_idx;
                    let seq = seq.clone();
                    let _ = qt_thread.queue(move |mut model: Pin<&mut ffi::GamesModel>| {
                        if seq.load(Ordering::SeqCst) != ticket {
                            return;
                        }
                        insert_sub_batch(model.as_mut(), batch);
                        let visible = model.visible_first_row;
                        model.as_mut().prefetch_around(visible);
                        if is_last {
                            model.as_mut().set_has_next_page(has_next_page);
                            if model.total_files != total {
                                model.as_mut().set_total_files(total);
                            }
                            // Clear the look-ahead gate now that the
                            // first follow-up chunk has fully landed.
                            // If the cover gate already drained
                            // (covers all cached or decode-settle
                            // fired while we held it open) we're the
                            // one releasing loading.
                            release_initial_lookahead_gate(model.as_mut());
                        }
                    });
                }
            });
        }
        Err(e) => {
            warn!("media.browse follow-up page failed: {}", e.message);
            // Surface the error so the user sees the cause; clearing
            // `loading_more` means the cue disappears even though the
            // fetch failed.
            model
                .as_mut()
                .set_error_message(QString::from(e.message.as_str()));
            model.as_mut().set_loading_more(false);
            // Even on a failed first prefetch, we have to release the
            // cover gate's hold or the user is stuck on Loading…
            // forever. The visible page is already in place from
            // `apply_initial_page`; the missing tail just won't be
            // there until the user retries.
            release_initial_lookahead_gate(model.as_mut());
        }
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

    use super::{
        chunk_for_subbatching, compute_unresolved_keys, cover_key_for_with, cover_placeholder_for,
        decide_initial, dedup_roots_drop_ancestors, detail_tags_from_tags, display_name,
        entry_system_id, is_media_capable_entry, is_strict_ancestor_path, leading_dir_count,
        media_key_for, meta_params_for_entry, position_of_game_path, prefetch_around_plan,
        project_status, run_text_for_entry, singleton_directory_needs_launch_resolution,
        transform_entries, InitialAction, Projection,
    };
    use crate::media_image_cache::{MediaImageCache, MediaKey};
    use std::collections::HashSet;
    use zaparoo_core::media_types::{BrowseEntry, MediaBrowseResult, Pagination, TagInfo};
    use zaparoo_core::platform::Platform;
    use zaparoo_core::remote_resource::ResourceStatus;

    fn folder(name: &str, path: &str) -> BrowseEntry {
        BrowseEntry {
            name: name.into(),
            path: path.into(),
            entry_type: "directory".into(),
            ..BrowseEntry::default()
        }
    }

    fn root(name: &str, path: &str, system_id: &str) -> BrowseEntry {
        BrowseEntry {
            name: name.into(),
            path: path.into(),
            entry_type: "root".into(),
            system_id: system_id.into(),
            system_ids: vec![system_id.into()],
            ..BrowseEntry::default()
        }
    }

    fn media(name: &str, path: &str, system_id: &str) -> BrowseEntry {
        BrowseEntry {
            name: name.into(),
            path: path.into(),
            entry_type: "media".into(),
            system_id: system_id.into(),
            zap_script: format!("@{system_id}/{name}"),
            ..BrowseEntry::default()
        }
    }

    #[test]
    fn media_entry_is_media_capable() {
        assert!(is_media_capable_entry(&media(
            "smb",
            "/games/NES/smb.nes",
            "NES"
        )));
    }

    #[test]
    fn directory_with_only_system_id_is_not_media_capable() {
        let mut entry = folder("Archive", "/games/GB/archive.zip");
        entry.system_id = "Gameboy".into();
        assert!(!is_media_capable_entry(&entry));
    }

    #[test]
    fn directory_with_media_id_is_media_capable() {
        let mut entry = folder("Single", "/games/GB/single.zip");
        entry.system_id = "Gameboy".into();
        entry.media_id = Some(42);
        assert!(is_media_capable_entry(&entry));
    }

    #[test]
    fn directory_with_zap_script_is_media_capable() {
        let mut entry = folder("Single", "/games/GB/single.zip");
        entry.system_id = "Gameboy".into();
        entry.zap_script = "@Gameboy/Single".into();
        assert!(is_media_capable_entry(&entry));
    }

    #[test]
    fn root_with_system_id_is_not_media_capable() {
        assert!(!is_media_capable_entry(&root("GB", "/games/GB", "Gameboy")));
    }

    #[test]
    fn idle_projects_to_pending() {
        match project_status(ResourceStatus::Idle) {
            Projection::Pending => {}
            other => panic!("expected Pending, got {other:?}"),
        }
    }

    #[test]
    fn loading_projects_to_pending() {
        match project_status(ResourceStatus::Loading) {
            Projection::Pending => {}
            other => panic!("expected Pending, got {other:?}"),
        }
    }

    #[test]
    fn ready_carries_full_result() {
        let result = MediaBrowseResult {
            path: "/games/NES".into(),
            entries: vec![media("smb", "/games/NES/smb.nes", "NES")],
            total_files: 1,
            pagination: None,
        };
        match project_status(ResourceStatus::Ready(result)) {
            Projection::Ready(r) => {
                assert_eq!(r.entries.len(), 1);
                assert_eq!(r.total_files, 1);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn errored_propagates_message() {
        match project_status(ResourceStatus::Errored {
            message: "rpc kaboom".into(),
            retrying: true,
        }) {
            Projection::Errored { message } => assert_eq!(message, "rpc kaboom"),
            other => panic!("expected Errored, got {other:?}"),
        }
    }

    #[test]
    fn decide_initial_with_single_root_eligible_returns_auto_nav() {
        let result = MediaBrowseResult {
            entries: vec![root("NES", "/roms/NES", "NES")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::AutoNavigate {
                path: "/roms/NES".into()
            }
        );
    }

    #[test]
    fn decide_initial_with_single_directory_eligible_returns_auto_nav() {
        let result = MediaBrowseResult {
            entries: vec![folder("Games", "/roms/Shared/Games")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::AutoNavigate {
                path: "/roms/Shared/Games".into()
            }
        );
    }

    #[test]
    fn decide_initial_with_single_folder_not_eligible_renders_as_apply_off_mister() {
        // `set_path`-driven loads must not auto-nav off MiSTer — the
        // user is explicitly navigating through folders and would lose
        // orientation otherwise.
        let result = MediaBrowseResult {
            entries: vec![folder("Sub", "/x/Sub")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, false, Some(&Platform::Linux), ""),
            InitialAction::Apply,
        );
        assert_eq!(
            decide_initial(&result, false, None, ""),
            InitialAction::Apply,
        );
    }

    #[test]
    fn decide_initial_with_single_folder_not_eligible_flattens_on_mister() {
        // On MiSTer, a `set_path` load that returns exactly one folder
        // also flattens — collections often unzip into nested
        // single-child folders.
        let result = MediaBrowseResult {
            entries: vec![folder("Sub", "/x/Sub")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, false, Some(&Platform::Mister), ""),
            InitialAction::AutoNavigate {
                path: "/x/Sub".into()
            },
        );
    }

    #[test]
    fn decide_initial_refuses_to_auto_nav_into_same_path() {
        // Cycle guard: if Core returns a folder whose path equals the
        // current browse path, recursing would loop forever (the cache
        // returns the same Ready). Render as-is instead.
        let result = MediaBrowseResult {
            entries: vec![folder("Self", "/x/Sub")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, false, Some(&Platform::Mister), "/x/Sub"),
            InitialAction::Apply,
        );
        assert_eq!(
            decide_initial(&result, true, None, "/x/Sub"),
            InitialAction::Apply,
        );
    }

    #[test]
    fn decide_initial_with_multi_root_returns_apply() {
        let result = MediaBrowseResult {
            entries: vec![
                root("primary", "/roms/A", "NES"),
                root("backup", "/roms/B", "NES"),
            ],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::Apply
        );
    }

    #[test]
    fn decide_initial_with_empty_returns_apply() {
        let result = MediaBrowseResult::default();
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::Apply
        );
    }

    #[test]
    fn decide_initial_with_single_media_returns_apply() {
        // A system with exactly one ROM at the root shouldn't be
        // treated as a folder; auto-nav into a media entry would
        // make no sense.
        let result = MediaBrowseResult {
            entries: vec![media("only", "/p/only", "NES")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::Apply
        );
    }

    #[test]
    fn decide_initial_with_single_root_blank_path_renders_as_apply() {
        // A root with no `path` cannot be navigated into. Rather than
        // recurse infinitely on a malformed payload, render the entry
        // as-is and let the user see something usable.
        let result = MediaBrowseResult {
            entries: vec![root("ghost", "", "NES")],
            ..MediaBrowseResult::default()
        };
        assert_eq!(
            decide_initial(&result, true, None, ""),
            InitialAction::Apply
        );
    }

    #[test]
    fn display_name_strips_single_leading_underscore_on_mister() {
        assert_eq!(
            display_name("_Arcade", Some(&Platform::Mister)).as_ref(),
            "Arcade",
        );
    }

    #[test]
    fn display_name_only_strips_one_underscore_on_mister() {
        assert_eq!(
            display_name("__deep", Some(&Platform::Mister)).as_ref(),
            "_deep",
        );
    }

    #[test]
    fn display_name_preserves_off_mister() {
        assert_eq!(
            display_name("_Arcade", Some(&Platform::Linux)).as_ref(),
            "_Arcade",
        );
        assert_eq!(display_name("_Arcade", None).as_ref(), "_Arcade");
    }

    #[test]
    fn transform_entries_strips_underscore_only_on_mister() {
        let entries = vec![folder("_Arcade", "/A"), media("_smb", "/A/_smb", "NES")];
        let on_mister = transform_entries(entries.clone(), Some(&Platform::Mister));
        assert_eq!(on_mister[0].name, "Arcade");
        assert_eq!(on_mister[1].name, "smb");
        // Path is left alone.
        assert_eq!(on_mister[0].path, "/A");
        assert_eq!(on_mister[1].path, "/A/_smb");

        let off_mister = transform_entries(entries, Some(&Platform::Linux));
        assert_eq!(off_mister[0].name, "_Arcade");
        assert_eq!(off_mister[1].name, "_smb");
    }

    #[test]
    fn leading_dir_count_counts_only_leading_folders() {
        let entries = vec![
            folder("a", "/a"),
            folder("b", "/b"),
            media("smb", "/smb", "NES"),
            // A folder appearing after files would be a Core bug —
            // the API contract is dirs-then-files. We don't count it.
            folder("c", "/c"),
        ];
        assert_eq!(leading_dir_count(&entries), 2);
    }

    #[test]
    fn leading_dir_count_zero_when_first_is_media() {
        let entries = vec![media("smb", "/smb", "NES"), media("zelda", "/z", "NES")];
        assert_eq!(leading_dir_count(&entries), 0);
    }

    #[test]
    fn leading_dir_count_zero_on_empty() {
        assert_eq!(leading_dir_count(&[]), 0);
    }

    #[test]
    fn is_strict_ancestor_path_matches_parent_of_child() {
        assert!(is_strict_ancestor_path(
            "/media/fat/games",
            "/media/fat/games/Genesis",
        ));
        assert!(is_strict_ancestor_path(
            "/media/fat/games",
            "/media/fat/games/MegaDrive",
        ));
    }

    #[test]
    fn is_strict_ancestor_path_rejects_equal_paths() {
        assert!(!is_strict_ancestor_path("/a/b", "/a/b"));
    }

    #[test]
    fn is_strict_ancestor_path_rejects_unrelated_paths() {
        assert!(!is_strict_ancestor_path("/a/b", "/a/bc"));
        assert!(!is_strict_ancestor_path("/a/b", "/x/b/c"));
    }

    #[test]
    fn is_strict_ancestor_path_rejects_empty_inputs() {
        assert!(!is_strict_ancestor_path("", "/a"));
        assert!(!is_strict_ancestor_path("/a", ""));
    }

    #[test]
    fn dedup_roots_drops_ancestor_root_when_sibling_is_descendant() {
        // The Genesis 3-root payload Core surfaced on a real MiSTer:
        // /media/fat/games appears alongside the per-system roots beneath
        // it. The shared parent dir must not reach the frontend's roots
        // screen as a third option.
        let entries = vec![
            root("MegaDrive", "/media/fat/games/MegaDrive", "Genesis"),
            root("Genesis", "/media/fat/games/Genesis", "Genesis"),
            root("games", "/media/fat/games", "Genesis"),
        ];
        let kept = dedup_roots_drop_ancestors(entries);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|e| e.path == "/media/fat/games/MegaDrive"));
        assert!(kept.iter().any(|e| e.path == "/media/fat/games/Genesis"));
        assert!(!kept.iter().any(|e| e.path == "/media/fat/games"));
    }

    #[test]
    fn dedup_roots_passes_through_unrelated_roots() {
        let entries = vec![
            root("primary", "/roms/A", "NES"),
            root("backup", "/roms/B", "NES"),
        ];
        let kept = dedup_roots_drop_ancestors(entries);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn dedup_roots_ignores_non_root_entries() {
        // A directory listing that contains a folder and its subfolder is
        // legitimate (e.g. inside a system browse). The filter only
        // applies to root-typed entries.
        let entries = vec![
            folder("parent", "/x"),
            folder("child", "/x/sub"),
            media("smb", "/x/sub/smb.nes", "NES"),
        ];
        let kept = dedup_roots_drop_ancestors(entries);
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn dedup_roots_keeps_root_with_blank_path() {
        // Blank-path roots cannot be navigated into (handled separately
        // by decide_initial). They never participate as ancestors and
        // never get dropped as descendants.
        let entries = vec![root("ghost", "", "NES"), root("real", "/roms/NES", "NES")];
        let kept = dedup_roots_drop_ancestors(entries);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn dedup_roots_handles_trailing_slash_on_either_side() {
        let entries = vec![
            root("parent", "/media/fat/games/", "Genesis"),
            root("child", "/media/fat/games/Genesis", "Genesis"),
        ];
        let kept = dedup_roots_drop_ancestors(entries);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, "/media/fat/games/Genesis");
    }

    #[test]
    fn entry_system_id_prefers_singular_field() {
        let entry = BrowseEntry {
            system_id: "SNES".into(),
            system_ids: vec!["SNES".into(), "Stub".into()],
            ..BrowseEntry::default()
        };
        assert_eq!(entry_system_id(&entry), "SNES");
    }

    #[test]
    fn entry_system_id_falls_back_to_first_of_system_ids_for_multi_system_root() {
        let entry = BrowseEntry {
            system_id: String::new(),
            system_ids: vec!["NES".into(), "FDS".into()],
            ..BrowseEntry::default()
        };
        assert_eq!(entry_system_id(&entry), "NES");
    }

    #[test]
    fn cover_key_for_folder_returns_folder_icon() {
        let entry = folder("Games", "/x");
        assert_eq!(
            cover_key_for_with(&entry, None, false, false),
            "icons/Folder"
        );
        // Folders never carry a cached cover, but explicit assertion
        // documents that even if a cache hit somehow surfaced for a
        // folder key we keep the folder glyph.
        let stale_key = MediaKey::new("NES", "/x");
        assert_eq!(
            cover_key_for_with(&entry, Some(&stale_key), true, false),
            "icons/Folder"
        );
    }

    #[test]
    fn cover_key_for_media_in_flight_returns_loading_icon() {
        let entry = media("smb", "/p/smb", "NES");
        let key = media_key_for(&entry).expect("media has key");
        // Not cached, not negative → fetch is in flight, show hourglass.
        assert_eq!(
            cover_key_for_with(&entry, Some(&key), false, false),
            "icons/Loading"
        );
    }

    #[test]
    fn cover_placeholder_never_returns_loading_icon() {
        assert_eq!(
            cover_placeholder_for(&media("smb", "/p/smb", "NES")),
            "icons/File"
        );
        assert_eq!(
            cover_placeholder_for(&folder("Games", "/x")),
            "icons/Folder"
        );
    }

    #[test]
    fn cover_key_for_media_negatively_memoed_returns_file_icon() {
        let entry = media("smb", "/p/smb", "NES");
        let key = media_key_for(&entry).expect("media has key");
        // Not cached, but negatively memoed → there is no image to
        // wait for, fall back to the plain file icon (no stuck
        // hourglass).
        assert_eq!(
            cover_key_for_with(&entry, Some(&key), false, true),
            "icons/File"
        );
    }

    #[test]
    fn cover_key_for_media_soft_missed_returns_file_icon() {
        let entry = media("smb", "/p/smb", "NES");
        let key = media_key_for(&entry).expect("media has key");
        assert_eq!(
            cover_key_for_with(&entry, Some(&key), false, true),
            "icons/File"
        );
    }

    #[test]
    fn cover_key_for_media_with_cache_returns_media_image_key() {
        let entry = media("smb", "/p/smb", "NES");
        let key = media_key_for(&entry).expect("media has key");
        let expected = MediaImageCache::image_key_for(&key);
        assert_eq!(
            cover_key_for_with(&entry, Some(&key), true, false),
            expected
        );
        assert!(expected.starts_with("media-image/"));
    }

    #[test]
    fn cover_key_for_unattributed_media_returns_file_icon() {
        let entry = BrowseEntry {
            entry_type: "media".into(),
            ..BrowseEntry::default()
        };
        // No system id and no path → no MediaKey.
        assert!(media_key_for(&entry).is_none());
        assert_eq!(cover_key_for_with(&entry, None, false, false), "icons/File");
    }

    #[test]
    fn media_key_for_skips_folders_and_unattributed_entries() {
        assert!(media_key_for(&folder("Games", "/x")).is_none());
        assert!(media_key_for(&root("NES", "/roms/NES", "NES")).is_none());
        let unattributed = BrowseEntry {
            entry_type: "media".into(),
            path: "/p".into(),
            ..BrowseEntry::default()
        };
        assert!(media_key_for(&unattributed).is_none());
        let pathless = BrowseEntry {
            entry_type: "media".into(),
            system_id: "NES".into(),
            ..BrowseEntry::default()
        };
        assert!(media_key_for(&pathless).is_none());
        let key = media_key_for(&media("smb", "/p/smb", "NES")).expect("ok");
        assert_eq!(key.system_id.as_ref(), "NES");
        assert_eq!(key.path.as_ref(), "/p/smb");
    }

    #[test]
    fn singleton_directory_uses_media_id_for_meta_but_container_as_fallback_run_text() {
        let entry = BrowseEntry {
            media_id: Some(42),
            name: "Archive".into(),
            path: "/roms/NES/archive.zip".into(),
            entry_type: "directory".into(),
            system_id: "NES".into(),
            zap_script: "@NES/Super Mario Bros.".into(),
            ..BrowseEntry::default()
        };
        assert!(singleton_directory_needs_launch_resolution(&entry));
        let params = meta_params_for_entry(&entry).expect("meta params");
        assert_eq!(params.media_id, Some(42));
        assert!(params.system.is_empty());
        assert!(params.path.is_empty());
        assert_eq!(run_text_for_entry(&entry), "/roms/NES/archive.zip");
    }

    #[test]
    fn normal_media_does_not_need_launch_resolution() {
        let entry = BrowseEntry {
            media_id: Some(7),
            ..media("smb", "/roms/NES/smb.nes", "NES")
        };
        assert!(!singleton_directory_needs_launch_resolution(&entry));
        assert_eq!(run_text_for_entry(&entry), "/roms/NES/smb.nes");
    }

    #[test]
    fn position_of_game_path_returns_index_on_case_exact_match() {
        let entries = vec![
            media("smb", "/p/smb", "NES"),
            media("zelda", "/p/zelda", "NES"),
        ];
        assert_eq!(position_of_game_path(&entries, "/p/zelda"), 1);
    }

    #[test]
    fn position_of_game_path_is_case_sensitive() {
        let entries = vec![media("smb", "/p/smb", "NES")];
        assert_eq!(position_of_game_path(&entries, "/P/SMB"), -1);
        assert_eq!(position_of_game_path(&entries, "/p/SMB"), -1);
    }

    #[test]
    fn position_of_game_path_empty_needle_returns_minus_one() {
        let entries = vec![media("smb", "/p/smb", "NES")];
        assert_eq!(position_of_game_path(&entries, ""), -1);
    }

    #[test]
    fn position_of_game_path_missing_returns_minus_one() {
        let entries = vec![media("smb", "/p/smb", "NES")];
        assert_eq!(position_of_game_path(&entries, "/p/missing"), -1);
    }

    #[test]
    fn position_of_directory_path_returns_index() {
        // The path stack restore pulls indices for directory entries
        // too, not just media; guard against future regressions
        // accidentally narrowing this to media-only.
        let entries = vec![folder("A", "/x/A"), folder("B", "/x/B")];
        assert_eq!(position_of_game_path(&entries, "/x/B"), 1);
    }

    #[test]
    fn pagination_helpers_carry_cursor_through() {
        let result = MediaBrowseResult {
            pagination: Some(Pagination {
                has_next_page: true,
                page_size: 100,
                next_cursor: Some("c".into()),
            }),
            ..MediaBrowseResult::default()
        };
        assert!(result.has_next_page());
        assert_eq!(result.next_cursor(), Some("c".into()));
    }

    #[test]
    fn pagination_helpers_default_to_no_more_pages() {
        let result = MediaBrowseResult::default();
        assert!(!result.has_next_page());
        assert_eq!(result.next_cursor(), None);
    }

    #[test]
    fn compute_unresolved_keys_folder_only_page_returns_empty() {
        let entries = vec![folder("Games", "/x/Games"), folder("Saves", "/x/Saves")];
        let unresolved = compute_unresolved_keys(&entries, |_| false, |_| false);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn compute_unresolved_keys_all_cached_returns_empty() {
        let entries = vec![
            media("smb", "/p/smb", "NES"),
            media("zelda", "/p/zelda", "NES"),
        ];
        let unresolved = compute_unresolved_keys(&entries, |_| true, |_| false);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn compute_unresolved_keys_mixed_returns_only_uncached() {
        let cached_path = "/p/smb";
        let entries = vec![
            media("smb", cached_path, "NES"),
            media("zelda", "/p/zelda", "NES"),
            folder("Saves", "/p/Saves"),
        ];
        let unresolved =
            compute_unresolved_keys(&entries, |k| k.path.as_ref() == cached_path, |_| false);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/zelda")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }

    #[test]
    fn compute_unresolved_keys_excludes_negative_memo() {
        // Negative-memoised keys are "Core said no image" — gate must
        // not wait on them, otherwise the gate stays armed until the
        // safety timer fires every time.
        let negative_path = "/p/no-image";
        let entries = vec![
            media("smb", "/p/smb", "NES"),
            media("orphan", negative_path, "NES"),
        ];
        let unresolved =
            compute_unresolved_keys(&entries, |_| false, |k| k.path.as_ref() == negative_path);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/smb")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }

    #[test]
    fn compute_unresolved_keys_skips_unattributed_entries() {
        // Browse entries without enough info to key on — folder, root,
        // unattributed media — never produce a MediaKey, so the gate
        // doesn't wait on them.
        let entries = vec![
            folder("Games", "/x/Games"),
            root("NES", "/roms/NES", "NES"),
            BrowseEntry {
                entry_type: "media".into(),
                ..BrowseEntry::default()
            },
        ];
        let unresolved = compute_unresolved_keys(&entries, |_| false, |_| false);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn chunk_for_subbatching_empty_returns_empty() {
        let out = chunk_for_subbatching(Vec::new(), 25);
        assert!(out.is_empty());
    }

    #[test]
    fn chunk_for_subbatching_zero_size_returns_empty() {
        let entries = vec![media("a", "/a", "NES")];
        let out = chunk_for_subbatching(entries, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn chunk_for_subbatching_under_size_returns_single_batch() {
        let entries: Vec<BrowseEntry> = (0..10)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let out = chunk_for_subbatching(entries, 25);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 10);
    }

    #[test]
    fn chunk_for_subbatching_exact_multiple_splits_evenly() {
        let entries: Vec<BrowseEntry> = (0..50)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let out = chunk_for_subbatching(entries, 25);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 25);
        assert_eq!(out[1].len(), 25);
    }

    #[test]
    fn chunk_for_subbatching_remainder_lands_in_final_batch() {
        let entries: Vec<BrowseEntry> = (0..60)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let out = chunk_for_subbatching(entries, 25);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].len(), 25);
        assert_eq!(out[1].len(), 25);
        assert_eq!(out[2].len(), 10);
    }

    #[test]
    fn chunk_for_subbatching_preserves_order() {
        let entries: Vec<BrowseEntry> = (0..30)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let out = chunk_for_subbatching(entries, 25);
        assert_eq!(out[0][0].path, "/g/0");
        assert_eq!(out[0][24].path, "/g/24");
        assert_eq!(out[1][0].path, "/g/25");
        assert_eq!(out[1][4].path, "/g/29");
    }

    #[test]
    fn prefetch_around_plan_orders_current_next_previous() {
        let entries: Vec<BrowseEntry> = (0..45)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let plan = prefetch_around_plan(&entries, 45, 15, 15);
        let paths: Vec<String> = plan.iter().map(|(k, _)| k.path.to_string()).collect();
        let mut expected: Vec<String> = (15..30).map(|i| format!("/g/{i}")).collect();
        expected.extend((30..45).map(|i| format!("/g/{i}")));
        expected.extend((0..15).map(|i| format!("/g/{i}")));
        assert_eq!(paths, expected);
    }

    #[test]
    fn prefetch_around_plan_skips_folders_and_unattributed() {
        // Folder in-page slot — should be silently skipped, not break
        // the order of the surrounding media entries.
        let entries: Vec<BrowseEntry> = vec![
            folder("dir", "/x/dir"),
            media("g0", "/g/0", "NES"),
            folder("dir2", "/x/dir2"),
            media("g1", "/g/1", "NES"),
        ];
        let plan = prefetch_around_plan(&entries, 4, 4, 0);
        let paths: Vec<String> = plan.iter().map(|(k, _)| k.path.to_string()).collect();
        assert_eq!(paths, vec!["/g/0".to_string(), "/g/1".to_string()]);
    }

    #[test]
    fn prefetch_around_plan_handles_short_tail() {
        // 20 rows, page 1 visible (15..20). No next page; previous
        // page follows the visible tail.
        let entries: Vec<BrowseEntry> = (0..20)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let plan = prefetch_around_plan(&entries, 20, 15, 15);
        let paths: Vec<String> = plan.iter().map(|(k, _)| k.path.to_string()).collect();
        let mut expected: Vec<String> = (15..20).map(|i| format!("/g/{i}")).collect();
        expected.extend((0..15).map(|i| format!("/g/{i}")));
        assert_eq!(paths, expected);
    }

    #[test]
    fn prefetch_around_plan_empty_for_empty_model() {
        let entries: Vec<BrowseEntry> = Vec::new();
        let plan = prefetch_around_plan(&entries, 0, 15, 0);
        assert!(plan.is_empty());
    }

    #[test]
    fn prefetch_around_plan_clamps_negative_first_visible_row() {
        let entries: Vec<BrowseEntry> = (0..30)
            .map(|i| media(&format!("g{i}"), &format!("/g/{i}"), "NES"))
            .collect();
        let plan = prefetch_around_plan(&entries, 30, 15, -100);
        let paths: Vec<String> = plan.iter().map(|(k, _)| k.path.to_string()).collect();
        // Clamped to 0: visible rows 0..15, next 15..30.
        let mut expected: Vec<String> = (0..15).map(|i| format!("/g/{i}")).collect();
        expected.extend((15..30).map(|i| format!("/g/{i}")));
        assert_eq!(paths, expected);
    }

    #[test]
    fn detail_tags_emit_fixed_rows_with_blank_values() {
        let tags = vec![TagInfo {
            tag_type: "genre".into(),
            tag: "Platformer".into(),
        }];
        let detail = detail_tags_from_tags(&tags);
        let rows: Vec<&str> = detail.split('\n').collect();
        assert_eq!(
            rows,
            vec![
                "Year\t",
                "Genre\tPlatformer",
                "Players\t",
                "Developer\t",
                "Publisher\t",
                "Rating\t",
            ]
        );
    }

    #[test]
    fn detail_tags_match_aliases_and_join_multiple_values() {
        let tags = vec![
            TagInfo {
                tag_type: "platform".into(),
                tag: "Arcade".into(),
            },
            TagInfo {
                tag_type: "release_date".into(),
                tag: "1984".into(),
            },
            TagInfo {
                tag_type: "gamegenre".into(),
                tag: "Action".into(),
            },
            TagInfo {
                tag_type: "genre".into(),
                tag: "Shooter".into(),
            },
        ];
        let detail = detail_tags_from_tags(&tags);
        let rows: Vec<&str> = detail.split('\n').collect();
        assert_eq!(rows[0], "Year\t1984");
        assert_eq!(rows[1], "Genre\tAction, Shooter");
    }
}
