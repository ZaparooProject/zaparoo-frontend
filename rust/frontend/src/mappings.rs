// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Mappings page under Readers: Core's token mappings, each with
// Enabled, Change game and Delete. Mappings from a file are Core's to
// keep and offer nothing. New mappings come from the token inspector's
// Map to a game; match types and patterns stay the App's.

use zaparoo_core::readers_types::{Mapping, UpdateMappingParams};

use crate::game_picker::{PickedGame, Purpose};
use crate::router::{lock, Ctx, ListContext};
use crate::App;
use slint::ComponentHandle;

fn mapping_at(ctx: &Ctx, index: usize) -> Option<Mapping> {
    lock(&ctx.shared)
        .core
        .mappings
        .as_ref()
        .and_then(|m| m.get(index))
        .cloned()
}

/// The mapping with this ID, wherever a refetch has moved it.
fn mapping_by_id(ctx: &Ctx, id: &str) -> Option<Mapping> {
    lock(&ctx.shared)
        .core
        .mappings
        .as_ref()
        .and_then(|m| m.iter().find(|m| m.id == id))
        .cloned()
}

/// Accept or Options on a mapping: its actions, unless it is a file's.
pub(crate) fn open_options(ctx: &Ctx, app: &App, index: usize) {
    let Some(mapping) = mapping_at(ctx, index) else {
        return;
    };
    if mapping.read_only {
        return;
    }
    let toggle = if mapping.enabled {
        crate::router::menu_row_keyed("toggle", "mapping:disable", "")
    } else {
        crate::router::menu_row_keyed("toggle", "mapping:enable", "")
    };
    let rows = vec![
        toggle,
        crate::router::menu_row_keyed("change", "mapping:change_game", ""),
        crate::router::menu_row_keyed("delete", "mapping:delete", ""),
    ];
    crate::router::present_list(
        ctx,
        app,
        ListContext::MappingOptions(mapping.id.clone()),
        "title:options",
        rows,
    );
}

/// A mapping's action was picked.
pub(crate) fn option_picked(ctx: &Ctx, app: &App, mapping_id: &str, action: &str) {
    let Some(mapping) = mapping_by_id(ctx, mapping_id) else {
        return;
    };
    if mapping.read_only {
        return;
    }
    let Ok(id) = mapping.id.parse::<i64>() else {
        return;
    };
    match action {
        "toggle" => {
            let on = !mapping.enabled;
            if let Some(m) = lock(&ctx.shared)
                .core
                .mappings
                .as_mut()
                .and_then(|m| m.iter_mut().find(|m| m.id == mapping_id))
            {
                m.enabled = on;
            }
            crate::settings::refresh(ctx, app);
            update(
                ctx,
                app,
                UpdateMappingParams {
                    id,
                    enabled: Some(on),
                    ..UpdateMappingParams::default()
                },
            );
        }
        "change" => crate::game_picker::open(ctx, app, Purpose::ChangeMapping { id }),
        "delete" => {
            crate::core_settings::confirm_delete_mapping(ctx, app, id, &mapping.label);
        }
        _ => {}
    }
}

fn update(ctx: &Ctx, app: &App, params: UpdateMappingParams) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.mappings_update(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = result {
                tracing::warn!("mappings.update failed: {}", error.message);
                crate::router::report_action_error(&ctx2, &app, "mapping", "");
            }
            crate::core_settings::fetch_mappings(&ctx2, &app);
            crate::token_inspector::mapping_changed(&ctx2, &app);
        });
    });
}

/// A delete confirmed.
pub(crate) fn delete(ctx: &Ctx, app: &App, id: i64) {
    let id_text = id.to_string();
    if let Some(list) = lock(&ctx.shared).core.mappings.as_mut() {
        list.retain(|m| m.id != id_text);
    }
    crate::settings::reseat(ctx, app);
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.mappings_delete(id).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = result {
                tracing::warn!("mappings.delete failed: {}", error.message);
                crate::router::report_action_error(&ctx2, &app, "mapping", "delete");
            }
            crate::core_settings::fetch_mappings(&ctx2, &app);
            crate::token_inspector::mapping_changed(&ctx2, &app);
        });
    });
}

/// The Game picker's answer for Change game.
pub fn picked(ctx: &Ctx, app: &App, id: i64, game: Option<PickedGame>) {
    let Some(game) = game else { return };
    update(
        ctx,
        app,
        UpdateMappingParams {
            id,
            label: Some(game.name),
            r#override: Some(game.zapscript),
            ..UpdateMappingParams::default()
        },
    );
}
