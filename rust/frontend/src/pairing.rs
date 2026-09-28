// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The "Pair a device" panel behind Settings > About: asks Core for a PIN,
// shows it while it is worth reading out, names the device that uses it,
// and calls the pairing off on every other way out. The rules (phases,
// stale answers, who owes Core a cancel) live in `zaparoo_app::pairing`;
// this is the Slint adapter and the Core traffic. Every answer from Core
// lands through one of the `observe_*` functions, on the UI thread.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, SharedString};
use zaparoo_app::pairing::{self as rules, Started, Tick};
use zaparoo_core::client::{Client, Notification};
use zaparoo_core::input_actions::actions;

use crate::router::{lock, Ctx};
use crate::{App, Overlays, PairPhase};

/// Core's `clients.paired` notification: the display name of the device
/// that just finished pairing. Nothing else on the notification channel
/// concerns this panel.
pub fn paired_client(notification: &Notification) -> Option<String> {
    if notification.method != "clients.paired" {
        return None;
    }
    Some(
        notification
            .params
            .get("clientName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// Push the session into the panel's globals.
fn render(ctx: &Ctx, app: &App) {
    let overlays = app.global::<Overlays>();
    let shared = lock(&ctx.shared);
    let session = &shared.pairing;
    let phase = PairPhase::from(session.phase);
    overlays.set_pair_phase(phase);
    overlays.set_pair_pin(SharedString::from(session.pin.as_str()));
    overlays.set_pair_expires_in(i32::try_from(session.expires_in).unwrap_or(0));
    overlays.set_pair_client(SharedString::from(session.client_name.as_str()));
    overlays.set_pair_open(phase != PairPhase::Closed);
}

/// Tell Core to drop whatever pairing it still holds. Safe to send when
/// there is none: this is the call that keeps the panel's promise, so it
/// is never made conditional on anything Core says back.
fn send_cancel(ctx: &Ctx) {
    let client = ctx.store.client();
    ctx.handle.spawn(async move {
        if let Err(error) = client.clients_pair_cancel().await {
            tracing::warn!("cancelling the pairing failed: {}", error.message);
        }
    });
}

/// Accepted the row: open the panel and ask Core for a PIN. Answers the
/// ticket this run's replies carry.
pub fn open(ctx: &Ctx, app: &App) -> u64 {
    crate::press_feedback::cancel(app);
    let ticket = lock(&ctx.shared).pairing.begin();
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.clients_pair_start().await;
        if let Err(error) = &result {
            tracing::warn!("starting a pairing failed: {}", error.message);
        }
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(started) => observe_started(&ctx, &app, ticket, &started.pin, started.expires_at),
            Err(_) => observe_failed(&ctx, &app, ticket),
        });
    });
    ticket
}

/// Core minted a PIN. One that arrived for a panel nobody is watching is
/// called off here rather than dropped.
pub(crate) fn observe_started(ctx: &Ctx, app: &App, ticket: u64, pin: &str, expires_at: i64) {
    let seconds = rules::window(expires_at, now_unix());
    let outcome = lock(&ctx.shared).pairing.started(ticket, pin, seconds);
    match outcome {
        Started::Shown => {
            render(ctx, app);
            arm_countdown(ctx, app, ticket);
        }
        Started::Abandoned => send_cancel(ctx),
    }
}

/// Core refused to start a pairing. The panel goes away and the failure
/// takes the alert every other failed action uses.
pub(crate) fn observe_failed(ctx: &Ctx, app: &App, ticket: u64) {
    if !lock(&ctx.shared).pairing.failed(ticket) {
        return;
    }
    render(ctx, app);
    crate::settings::refresh(ctx, app);
    crate::router::report_action_error(ctx, app, "pairing", "");
}

/// A second passed on the PIN: count it down, or retire it and tell Core.
pub(crate) fn observe_tick(ctx: &Ctx, app: &App, ticket: u64) {
    let tick = lock(&ctx.shared).pairing.tick(ticket);
    match tick {
        Tick::Counting(_) => {
            render(ctx, app);
            arm_countdown(ctx, app, ticket);
        }
        Tick::Expired => {
            send_cancel(ctx);
            render(ctx, app);
        }
        Tick::Stale => {}
    }
}

/// A device finished pairing. Only the run holding the PIN it used cares.
pub(crate) fn observe_paired(ctx: &Ctx, app: &App, client_name: &str) {
    if !lock(&ctx.shared).pairing.paired(client_name) {
        return;
    }
    render(ctx, app);
}

fn arm_countdown(ctx: &Ctx, app: &App, ticket: u64) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = weak.upgrade_in_event_loop(move |app| observe_tick(&ctx, &app, ticket));
    });
}

/// Leave the panel, calling the pairing off if Core still holds one.
pub fn close(ctx: &Ctx, app: &App) {
    if lock(&ctx.shared).pairing.leave() {
        send_cancel(ctx);
    }
    crate::press_feedback::cancel(app);
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// The panel owns input while it is up: Back always leaves, and Accept
/// only means anything once there is nothing left to wait for.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let phase = lock(&ctx.shared).pairing.phase;
    let finished = matches!(phase, rules::Phase::Paired | rules::Phase::Expired);
    if action == actions::CANCEL || (action == actions::ACCEPT && finished) {
        close(ctx, app);
    }
}

/// Watch the notification channel for devices completing a pairing. The
/// same subscription the status line takes; this one reads only
/// `clients.paired`.
pub fn bind_events(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    let mut rx = client.subscribe_notifications();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            match rx.recv().await {
                Ok(notification) => {
                    let Some(name) = paired_client(&notification) else {
                        continue;
                    };
                    let ctx = ctx.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        observe_paired(&ctx, &app, &name);
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
    use super::paired_client;
    use serde_json::json;
    use zaparoo_core::client::Notification;

    #[test]
    fn only_a_completed_pairing_names_a_device() {
        let n = |method: &str, params: serde_json::Value| Notification {
            method: method.into(),
            params,
        };
        assert_eq!(
            paired_client(&n(
                "clients.paired",
                json!({"clientId": "c1", "clientName": "Wizzo's phone"})
            )),
            Some("Wizzo's phone".to_string())
        );
        // Core names the client it paired, but the name can be missing;
        // the panel has its own words for that.
        assert_eq!(
            paired_client(&n("clients.paired", json!({"clientId": "c1"}))),
            Some(String::new())
        );
        assert_eq!(paired_client(&n("tokens.added", json!({}))), None);
    }
}
