// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.HubLayout` — the Hub's persisted `[[hub.items]]` layout
// (`zaparoo_core::hub_layout`), exposed as a flat, index-accessed list —
// `item_count`/`item_kind_at`/etc — the same query style
// `CategoriesModel`/`SystemsModel` already use (`category_at(i)`,
// `system_id_at(i)`), not a full `QAbstractListModel`. Deliberately narrow:
// a real list model would mean re-deriving, in Rust, all of the
// cross-model logic that currently decides Resume/Update's live visibility
// (Recents state, internet reachability, build flags) — see
// docs/plans/ui-geometry-refresh.md. `HubScreen.qml` builds its own JS
// array by looping these accessors and resolving each kind (its
// `_resolveCategoryEntry`/`_resolveActionEntry`/etc), folding in whatever
// live visibility rule that kind needs — the same shape it already used to
// build its list from `CategoriesModel` before this replaced it, and keeps
// that visibility filtering exactly where it is.
//
// Only entries `zaparoo_core::hub_layout::HubLayout::visible()` yields are
// indexed here — the reserved `collection` kind and any `type` this build
// doesn't recognise are skipped (still round-tripped on save, just not
// exposed to QML; see that function's doc comment).
//
// `item_*_at` are `#[qinvokable]` METHODS, not qproperties — a QML binding
// that only calls one does not pick up an automatic dependency on the
// state it reads internally (the same gotcha the now-retired
// `Browse.HubState.is_item_hidden`/`item_order_index` used to carry).
// `revision` is a real qproperty (READ + NOTIFY) that bumps on every
// mutation as the escape hatch: a binding that reads
// `Browse.HubLayout.revision` (even unused) picks up the NOTIFY and
// re-evaluates, which is what picks up whatever `item_*_at` now returns.

use crate::media_image_cache::{global_media_image_cache, MediaImageCache, MediaKey};
use crate::models::action_error::report_action_error;
use crate::models::{
    global_handle, global_store, with_hidden_browse_prefs_read, with_hub_layout_mut,
    with_hub_layout_mut_unsaved, with_hub_layout_read,
};
use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::warn;
use zaparoo_core::endpoints::run::RunMutation;
use zaparoo_core::hub_layout::{HubItem, HubLayout as CoreHubLayout};
use zaparoo_core::media_types::RunParams;

/// `items_snapshot`'s delimited format — plain ASCII control characters
/// rather than JSON, so this crate doesn't need a new dependency (see
/// CLAUDE.md's "ask first before adding dependencies") for what's
/// otherwise a batch of already-plain strings. Neither character can occur
/// in a TOML-sourced field in practice; even if one did, `.split()` on the
/// QML side just yields an extra empty segment for that one row rather
/// than corrupting any other row.
const FIELD_SEP: char = '\u{1f}';
const RECORD_SEP: char = '\u{1e}';

#[derive(Default)]
pub struct HubLayoutRust {
    revision: u32,
    cover_subscription: Option<JoinHandle<()>>,
    /// Set for the duration of a Hub Move-mode session (Options -> Move,
    /// through Accept/Cancel) — a snapshot of the layout at the moment
    /// `begin_move` was called. Every intermediate `move_held_to` press
    /// mutates the live layout in memory only, via
    /// `with_hub_layout_mut_unsaved`; `cancel_move` restores this snapshot
    /// verbatim (nothing was ever written to disk, so there's nothing to
    /// revert on disk either), and `commit_move` drops it and performs the
    /// session's one real save. `None` outside a session.
    move_snapshot: Option<CoreHubLayout>,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(u32, revision, READ, NOTIFY)]
        type HubLayout = super::HubLayoutRust;

        /// Number of visible (renderable-kind) entries.
        #[qinvokable]
        fn item_count(self: &HubLayout) -> i32;

        /// `"category"` / `"action"` / `"system"` / `"folder"` /
        /// `"zapscript"` / `"blank"`. Empty for an out-of-range index.
        #[qinvokable]
        fn item_kind_at(self: &HubLayout, index: i32) -> QString;

        /// Category id / action id / system id, per kind. Empty for kinds
        /// that don't use it.
        #[qinvokable]
        fn item_id_at(self: &HubLayout, index: i32) -> QString;

        /// Folder path (`folder` kind), or a linked game's path on a
        /// `zapscript` entry (paired with `item_system_at` for cover-art
        /// resolution — see that qinvokable's doc comment) — not the
        /// script text itself, and not necessarily the launch target
        /// (that's `item_script_at`, which may differ, e.g. a ZapScript
        /// directive rather than a raw path).
        #[qinvokable]
        fn item_path_at(self: &HubLayout, index: i32) -> QString;

        /// ZapScript text (`zapscript` kind).
        #[qinvokable]
        fn item_script_at(self: &HubLayout, index: i32) -> QString;

        /// Optional display-name override, any kind.
        #[qinvokable]
        fn item_name_at(self: &HubLayout, index: i32) -> QString;

        /// Optional icon-name override, any kind — resolved the same way
        /// as a built-in icon key (`custom/hub/<name>` then
        /// `icons/<name>`); see docs/customization.md.
        #[qinvokable]
        fn item_icon_at(self: &HubLayout, index: i32) -> QString;

        /// Optional system id on a `zapscript` entry — not a launch
        /// target, purely for cover-art resolution. Paired with
        /// `item_path_at` (a game's stable identity, same as `folder`
        /// tiles) it drives `resolve_media_cover_key`'s real Core lookup;
        /// alone, it's a softer hint for `resolve_system_cover_key`'s
        /// system-logo fallback (a script with no single game to fetch
        /// art for, e.g. "launch a random game in this system").
        #[qinvokable]
        fn item_system_at(self: &HubLayout, index: i32) -> QString;

        /// Reconcile the layout against Core's currently-detected category
        /// ids — see `zaparoo_core::hub_layout::HubLayout::reconcile`.
        /// Call once per real category-list refresh (`Main.qml`, alongside
        /// `HubScreen.restoreFromCategoriesReset`); a no-op call (nothing
        /// changed, or categories haven't loaded yet) does not bump
        /// `revision`.
        #[qinvokable]
        fn reconcile(self: Pin<&mut HubLayout>, category_ids: &QStringList);

        /// Display name for a `system`-kind entry's bare system id,
        /// independent of any live category/catalog row (a `system` tile
        /// can point at a system whose category isn't currently active) —
        /// same override-then-localized priority `SystemsModel` uses,
        /// minus the Core catalog name (no live row to read it from),
        /// falling back to the bare id. Empty input returns empty.
        #[qinvokable]
        fn resolve_system_name(self: &HubLayout, system_id: &QString) -> QString;

        /// Cover key for a `system`-kind entry's bare system id, same
        /// override-then-bundled-logo resolution `SystemsModel` uses.
        /// Empty input returns empty.
        #[qinvokable]
        fn resolve_system_cover_key(self: &HubLayout, system_id: &QString) -> QString;

        /// Cover key for a game LINKED to the Hub (a `zapscript` entry
        /// whose `system`/`path` are the game's own stable identity, not
        /// just a cover hint) — resolved through Core's media-image cache
        /// the same way a Favorites/Recents row is: real scraped art when
        /// cached, `"icons/Loading"` while a fetch is in flight (kicking
        /// one off if none is queued yet), a system-logo fallback when
        /// Core has confirmed there's nothing to fetch. This is what makes
        /// a linked game show its real cover instead of a generic icon —
        /// "as if it was a browse games grid." Empty input (either arg)
        /// returns empty; the caller falls back to a hint-only resolution.
        #[qinvokable]
        fn resolve_media_cover_key(
            self: &HubLayout,
            system_id: &QString,
            path: &QString,
        ) -> QString;

        /// Run arbitrary ZapScript — the `zapscript`-kind Hub tile's launch
        /// path. `Client::run`/`RunParams { text }` (`rust/zaparoo-core/src/
        /// client.rs`) is the only launch API in the app; every other
        /// launch invokable (`SystemsModel::launch_at`, etc.) already
        /// forwards to it from a resolved path/id — this is the one entry
        /// point that forwards a caller-supplied string directly, since a
        /// `zapscript` tile's `script` field already IS the ZapScript to
        /// run. No-op on an empty string.
        #[qinvokable]
        fn run_script(self: Pin<&mut HubLayout>, text: &QString);

        /// Arm a Move session on the item at `index` (visible index) — the
        /// Hub's Options -> Move. Snapshots the layout so every
        /// intermediate press during the session can mutate freely without
        /// touching disk; end the session with `commit_move` or
        /// `cancel_move`. Returns `false` (no snapshot taken) for an
        /// out-of-range index or if a session is already open.
        #[qinvokable]
        fn begin_move(self: Pin<&mut HubLayout>, index: i32) -> bool;

        /// Board-model swap: the item currently at `from` (visible index)
        /// trades places with whatever occupies `to` — a tile, or a
        /// `blank` — see `zaparoo_core::hub_layout::HubLayout::
        /// place_visible_item`. Only valid inside a Move session (between
        /// `begin_move` and `commit_move`/`cancel_move`); mutates the
        /// in-memory layout only, never persists. Bumps `revision` and
        /// returns whether anything actually moved.
        #[qinvokable]
        fn move_held_to(self: Pin<&mut HubLayout>, from: i32, to: i32) -> bool;

        /// End a Move session, keeping the result: trims any dangling
        /// trailing blanks (see `trim_trailing_blanks`) and performs the
        /// session's one real save. No-op if no session is open.
        #[qinvokable]
        fn commit_move(self: Pin<&mut HubLayout>);

        /// End a Move session, discarding every press made during it —
        /// restores the layout exactly as `begin_move` found it. Nothing
        /// was ever written to disk during the session, so this touches no
        /// file. No-op if no session is open.
        #[qinvokable]
        fn cancel_move(self: Pin<&mut HubLayout>);

        /// Remove the item at `index` (visible index) by turning its cell
        /// into a `blank` — the Hub's Options -> Hide/Delete. Board-model
        /// removal: nothing after it shifts. The tile stays out for good
        /// (see `zaparoo_core::hub_layout::HubLayout`'s `known` doc
        /// comment); use `add_item` to bring it back. Persists and bumps
        /// `revision` when it actually removed something; returns whether
        /// it did.
        #[qinvokable]
        fn remove_item(self: Pin<&mut HubLayout>, index: i32) -> bool;

        /// Number of entries the "Add item…" grab bag currently offers —
        /// every `known` category/action key not currently in the layout.
        /// Does NOT include the always-available "blank" option; that's a
        /// fixed entry the caller (`HubScreen`'s View menu) adds itself.
        #[qinvokable]
        fn available_count(self: &HubLayout) -> i32;

        /// `"category"` / `"action"` for the `index`-th "Add item…" entry.
        /// Empty for an out-of-range index.
        #[qinvokable]
        fn available_kind_at(self: &HubLayout, index: i32) -> QString;

        /// Category id / action id for the `index`-th "Add item…" entry.
        /// Empty for an out-of-range index.
        #[qinvokable]
        fn available_id_at(self: &HubLayout, index: i32) -> QString;

        /// Add an entry back from the "Add item…" grab bag (`kind` +
        /// `id` from `available_kind_at`/`available_id_at`), or a fresh
        /// blank spacer (`kind == "blank"`, `id` ignored). Board-model
        /// placement: lands on the `target`-th visible position (the Hub's
        /// current cursor cell) when that cell is a `blank`; otherwise
        /// appended after the last real tile — see
        /// `zaparoo_core::hub_layout::HubLayout::add_item`. Persists and
        /// bumps `revision` on success; returns whether it added anything.
        #[qinvokable]
        fn add_item(self: Pin<&mut HubLayout>, kind: &QString, id: &QString, target: i32) -> bool;

        /// Wipe the layout and reseed from Core's currently-detected
        /// category ids plus the built-in actions — the Hub's View ->
        /// Reset layout. Always persists and bumps `revision`, even when
        /// `category_ids` is empty (that still clears down to the built-in
        /// actions; see `zaparoo_core::hub_layout::HubLayout::reset`).
        #[qinvokable]
        fn reset_layout(self: Pin<&mut HubLayout>, category_ids: &QStringList);

        /// True when the layout has never been seeded — see
        /// `zaparoo_core::hub_layout::HubLayout::is_unseeded`.
        /// `HubScreen.items` uses this instead of `item_count() == 0` to
        /// pick its bootstrap placeholder branch: a seeded-but-emptied
        /// layout (every item removed, trailing blanks trimmed away) is a
        /// real, reachable `item_count() == 0` state that must render as
        /// an empty Hub, not fall back to fake placeholder tiles.
        #[qinvokable]
        fn is_unseeded(self: &HubLayout) -> bool;

        /// Every visible entry's fields in one call, `RECORD_SEP`-joined
        /// rows of `FIELD_SEP`-joined fields (kind, id, path, script, name,
        /// icon, system) — see the impl. `HubScreen.qml`'s `items` rebuild
        /// uses this instead of looping the `item_*_at` accessors per
        /// entry, cutting a Hub-sized rebuild's FFI cost from several
        /// locked, cloned round trips per tile to one round trip total.
        #[qinvokable]
        fn items_snapshot(self: &HubLayout) -> QString;
    }

    impl cxx_qt::Initialize for HubLayout {}
    impl cxx_qt::Threading for HubLayout {}
}

impl Initialize for ffi::HubLayout {
    fn initialize(mut self: Pin<&mut Self>) {
        crate::startup_trace("rust:model HubLayout init");
        // One subscriber for the process lifetime (a qml_singleton's
        // Initialize fires exactly once) — mirrors GamesModel's
        // `ensure_cover_subscription`. A `revision` bump forces
        // `HubScreen.items` to fully re-resolve and, in turn, PagedGrid to
        // resync the whole grid — not "cheap" the way the original comment
        // here assumed, so this only fires for a cache update that could
        // actually change what's on screen: a `zapscript` entry whose
        // `(system, path)` is this update's key. Hub items rarely link a
        // game at all, so almost every cache update is now filtered out
        // instead of forcing a rebuild.
        let cache = global_media_image_cache();
        let mut rx = cache.subscribe();
        let qt_thread = self.qt_thread();
        let handle = global_handle().spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        let relevant = with_hub_layout_read(|layout| {
                            layout.visible().any(|item| {
                                item.kind() == zaparoo_core::hub_layout::HubItemKind::ZapScript
                                    && !item.system.is_empty()
                                    && !item.path.is_empty()
                                    && item.system.as_str() == update.key.system_id.as_ref()
                                    && item.path.as_str() == update.key.path.as_ref()
                            })
                        });
                        if !relevant {
                            continue;
                        }
                        let _ = qt_thread.queue(|mut model| {
                            let next = model.revision.wrapping_add(1);
                            model.as_mut().rust_mut().revision = next;
                            model.as_mut().revision_changed();
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

fn visible_item_at(index: i32) -> Option<HubItem> {
    let index = usize::try_from(index).ok()?;
    with_hub_layout_read(|layout| layout.visible().nth(index).cloned())
}

/// `(kind, id)` for the `index`-th "Add item…" entry — splits an
/// `available_known()` key (`"category:<id>"` / `"action:<id>"`) on its
/// first `:`, since ids never contain one themselves (category/action ids
/// are internal slugs, not user text).
fn available_entry_at(index: i32) -> Option<(String, String)> {
    let index = usize::try_from(index).ok()?;
    with_hub_layout_read(|layout| {
        let key = layout.available_known().into_iter().nth(index)?;
        let (kind, id) = key.split_once(':')?;
        Some((kind.to_string(), id.to_string()))
    })
}

impl ffi::HubLayout {
    fn item_count(&self) -> i32 {
        with_hub_layout_read(|layout| i32::try_from(layout.visible().count()).unwrap_or(i32::MAX))
    }

    fn item_kind_at(&self, index: i32) -> QString {
        visible_item_at(index).map_or_else(QString::default, |item| {
            QString::from(item.kind_raw.as_str())
        })
    }

    fn item_id_at(&self, index: i32) -> QString {
        visible_item_at(index).map_or_else(QString::default, |item| QString::from(item.id.as_str()))
    }

    fn item_path_at(&self, index: i32) -> QString {
        visible_item_at(index)
            .map_or_else(QString::default, |item| QString::from(item.path.as_str()))
    }

    fn item_script_at(&self, index: i32) -> QString {
        visible_item_at(index)
            .map_or_else(QString::default, |item| QString::from(item.script.as_str()))
    }

    fn item_name_at(&self, index: i32) -> QString {
        visible_item_at(index)
            .map_or_else(QString::default, |item| QString::from(item.name.as_str()))
    }

    fn item_icon_at(&self, index: i32) -> QString {
        visible_item_at(index)
            .map_or_else(QString::default, |item| QString::from(item.icon.as_str()))
    }

    fn item_system_at(&self, index: i32) -> QString {
        visible_item_at(index)
            .map_or_else(QString::default, |item| QString::from(item.system.as_str()))
    }

    fn reconcile(mut self: Pin<&mut Self>, category_ids: &QStringList) {
        let ids: Vec<String> = category_ids.iter().map(String::from).collect();
        // One-time migration input only — consulted by `reconcile` solely
        // while the layout is still unseeded; a no-op read on every later
        // call. See `SettingsConfig::hidden_categories`'s doc comment.
        let migrate_hidden = with_hidden_browse_prefs_read(|p| p.hidden_categories.clone());
        let changed = with_hub_layout_mut(|layout| layout.reconcile(&ids, &migrate_hidden));
        if changed {
            let next = self.revision.wrapping_add(1);
            self.as_mut().rust_mut().revision = next;
            self.as_mut().revision_changed();
        }
    }

    fn resolve_system_name(&self, system_id: &QString) -> QString {
        let id = system_id.to_string();
        if id.is_empty() {
            return QString::default();
        }
        QString::from(system_display_name(&id).as_str())
    }

    fn resolve_system_cover_key(&self, system_id: &QString) -> QString {
        let id = system_id.to_string();
        if id.is_empty() {
            return QString::default();
        }
        QString::from(system_cover_key(&id).as_str())
    }

    fn resolve_media_cover_key(&self, system_id: &QString, path: &QString) -> QString {
        let system_id = system_id.to_string();
        let path = path.to_string();
        if system_id.is_empty() || path.is_empty() {
            return QString::default();
        }
        // No media_id: a Hub-linked game's stable identity is (system,
        // path), same as `folder` tiles already address by path alone —
        // Core's internal row id isn't guaranteed stable across a
        // reindex, and every launch path in this app already prefers
        // exact paths over an opaque id for the same reason.
        let key = MediaKey::new(system_id.clone(), path).with_current_cover_preference();
        let cache = global_media_image_cache();
        if cache.is_cached(&key) {
            return QString::from(MediaImageCache::image_key_for(&key).as_str());
        }
        if cache.is_negative(&key) || cache.is_soft_no_image(&key) {
            return QString::from(format!("systems/{system_id}").as_str());
        }
        // Soft-miss policy, matching Favorites/Recents' own choice for the
        // same reason: this row isn't backed by a live browse listing, so
        // a miss here must not poison the cache's global negative memo —
        // the same game might still resolve fine through a real browse
        // path elsewhere.
        cache.enqueue_search_cover_with_media_id(key, None, 1);
        QString::from("icons/Loading")
    }

    fn begin_move(mut self: Pin<&mut Self>, index: i32) -> bool {
        if self.move_snapshot.is_some() {
            return false;
        }
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let in_range = with_hub_layout_read(|layout| index < layout.visible().count());
        if !in_range {
            return false;
        }
        let snapshot = with_hub_layout_read(CoreHubLayout::clone);
        self.as_mut().rust_mut().move_snapshot = Some(snapshot);
        true
    }

    fn move_held_to(mut self: Pin<&mut Self>, from: i32, to: i32) -> bool {
        let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
            return false;
        };
        let moved = with_hub_layout_mut_unsaved(|layout| layout.place_visible_item(from, to));
        if moved {
            let next = self.revision.wrapping_add(1);
            self.as_mut().rust_mut().revision = next;
            self.as_mut().revision_changed();
        }
        moved
    }

    fn commit_move(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().move_snapshot.take().is_none() {
            return;
        }
        let changed = with_hub_layout_mut(|layout| {
            let before_len = layout.items.len();
            layout.trim_trailing_blanks();
            layout.items.len() != before_len
        });
        if changed {
            let next = self.revision.wrapping_add(1);
            self.as_mut().rust_mut().revision = next;
            self.as_mut().revision_changed();
        }
    }

    fn cancel_move(mut self: Pin<&mut Self>) {
        let Some(snapshot) = self.as_mut().rust_mut().move_snapshot.take() else {
            return;
        };
        with_hub_layout_mut_unsaved(|layout| *layout = snapshot);
        let next = self.revision.wrapping_add(1);
        self.as_mut().rust_mut().revision = next;
        self.as_mut().revision_changed();
    }

    fn remove_item(mut self: Pin<&mut Self>, index: i32) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let removed = with_hub_layout_mut(|layout| layout.remove_visible_item(index));
        if removed {
            let next = self.revision.wrapping_add(1);
            self.as_mut().rust_mut().revision = next;
            self.as_mut().revision_changed();
        }
        removed
    }

    fn available_count(&self) -> i32 {
        with_hub_layout_read(|layout| {
            i32::try_from(layout.available_known().len()).unwrap_or(i32::MAX)
        })
    }

    fn available_kind_at(&self, index: i32) -> QString {
        available_entry_at(index)
            .map_or_else(QString::default, |(kind, _)| QString::from(kind.as_str()))
    }

    fn available_id_at(&self, index: i32) -> QString {
        available_entry_at(index)
            .map_or_else(QString::default, |(_, id)| QString::from(id.as_str()))
    }

    fn add_item(mut self: Pin<&mut Self>, kind: &QString, id: &QString, target: i32) -> bool {
        let kind = kind.to_string();
        let id = id.to_string();
        // A negative sentinel (no meaningful cursor cell, e.g. a
        // programmatic call) maps to an index no visible position can ever
        // reach, so `add_item` always falls through to its append path.
        let target = usize::try_from(target).unwrap_or(usize::MAX);
        let added = with_hub_layout_mut(|layout| layout.add_item(&kind, &id, target));
        if added {
            let next = self.revision.wrapping_add(1);
            self.as_mut().rust_mut().revision = next;
            self.as_mut().revision_changed();
        }
        added
    }

    fn reset_layout(mut self: Pin<&mut Self>, category_ids: &QStringList) {
        let ids: Vec<String> = category_ids.iter().map(String::from).collect();
        with_hub_layout_mut(|layout| layout.reset(&ids));
        let next = self.revision.wrapping_add(1);
        self.as_mut().rust_mut().revision = next;
        self.as_mut().revision_changed();
    }

    fn is_unseeded(&self) -> bool {
        with_hub_layout_read(CoreHubLayout::is_unseeded)
    }

    fn items_snapshot(&self) -> QString {
        let text = with_hub_layout_read(|layout| {
            layout
                .visible()
                .map(|item| {
                    [
                        item.kind_raw.as_str(),
                        item.id.as_str(),
                        item.path.as_str(),
                        item.script.as_str(),
                        item.name.as_str(),
                        item.icon.as_str(),
                        item.system.as_str(),
                    ]
                    .join(&FIELD_SEP.to_string())
                })
                .collect::<Vec<_>>()
                .join(&RECORD_SEP.to_string())
        });
        QString::from(text.as_str())
    }

    fn run_script(self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        if text.trim().is_empty() {
            return;
        }
        let store = global_store();
        global_handle().spawn(async move {
            if let Err(e) = store
                .run_mutation::<RunMutation>(RunParams { text: text.clone() })
                .await
            {
                warn!("hub zapscript run failed for \"{text}\": {}", e.message);
                report_action_error("launch", text);
            }
        });
    }
}

/// Same override-then-bundled-logo priority `systems.rs::rows_for_category`
/// uses, minus the live catalog row it doesn't need here.
fn system_cover_key(id: &str) -> String {
    let region = crate::system_region::current_region();
    crate::image_overrides::override_path("systems", id).map_or_else(
        || {
            format!(
                "systems/{}",
                crate::system_logos::logo_artwork_stem(id, region)
            )
        },
        |p| format!("custom-image/{}", p.display()),
    )
}

/// Same override-then-localized priority `systems.rs::rows_for_category`
/// uses; falls back to the bare id (rather than a Core catalog name, which
/// needs a live row this function doesn't have).
fn system_display_name(id: &str) -> String {
    let region = crate::system_region::current_region();
    crate::system_name_overrides::lookup(id)
        .or_else(|| crate::system_names::localized_name(id, region))
        .unwrap_or_else(|| id.to_string())
}
