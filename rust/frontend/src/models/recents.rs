// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.RecentsModel` — flat list of recently-played media, surfaced
// from Core's `media.history` endpoint.
//
// Two paths into the model:
//
//   * `bind_to_endpoint!` seeds page 1 from `MediaHistoryEndpoint` so
//     a screen flip into Recents has data on the first paint when the
//     resource is already `Ready`. The fixed args (`limit = 25`, no
//     `systems` filter) match what the UI requests; if a future filter
//     is added, switch to a per-arg pattern like `GamesModel`.
//
//   * `fetch_more()` — cursor-driven follow-ups bypass the cache and
//     call `Client::media_history` directly, just like games. The
//     model owns the cursor, the in-flight `loading_more` debounce,
//     and the seq ticket that disarms stale callbacks.
//
// History is flat (no folder navigation, no auto-nav) so this model
// stays a fraction of the size of `GamesModel`. Card-write isn't wired
// here yet — recents launches by `run`-ing the entry's launcher route.

use crate::media_image_cache::{global_media_image_cache, MediaImageCache, MediaKey};
use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant,
};
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use zaparoo_core::client::ClientError;
use zaparoo_core::endpoints::media_history::{HistoryArgs, MediaHistoryEndpoint};
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::media_types::{
    MediaHistoryEntry, MediaHistoryParams, MediaHistoryResult, MediaMeta, MediaMetaParams,
    RunParams, TagInfo,
};
use zaparoo_core::remote_resource::ResourceStatus;

const NAME_ROLE: i32 = 256 + 1;
const PATH_ROLE: i32 = 256 + 2;
const SYSTEM_ID_ROLE: i32 = 256 + 3;
const COVER_KEY_ROLE: i32 = 256 + 4;
const LAUNCHER_ID_ROLE: i32 = 256 + 5;
const FAVORITE_ROLE: i32 = 256 + 6;
const FILE_STEM_ROLE: i32 = 256 + 7;

// Page size for the initial load and every cursor follow-up. Core caps
// `limit` at 100; history rows are tiny (one tile + one caption per row)
// so 25 fills several screens of the recents grid without stressing the
// over-the-wire payload. Bumping this only saves a round trip — it
// doesn't change the UI cap.
const PAGE_SIZE: u32 = 25;

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the bools are independent qproperties surfaced to QML; collapsing them \
              into an enum would force the QML side to read a single state property \
              and re-derive each flag locally, which is exactly the work the bridge \
              avoids"
)]
pub struct RecentsModelRust {
    entries: Vec<MediaHistoryEntry>,
    count: i32,
    loading: bool,
    loading_more: bool,
    error_message: QString,
    has_next_page: bool,
    next_cursor: Option<String>,
    current_detail_loading: bool,
    current_detail_tags: QString,
    current_detail_image_key: QString,
    current_detail_media_key: Option<MediaKey>,
    current_detail_media_id: Option<i64>,
    detail_seq: Arc<AtomicU64>,
    // Bumped whenever the cursor chain is reset by an initial
    // `apply_state` so any in-flight `fetch_more` callback can detect
    // its append no longer belongs to the current chain.
    seq: Arc<AtomicU64>,
    // Long-lived bridge from `media_image_cache` broadcast updates
    // onto `dataChanged(coverKey)` emits for matching rows. Spun up
    // lazily on the first page apply so the model singleton owns
    // exactly one subscriber for the whole process lifetime.
    cover_subscription: Option<JoinHandle<()>>,
    // Keys whose first-paint we're still waiting on. While non-empty
    // we hold `loading = true` so the screen-flip overlay covers the
    // gap between "page rendered with system logos" and "covers
    // cached". Drained by `notify_cover_update` as each cover lands;
    // force-cleared by the gate timer or a Pending/Errored transition.
    pending_first_paint_keys: HashSet<MediaKey>,
    // Safety timer that force-releases the cover gate after a bounded
    // delay, so a stalled bulk RPC can't park the user on `Loading…`
    // forever.
    cover_gate_timer: Option<JoinHandle<()>>,
    // Bumped on every cover-gate arm and on every Pending/Errored
    // disarm. The timer's queued closure compares against the current
    // value and bails on a mismatch — necessary because aborting the
    // JoinHandle doesn't cancel a callback already queued onto the Qt
    // thread between sleep-completion and abort.
    cover_gate_seq: Arc<AtomicU64>,
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
        #[qproperty(bool, current_detail_loading)]
        #[qproperty(QString, current_detail_tags)]
        #[qproperty(QString, current_detail_image_key)]
        type RecentsModel = super::RecentsModelRust;

        #[qinvokable]
        fn fetch_more(self: Pin<&mut RecentsModel>);

        #[qinvokable]
        fn launch_at(self: Pin<&mut RecentsModel>, index: i32);

        #[qinvokable]
        fn name_at(self: &RecentsModel, index: i32) -> QString;

        #[qinvokable]
        fn path_at(self: &RecentsModel, index: i32) -> QString;

        #[qinvokable]
        fn system_id_at(self: &RecentsModel, index: i32) -> QString;

        #[qinvokable]
        fn load_detail_at(self: Pin<&mut RecentsModel>, index: i32);

        #[qinvokable]
        fn clear_current_detail(self: Pin<&mut RecentsModel>);

        #[qinvokable]
        fn index_for_path(self: &RecentsModel, path: &QString) -> i32;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut RecentsModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut RecentsModel>);

        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(
            self: Pin<&mut RecentsModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );

        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut RecentsModel>);

        // Qt signal bound as a callable so the cover-cache bridge can
        // invoke it directly from the Qt thread when an async cover
        // fetch completes for a row that is already on screen.
        #[inherit]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut RecentsModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );

        #[cxx_name = "rowCount"]
        fn row_count(self: &RecentsModel, parent: &QModelIndex) -> i32;
        fn data(self: &RecentsModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_name = "roleNames"]
        fn role_names(self: &RecentsModel) -> QHash_i32_QByteArray;

        // Materialise a `QModelIndex` for `(row, column)` so the cover-
        // cache bridge can target individual rows in `dataChanged`.
        // Forwarded to the QAbstractListModel implementation.
        #[inherit]
        fn index(self: &RecentsModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
    }

    impl cxx_qt::Threading for RecentsModel {}
    impl cxx_qt::Initialize for RecentsModel {}
}

crate::bind_to_endpoint! {
    for ffi::RecentsModel,
    endpoint = MediaHistoryEndpoint,
    args = HistoryArgs::new(Vec::new(), PAGE_SIZE),
    select = project,
    apply = apply_state,
}

/// Snapshot of a single page that `apply_state` can write onto the
/// model. Carried by value so the closure is `Send + 'static` for the
/// `qt_thread` queue.
type PageSnapshot = (Vec<MediaHistoryEntry>, bool, Option<String>);

/// Project the resource status onto an `(Option<PageSnapshot>, error)`
/// tuple. `Idle`/`Loading` map to the same `(None, "")` shape so the
/// apply path can decide on its own whether to show the spinner.
fn project(status: &ResourceStatus<MediaHistoryResult>) -> (Option<PageSnapshot>, String) {
    match status {
        ResourceStatus::Ready(data) => (
            Some((
                data.entries.clone(),
                data.has_next_page(),
                data.next_cursor(),
            )),
            String::new(),
        ),
        ResourceStatus::Errored { message, .. } => (None, message.clone()),
        ResourceStatus::Idle | ResourceStatus::Loading => (None, String::new()),
    }
}

fn apply_state(
    mut model: Pin<&mut ffi::RecentsModel>,
    (data, err): (Option<PageSnapshot>, String),
) {
    if let Some((entries, has_next_page, next_cursor)) = data {
        // A fresh initial page resets the cursor chain — bump `seq` so
        // any in-flight `fetch_more` sees a stale ticket and bails.
        model.as_mut().rust_mut().seq.fetch_add(1, Ordering::SeqCst);
        model.as_mut().ensure_cover_subscription();
        enqueue_recents_covers(&entries);
        let count = i32::try_from(entries.len()).unwrap_or(i32::MAX);
        clear_current_detail_state(model.as_mut());
        model.as_mut().begin_reset_model();
        model.as_mut().rust_mut().entries = entries;
        model.as_mut().rust_mut().count = count;
        model.as_mut().rust_mut().next_cursor = next_cursor;
        model.as_mut().end_reset_model();
        model.as_mut().count_changed();
        if model.has_next_page != has_next_page {
            model.as_mut().set_has_next_page(has_next_page);
        }
        // Decide whether to release `loading` immediately or hold it
        // until covers are cached. `arm_cover_gate` flips loading off
        // itself when the page has nothing to wait on (every cover
        // already cached, or all rows unattributed); otherwise it
        // leaves loading=true and arms the safety timer.
        arm_cover_gate(model.as_mut());
        if model.loading_more {
            model.as_mut().set_loading_more(false);
        }
        // Look-ahead prefetch: warm page 2 so the first scroll past the
        // initial page doesn't surface a "Loading more…" cue. `fetch_more`
        // is itself guarded by `has_next_page` and `loading_more`.
        if has_next_page {
            model.as_mut().fetch_more();
        }
    } else if err.is_empty() {
        // Pending (Idle/Loading): show the spinner; don't touch entries.
        // Disarm pagination so a grid scroll during a refetch doesn't
        // fire `fetch_more` against a stale cursor — `has_next_page`
        // is re-set when Ready lands. Bump `seq` and null `next_cursor`
        // so an in-flight `fetch_more` queued during the prior Ready
        // can't slip a stale append in before the next Ready arrives.
        // Disarm the cover gate too: a stale timer firing during the
        // next Ready would clear loading prematurely.
        disarm_cover_gate(model.as_mut());
        clear_current_detail_state(model.as_mut());
        model.as_mut().rust_mut().seq.fetch_add(1, Ordering::SeqCst);
        model.as_mut().rust_mut().next_cursor = None;
        if !model.loading {
            model.as_mut().set_loading(true);
        }
        if model.has_next_page {
            model.as_mut().set_has_next_page(false);
        }
    } else {
        // Same disarm as the Pending branch — an Errored transition
        // doesn't reset entries, so a callback queued during the prior
        // Ready could otherwise append rows that don't belong to the
        // current chain.
        disarm_cover_gate(model.as_mut());
        clear_current_detail_state(model.as_mut());
        model.as_mut().rust_mut().seq.fetch_add(1, Ordering::SeqCst);
        model.as_mut().rust_mut().next_cursor = None;
        if model.loading {
            model.as_mut().set_loading(false);
        }
        if model.has_next_page {
            model.as_mut().set_has_next_page(false);
        }
    }
    let qerr = QString::from(err.as_str());
    if model.error_message != qerr {
        model.as_mut().set_error_message(qerr);
    }
}

impl ffi::RecentsModel {
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
            NAME_ROLE => QVariant::from(&QString::from(entry.media_name.as_str())),
            PATH_ROLE => QVariant::from(&QString::from(entry.media_path.as_str())),
            SYSTEM_ID_ROLE => QVariant::from(&QString::from(entry.system_id.as_str())),
            COVER_KEY_ROLE => QVariant::from(&QString::from(cover_key_for(entry).as_str())),
            LAUNCHER_ID_ROLE => QVariant::from(&QString::from(entry.launcher_id.as_str())),
            FAVORITE_ROLE => QVariant::from(&0_i32),
            FILE_STEM_ROLE => QVariant::from(&QString::from(file_stem_or_name(
                &entry.media_path,
                &entry.media_name,
            ))),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut h = QHash::<QHashPair_i32_QByteArray>::default();
        h.insert(NAME_ROLE, QByteArray::from("name"));
        h.insert(PATH_ROLE, QByteArray::from("path"));
        h.insert(SYSTEM_ID_ROLE, QByteArray::from("systemId"));
        h.insert(COVER_KEY_ROLE, QByteArray::from("coverKey"));
        h.insert(LAUNCHER_ID_ROLE, QByteArray::from("launcherId"));
        h.insert(FAVORITE_ROLE, QByteArray::from("favorite"));
        h.insert(FILE_STEM_ROLE, QByteArray::from("fileStem"));
        h
    }

    fn fetch_more(mut self: Pin<&mut Self>) {
        if self.loading_more || !self.has_next_page {
            return;
        }
        let cursor = self.next_cursor.clone();
        let expected_prev_cursor = cursor.clone();
        let seq = self.rust().seq.clone();
        let ticket = seq.load(Ordering::SeqCst);
        self.as_mut().set_loading_more(true);
        let qt_thread = self.qt_thread();
        let store = global_store();
        global_handle().spawn(async move {
            let result = store
                .client()
                .media_history(MediaHistoryParams {
                    limit: Some(PAGE_SIZE),
                    cursor,
                    systems: Vec::new(),
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

    fn launch_at(self: Pin<&mut Self>, index: i32) {
        if index < 0 || index >= self.count {
            return;
        }
        let entry = &self.entries[index as usize];
        let text = launch_text_for(entry);
        if text.is_empty() {
            return;
        }
        let name = entry.media_name.clone();
        let store = global_store();
        global_handle().spawn(async move {
            if let Err(e) = store.run_mutation::<RunMutation>(RunParams { text }).await {
                warn!("run failed for {name}: {}", e.message);
            }
        });
    }

    fn name_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].media_name.as_str())
    }

    fn path_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].media_path.as_str())
    }

    fn system_id_at(&self, index: i32) -> QString {
        if index < 0 || index >= self.count {
            return QString::default();
        }
        QString::from(self.entries[index as usize].system_id.as_str())
    }

    fn load_detail_at(mut self: Pin<&mut Self>, index: i32) {
        let ticket = self
            .as_mut()
            .rust_mut()
            .detail_seq
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        if index < 0 || index >= self.count {
            clear_current_detail_state(self.as_mut());
            return;
        }
        let entry = &self.entries[index as usize];
        let system = entry.system_id.clone();
        let path = entry.media_path.clone();
        let media_id = entry.media_id;
        if system.trim().is_empty() || path.trim().is_empty() {
            clear_current_detail_state(self.as_mut());
            return;
        }
        let detail_key = match media_id {
            Some(id) => MediaKey::with_media_id(system.clone(), path.clone(), id),
            None => MediaKey::new(system.clone(), path.clone()),
        }
        .with_current_cover_preference();
        self.as_mut().rust_mut().current_detail_media_key = Some(detail_key);
        self.as_mut().rust_mut().current_detail_media_id = media_id;
        sync_current_detail_image_key(self.as_mut());
        self.as_mut().set_current_detail_loading(true);
        self.as_mut().set_current_detail_tags(QString::default());
        let seq = self.rust().detail_seq.clone();
        let qt_thread = self.qt_thread();
        let store = global_store();
        global_handle().spawn(async move {
            let result = store
                .client()
                .media_meta(MediaMetaParams::for_media(system, path.clone()))
                .await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                match result {
                    Ok(result) => model.as_mut().set_current_detail_tags(QString::from(
                        detail_tags_from_meta(&result.media).as_str(),
                    )),
                    Err(e) => {
                        model.as_mut().set_current_detail_tags(QString::default());
                        warn!("recent detail fetch failed for {path}: {}", e.message);
                    }
                }
                model.as_mut().set_current_detail_loading(false);
            });
        });
    }

    fn clear_current_detail(self: Pin<&mut Self>) {
        clear_current_detail_state(self);
    }

    fn index_for_path(&self, path: &QString) -> i32 {
        position_of_path(&self.entries, &path.to_string())
    }

    /// Spin up the long-lived cover-cache subscriber on first use.
    /// Subsequent calls are no-ops — the model singleton owns exactly
    /// one subscriber for the whole process lifetime, decoupled from
    /// `seq` because cover updates are not tied to a particular page
    /// chain. Lagged broadcast frames are dropped silently; the
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
}

/// Resolve the cover URL key for a recents row. Mirrors `GamesModel`'s
/// path: when the in-memory cache has bytes for `(systemId, mediaPath)`
/// we hand back the `media-image/<encoded>` key the
/// `QQuickImageProvider` resolves to RAM bytes; otherwise we enqueue a
/// fetch (carrying the optional `mediaId` hint) and fall back to the
/// system logo as a nicer placeholder than the generic file glyph.
fn cover_key_for(entry: &MediaHistoryEntry) -> String {
    if entry.system_id.is_empty() {
        return "icons/File".to_string();
    }
    let media_key = media_key_for(entry).map(MediaKey::with_current_cover_preference);
    let cache = global_media_image_cache();
    let cached = media_key.as_ref().is_some_and(|k| cache.is_cached(k));
    let negative = media_key.as_ref().is_some_and(|k| cache.is_negative(k));
    let soft_no_image = media_key
        .as_ref()
        .is_some_and(|k| cache.is_soft_no_image(k));
    if !cached && !negative && !soft_no_image {
        // Miss-driven re-enqueue, same rationale as GamesModel's
        // `cover_key_for`: tiles re-bound after LRU eviction or stale-
        // enqueue truncation will hit this branch and re-arm the fetch.
        if let Some(k) = media_key.as_ref() {
            cache.enqueue_search_cover_with_media_id(k.clone(), entry.media_id, PAGE_SIZE);
        }
    }
    cover_key_for_with(entry, media_key.as_ref(), cached, negative, soft_no_image)
}

/// Build the canonical `(systemId, mediaPath)` identifier for a history
/// row. Returns `None` for rows without enough info to key on.
fn media_key_for(entry: &MediaHistoryEntry) -> Option<MediaKey> {
    if entry.system_id.is_empty() || entry.media_path.is_empty() {
        return None;
    }
    match entry.media_id {
        Some(media_id) => Some(MediaKey::with_media_id(
            entry.system_id.clone(),
            entry.media_path.clone(),
            media_id,
        )),
        None => Some(MediaKey::new(
            entry.system_id.clone(),
            entry.media_path.clone(),
        )),
    }
}

/// Pure helper for `cover_key_for`. Split out so tests can drive the
/// branches (cached, in-flight, negative-memoed, unattributed)
/// without spinning up the global cover cache and its tokio runtime.
///
/// In-flight (has a key, not cached, not negatively memoed) returns
/// the hourglass — same convention as `GamesModel`. Negatively memoed
/// rows fall back to the system logo, which is a friendlier "no cover
/// available" cue for the favorites/recents lists than `icons/File`.
fn cover_key_for_with(
    entry: &MediaHistoryEntry,
    key: Option<&MediaKey>,
    cached: bool,
    negative: bool,
    soft_no_image: bool,
) -> String {
    if entry.system_id.is_empty() {
        return "icons/File".to_string();
    }
    match key {
        Some(k) if cached => MediaImageCache::image_key_for(k),
        Some(_) if !negative && !soft_no_image => "icons/Loading".to_string(),
        _ => format!("systems/{}", entry.system_id),
    }
}

fn detail_tags_from_meta(meta: &MediaMeta) -> String {
    let source = if meta.title.tags.is_empty() {
        meta.tags.as_slice()
    } else {
        meta.title.tags.as_slice()
    };
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

fn clear_current_detail_state(mut model: Pin<&mut ffi::RecentsModel>) {
    model
        .as_mut()
        .rust_mut()
        .detail_seq
        .fetch_add(1, Ordering::SeqCst);
    model.as_mut().rust_mut().current_detail_media_key = None;
    model.as_mut().rust_mut().current_detail_media_id = None;
    model.as_mut().set_current_detail_loading(false);
    model.as_mut().set_current_detail_tags(QString::default());
    model
        .as_mut()
        .set_current_detail_image_key(QString::default());
}

fn sync_current_detail_image_key(mut model: Pin<&mut ffi::RecentsModel>) {
    let Some(key) = model.current_detail_media_key.clone() else {
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
        cache.enqueue_search_cover_with_media_id(key, model.current_detail_media_id, 1);
        model
            .as_mut()
            .set_current_detail_image_key(QString::from("icons/Loading"));
    }
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

/// Schedule a cover fetch for every history row with a non-empty
/// `(systemId, mediaPath)`. The cache enqueue is idempotent —
/// already-cached, already-pending, or negatively-memoised keys are
/// dropped — so spamming this from `apply_state` / `apply_append_page`
/// is cheap.
///
/// Iterates `entries` in reverse so the LIFO fetch queue drains in
/// visual order: the last entry pushed is `entries[0]`, which the
/// driver pops first. Forward iteration starves the top of the page.
fn enqueue_recents_covers(entries: &[MediaHistoryEntry]) {
    let cache = global_media_image_cache();
    for entry in entries.iter().rev() {
        if let Some(key) = media_key_for(entry).map(MediaKey::with_current_cover_preference) {
            cache.enqueue_search_cover_with_media_id(key, entry.media_id, PAGE_SIZE);
        }
    }
}

/// Emit `dataChanged(coverKey)` for every row whose entry's
/// `(systemId, mediaPath)` matches `key`. Cheap walk of the current
/// `entries` vec — recents pages top out at a few hundred rows after
/// look-ahead, and the bridge runs only when the cover-cache fetch
/// driver delivers a result.
///
/// Also drains `pending_first_paint_keys`: each cover landing during
/// the gate's hold ticks the set down, and emptying the set releases
/// the gate so the screen-flip overlay clears.
fn notify_cover_update(mut model: Pin<&mut ffi::RecentsModel>, key: &MediaKey) {
    let rows: Vec<i32> = model
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            key.is_cover_key()
                && match (key.media_id, e.media_id) {
                    (Some(a), Some(b)) => a == b,
                    _ => e.media_path == *key.path && e.system_id == *key.system_id,
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
        .current_detail_media_key
        .as_ref()
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
        // decode them. The hidden cover pre-warmer in
        // `RecentsScreen.qml` dispatches all N requests at once and the
        // provider's 4-worker pool decodes them in ~75–150 ms; without
        // this settle window the gate flips `loading=false` before the
        // last few decodes complete and the grid materialises with
        // those tiles still showing the procedural fallback. Mirrors
        // the same hand-off in `games.rs::notify_cover_update`. Same
        // seq-ticket guard as the safety timer so a model reset
        // cancels the pending release.
        info!("recents: cover gate bytes settled — entering decode-settle window");
        let seq = model.rust().cover_gate_seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
        let qt_thread = model.qt_thread();
        let handle = global_handle().spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = qt_thread.queue(move |mut model: Pin<&mut ffi::RecentsModel>| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                model.as_mut().rust_mut().cover_gate_timer = None;
                if model.loading {
                    info!("recents: cover gate released after decode-settle window");
                    model.as_mut().set_loading(false);
                }
            });
        });
        model.as_mut().rust_mut().cover_gate_timer = Some(handle);
    }
}

/// Compute the set of media keys on the current page whose covers we
/// must wait on before releasing the cover gate. Rows without enough
/// info to key on, already-cached keys, and negatively-memoised keys
/// are all excluded. Pure helper so the gate's binning logic is unit-
/// testable without spinning up the global cache + tokio runtime.
fn compute_unresolved_keys<F, G>(
    entries: &[MediaHistoryEntry],
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

/// Decide whether to hold `loading=true` until the page's covers are
/// cached, or release immediately. Called once per Ready `apply_state`.
///
/// - If every history row's cover is already cached or negatively-
///   memoised, set loading=false right now — the screen-flip overlay
///   clears.
/// - Otherwise, store the unresolved set on the model, arm a 3 s
///   safety timer, and leave loading=true. `notify_cover_update` will
///   drain the set as covers land; whichever happens first (set
///   empties or timer fires) releases the gate.
fn arm_cover_gate(mut model: Pin<&mut ffi::RecentsModel>) {
    if let Some(handle) = model.as_mut().rust_mut().cover_gate_timer.take() {
        handle.abort();
    }
    let cache = global_media_image_cache();
    let unresolved = compute_unresolved_keys(
        &model.entries,
        |k| cache.is_cached(k),
        |k| cache.is_negative(k) || cache.is_soft_no_image(k),
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
        "recents: arm cover gate (holding loading until covers cached)"
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

/// Tear down any active cover gate. Used by Pending/Errored apply paths
/// to invalidate an in-flight timer's queued callback (via the seq
/// bump) before the next Ready installs a fresh one.
fn disarm_cover_gate(mut model: Pin<&mut ffi::RecentsModel>) {
    if let Some(handle) = model.as_mut().rust_mut().cover_gate_timer.take() {
        handle.abort();
    }
    model.as_mut().rust_mut().pending_first_paint_keys.clear();
    model.rust().cover_gate_seq.fetch_add(1, Ordering::SeqCst);
}

/// Force-release the cover gate from the safety timer. Called only via
/// the timer's queued callback after a seq-match check; the
/// notify-driven release path lives inline in `notify_cover_update`.
fn release_cover_gate_after_timeout(mut model: Pin<&mut ffi::RecentsModel>) {
    let pending = model.pending_first_paint_keys.len();
    info!(pending, "recents: cover gate timed out, releasing");
    model.as_mut().rust_mut().pending_first_paint_keys.clear();
    model.as_mut().rust_mut().cover_gate_timer = None;
    if model.loading {
        model.as_mut().set_loading(false);
    }
}

/// Build the `text` payload sent to Core's `run` for a history entry.
/// Runtime relaunches prefer the exact path Core recorded; portable
/// `ZapScript` is not available on history rows and launcher affinity is
/// less important than avoiding title-resolution ambiguity.
fn launch_text_for(entry: &MediaHistoryEntry) -> String {
    entry.media_path.clone()
}

fn position_of_path(entries: &[MediaHistoryEntry], needle: &str) -> i32 {
    if needle.is_empty() {
        return -1;
    }
    entries
        .iter()
        .position(|e| e.media_path == needle)
        .map_or(-1, |i| i as i32)
}

fn apply_append_page(
    mut model: Pin<&mut ffi::RecentsModel>,
    result: Result<MediaHistoryResult, ClientError>,
    expected_prev_cursor: Option<&str>,
) {
    if model.next_cursor.as_deref() != expected_prev_cursor {
        if model.loading_more {
            model.as_mut().set_loading_more(false);
        }
        return;
    }
    match result {
        Ok(result) => {
            let has_next_page = result.has_next_page();
            let next_cursor = result.next_cursor();
            let new_count = i32::try_from(result.entries.len()).unwrap_or(i32::MAX - model.count);
            enqueue_recents_covers(&result.entries);
            if new_count > 0 {
                let first = model.count;
                let last = first.saturating_add(new_count).saturating_sub(1);
                let parent = QModelIndex::default();
                model.as_mut().begin_insert_rows(&parent, first, last);
                model.as_mut().rust_mut().entries.extend(result.entries);
                model.as_mut().rust_mut().count = first.saturating_add(new_count);
                model.as_mut().end_insert_rows();
                model.as_mut().count_changed();
            }
            model.as_mut().rust_mut().next_cursor = next_cursor;
            model.as_mut().set_has_next_page(has_next_page);
            model.as_mut().set_loading_more(false);
        }
        Err(e) => {
            warn!("media.history follow-up page failed: {}", e.message);
            model
                .as_mut()
                .set_error_message(QString::from(e.message.as_str()));
            model.as_mut().set_loading_more(false);
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
        compute_unresolved_keys, cover_key_for_with, launch_text_for, media_key_for,
        position_of_path, project,
    };
    use crate::media_image_cache::{MediaImageCache, MediaKey};
    use std::collections::HashSet;
    use zaparoo_core::media_types::{MediaHistoryEntry, MediaHistoryResult, Pagination};
    use zaparoo_core::remote_resource::ResourceStatus;

    fn entry(name: &str, path: &str, system_id: &str, launcher_id: &str) -> MediaHistoryEntry {
        MediaHistoryEntry {
            media_name: name.into(),
            media_path: path.into(),
            system_id: system_id.into(),
            launcher_id: launcher_id.into(),
            ..MediaHistoryEntry::default()
        }
    }

    #[test]
    fn cover_key_uses_loading_icon_when_in_flight() {
        let e = entry("smb", "/p/smb", "NES", "NES");
        let key = media_key_for(&e).expect("media has key");
        // Has a key, not cached, not negative → fetch is in flight,
        // show the hourglass over the tile.
        assert_eq!(
            cover_key_for_with(&e, Some(&key), false, false, false),
            "icons/Loading"
        );
    }

    #[test]
    fn cover_key_falls_back_to_system_logo_when_negatively_memoed() {
        let e = entry("smb", "/p/smb", "NES", "NES");
        let key = media_key_for(&e).expect("media has key");
        // Has a key, not cached, negatively memoed → no image to wait
        // for. Fall back to the system logo (friendlier than icons/File
        // for favorites/recents lists).
        assert_eq!(
            cover_key_for_with(&e, Some(&key), false, true, false),
            "systems/NES"
        );
    }

    #[test]
    fn cover_key_falls_back_to_system_logo_when_soft_missed() {
        let e = entry("smb", "/p/smb", "NES", "NES");
        let key = media_key_for(&e).expect("media has key");
        assert_eq!(
            cover_key_for_with(&e, Some(&key), false, false, true),
            "systems/NES"
        );
    }

    #[test]
    fn cover_key_returns_media_image_key_when_cached() {
        let e = entry("smb", "/p/smb", "NES", "NES");
        let key = media_key_for(&e).expect("media has key");
        let expected = MediaImageCache::image_key_for(&key);
        assert_eq!(
            cover_key_for_with(&e, Some(&key), true, false, false),
            expected
        );
        assert!(expected.starts_with("media-image/"));
    }

    #[test]
    fn cover_key_falls_back_to_file_glyph_when_system_missing() {
        let e = entry("orphan", "/p/orphan", "", "");
        assert_eq!(
            cover_key_for_with(&e, None, false, false, false),
            "icons/File"
        );
    }

    #[test]
    fn media_key_for_skips_rows_without_path_or_system() {
        let pathless = entry("ghost", "", "NES", "NES");
        assert!(media_key_for(&pathless).is_none());
        let unattributed = entry("ghost", "/p", "", "");
        assert!(media_key_for(&unattributed).is_none());
        let key =
            media_key_for(&entry("smb", "/p/smb", "NES", "NES")).expect("complete entry has key");
        assert_eq!(key.system_id.as_ref(), "NES");
        assert_eq!(key.path.as_ref(), "/p/smb");
    }

    #[test]
    fn launch_text_uses_raw_media_path_when_launcher_known() {
        let e = entry("smb", "/p/smb.nes", "NES", "NES");
        assert_eq!(launch_text_for(&e), "/p/smb.nes");
    }

    #[test]
    fn launch_text_uses_raw_media_path_when_launcher_missing() {
        let e = entry("smb", "/p/smb.nes", "NES", "");
        assert_eq!(launch_text_for(&e), "/p/smb.nes");
    }

    #[test]
    fn launch_text_preserves_path_with_spaces_and_metacharacters() {
        let path = "/media/fat/cifs/games/Genesis/1 US - A-F/B.O.B. (USA,Europe) (Rev A).md";
        let e = entry("bob", path, "Genesis", "Genesis");
        assert_eq!(launch_text_for(&e), path);
    }

    #[test]
    fn launch_text_preserves_backslashes_and_quotes_in_path() {
        let path = r#"C:\Games\say "hi".rom"#;
        let e = entry("weird", path, "DOS", "DOS");
        assert_eq!(launch_text_for(&e), path);
    }

    #[test]
    fn launch_text_is_empty_when_path_missing_and_launcher_present() {
        // A history row with a launcher id but no path is malformed.
        // Empty here suppresses the run entirely.
        let e = entry("ghost", "", "NES", "NES");
        assert_eq!(launch_text_for(&e), "");
    }

    #[test]
    fn position_of_path_returns_index_on_match() {
        let entries = vec![
            entry("smb", "/p/smb", "NES", "NES"),
            entry("zelda", "/p/zelda", "NES", "NES"),
        ];
        assert_eq!(position_of_path(&entries, "/p/zelda"), 1);
    }

    #[test]
    fn position_of_path_empty_needle_returns_minus_one() {
        let entries = vec![entry("smb", "/p/smb", "NES", "NES")];
        assert_eq!(position_of_path(&entries, ""), -1);
    }

    #[test]
    fn position_of_path_missing_returns_minus_one() {
        let entries = vec![entry("smb", "/p/smb", "NES", "NES")];
        assert_eq!(position_of_path(&entries, "/missing"), -1);
    }

    #[test]
    fn project_idle_yields_empty_pending() {
        let (page, err) = project(&ResourceStatus::Idle);
        assert!(page.is_none());
        assert!(err.is_empty());
    }

    #[test]
    fn project_loading_yields_empty_pending() {
        let (page, err) = project(&ResourceStatus::Loading);
        assert!(page.is_none());
        assert!(err.is_empty());
    }

    #[test]
    fn project_ready_carries_entries_and_pagination() {
        let result = MediaHistoryResult {
            entries: vec![entry("smb", "/p/smb", "NES", "NES")],
            pagination: Some(Pagination {
                has_next_page: true,
                page_size: 25,
                next_cursor: Some("cursor-2".into()),
            }),
        };
        let (page, err) = project(&ResourceStatus::Ready(result));
        assert!(err.is_empty());
        let (entries, has_next, cursor) = page.expect("ready snapshot");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].media_path, "/p/smb");
        assert!(has_next);
        assert_eq!(cursor.as_deref(), Some("cursor-2"));
    }

    #[test]
    fn project_ready_without_pagination_disarms_next_page() {
        // Core docs say pagination is omitted when no entries are
        // returned. The projection must surface that as `has_next_page
        // = false` so the model disarms `fetch_more` instead of looping
        // on a stale cursor.
        let result = MediaHistoryResult::default();
        let (page, err) = project(&ResourceStatus::Ready(result));
        assert!(err.is_empty());
        let (entries, has_next, cursor) = page.expect("ready snapshot");
        assert!(entries.is_empty());
        assert!(!has_next);
        assert!(cursor.is_none());
    }

    #[test]
    fn project_errored_carries_message_with_no_snapshot() {
        let (page, err) = project(&ResourceStatus::Errored {
            message: "rpc kaboom".into(),
            retrying: true,
        });
        assert!(page.is_none());
        assert_eq!(err, "rpc kaboom");
    }

    #[test]
    fn compute_unresolved_keys_empty_entries_returns_empty() {
        let unresolved = compute_unresolved_keys(&[], |_| false, |_| false);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn compute_unresolved_keys_all_cached_returns_empty() {
        let entries = vec![
            entry("smb", "/p/smb", "NES", "NES"),
            entry("zelda", "/p/zelda", "NES", "NES"),
        ];
        let unresolved = compute_unresolved_keys(&entries, |_| true, |_| false);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn compute_unresolved_keys_mixed_returns_only_uncached() {
        let cached_path = "/p/smb";
        let entries = vec![
            entry("smb", cached_path, "NES", "NES"),
            entry("zelda", "/p/zelda", "NES", "NES"),
        ];
        let unresolved =
            compute_unresolved_keys(&entries, |k| k.path.as_ref() == cached_path, |_| false);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/zelda")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }

    #[test]
    fn compute_unresolved_keys_excludes_negative_memo() {
        let negative_path = "/p/no-image";
        let entries = vec![
            entry("smb", "/p/smb", "NES", "NES"),
            entry("orphan", negative_path, "NES", "NES"),
        ];
        let unresolved =
            compute_unresolved_keys(&entries, |_| false, |k| k.path.as_ref() == negative_path);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/smb")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }

    #[test]
    fn compute_unresolved_keys_excludes_soft_no_image() {
        let soft_path = "/p/soft-no-image";
        let entries = vec![
            entry("smb", "/p/smb", "NES", "NES"),
            entry("orphan", soft_path, "NES", "NES"),
        ];
        let unresolved =
            compute_unresolved_keys(&entries, |_| false, |k| k.path.as_ref() == soft_path);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/smb")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }

    #[test]
    fn compute_unresolved_keys_skips_unattributed_rows() {
        // Rows without a system id or path never key into the cache —
        // gate must not wait on them.
        let entries = vec![
            entry("smb", "/p/smb", "NES", "NES"),
            entry("orphan", "", "NES", "NES"),
            entry("ghost", "/p/ghost", "", ""),
        ];
        let unresolved = compute_unresolved_keys(&entries, |_| false, |_| false);
        let expected: HashSet<MediaKey> = [MediaKey::new("NES", "/p/smb")].into_iter().collect();
        assert_eq!(unresolved, expected);
    }
}
