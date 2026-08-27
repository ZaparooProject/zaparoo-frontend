// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Per-media launcher override, read and written from the game context
// menu's "Change launcher" entry. Mirrors `system_launchers` in shape
// (same `Default`-first picker list, same "__default__" clear sentinel,
// shared via `system_launchers::{DEFAULT_LAUNCHER_ID, launchers_for_system,
// picker_entries_for_system}") but reads/writes Core's per-media
// `media.meta`/`media.meta.update` instead of the per-system
// `settings`/`settings.update` pair `SystemLaunchers` uses.
//
// `prepare_game` is a "small 1-item query": it checks the process-wide
// `MediaMetaCache` first (a warm neighbor from the list-detail pane or a
// prior menu open answers instantly), and only issues one `media.meta`
// fetch on a cold miss. Nothing here ever runs across a whole page of
// rows — see CLAUDE.md's caching/RAM constraints.

use crate::media_image_cache::MediaKey;
use crate::media_meta_cache::{
    fetch_media_meta_with_path_fallback, global_media_meta_cache, MetaLookup,
};
use crate::models::system_launchers::{
    launchers_for_system, picker_entries_for_system, DEFAULT_LAUNCHER_ID,
};
use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::warn;
use zaparoo_core::endpoints::launchers::LaunchersEndpoint;
use zaparoo_core::media_types::{LauncherInfo, MediaMeta, MediaMetaParams, MediaMetaUpdateParams};
use zaparoo_core::remote_resource::ResourceStatus;

#[derive(Default)]
pub struct GameLauncherOverrideRust {
    loading: bool,
    error_message: QString,
    current_override: QString,
    picker_ids: QStringList,
    picker_labels: QStringList,
    update_pending: bool,
    update_error: QString,
    launchers: Vec<LauncherInfo>,
    seq: Arc<AtomicU64>,
    update_seq: Arc<AtomicU64>,
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
        #[qproperty(bool, loading)]
        #[qproperty(QString, error_message)]
        #[qproperty(QString, current_override)]
        #[qproperty(QStringList, picker_ids)]
        #[qproperty(QStringList, picker_labels)]
        #[qproperty(bool, update_pending)]
        #[qproperty(QString, update_error)]
        type GameLauncherOverride = super::GameLauncherOverrideRust;

        #[qinvokable]
        fn prepare_game(self: Pin<&mut GameLauncherOverride>, system_id: QString, path: QString);

        #[qinvokable]
        fn set_game_launcher(
            self: Pin<&mut GameLauncherOverride>,
            system_id: QString,
            path: QString,
            launcher_id: QString,
        );
    }

    impl cxx_qt::Initialize for GameLauncherOverride {}
    impl cxx_qt::Threading for GameLauncherOverride {}
}

impl Initialize for ffi::GameLauncherOverride {
    fn initialize(mut self: Pin<&mut Self>) {
        // Shares the same cached `RemoteResource` `SystemLaunchers` already
        // subscribes — `Store::subscribe` returns the same `Arc` for equal
        // (endpoint, args), so this is a second cheap watcher, not a second
        // fetch.
        let mut launchers_rx = global_store()
            .subscribe::<LaunchersEndpoint>(())
            .subscribe();
        apply_launchers(self.as_mut(), &launchers_rx.borrow_and_update());
        let qt_thread = self.qt_thread();
        global_handle().spawn(async move {
            while launchers_rx.changed().await.is_ok() {
                let status = launchers_rx.borrow_and_update().clone();
                let _ = qt_thread.queue(move |model| apply_launchers(model, &status));
            }
        });
    }
}

fn apply_launchers(
    mut model: Pin<&mut ffi::GameLauncherOverride>,
    status: &ResourceStatus<zaparoo_core::media_types::LaunchersResult>,
) {
    if let ResourceStatus::Ready(data) = status {
        model
            .as_mut()
            .rust_mut()
            .launchers
            .clone_from(&data.launchers);
    }
}

impl ffi::GameLauncherOverride {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn prepare_game(mut self: Pin<&mut Self>, system_id: QString, path: QString) {
        let system_id = system_id.to_string();
        let path = path.to_string();
        self.as_mut().rust_mut().seq.fetch_add(1, Ordering::SeqCst);
        let ticket = self.rust().seq.load(Ordering::SeqCst);
        self.as_mut().set_current_override(QString::default());
        self.as_mut().set_error_message(QString::default());
        self.as_mut().set_picker_ids(QStringList::default());
        self.as_mut().set_picker_labels(QStringList::default());
        if system_id.trim().is_empty() || path.trim().is_empty() {
            self.as_mut().set_loading(false);
            return;
        }
        self.as_mut().set_loading(true);

        let key = MediaKey::new(system_id.clone(), path.clone());
        match global_media_meta_cache().lookup(&key) {
            MetaLookup::Hit(meta) => {
                let launchers = self.rust().launchers.clone();
                apply_launcher_override(self.as_mut(), &launchers, &system_id, &meta);
                self.as_mut().set_loading(false);
                return;
            }
            MetaLookup::Negative => {
                self.as_mut().set_loading(false);
                return;
            }
            MetaLookup::Miss => {}
        }

        let params = MediaMetaParams::for_media(system_id.clone(), path.clone());
        let seq = self.rust().seq.clone();
        let qt_thread = self.qt_thread();
        let fallback_system = system_id.clone();
        let fallback_path = path.clone();
        global_handle().spawn(async move {
            let result =
                fetch_media_meta_with_path_fallback(params, fallback_system, fallback_path).await;
            global_media_meta_cache().store_fetch_result(key, &result);
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                model.as_mut().set_loading(false);
                match result {
                    Ok(result) => {
                        let launchers = model.rust().launchers.clone();
                        apply_launcher_override(
                            model.as_mut(),
                            &launchers,
                            &system_id,
                            &result.media,
                        );
                    }
                    Err(e) => {
                        warn!(
                            "game launcher override fetch failed for {path}: {}",
                            e.message
                        );
                        model
                            .as_mut()
                            .set_error_message(QString::from(e.message.as_str()));
                    }
                }
            });
        });
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_game_launcher(
        mut self: Pin<&mut Self>,
        system_id: QString,
        path: QString,
        launcher_id: QString,
    ) {
        let system_id = system_id.to_string();
        let path = path.to_string();
        let launcher = launcher_id.to_string();
        let launcher_override = if launcher == DEFAULT_LAUNCHER_ID {
            None
        } else {
            Some(launcher)
        };
        let store = global_store();
        let seq = self.rust().update_seq.clone();
        let ticket = seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.as_mut().set_update_error(QString::default());
        self.as_mut().set_update_pending(true);
        let qt_thread = self.qt_thread();
        let cache_key = MediaKey::new(system_id.clone(), path.clone());
        let params =
            MediaMetaUpdateParams::for_media(system_id.clone(), path.clone(), launcher_override);
        global_handle().spawn(async move {
            let result = store.client().media_meta_update(params).await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                model.as_mut().set_update_pending(false);
                match result {
                    Ok(result) => {
                        model.as_mut().set_update_error(QString::default());
                        global_media_meta_cache().store(cache_key, Some(result.media.clone()));
                        let launchers = model.rust().launchers.clone();
                        apply_launcher_override(
                            model.as_mut(),
                            &launchers,
                            &system_id,
                            &result.media,
                        );
                    }
                    Err(e) => {
                        warn!("game launcher override update failed: {}", e.message);
                        model
                            .as_mut()
                            .set_update_error(QString::from(e.message.as_str()));
                    }
                }
            });
        });
    }
}

/// Apply a fetched `MediaMeta`'s `launcher_override` to the model's
/// `current_override` and rebuild the picker list against it. Shared by
/// both the read path (`prepare_game`) and the write path
/// (`set_game_launcher`'s success arm, whose `media.meta.update` response
/// carries the same updated shape).
fn apply_launcher_override(
    mut model: Pin<&mut ffi::GameLauncherOverride>,
    launchers: &[LauncherInfo],
    system_id: &str,
    meta: &MediaMeta,
) {
    let current = meta.launcher_override.clone();
    let matching = launchers_for_system(launchers, system_id);
    let entries = picker_entries_for_system(&matching, current.as_deref());
    let mut ids = QStringList::default();
    let mut labels = QStringList::default();
    for entry in entries {
        ids.append(QString::from(entry.id.as_str()));
        labels.append(QString::from(entry.label.as_str()));
    }
    model.as_mut().set_picker_ids(ids);
    model.as_mut().set_picker_labels(labels);
    model
        .as_mut()
        .set_current_override(QString::from(current.unwrap_or_default().as_str()));
}
