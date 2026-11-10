// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Core's UI events (`ui`, `ui.changed`, `ui.respond`): the pickers,
//! confirmations, loaders and notices Core asks every client to show. On
//! `MiSTer`, while Main reports this frontend active, Core shows none of
//! them itself, so this is the only place they appear.
//!
//! A picker is its own modal panel (`CoreUiView`), a confirmation the
//! decision dialog (`DialogKind::CoreConfirm`), and a loader or notice the
//! header status line. Core owns the event: it is never persisted, it is
//! asked for again on every connect, after the boot curtain lifts and when
//! the frontend comes back from dormancy, and it closes here when Core
//! says it is resolved, whoever answered it.

use std::sync::Arc;

use slint::{ComponentHandle as _, Model as _, ModelRc, SharedString, VecModel};
use zaparoo_app::covers::Fit;
use zaparoo_app::status_line::CoreLine;
use zaparoo_core::client::{Client, ConnectionState};
use zaparoo_core::input_actions::actions;
use zaparoo_core::ui_events::{UiAction, UiChoice, UiEvent, UiEventKind, UiRespondParams, UiState};

use crate::media_cache::MediaKey;
use crate::router::{lock, Ctx};
use crate::{App, CoreUiKind, CoreUiView, DialogButton, DialogKind, Overlays, Shell};

/// What Core's event state is and what of it is on screen.
#[derive(Debug, Default)]
pub struct Model {
    revisions: zaparoo_app::core_ui::Revisions,
    /// Core's newest state, kept while the frontend cannot show it (the
    /// boot curtain, dormancy) and applied when it can.
    latest: Option<UiState>,
    /// The event on screen.
    shown: Option<Shown>,
    /// The event this frontend answered. Its closing `ui.changed` may come
    /// after a state that still lists it, and it must not open again.
    answered: Option<String>,
    /// The last event Core was told is on screen (`ui.shown`), so a picker
    /// that closes and opens again is not reported twice.
    reported: Option<String>,
    /// The picker's choices, in the order its rows show them.
    choices: Vec<UiChoice>,
    /// The picker's cover box.
    fit: Fit,
    /// The cover the picker is showing or waiting for.
    cover: Option<MediaKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    id: String,
    kind: UiEventKind,
    dismissible: bool,
}

/// Watch `ui.changed`, ask for the state on every connect, and take the
/// picker's cover box.
pub fn bind(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    {
        let mut rx = client.subscribe_notifications();
        let weak = app.as_weak();
        let ctx = ctx.clone();
        ctx.handle.clone().spawn(async move {
            loop {
                let state = match rx.recv().await {
                    Ok(notification) if notification.method == "ui.changed" => {
                        match serde_json::from_value::<UiState>(notification.params) {
                            Ok(state) => Some(state),
                            Err(e) => {
                                tracing::debug!("ui.changed not understood: {e}");
                                None
                            }
                        }
                    }
                    Ok(_) => continue,
                    // A missed notification may have been this one: ask.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                };
                let ctx = ctx.clone();
                let _ = weak.upgrade_in_event_loop(move |app| match state {
                    Some(state) => apply_state(&ctx, &app, state),
                    None => query(&ctx, &app),
                });
            }
        });
    }
    {
        let mut rx = client.connection.subscribe();
        let weak = app.as_weak();
        let ctx = ctx.clone();
        ctx.handle.clone().spawn(async move {
            let mut connected = false;
            loop {
                let now = matches!(*rx.borrow_and_update(), ConnectionState::Connected);
                if now && !connected {
                    let ctx = ctx.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        // A new connection may be a restarted Core that
                        // counts its revisions from zero again.
                        lock(&ctx.shared).core_ui.revisions.reset();
                        query(&ctx, &app);
                    });
                }
                connected = now;
                if rx.changed().await.is_err() {
                    return;
                }
            }
        });
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.global::<CoreUiView>().on_art_box(move |width, height| {
        // A turn later: the view reports its box while it is being laid
        // out, which can be inside this module's own call.
        let ctx = ctx.clone();
        let weak = weak.clone();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            let Some(app) = weak.upgrade() else { return };
            let fit = Fit::new(width, height);
            {
                let mut shared = lock(&ctx.shared);
                if shared.core_ui.fit == fit {
                    return;
                }
                shared.core_ui.fit = fit;
            }
            show_cover(&ctx, &app);
        });
    });
}

/// Ask Core for its current state.
pub fn query(ctx: &Arc<Ctx>, app: &App) {
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        match client.ui().await {
            Ok(state) => {
                let _ = weak.upgrade_in_event_loop(move |app| apply_state(&ctx, &app, state));
            }
            // An older Core without UI events answers with an error; there
            // is nothing to show either way.
            Err(e) => tracing::debug!("ui query failed: {}", e.message),
        }
    });
}

pub(crate) fn apply_state(ctx: &Ctx, app: &App, state: UiState) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.core_ui;
        if !model.revisions.accept(state.revision) {
            return;
        }
        // Once Core no longer lists the answered event, it is over.
        if model
            .answered
            .as_ref()
            .is_some_and(|id| !state.events.iter().any(|event| &event.id == id))
        {
            model.answered = None;
        }
        model.latest = Some(state);
    }
    present(ctx, app);
}

/// Bring the screen in line with Core's newest state: close what Core no
/// longer lists, show what it does. Waits while the boot curtain is up or
/// the frontend is dormant; the curtain's lift and the return from
/// dormancy call here again.
pub fn present(ctx: &Ctx, app: &App) {
    let shell = app.global::<Shell>();
    if shell.get_boot_curtain() || shell.get_dormant() {
        return;
    }
    let (current, shown) = {
        let shared = lock(&ctx.shared);
        let model = &shared.core_ui;
        let current = model
            .latest
            .as_ref()
            .and_then(UiState::current)
            .filter(|event| model.answered.as_ref() != Some(&event.id))
            .cloned();
        (current, model.shown.clone())
    };
    if let Some(shown) = shown {
        if current
            .as_ref()
            .is_none_or(|event| event.id != shown.id || event.kind != shown.kind)
        {
            close_shown(ctx, app);
        }
    }
    if let Some(event) = current {
        show(ctx, app, &event);
    }
}

/// A dialog closed. A confirmation that waited for the dialog takes it.
pub fn surface_free(ctx: &Ctx, app: &App) {
    present(ctx, app);
}

fn show(ctx: &Ctx, app: &App, event: &UiEvent) {
    let already = lock(&ctx.shared)
        .core_ui
        .shown
        .as_ref()
        .is_some_and(|shown| shown.id == event.id);
    match event.kind {
        UiEventKind::Loader | UiEventKind::Notice => {
            let text = zaparoo_app::core_ui::line_text(&event.title, &event.message).to_string();
            let line = if event.kind == UiEventKind::Loader {
                CoreLine::Loader(text)
            } else {
                CoreLine::Notice(text)
            };
            // A loader's text may change while it stays open.
            crate::status::set_core_line(&ctx.status, app, &ctx.handle, Some(line));
            app.global::<CoreUiView>()
                .set_kind(if event.kind == UiEventKind::Loader {
                    CoreUiKind::Loader
                } else {
                    CoreUiKind::Notice
                });
        }
        UiEventKind::Picker => {
            if already {
                return;
            }
            show_picker(ctx, app, event);
            wake(ctx, app);
        }
        UiEventKind::Confirm => {
            if already {
                return;
            }
            let overlays = app.global::<Overlays>();
            // One dialog at a time: a confirmation waits for the one up now.
            if overlays.get_dialog_open() {
                return;
            }
            let buttons: &[DialogButton] = if event.dismissible {
                &[DialogButton::Ok, DialogButton::Cancel]
            } else {
                &[DialogButton::Ok]
            };
            crate::router::open_dialog(
                app,
                DialogKind::CoreConfirm,
                event.title.trim(),
                event.message.trim(),
                buttons,
                0,
            );
            app.global::<CoreUiView>().set_kind(CoreUiKind::Confirm);
            wake(ctx, app);
        }
        UiEventKind::Unknown => return,
    }
    let report = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.core_ui;
        model.shown = Some(Shown {
            id: event.id.clone(),
            kind: event.kind,
            dismissible: event.dismissible,
        });
        let wants_answer = matches!(event.kind, UiEventKind::Picker | UiEventKind::Confirm);
        let first = model.reported.as_ref() != Some(&event.id);
        if wants_answer && first {
            model.reported = Some(event.id.clone());
        }
        wants_answer && first
    };
    if report {
        report_shown(ctx, event.id.clone());
    }
}

/// Tell Core an event that wants an answer is on screen now (`ui.shown`),
/// so its time to answer starts here rather than when Core posted it. An
/// older Core does not know the method; that is only logged.
fn report_shown(ctx: &Ctx, id: String) {
    let client = ctx.store.client();
    ctx.handle.spawn(async move {
        if let Err(e) = client.ui_shown(&id).await {
            tracing::debug!("ui.shown failed: {}", e.message);
        }
    });
}

/// An event that wants an answer brings the screen back from the
/// screensaver.
fn wake(ctx: &Ctx, app: &App) {
    crate::screensaver::disarm(app);
    crate::router::reset_idle(ctx, app);
}

fn show_picker(ctx: &Ctx, app: &App, event: &UiEvent) {
    let rows: Vec<crate::CorePickerRow> = {
        let shared = lock(&ctx.shared);
        event
            .choices
            .iter()
            .map(|choice| crate::CorePickerRow {
                label: SharedString::from(choice.label.trim()),
                detail: choice
                    .media
                    .as_ref()
                    .map_or_else(SharedString::default, |media| {
                        SharedString::from(crate::systems::display_name(&shared, &media.system))
                    }),
            })
            .collect()
    };
    let ids: Vec<&str> = event
        .choices
        .iter()
        .map(|choice| choice.id.as_str())
        .collect();
    let index = zaparoo_app::core_ui::initial_choice(&ids, &event.selected_choice_id);
    let has_art = event.choices.iter().any(|choice| choice.media.is_some());
    lock(&ctx.shared).core_ui.choices.clone_from(&event.choices);
    let view = app.global::<CoreUiView>();
    view.set_title(SharedString::from(event.title.trim()));
    view.set_rows(ModelRc::new(VecModel::from(rows)));
    view.set_index(i32::try_from(index).unwrap_or(0));
    view.set_dismissible(event.dismissible);
    view.set_has_art(has_art);
    view.set_kind(CoreUiKind::Picker);
    show_cover(ctx, app);
}

/// Take whatever is on screen down without answering it.
fn close_shown(ctx: &Ctx, app: &App) {
    let shown = {
        let mut shared = lock(&ctx.shared);
        shared.core_ui.choices.clear();
        shared.core_ui.cover = None;
        shared.core_ui.shown.take()
    };
    let Some(shown) = shown else { return };
    let view = app.global::<CoreUiView>();
    let asked = asking(app);
    view.set_kind(CoreUiKind::None);
    match shown.kind {
        UiEventKind::Loader | UiEventKind::Notice => {
            crate::status::set_core_line(&ctx.status, app, &ctx.handle, None);
        }
        UiEventKind::Picker => {
            crate::press_feedback::cancel(app);
            view.set_rows(ModelRc::new(VecModel::from(
                Vec::<crate::CorePickerRow>::new(),
            )));
            view.set_has_cover(false);
            view.set_cover(slint::Image::default());
        }
        UiEventKind::Confirm => {
            let overlays = app.global::<Overlays>();
            if overlays.get_dialog_open() && overlays.get_dialog_kind() == DialogKind::CoreConfirm {
                crate::router::close_dialog(app);
                crate::router::dialog_surface_freed(ctx, app);
            }
        }
        UiEventKind::Unknown => {}
    }
    // The screensaver waited out the question; its clock starts again now.
    if asked {
        crate::router::reset_idle(ctx, app);
    }
}

/// Core is waiting on an answer from what is on screen: the screensaver
/// must not cover it, or the waking press is swallowed while the question's
/// time runs out.
pub fn asking(app: &App) -> bool {
    matches!(
        app.global::<CoreUiView>().get_kind(),
        CoreUiKind::Picker | CoreUiKind::Confirm
    )
}

/// The picker is on screen and takes input.
pub fn picker_open(app: &App) -> bool {
    app.global::<CoreUiView>().get_kind() == CoreUiKind::Picker
}

/// Input while the picker is up. It owns every press: Core asked for an
/// answer, and nothing under it may act until there is one.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let view = app.global::<CoreUiView>();
    let len = view.get_rows().row_count();
    let index = usize::try_from(view.get_index()).unwrap_or(0);
    match action {
        actions::UP | actions::DOWN => {
            let next = zaparoo_app::core_ui::step(len, index, action == actions::DOWN);
            focus(ctx, app, next);
        }
        actions::ACCEPT => {
            let choice = lock(&ctx.shared)
                .core_ui
                .choices
                .get(index)
                .map(|choice| choice.id.clone());
            if let Some(choice) = choice {
                respond(ctx, app, UiAction::Select, choice);
            }
        }
        actions::CANCEL if dismissible(ctx) => {
            respond(ctx, app, UiAction::Dismiss, String::new());
        }
        _ => {}
    }
}

/// Move the picker's focus, from a press or the pointer.
pub fn focus(ctx: &Ctx, app: &App, index: usize) {
    let view = app.global::<CoreUiView>();
    if index >= view.get_rows().row_count() {
        return;
    }
    let index = i32::try_from(index).unwrap_or(0);
    if view.get_index() == index {
        return;
    }
    view.set_index(index);
    show_cover(ctx, app);
}

fn dismissible(ctx: &Ctx) -> bool {
    lock(&ctx.shared)
        .core_ui
        .shown
        .as_ref()
        .is_some_and(|shown| shown.dismissible)
}

/// The confirmation dialog was answered: A on OK confirms, A on Cancel or
/// Back dismisses. A confirmation Core will not let go of stays up on
/// Back.
pub fn confirm_answered(ctx: &Ctx, app: &App, confirmed: bool) {
    if confirmed {
        respond(ctx, app, UiAction::Confirm, String::new());
    } else if dismissible(ctx) {
        respond(ctx, app, UiAction::Dismiss, String::new());
    }
}

/// Answer the event on screen. It closes at once; Core's `ui.changed`
/// confirms it, and an answer Core refuses (the event expired or another
/// client answered first) leaves nothing more to do.
fn respond(ctx: &Ctx, app: &App, action: UiAction, choice_id: String) {
    let Some(id) = lock(&ctx.shared)
        .core_ui
        .shown
        .as_ref()
        .map(|shown| shown.id.clone())
    else {
        return;
    };
    lock(&ctx.shared).core_ui.answered = Some(id.clone());
    close_shown(ctx, app);
    let client = ctx.store.client();
    ctx.handle.spawn(async move {
        if let Err(e) = client
            .ui_respond(UiRespondParams {
                id,
                action,
                choice_id,
            })
            .await
        {
            tracing::debug!("ui.respond failed: {}", e.message);
        }
    });
}

/// Show the focused choice's cover, or ask for it.
fn show_cover(ctx: &Ctx, app: &App) {
    let view = app.global::<CoreUiView>();
    if view.get_kind() != CoreUiKind::Picker || !view.get_has_art() {
        return;
    }
    let index = usize::try_from(view.get_index()).unwrap_or(0);
    let key = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.core_ui;
        let fit = model.fit;
        let key = model
            .choices
            .get(index)
            .and_then(|choice| choice.media.as_ref())
            .filter(|_| fit.width > 0 && fit.height > 0)
            .map(|media| MediaKey {
                media_id: None,
                system: media.system.clone(),
                path: media.path.clone(),
                max_size: crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
                fit,
                image_type: None,
                folder: false,
                prefer: ctx.media.artwork_for(&media.system, ""),
            });
        model.cover.clone_from(&key);
        key
    };
    view.set_has_cover(false);
    view.set_cover(slint::Image::default());
    let no_media = lock(&ctx.shared)
        .core_ui
        .choices
        .get(index)
        .is_none_or(|choice| choice.media.is_none());
    view.set_cover_absent(no_media || key.as_ref().is_some_and(|key| ctx.media.is_negative(key)));
    let Some(key) = key else { return };
    if let Some(decoded) = ctx.media.get(&key) {
        view.set_cover(slint::Image::from_rgba8(decoded.buffer.clone()));
        view.set_has_cover(true);
    } else if !ctx.media.is_negative(&key) {
        ctx.media.enqueue(key);
    }
}

/// Covers settled: the picker's, if it is one of them.
pub fn covers_landed(ctx: &Ctx, app: &App, keys: &[MediaKey]) {
    let wanted = lock(&ctx.shared).core_ui.cover.clone();
    let Some(wanted) = wanted.filter(|wanted| keys.contains(wanted)) else {
        return;
    };
    let view = app.global::<CoreUiView>();
    if let Some(decoded) = ctx.media.get(&wanted) {
        view.set_cover(slint::Image::from_rgba8(decoded.buffer.clone()));
        view.set_has_cover(true);
    } else if ctx.media.is_negative(&wanted) {
        view.set_cover_absent(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_connection_accepts_a_restarted_cores_low_revision() {
        let mut model = Model::default();
        assert!(model.revisions.accept(40));
        assert!(!model.revisions.accept(3));
        model.revisions.reset();
        assert!(model.revisions.accept(3));
    }
}
