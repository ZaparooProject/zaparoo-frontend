// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Playtime as the person playing sees it: the time left in the header,
//! the status-line notices for a limit reached or extra time granted, and
//! the Extra time row on Settings > Profiles > Playtime limits. Rules live
//! in `zaparoo_app::playtime`; Core has no notification for the time left
//! changing, so `playtime` is asked again on every event that moves it.

use std::time::{Duration, Instant};

use slint::ComponentHandle;
use zaparoo_app::playtime::{self as rules, Grant, LimitReason, Notice, NoticeGate};
use zaparoo_app::status_line::Event;
use zaparoo_core::client::Notification;
use zaparoo_core::profile_types::{ExtendPlaytimeParams, PlaytimeStatus};

use crate::router::{lock, Ctx};
use crate::App;

#[derive(Debug, Default)]
pub struct Model {
    /// Core's last answer; `None` before one, or from a Core without it.
    pub status: Option<PlaytimeStatus>,
    /// The header's time left as Core reported it, and when.
    remaining: Option<(Duration, Instant)>,
    /// Ticket of the latest `playtime` request: an older answer is dropped.
    seq: u64,
    /// Generation of the armed countdown tick.
    tick: u64,
    gate: NoticeGate,
    /// Our own grant is waiting on Core.
    pub granting: bool,
}

/// A playtime notification, read off Core's params.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming {
    Reached(LimitReason),
    Extended(Grant),
}

pub fn classify(notification: &Notification) -> Option<Incoming> {
    let field = |key: &str| notification.params.get(key).and_then(|v| v.as_str());
    match notification.method.as_str() {
        "playtime.limit.reached" => {
            LimitReason::from_token(field("reason")?).map(Incoming::Reached)
        }
        "playtime.extended" => {
            Grant::from_params(field("mode")?, field("duration")).map(Incoming::Extended)
        }
        _ => None,
    }
}

/// A playtime notification arrived. `game_running` is whether Core had a
/// game running as it did.
pub fn notified(ctx: &Ctx, app: &App, incoming: Incoming, game_running: bool) {
    let dormant = *ctx.dormant.borrow();
    let notice = {
        let mut shared = lock(&ctx.shared);
        let gate = &mut shared.playtime.gate;
        match incoming {
            Incoming::Reached(reason) => gate.reached(reason, game_running, dormant),
            Incoming::Extended(grant) => gate.extended(grant, dormant, Instant::now()),
        }
    };
    show(ctx, app, notice);
    refresh(ctx, app);
}

/// The frontend came back from a game: a notice held while it was away
/// shows now, its dwell starting here, and the time left is asked again.
pub fn resumed(ctx: &Ctx, app: &App) {
    let notice = lock(&ctx.shared).playtime.gate.resume();
    show(ctx, app, notice);
    refresh(ctx, app);
}

fn show(ctx: &Ctx, app: &App, notice: Option<Notice>) {
    let event = match notice {
        Some(Notice::Reached(reason)) => Event::PlaytimeReached(reason),
        Some(Notice::Extended(grant)) => Event::PlaytimeExtended(grant),
        None => return,
    };
    crate::status::observe_event(&ctx.status, app, &ctx.handle, &event);
}

/// Ask Core where playtime stands, and show it.
pub fn refresh(ctx: &Ctx, app: &App) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        shared.playtime.seq = shared.playtime.seq.wrapping_add(1);
        shared.playtime.seq
    };
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.playtime().await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let status = match result {
                Ok(status) => Some(status),
                Err(error) => {
                    tracing::debug!("playtime status failed: {}", error.message);
                    None
                }
            };
            {
                let mut shared = lock(&ctx2.shared);
                let model = &mut shared.playtime;
                if model.seq != seq {
                    return;
                }
                model.remaining = status.as_ref().and_then(|s| {
                    rules::header_remaining(
                        s.limits_enabled,
                        s.session_remaining.as_deref(),
                        s.daily_remaining.as_deref(),
                    )
                    .map(|left| (left, Instant::now()))
                });
                model.status = status;
            }
            publish(&ctx2, &app);
            crate::settings::refresh(&ctx2, &app);
        });
    });
}

/// Show the time left in the header, and arm the tick for the next
/// minute while a game is counting it down.
fn publish(ctx: &Ctx, app: &App) {
    let dormant = *ctx.dormant.borrow();
    let (reading, next, tick) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.playtime;
        model.tick = model.tick.wrapping_add(1);
        let session_active = model.status.as_ref().is_some_and(|s| s.session_active);
        let left = model
            .remaining
            .map(|(left, at)| rules::left(left, at.elapsed(), session_active));
        let next = left
            .filter(|_| session_active && !dormant)
            .and_then(rules::next_tick);
        (left.map(rules::reading), next, model.tick)
    };
    let shell = app.global::<crate::Shell>();
    let (hours, minutes) = reading.map_or((0, -1), |reading| {
        let (hours, minutes) = reading.parts();
        (
            i32::try_from(hours).unwrap_or(i32::MAX),
            i32::try_from(minutes).unwrap_or(0),
        )
    });
    shell.set_playtime_hours(hours);
    shell.set_playtime_minutes(minutes);
    let Some(next) = next else {
        return;
    };
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        tokio::time::sleep(next).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if lock(&ctx2.shared).playtime.tick == tick {
                publish(&ctx2, &app);
            }
        });
    });
}

/// The Extra time row's value.
pub(crate) fn extra_time_value(shared: &crate::router::Shared) -> String {
    let status = shared.playtime.status.as_ref();
    rules::extra_time_value(
        status.and_then(|s| s.session_extension.as_deref()),
        status.and_then(|s| s.session_extended_until.as_deref()),
    )
}

/// Whether the Extra time row has a session to act on.
pub(crate) fn can_extend(shared: &crate::router::Shared) -> bool {
    let model = &shared.playtime;
    !model.granting
        && model
            .status
            .as_ref()
            .is_some_and(|s| rules::can_extend(s.limits_enabled, &s.state))
}

/// An admin chose extra time on the Playtime limits page.
pub fn extend(ctx: &Ctx, app: &App, choice: &str) {
    let Some((mode, duration)) = rules::extend_request(choice) else {
        return;
    };
    {
        let mut shared = lock(&ctx.shared);
        if shared.playtime.granting {
            return;
        }
        shared.playtime.granting = true;
        shared.playtime.gate.begin_own_grant();
    }
    crate::settings::refresh(ctx, app);
    let params = ExtendPlaytimeParams {
        mode: mode.to_string(),
        duration,
        request_id: String::new(),
    };
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.playtime_extend(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx2.shared);
                shared.playtime.granting = false;
                shared
                    .playtime
                    .gate
                    .end_own_grant(result.is_ok(), Instant::now());
            }
            if let Err(error) = result {
                tracing::warn!("playtime extend failed: {}", error.message);
                crate::router::report_action_error(&ctx2, &app, "playtime_extend", "");
            }
            refresh(&ctx2, &app);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zaparoo_app::top_played::PlayTime;

    fn n(method: &str, params: serde_json::Value) -> Notification {
        Notification {
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn classifies_playtime_notifications() {
        assert_eq!(
            classify(&n("playtime.limit.reached", json!({"reason": "daily"}))),
            Some(Incoming::Reached(LimitReason::Daily))
        );
        assert_eq!(
            classify(&n("playtime.limit.reached", json!({"reason": "other"}))),
            None
        );
        assert_eq!(
            classify(&n(
                "playtime.extended",
                json!({"mode": "duration", "duration": "30m0s", "sessionExtension": "30m0s"})
            )),
            Some(Incoming::Extended(Grant::Time(PlayTime::Minutes(30))))
        );
        assert_eq!(
            classify(&n(
                "playtime.extended",
                json!({"mode": "today", "expires": "2026-11-25T00:00:00Z"})
            )),
            Some(Incoming::Extended(Grant::RestOfToday))
        );
        assert_eq!(classify(&n("playtime.limit.warning", json!({}))), None);
    }
}
