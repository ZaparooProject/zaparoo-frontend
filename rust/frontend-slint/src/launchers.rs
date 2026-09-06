// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The two "Change launcher" pickers: the system default
// (`models/system_launchers.rs`) and the per-media override
// (`models/game_launcher_override.rs`). Both build the same list from
// `zaparoo_app::launchers`; they differ only in where the choice is
// read from and written to - Core's settings for a system, the media
// row's own metadata for a game.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::launchers as rules;
use zaparoo_core::media_types::{MediaMetaParams, MediaMetaUpdateParams};

use crate::router::{lock, Ctx, ListContext};
use crate::App;

/// The launcher ids Core offers for a system.
fn launcher_ids(ctx: &Ctx, system_id: &str) -> Vec<String> {
    lock(&ctx.shared)
        .launchers
        .iter()
        .filter(|l| l.system_id == system_id)
        .map(|l| l.id.clone())
        .collect()
}

/// Present the picker for a launcher list and the stored choice.
fn present(ctx: &Ctx, app: &App, context: ListContext, ids: &[String], current: Option<&str>) {
    let rows = rules::picker_rows(ids, current);
    let index = rules::picker_index(&rows, current);
    let entries: Vec<crate::MenuEntry> = rows
        .iter()
        .map(|row| {
            if row.key.is_empty() {
                crate::router::menu_entry(&row.id, &row.id)
            } else {
                crate::router::menu_row_keyed(&row.id, row.key, &row.id)
            }
        })
        .collect();
    lock(&ctx.shared).list_context = context;
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_setting_id(SharedString::default());
    overlays.set_list_title(SharedString::from("title:change_launcher"));
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(i32::try_from(index).unwrap_or(0));
    overlays.set_list_open(true);
}

/// Systems screen: the launcher every game of this system uses unless
/// it overrides it.
pub fn open_system_picker(ctx: &Ctx, app: &App, system_id: &str) {
    let ids = launcher_ids(ctx, system_id);
    let current = lock(&ctx.shared)
        .system_defaults
        .iter()
        .find(|d| d.system == system_id)
        .map(|d| d.launcher.clone())
        .filter(|l| !l.is_empty());
    present(
        ctx,
        app,
        ListContext::SystemLauncher(system_id.to_string()),
        &ids,
        current.as_deref(),
    );
}

/// Persist a system default through the store mutation (which owns the
/// settings invalidation), then mirror it into the local defaults so
/// the next open reflects it immediately.
pub fn set_system_launcher(ctx: &Ctx, app: &App, system_id: &str, launcher_id: &str) {
    use zaparoo_core::endpoints::system_launcher_default::{
        SetSystemLauncherDefaultArgs, SetSystemLauncherDefaultMutation,
    };
    let launcher = rules::stored_value(launcher_id).unwrap_or_default();
    let store = ctx.store.clone();
    let shared = ctx.shared.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let system_id = system_id.to_string();
    ctx.handle.spawn(async move {
        let args = SetSystemLauncherDefaultArgs {
            system_id: system_id.clone(),
            launcher: launcher.clone(),
        };
        match store
            .run_mutation::<SetSystemLauncherDefaultMutation>(args)
            .await
        {
            Ok(()) => {
                let mut guard = lock(&shared);
                if let Some(existing) = guard
                    .system_defaults
                    .iter_mut()
                    .find(|d| d.system == system_id)
                {
                    existing.launcher = launcher;
                } else {
                    guard
                        .system_defaults
                        .push(zaparoo_core::media_types::SystemDefault {
                            system: system_id,
                            launcher,
                            before_exit: String::new(),
                        });
                }
            }
            Err(e) => {
                tracing::warn!("set launcher failed: {}", e.message);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    crate::router::report_action_error(&ctx2, &app, "launcher", "");
                });
            }
        }
    });
}

/// Games screen: the launcher this one game uses. The stored value
/// lives on the media row, so the picker waits for one `media.meta`
/// read (Qt's `prepare_game`) before it can show the current choice.
pub fn open_game_picker(ctx: &Ctx, app: &App, system_id: &str, path: &str, media_id: Option<i64>) {
    let ids = launcher_ids(ctx, system_id);
    if ids.is_empty() {
        return;
    }
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.game_launcher_seq += 1;
        shared.game_launcher_seq
    };
    let client = ctx.store.client();
    let params = MediaMetaParams {
        media_id,
        system: system_id.to_string(),
        path: path.to_string(),
    };
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let system_id = system_id.to_string();
    let path = path.to_string();
    ctx.handle.spawn(async move {
        let current = match client.media_meta(params).await {
            Ok(result) => result.media.launcher_override,
            Err(e) => {
                // A metadata read that fails still leaves a usable
                // picker: it just cannot show which row is current.
                tracing::warn!("launcher override read failed for {path}: {}", e.message);
                None
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            if lock(&ctx2.shared).game_launcher_seq != ticket {
                return;
            }
            present(
                &ctx2,
                &app,
                ListContext::GameLauncher(system_id, path),
                &ids,
                current.as_deref(),
            );
        });
    });
}

/// Write the per-game override (`media.meta.update`); the sentinel row
/// clears it and the game falls back to its system's launcher.
pub fn set_game_launcher(ctx: &Ctx, app: &App, system_id: &str, path: &str, launcher_id: &str) {
    let params = MediaMetaUpdateParams::for_media(
        system_id.to_string(),
        path.to_string(),
        rules::stored_value(launcher_id),
    );
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let path = path.to_string();
    ctx.handle.spawn(async move {
        if let Err(e) = client.media_meta_update(params).await {
            tracing::warn!("launcher override write failed for {path}: {}", e.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                crate::router::report_action_error(&ctx2, &app, "launcher", "");
            });
        }
    });
}
