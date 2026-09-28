// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Library > Online: link this device to a Zaparoo Online account
// and choose, separately, whether its play history is uploaded. Core owns
// the account token and every upload; this module shows Core's state, runs
// the link panel and forwards the user's choices. The panel's rules live in
// `zaparoo_app::online_link`. Every answer from Core lands through one of
// the `observe_*` functions, on the UI thread.

use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle, SharedString};
use zaparoo_app::online_link::{self as rules, Started, Tick};
use zaparoo_core::client::{Client, Notification};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::UpdateSettingsParams;

use crate::router::{lock, Ctx};
use crate::{App, Overlays};

/// What Core last said about the account and upload consent.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Core answered `playtimeSyncEnabled`: this client may change it and
    /// link an account.
    pub available: bool,
    pub linked: bool,
    pub sync_enabled: bool,
}

/// Core's `auth.link.status` notification: the flow's new status. The
/// notification never carries the code or URLs.
pub fn link_status(notification: &Notification) -> Option<String> {
    if notification.method != "auth.link.status" {
        return None;
    }
    notification
        .params
        .get("status")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Seconds from now until an RFC 3339 deadline; zero when unreadable.
fn seconds_until(expires_at: &str) -> i64 {
    let Ok(deadline) =
        time::OffsetDateTime::parse(expires_at, &time::format_description::well_known::Rfc3339)
    else {
        return 0;
    };
    (deadline - time::OffsetDateTime::now_utc()).whole_seconds()
}

/// Ask Core for the account and consent state, then redraw the rows.
pub fn refresh(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let model = match client.settings().await {
            Ok(settings) => {
                let base_url = settings.online_base_url.unwrap_or_default();
                let linked = if base_url.is_empty() {
                    false
                } else {
                    client
                        .settings_auth_status(&base_url)
                        .await
                        .is_ok_and(|status| status.linked)
                };
                Model {
                    available: settings.playtime_sync_enabled.is_some(),
                    linked,
                    sync_enabled: settings.playtime_sync_enabled.unwrap_or(false),
                }
            }
            Err(error) => {
                tracing::debug!("reading Online settings failed: {}", error.message);
                Model::default()
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            lock(&ctx.shared).online = model;
            crate::settings::refresh(&ctx, &app);
        });
    });
}

/// Flip play history upload consent. The row shows the new value at once
/// and goes back if Core refuses.
pub fn toggle_sync(ctx: &Ctx, app: &App) {
    let enabled = {
        let mut shared = lock(&ctx.shared);
        if !shared.online.available {
            return;
        }
        shared.online.sync_enabled = !shared.online.sync_enabled;
        shared.online.sync_enabled
    };
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client
            .settings_update(UpdateSettingsParams {
                playtime_sync_enabled: Some(enabled),
                ..Default::default()
            })
            .await;
        if let Err(error) = result {
            tracing::warn!("changing play history sync failed: {}", error.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                lock(&ctx.shared).online.sync_enabled = !enabled;
                crate::settings::refresh(&ctx, &app);
                crate::router::report_action_error(&ctx, &app, "online", "");
            });
        }
    });
}

/// Accepted the account row: link when unlinked, confirm first to unlink.
pub fn accept_account(ctx: &Ctx, app: &App) {
    if lock(&ctx.shared).online.linked {
        crate::router::confirm_unlink_online(app);
    } else {
        open(ctx, app);
    }
}

/// The user confirmed unlinking.
pub fn unlink(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_auth_unlink().await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = result {
                tracing::warn!("unlinking the Online account failed: {}", error.message);
                crate::router::report_action_error(&ctx, &app, "online", "");
            }
            refresh(&ctx, &app);
        });
    });
}

fn render(ctx: &Ctx, app: &App) {
    let overlays = app.global::<Overlays>();
    let shared = lock(&ctx.shared);
    let session = &shared.online_link;
    let phase = crate::OnlineLinkPhase::from(session.phase);
    overlays.set_online_phase(phase);
    overlays.set_online_code(SharedString::from(session.code.as_str()));
    overlays.set_online_url(SharedString::from(session.url.as_str()));
    overlays.set_online_expires_in(i32::try_from(session.expires_in).unwrap_or(0));
    overlays.set_online_open(phase != crate::OnlineLinkPhase::Closed);
}

/// Tell Core to drop a pending link. Core errors when there is none, which
/// is the outcome wanted anyway.
fn send_cancel(ctx: &Ctx) {
    let client = ctx.store.client();
    ctx.handle.spawn(async move {
        if let Err(error) = client.settings_auth_link_cancel().await {
            tracing::debug!("cancelling the Online link: {}", error.message);
        }
    });
}

/// Open the panel and ask Core for a code. Answers the run's ticket.
pub fn open(ctx: &Ctx, app: &App) -> u64 {
    crate::press_feedback::cancel(app);
    let ticket = lock(&ctx.shared).online_link.begin();
    app.global::<Overlays>().set_online_qr_modules(0);
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_auth_link().await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(link) if link.status == "pending" && !link.user_code.is_empty() => observe_started(
                &ctx,
                &app,
                ticket,
                &link.user_code,
                &link.verification_url,
                &link.verification_url_complete,
                seconds_until(&link.expires_at),
            ),
            Ok(link) => {
                tracing::warn!("Online link did not start: {} {}", link.status, link.error);
                observe_failed(&ctx, &app, ticket);
            }
            Err(error) => {
                tracing::warn!("starting the Online link failed: {}", error.message);
                observe_failed(&ctx, &app, ticket);
            }
        });
    });
    ticket
}

/// Core answered a code. One for a panel nobody is watching is cancelled.
pub(crate) fn observe_started(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    code: &str,
    url: &str,
    url_complete: &str,
    expires_in: i64,
) {
    let seconds = rules::window(expires_in);
    let outcome = lock(&ctx.shared)
        .online_link
        .started(ticket, code, url, url_complete, seconds);
    match outcome {
        Started::Shown => {
            let target = lock(&ctx.shared).online_link.url_complete.clone();
            if let Some((image, modules)) =
                crate::qr::qr_image(&target, crate::qr::code_colors(app))
            {
                let overlays = app.global::<Overlays>();
                overlays.set_online_qr(image);
                overlays.set_online_qr_modules(i32::try_from(modules).unwrap_or(0));
            }
            render(ctx, app);
            arm_countdown(ctx, app, ticket);
        }
        Started::Abandoned => send_cancel(ctx),
    }
}

pub(crate) fn observe_failed(ctx: &Ctx, app: &App, ticket: u64) {
    if lock(&ctx.shared).online_link.failed(ticket) {
        render(ctx, app);
    }
}

/// Core reported the flow's status, by notification or poll.
pub(crate) fn observe_status(ctx: &Ctx, app: &App, status: &str) {
    if !lock(&ctx.shared).online_link.status(status) {
        return;
    }
    render(ctx, app);
    refresh(ctx, app);
}

pub(crate) fn observe_tick(ctx: &Ctx, app: &App, ticket: u64) {
    let tick = lock(&ctx.shared).online_link.tick(ticket);
    match tick {
        Tick::Counting { poll } => {
            render(ctx, app);
            if poll {
                poll_status(ctx, app);
            }
            arm_countdown(ctx, app, ticket);
        }
        Tick::Expired => {
            send_cancel(ctx);
            render(ctx, app);
        }
        Tick::Stale => {}
    }
}

/// A missed notification must not leave an approved link looking pending.
fn poll_status(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        if let Ok(link) = client.settings_auth_link_status().await {
            let _ = weak.upgrade_in_event_loop(move |app| {
                observe_status(&ctx, &app, &link.status);
            });
        }
    });
}

fn arm_countdown(ctx: &Ctx, app: &App, ticket: u64) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = weak.upgrade_in_event_loop(move |app| observe_tick(&ctx, &app, ticket));
    });
}

/// Leave the panel, cancelling a link Core may still hold.
pub fn close(ctx: &Ctx, app: &App) {
    if lock(&ctx.shared).online_link.leave() {
        send_cancel(ctx);
    }
    crate::press_feedback::cancel(app);
    app.global::<Overlays>().set_online_qr_modules(0);
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// The panel owns input while it is up: Back always leaves, Accept only
/// once there is an outcome.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let finished = lock(&ctx.shared).online_link.finished();
    if action == actions::CANCEL || (action == actions::ACCEPT && finished) {
        close(ctx, app);
    }
}

/// Watch for link progress Core pushes.
pub fn bind_events(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    let mut rx = client.subscribe_notifications();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            match rx.recv().await {
                Ok(notification) => {
                    let Some(status) = link_status(&notification) else {
                        continue;
                    };
                    let ctx = ctx.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        observe_status(&ctx, &app, &status);
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{link_status, seconds_until};
    use serde_json::json;
    use zaparoo_core::client::Notification;

    #[test]
    fn only_link_status_notifications_are_read() {
        let n = |method: &str, params: serde_json::Value| Notification {
            method: method.into(),
            params,
        };
        assert_eq!(
            link_status(&n("auth.link.status", json!({"status": "approved"}))),
            Some("approved".to_string())
        );
        assert_eq!(link_status(&n("auth.link.status", json!({}))), None);
        assert_eq!(
            link_status(&n("clients.paired", json!({"status": "x"}))),
            None
        );
    }

    #[test]
    fn unreadable_deadlines_give_no_time() {
        assert_eq!(seconds_until("not a time"), 0);
        assert!(seconds_until("2000-01-01T00:00:00Z") < 0);
        let later = (time::OffsetDateTime::now_utc() + time::Duration::minutes(10))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        assert!((590..=600).contains(&seconds_until(&later)));
    }
}
