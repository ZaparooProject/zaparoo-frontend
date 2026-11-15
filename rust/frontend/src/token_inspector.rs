// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Read a token: the Readers page's token inspector. While the page is open
// ZapScript must not run, so it holds Core's `settings.zapscript.hold` on
// a second connection of its own, which lives exactly as long as the page:
// closing the page drops the connection and Core lets ZapScript run again.
// A read token is shown with what `tokens.resolve` says it would do.

use std::sync::Arc;

use zaparoo_app::readers::{self as readers_rules, Inspector, Token};
use zaparoo_core::client::{Client, ConnectionState};
use zaparoo_core::media_types::RunParams;
use zaparoo_core::readers_types::{NewMappingParams, TokenResolution};

use crate::game_picker::{PickedGame, Purpose};
use crate::router::{lock, Ctx, Shared};
use crate::{App, SettingsPage};
use slint::{ComponentHandle, SharedString};

#[derive(Default)]
pub struct Model {
    /// The connection holding `ZapScript` off, while the page is open.
    hold: Option<Arc<Client>>,
    /// What the last token read resolves to, since the page opened.
    resolution: Option<TokenResolution>,
    /// Bumped on open and close, so a late answer for a page that was
    /// left is dropped.
    ticket: u64,
    /// Bumped on every read, so an older read's late answer never
    /// replaces a newer one's.
    read: u64,
    /// Keeps the hold on that connection, again after each reconnect.
    holder: Option<tokio::task::JoinHandle<()>>,
}

impl Model {
    fn open(&self) -> bool {
        self.hold.is_some()
    }
}

fn connected(shared: &Shared) -> Vec<&zaparoo_core::media_types::ReaderInfo> {
    shared.readers.iter().filter(|r| r.connected).collect()
}

/// The reader a token was read on.
fn token_reader<'a>(
    shared: &'a Shared,
    resolution: &TokenResolution,
) -> Option<&'a zaparoo_core::media_types::ReaderInfo> {
    let id = resolution.token.reader_id.as_str();
    if id.is_empty() {
        return None;
    }
    shared
        .readers
        .iter()
        .find(|r| r.reader_id == id || r.id == id)
}

pub(crate) fn fill_inputs(shared: &Shared, inputs: &mut readers_rules::Inputs) {
    let model = &shared.core.inspector;
    inputs.inspector = match &model.resolution {
        Some(_) => Inspector::Scanned,
        None if connected(shared).is_empty() => Inspector::NoReader,
        None => Inspector::Waiting,
    };
    if let Some(resolution) = &model.resolution {
        let writable = token_reader(shared, resolution)
            .is_some_and(|r| r.capabilities.iter().any(|c| c == "write"));
        let database = resolution
            .mapping
            .as_ref()
            .is_some_and(|m| m.source == "database");
        inputs.token = Token {
            launchable: !resolution.redacted && !resolution.script.is_empty(),
            writable,
            has_id: !resolution.token.uid.is_empty(),
            database_mapping: database,
            mapped: resolution.mapping.is_some(),
        };
    }
}

pub(crate) fn current_value(shared: &Shared, id: &str) -> Option<String> {
    let model = &shared.core.inspector;
    Some(match id {
        "inspectorReaders" => connected(shared).len().to_string(),
        "inspectorState" => if connected(shared).is_empty() {
            "noReader"
        } else {
            "waiting"
        }
        .to_string(),
        "tokenValue" => {
            let resolution = model.resolution.as_ref()?;
            if resolution.redacted {
                "redacted".to_string()
            } else if resolution.token.text.is_empty() {
                "none".to_string()
            } else {
                String::new()
            }
        }
        "tokenResolves" => {
            let resolution = model.resolution.as_ref()?;
            if resolution.media.is_some() || !resolution.script.is_empty() {
                String::new()
            } else {
                "nothing".to_string()
            }
        }
        _ => return None,
    })
}

/// The driver an inspector row is about, so the view names it.
pub(crate) fn field_driver(shared: &Shared, id: &str) -> Option<String> {
    match id {
        "inspectorReaders" => {
            let readers = connected(shared);
            let drivers: Vec<&str> = readers.iter().map(|r| r.driver.as_str()).collect();
            readers_rules::reader_count_line(&drivers)
                .1
                .map(str::to_string)
        }
        "tokenReader" => {
            let resolution = shared.core.inspector.resolution.as_ref()?;
            token_reader(shared, resolution).map(|r| r.driver.clone())
        }
        _ => None,
    }
}

pub(crate) fn value_name(shared: &Shared, id: &str) -> Option<SharedString> {
    let model = &shared.core.inspector;
    let driver_name = |driver: &str| -> String {
        shared
            .core
            .drivers
            .iter()
            .find(|d| d.id == driver)
            .map_or(driver.to_string(), |d| d.description.clone())
    };
    if id == "inspectorReaders" {
        let readers = connected(shared);
        let drivers: Vec<&str> = readers.iter().map(|r| r.driver.as_str()).collect();
        let (_, one) = readers_rules::reader_count_line(&drivers);
        return Some(one.map(driver_name).unwrap_or_default().into());
    }
    let resolution = model.resolution.as_ref()?;
    let token = &resolution.token;
    Some(match id {
        "tokenTime" => token.scan_time.as_str().into(),
        "tokenId" => token.uid.as_str().into(),
        "tokenType" => token.kind.as_str().into(),
        "tokenValue" => token.text.as_str().into(),
        "tokenReader" => token_reader(shared, resolution)
            .map_or_else(|| token.reader_id.clone(), |r| driver_name(&r.driver))
            .into(),
        "tokenResolves" => match &resolution.media {
            Some(media) if !media.system_name.is_empty() => {
                format!("{} ({})", media.name, media.system_name).into()
            }
            Some(media) => media.name.as_str().into(),
            None => resolution.script.as_str().into(),
        },
        "tokenMapping" => resolution
            .mapping
            .as_ref()
            .map(|m| {
                if m.label.is_empty() {
                    m.id.clone()
                } else {
                    m.label.clone()
                }
            })
            .unwrap_or_default()
            .into(),
        _ => return None,
    })
}

/// The page opens: hold `ZapScript` off on a connection of its own.
pub(crate) fn open(ctx: &Ctx, app: &App) {
    let (client, ticket) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.core.inspector;
        if model.open() {
            return;
        }
        model.ticket = model.ticket.wrapping_add(1);
        model.resolution = None;
        let Some(transport) = ctx.store.client().current_transport() else {
            return;
        };
        let client = Client::with_transport(transport, &ctx.handle);
        model.hold = Some(client.clone());
        (client, model.ticket)
    };
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let holder = ctx.handle.spawn(async move {
        let mut state = client.connection.subscribe();
        loop {
            // A hold lasts as long as the link it was made on, so it is
            // asked for again each time the connection comes up.
            while *state.borrow_and_update() != ConnectionState::Connected {
                if state.changed().await.is_err() {
                    return;
                }
            }
            if !hold(&client).await {
                let _ = weak.upgrade_in_event_loop(move |app| hold_failed(&ctx2, &app, ticket));
                return;
            }
            loop {
                if state.changed().await.is_err() {
                    return;
                }
                if *state.borrow_and_update() != ConnectionState::Connected {
                    break;
                }
            }
        }
    });
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.core.inspector;
    if model.ticket == ticket {
        model.holder = Some(holder);
    } else {
        holder.abort();
    }
}

/// Ask Core to hold `ZapScript` off; one retry covers a refusal while the
/// connection settles.
async fn hold(client: &Client) -> bool {
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        match client.settings_zapscript_hold().await {
            Ok(()) => return true,
            Err(error) => tracing::warn!("settings.zapscript.hold failed: {}", error.message),
        }
    }
    false
}

/// Core would not hold `ZapScript` off: a token read now would run, so
/// the page closes and says so.
fn hold_failed(ctx: &Ctx, app: &App, ticket: u64) {
    {
        let shared = lock(&ctx.shared);
        let model = &shared.core.inspector;
        if model.ticket != ticket || !model.open() {
            return;
        }
    }
    close(ctx);
    if app.global::<crate::SettingsView>().get_page() == SettingsPage::TokenInspector {
        crate::settings::return_to_row(ctx, app, SettingsPage::Readers, "readToken");
    }
    crate::router::report_action_error(ctx, app, "setting", "");
}

/// The page closed: drop the connection, and with it the hold.
pub(crate) fn close(ctx: &Ctx) {
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.core.inspector;
    model.ticket = model.ticket.wrapping_add(1);
    if let Some(holder) = model.holder.take() {
        holder.abort();
    }
    model.hold = None;
    model.resolution = None;
}

/// Core reported a token read: ask what it resolves to, while the page is
/// open.
pub(crate) fn token_added(ctx: &Ctx, app: &App, reader_id: &str) {
    let (ticket, read) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.core.inspector;
        if !model.open() {
            return;
        }
        model.read = model.read.wrapping_add(1);
        (model.ticket, model.read)
    };
    let client = ctx.store.client();
    let reader = (!reader_id.is_empty()).then(|| reader_id.to_string());
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.tokens_resolve(reader.as_deref()).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.core.inspector;
                if model.ticket != ticket || model.read != read || !model.open() {
                    return;
                }
                match result {
                    Ok(resolution) => model.resolution = Some(resolution),
                    Err(error) => {
                        tracing::warn!("tokens.resolve failed: {}", error.message);
                        return;
                    }
                }
            }
            refresh(&ctx, &app);
        });
    });
}

/// The readers changed: the reader line and state follow.
pub(crate) fn readers_changed(ctx: &Ctx, app: &App) {
    if lock(&ctx.shared).core.inspector.open() {
        refresh(&ctx.clone(), app);
    }
}

fn refresh(ctx: &Ctx, app: &App) {
    if app.global::<crate::SettingsView>().get_page() != SettingsPage::TokenInspector {
        return;
    }
    crate::settings::reseat(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// Accept on one of the inspector's action rows.
pub(crate) fn accept(ctx: &Ctx, app: &App, id: &str) -> bool {
    let resolution = lock(&ctx.shared).core.inspector.resolution.clone();
    match id {
        "launchToken" => {
            let Some(resolution) = resolution.filter(|r| !r.redacted) else {
                return true;
            };
            // ZapScript is held off while the page is open: let it go
            // before asking Core to run, then leave for the Readers page.
            close(ctx);
            crate::settings::return_to_row(ctx, app, SettingsPage::Readers, "readToken");
            let client = ctx.store.client();
            let ctx2 = ctx.clone();
            let weak = app.as_weak();
            ctx.handle.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let result = client
                    .run(RunParams {
                        text: resolution.script.clone(),
                    })
                    .await;
                if let Err(error) = result {
                    tracing::warn!("run failed: {}", error.message);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        crate::router::report_action_error(&ctx2, &app, "launch", "");
                    });
                }
            });
        }
        "writeToken" => {
            let reader = resolution
                .map(|r| r.token.reader_id)
                .filter(|id| !id.is_empty());
            crate::game_picker::open(ctx, app, Purpose::WriteToken { reader_id: reader });
        }
        "mapToken" => {
            let Some(uid) = resolution.map(|r| r.token.uid).filter(|u| !u.is_empty()) else {
                return true;
            };
            let retry = {
                let uid = uid.clone();
                move |ctx: &Ctx, app: &App| {
                    crate::game_picker::open(ctx, app, Purpose::MapToken { uid });
                }
            };
            if crate::core_settings::admitted(ctx, app, retry) {
                crate::game_picker::open(ctx, app, Purpose::MapToken { uid });
            }
        }
        "removeMapping" => {
            let Some(mapping) = resolution.and_then(|r| r.mapping) else {
                return true;
            };
            let Ok(id) = mapping.id.parse::<i64>() else {
                return true;
            };
            let name = if mapping.label.is_empty() {
                mapping.id.clone()
            } else {
                mapping.label.clone()
            };
            let retry = {
                let name = name.clone();
                move |ctx: &Ctx, app: &App| {
                    crate::core_settings::confirm_delete_mapping(ctx, app, id, &name);
                }
            };
            if crate::core_settings::admitted(ctx, app, retry) {
                crate::core_settings::confirm_delete_mapping(ctx, app, id, &name);
            }
        }
        _ => return false,
    }
    true
}

/// A mapping was deleted: what the token resolves to changed.
pub(crate) fn mapping_changed(ctx: &Ctx, app: &App) {
    let reader = lock(&ctx.shared)
        .core
        .inspector
        .resolution
        .as_ref()
        .map(|r| r.token.reader_id.clone());
    if let Some(reader) = reader {
        token_added(ctx, app, &reader);
    }
}

/// The Game picker's answer for Map to a game: an exact match on the
/// token's ID that launches the game.
pub fn picked(ctx: &Ctx, app: &App, uid: &str, game: Option<PickedGame>) {
    let Some(game) = game else { return };
    let params = NewMappingParams {
        label: game.name.clone(),
        kind: "id".to_string(),
        match_kind: "exact".to_string(),
        pattern: uid.to_string(),
        r#override: game.zapscript.clone(),
        enabled: true,
    };
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.mappings_new(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = result {
                tracing::warn!("mappings.new failed: {}", error.message);
                crate::router::report_action_error(&ctx2, &app, "mapping", "");
                return;
            }
            mapping_changed(&ctx2, &app);
        });
    });
}
