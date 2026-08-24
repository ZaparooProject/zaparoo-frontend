// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Scripted transient (non-task) notifications: a card scan, a playtime
// warning, an inbox message — the events `Browse.StatusEvents` on the
// frontend side listens for. Off by default (`MOCK_CORE_EVENTS` unset)
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

/// Rotates through the three transient event kinds this mock emits.
/// Dates are fixed illustrative values (matching the convention already
/// used for `fixtures.rs`'s history fixtures) rather than the real
/// clock — nothing here depends on wall-clock time.
pub async fn run(notifier: Notifier) {
    let mut tick: u64 = 0;
    loop {
        sleep(EVENT_INTERVAL).await;
        tick += 1;
        match tick % 3 {
            1 => notifier_send_tokens_added(&notifier),
            2 => notifier_send_playtime_warning(&notifier),
            _ => notifier_send_inbox_added(&notifier, tick),
        }
    }
}

fn notifier_send_tokens_added(notifier: &Notifier) {
    notifier.send(
        "tokens.added",
        &json!({
            "type": "nfc",
            "uid": "04a1b2c3d4e5f6",
            "text": "Super Mario World",
            "scanTime": "2026-04-29T23:00:00Z",
            "readerId": "mock-reader-1",
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
