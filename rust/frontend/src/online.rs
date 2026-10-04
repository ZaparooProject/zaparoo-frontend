// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Online: link this device to a Zaparoo Online account
// and choose, separately, whether its play history is uploaded. Core owns
// the account token and every upload; this module shows Core's state, runs
// the link panel and forwards the user's choices. The panel's rules live in
// `zaparoo_app::online_link`. Every answer from Core lands through one of
// the `observe_*` functions, on the UI thread.

use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle, SharedString};
use zaparoo_app::online_link::{self as rules, Started, Tick};
use zaparoo_app::online_settings::{OnlineFeatureChanges, OnlineFeatures};
use zaparoo_core::client::{Client, ConnectionState, Notification};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::UpdateSettingsParams;

use crate::router::{lock, Ctx};
use crate::{App, Overlays};

/// What Core last said about the account, Warp, and the four consent
/// settings. Account identity (`linked`/`device_name`) and
/// Warp come from `settings.backup.status`'s `remote` side, the same as
/// Core's own TUI reads them, not from a separate auth-status call.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Core answered `playtimeSyncEnabled`: this client may change every
    /// consent setting here and link an account.
    pub available: bool,
    pub linked: bool,
    pub device_name: Option<String>,
    /// "", "available", or "unavailable".
    pub warp_availability: String,
    pub features: OnlineFeatures,
    /// "daily", "weekly", or "manual".
    pub backup_schedule: String,
    /// The remote-control poller's last observation: unknown, disabled,
    /// unlinked, connecting, waiting, `not_remote_device`, unavailable,
    /// `credential_rejected`, or error (empty when the fetch itself failed).
    pub remote_state: String,
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
                // Account identity and Warp come from the backup status
                // call's remote side, the same as Core's own TUI reads
                // them. A failure here still leaves every consent setting
                // and the schedule usable; it only loses the device name
                // and Warp display for this refresh.
                let remote = client
                    .settings_backup_status()
                    .await
                    .inspect_err(|error| {
                        tracing::debug!("reading Online backup status failed: {}", error.message);
                    })
                    .ok()
                    .map(|status| status.remote);
                // The remote-control poller's own status; soft-fail like
                // Core's TUI does, since it is one row, not the page.
                let activity = client
                    .remote_activity()
                    .await
                    .inspect_err(|error| {
                        tracing::debug!("reading Online remote activity failed: {}", error.message);
                    })
                    .ok();
                // A Core older than the backup status call, or one that
                // could not answer it just now, still knows whether it is
                // linked: never offer to link an account that already is.
                let linked = if let Some(remote) = &remote {
                    remote.linked
                } else {
                    let base_url = settings.online_base_url.clone().unwrap_or_default();
                    !base_url.is_empty()
                        && client
                            .settings_auth_status(&base_url)
                            .await
                            .is_ok_and(|status| status.linked)
                };
                Model {
                    available: settings.playtime_sync_enabled.is_some(),
                    linked,
                    device_name: remote.as_ref().and_then(|r| r.device_name.clone()),
                    warp_availability: remote.map(|r| r.availability).unwrap_or_default(),
                    features: OnlineFeatures {
                        remote_control: settings.remote_control_enabled.unwrap_or(false),
                        play_history: settings.playtime_sync_enabled.unwrap_or(false),
                        library: settings.library_sync_enabled.unwrap_or(false),
                        cloud_backup: settings.backup_remote_enabled.unwrap_or(false),
                    },
                    backup_schedule: settings.backup_remote_schedule.unwrap_or_default(),
                    remote_state: activity.map_or_else(String::new, |a| a.status.state),
                }
            }
            Err(error) => {
                tracing::debug!("reading Online settings failed: {}", error.message);
                Model::default()
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            lock(&ctx.shared).online = model;
            crate::settings::reseat(&ctx, &app);
            crate::settings::refresh(&ctx, &app);
        });
    });
}

/// "linked" or "unlinked": the view adds the device name itself.
pub fn status_value(model: &Model) -> &'static str {
    if model.linked {
        "linked"
    } else {
        "unlinked"
    }
}

/// "active", "inactive", or "checking" while Core's own check has not
/// resolved yet.
pub fn warp_value(model: &Model) -> &'static str {
    match model.warp_availability.as_str() {
        "available" => "active",
        "unavailable" => "inactive",
        _ => "checking",
    }
}

/// The remote-control poller's state, or "unknown" for one this build does
/// not name (or when the fetch failed): the same closed set Core's own TUI
/// words.
pub fn remote_status_value(model: &Model) -> &'static str {
    match model.remote_state.as_str() {
        "disabled" => "disabled",
        "unlinked" => "unlinked",
        "connecting" => "connecting",
        "waiting" => "waiting",
        "not_remote_device" => "not_remote_device",
        "unavailable" => "unavailable",
        "credential_rejected" => "credential_rejected",
        "error" => "error",
        _ => "unknown",
    }
}

/// One of the four consent settings the Online page's own feature toggles
/// and the "All online features" row both change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    RemoteControl,
    PlayHistory,
    Library,
    CloudBackup,
}

impl Feature {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "onlineRemoteControl" => Some(Self::RemoteControl),
            "playtimeSync" => Some(Self::PlayHistory),
            "onlineLibrarySync" => Some(Self::Library),
            "onlineCloudBackup" => Some(Self::CloudBackup),
            _ => None,
        }
    }

    fn get(self, features: OnlineFeatures) -> bool {
        match self {
            Self::RemoteControl => features.remote_control,
            Self::PlayHistory => features.play_history,
            Self::Library => features.library,
            Self::CloudBackup => features.cloud_backup,
        }
    }

    fn set(self, features: &mut OnlineFeatures, value: bool) {
        match self {
            Self::RemoteControl => features.remote_control = value,
            Self::PlayHistory => features.play_history = value,
            Self::Library => features.library = value,
            Self::CloudBackup => features.cloud_backup = value,
        }
    }

    fn params(self, value: bool) -> UpdateSettingsParams {
        let value = Some(value);
        match self {
            Self::RemoteControl => UpdateSettingsParams {
                remote_control_enabled: value,
                ..Default::default()
            },
            Self::PlayHistory => UpdateSettingsParams {
                playtime_sync_enabled: value,
                ..Default::default()
            },
            Self::Library => UpdateSettingsParams {
                library_sync_enabled: value,
                ..Default::default()
            },
            Self::CloudBackup => UpdateSettingsParams {
                backup_remote_enabled: value,
                ..Default::default()
            },
        }
    }

    /// The one change a toggle of this feature asks Core for.
    fn changes(self, value: bool) -> OnlineFeatureChanges {
        let mut changes = OnlineFeatureChanges::default();
        match self {
            Self::RemoteControl => changes.remote_control = Some(value),
            Self::PlayHistory => changes.play_history = Some(value),
            Self::Library => changes.library = Some(value),
            Self::CloudBackup => changes.cloud_backup = Some(value),
        }
        changes
    }

    fn log_name(self) -> &'static str {
        match self {
            Self::RemoteControl => "remote control",
            Self::PlayHistory => "play history sync",
            Self::Library => "library sync",
            Self::CloudBackup => "cloud backup",
        }
    }
}

/// Flip one consent setting. The row shows the new value at once and goes
/// back if Core refuses.
pub fn toggle_feature(ctx: &Ctx, app: &App, feature: Feature) {
    let (before, enabled) = {
        let mut shared = lock(&ctx.shared);
        if !shared.online.available {
            return;
        }
        let before = shared.online.features;
        let next = !feature.get(before);
        feature.set(&mut shared.online.features, next);
        (before, next)
    };
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_update(feature.params(enabled)).await;
        if let Err(error) = result {
            tracing::warn!("changing {} failed: {}", feature.log_name(), error.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                roll_back(&ctx, before, feature.changes(enabled));
                crate::settings::refresh(&ctx, &app);
                crate::router::report_action_error(&ctx, &app, "online", "");
            });
        }
    });
}

/// Put back what a refused request changed, leaving anything the user or
/// an overlapping request has changed since. See
/// `zaparoo_app::online_settings::roll_back`.
fn roll_back(ctx: &Ctx, before: OnlineFeatures, changes: OnlineFeatureChanges) {
    let mut shared = lock(&ctx.shared);
    shared.online.features =
        zaparoo_app::online_settings::roll_back(shared.online.features, before, changes);
}

/// Pick how often cloud backup runs on its own. The row shows the new value
/// at once and goes back if Core refuses (it validates the value itself).
pub fn set_backup_schedule(ctx: &Ctx, app: &App, schedule: &str) {
    let (previous, schedule) = {
        let mut shared = lock(&ctx.shared);
        if !shared.online.available || shared.online.backup_schedule == schedule {
            return;
        }
        let previous = std::mem::replace(&mut shared.online.backup_schedule, schedule.to_string());
        (previous, schedule.to_string())
    };
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client
            .settings_update(UpdateSettingsParams {
                backup_remote_schedule: Some(schedule.clone()),
                ..Default::default()
            })
            .await;
        if let Err(error) = result {
            tracing::warn!(
                "changing the cloud backup schedule failed: {}",
                error.message
            );
            let _ = weak.upgrade_in_event_loop(move |app| {
                {
                    // Only while the row still shows this request's value:
                    // a later pick that Core accepted must stand.
                    let mut shared = lock(&ctx.shared);
                    if shared.online.backup_schedule == schedule {
                        shared.online.backup_schedule = previous;
                    }
                }
                crate::settings::refresh(&ctx, &app);
                crate::router::report_action_error(&ctx, &app, "online", "");
            });
        }
    });
}

/// Accepted the "All online features" row: drive every one of the four to
/// fully on or fully off in one request. See
/// `zaparoo_app::online_settings::all_features_update` for exactly what
/// turning it on or off changes.
pub fn toggle_all_features(ctx: &Ctx, app: &App) {
    let (before, changes) = {
        let mut shared = lock(&ctx.shared);
        if !shared.online.available {
            return;
        }
        let before = shared.online.features;
        let warp_available = shared.online.warp_availability == "available";
        let on = before.tri_state(warp_available) != zaparoo_app::online_settings::TriState::On;
        let (next, changes) =
            zaparoo_app::online_settings::all_features_update(before, on, warp_available);
        shared.online.features = next;
        (before, changes)
    };
    let params = UpdateSettingsParams {
        remote_control_enabled: changes.remote_control,
        playtime_sync_enabled: changes.play_history,
        library_sync_enabled: changes.library,
        backup_remote_enabled: changes.cloud_backup,
        ..Default::default()
    };
    crate::settings::refresh(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_update(params).await;
        if let Err(error) = result {
            tracing::warn!("changing all online features failed: {}", error.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                roll_back(&ctx, before, changes);
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
    overlays.set_online_can_open_url(ctx.open_url.available());
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

/// The panel owns input while it is up: Back always leaves. What Accept
/// does is `rules::accept_action`'s call; see it for the three outcomes.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if action == actions::CANCEL {
        close(ctx, app);
        return;
    }
    if action != actions::ACCEPT {
        return;
    }
    let (finished, showing, url_complete) = {
        let shared = lock(&ctx.shared);
        (
            shared.online_link.finished(),
            shared.online_link.phase == rules::Phase::Showing,
            shared.online_link.url_complete.clone(),
        )
    };
    match rules::accept_action(finished, showing, ctx.open_url.available()) {
        rules::AcceptAction::Close => close(ctx, app),
        rules::AcceptAction::OpenUrl => {
            if !ctx.open_url.open(&url_complete) {
                crate::router::report_action_error(ctx, app, "online", "");
            }
        }
        rules::AcceptAction::Nothing => {}
    }
}

/// Watch for link progress Core pushes.
pub fn bind_events(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    // Ask as soon as Core is reachable, and again after every reconnect,
    // so the Online page opens on rows that are already settled instead of
    // redrawing under the cursor when the first answer lands.
    {
        let mut connection = client.connection.subscribe();
        let weak = app.as_weak();
        let ctx = ctx.clone();
        ctx.handle.clone().spawn(async move {
            loop {
                if matches!(*connection.borrow_and_update(), ConnectionState::Connected) {
                    let ctx = ctx.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| refresh(&ctx, &app));
                }
                if connection.changed().await.is_err() {
                    return;
                }
            }
        });
    }
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
