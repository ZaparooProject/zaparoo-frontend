// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Scripted transient (non-task) notifications: a card scan, a playtime
// warning, extra time granted, a limit reached, an inbox message: the
// events the frontend's header status
// line listens for. Off by default (`MOCK_CORE_EVENTS` unset)
// so a plain `just mock-core` session stays quiet; set the env var to
// exercise the header status line's transient-message path without
// waiting on a real reader or a real playtime limit.

use crate::media_state::Notifier;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

const EVENT_INTERVAL: Duration = Duration::from_secs(15);

pub fn events_enabled() -> bool {
    std::env::var_os("MOCK_CORE_EVENTS").is_some()
}

/// Rotates through the transient event kinds this mock emits.
/// Dates are fixed illustrative values (matching the convention already
/// used for `fixtures.rs`'s history fixtures) rather than the real
/// clock — nothing here depends on wall-clock time.
pub async fn run(notifier: Notifier) {
    let mut tick: u64 = 0;
    loop {
        sleep(EVENT_INTERVAL).await;
        tick += 1;
        match tick % 6 {
            1 => notifier_send_tokens_added(&notifier),
            2 => notifier_send_playtime_warning(&notifier),
            3 => notifier_send_blank_token(&notifier),
            4 => notifier_send_playtime_extended(&notifier),
            5 => notifier_send_playtime_reached(&notifier),
            _ => notifier_send_inbox_added(&notifier, tick),
        }
    }
}

fn notifier_send_tokens_added(notifier: &Notifier) {
    let token = json!({
        "type": "nfc",
        "uid": "04a1b2c3d4e5f6",
        "text": "Super Mario World",
        "data": "",
        "scanTime": "2026-04-29T23:00:00Z",
        "readerId": crate::readers::READER_ID,
    });
    crate::readers::remember_token(&token);
    notifier.send("tokens.added", &token);
}

/// A blank token: read, then refused with Core's `blank` category.
fn notifier_send_blank_token(notifier: &Notifier) {
    let token = crate::readers::blank_token();
    crate::readers::remember_token(&token);
    notifier.send("tokens.added", &token);
    notifier.send(
        "run.failed",
        &json!({
            "source": "reader",
            "readerId": crate::readers::READER_ID,
            "category": "blank",
            "message": "The token is blank",
        }),
    );
}

fn notifier_send_playtime_warning(notifier: &Notifier) {
    notifier.send(
        "playtime.limit.warning",
        &json!({
            "interval": "5m",
            "remaining": "4m58s",
        }),
    );
}

/// Extra time granted from another client.
fn notifier_send_playtime_extended(notifier: &Notifier) {
    notifier.send(
        "playtime.extended",
        &json!({
            "mode": "duration",
            "duration": "15m0s",
            "sessionExtension": "15m0s",
        }),
    );
}

/// The frontend shows this only while a game is running; with none it
/// is a launch refusal, which the launch reports itself.
fn notifier_send_playtime_reached(notifier: &Notifier) {
    notifier.send("playtime.limit.reached", &json!({"reason": "session"}));
}

fn notifier_send_inbox_added(notifier: &Notifier, tick: u64) {
    notifier.send(
        "inbox.added",
        &json!({
            "id": tick,
            "title": "Mock inbox message",
            "body": "",
            "severity": 0,
            "createdAt": "2026-04-29T23:00:00Z",
        }),
    );
}
