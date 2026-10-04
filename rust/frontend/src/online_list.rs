// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Online: the two lists behind it. Cloud backups (back up now,
// restore a snapshot) and the remote-control activity log share one modal.
// Core owns every snapshot and every upload; this module asks, shows what
// came back through `zaparoo_app::online_lists`, and forwards the user's
// choice. Every answer from Core lands on the UI thread through an
// `observe_*` function that ignores an answer for a list the user has
// already left.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::online_lists::{self as lists, ListRow};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{BackupRemoteListResult, RemoteActivityResult};

use crate::router::{lock, Ctx};
use crate::{
    App, DialogButton, DialogKind, OnlineListKind, OnlineListRow, OnlineListStatus, Overlays,
};

/// Rows the modal draws at once. A longer list scrolls inside it.
const WINDOW: usize = 6;

/// The action row that leads the backup list.
const RUN_ROW: &str = "run";

#[derive(Debug, Clone, Default)]
pub struct State {
    kind: Option<Kind>,
    rows: Vec<ListRow>,
    index: usize,
    start: usize,
    status: Status,
    note: String,
    /// Retires an answer for a list the user closed or reopened.
    ticket: u64,
    /// The snapshot a restore was asked about, and its label, while the
    /// confirmation is up.
    pending_restore: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Backups,
    Activity,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Status {
    #[default]
    Ready,
    Loading,
    Failed,
    Empty,
    Working,
}

impl From<Status> for OnlineListStatus {
    fn from(value: Status) -> Self {
        match value {
            Status::Ready => Self::Ready,
            Status::Loading => Self::Loading,
            Status::Failed => Self::Failed,
            Status::Empty => Self::Empty,
            Status::Working => Self::Working,
        }
    }
}

fn render(ctx: &Ctx, app: &App) {
    let overlays = app.global::<Overlays>();
    let shared = lock(&ctx.shared);
    let state = &shared.online_list;
    overlays.set_online_list_kind(match state.kind {
        Some(Kind::Backups) => OnlineListKind::Backups,
        Some(Kind::Activity) => OnlineListKind::Activity,
        None => OnlineListKind::None,
    });
    overlays.set_online_list_status(state.status.into());
    overlays.set_online_list_note(SharedString::from(state.note.as_str()));
    let start = lists::window_start(state.rows.len(), state.index, state.start, WINDOW);
    let shown: Vec<OnlineListRow> = state
        .rows
        .iter()
        .skip(start)
        .take(WINDOW)
        .map(|row| OnlineListRow {
            id: SharedString::from(row.id.as_str()),
            label: SharedString::from(row.label.as_str()),
            detail: SharedString::from(row.detail.as_str()),
            enabled: row.enabled,
            this_device: row.this_device,
        })
        .collect();
    overlays.set_online_list_more_above(start > 0);
    overlays.set_online_list_more_below(start + WINDOW < state.rows.len());
    overlays
        .set_online_list_index(i32::try_from(state.index - start.min(state.index)).unwrap_or(0));
    overlays.set_online_list_rows(ModelRc::new(VecModel::from(shown)));
    overlays.set_online_list_open(state.kind.is_some());
}

/// Start a fresh list: nothing shown yet, an answer wanted for the ticket
/// this returns.
fn begin(ctx: &Ctx, app: &App, kind: Kind) -> u64 {
    crate::press_feedback::cancel(app);
    let ticket = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        let ticket = state.ticket.wrapping_add(1);
        *state = State {
            kind: Some(kind),
            status: Status::Loading,
            ticket,
            ..State::default()
        };
        ticket
    };
    render(ctx, app);
    ticket
}

fn current(ctx: &Ctx, ticket: u64) -> bool {
    lock(&ctx.shared).online_list.ticket == ticket
}

/// Open the cloud backup list and ask Core for the account's snapshots and
/// storage use.
pub fn open_backups(ctx: &Ctx, app: &App) {
    let ticket = begin(ctx, app, Kind::Backups);
    fetch_backups(ctx, app, ticket);
}

fn fetch_backups(ctx: &Ctx, app: &App, ticket: u64) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_remote_list().await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(list) => observe_backups(&ctx, &app, ticket, &list),
            Err(error) => {
                tracing::warn!("listing cloud backups failed: {}", error.message);
                observe_failed(&ctx, &app, ticket);
            }
        });
    });
}

fn backup_rows(list: &BackupRemoteListResult) -> Vec<ListRow> {
    let mut snapshots: Vec<_> = list.items.iter().collect();
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut rows = vec![ListRow {
        id: RUN_ROW.to_string(),
        enabled: true,
        ..ListRow::default()
    }];
    rows.extend(snapshots.into_iter().map(|snapshot| {
        let device = snapshot.source_device.as_ref();
        lists::snapshot_row(&lists::Snapshot {
            id: &snapshot.id,
            created_at: &snapshot.created_at,
            size_bytes: snapshot.size_bytes,
            device_name: device.map(|d| d.name.as_str()),
            current_device: device.is_some_and(|d| d.current),
            incompatible: snapshot.incompatible,
        })
    }));
    rows
}

pub(crate) fn observe_backups(ctx: &Ctx, app: &App, ticket: u64, list: &BackupRemoteListResult) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.rows = backup_rows(list);
        state.index = state.index.min(state.rows.len().saturating_sub(1));
        state.status = if list.items.is_empty() {
            Status::Empty
        } else {
            Status::Ready
        };
        state.note = if list.storage_quota_bytes > 0 {
            format!(
                "{} / {}",
                lists::bytes(list.storage_used_bytes),
                lists::bytes(list.storage_quota_bytes)
            )
        } else {
            String::new()
        };
    }
    render(ctx, app);
}

/// Open the remote-control activity log.
pub fn open_activity(ctx: &Ctx, app: &App) {
    let ticket = begin(ctx, app, Kind::Activity);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.remote_activity().await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(activity) => observe_activity(&ctx, &app, ticket, &activity),
            Err(error) => {
                tracing::warn!("reading remote activity failed: {}", error.message);
                observe_failed(&ctx, &app, ticket);
            }
        });
    });
}

pub(crate) fn observe_activity(ctx: &Ctx, app: &App, ticket: u64, activity: &RemoteActivityResult) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.rows = activity
            .entries
            .iter()
            .map(|entry| {
                lists::activity_row(&lists::Activity {
                    created_at: &entry.created_at,
                    operation_type: &entry.operation_type,
                    origin_kind: &entry.origin_kind,
                    origin_key_name: &entry.origin_key_name,
                    state: &entry.state,
                    status: &entry.status,
                    error_code: &entry.error_code,
                })
            })
            .collect();
        state.status = if state.rows.is_empty() {
            Status::Empty
        } else {
            Status::Ready
        };
    }
    render(ctx, app);
}

fn observe_failed(ctx: &Ctx, app: &App, ticket: u64) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.rows.clear();
        state.status = Status::Failed;
    }
    render(ctx, app);
}

/// Leave the list. A restore waiting on its confirmation is dropped with it.
pub fn close(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let ticket = shared.online_list.ticket.wrapping_add(1);
        shared.online_list = State {
            ticket,
            ..State::default()
        };
    }
    crate::press_feedback::cancel(app);
    render(ctx, app);
}

/// The list owns input while it is up: Up and Down walk it, Back leaves,
/// Accept acts on the focused row (backups only).
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let (len, index, kind, status) = {
        let shared = lock(&ctx.shared);
        let state = &shared.online_list;
        (state.rows.len(), state.index, state.kind, state.status)
    };
    match action {
        actions::CANCEL | actions::PAGE_MENU => close(ctx, app),
        actions::UP if len > 0 => move_to(ctx, app, (index + len - 1) % len),
        actions::DOWN if len > 0 => move_to(ctx, app, (index + 1) % len),
        actions::ACCEPT if kind == Some(Kind::Backups) && status != Status::Working => {
            accept_backup_row(ctx, app, index);
        }
        _ => {}
    }
}

fn move_to(ctx: &Ctx, app: &App, index: usize) {
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.start = lists::window_start(state.rows.len(), index, state.start, WINDOW);
        state.index = index;
    }
    render(ctx, app);
}

fn accept_backup_row(ctx: &Ctx, app: &App, index: usize) {
    let row = lock(&ctx.shared).online_list.rows.get(index).cloned();
    let Some(row) = row else { return };
    if !row.enabled {
        return;
    }
    if row.id == RUN_ROW {
        run_backup(ctx, app);
        return;
    }
    lock(&ctx.shared).online_list.pending_restore = Some((row.id, row.label.clone()));
    crate::router::open_dialog(
        app,
        DialogKind::RestoreOnlineBackup,
        &row.label,
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
}

fn run_backup(ctx: &Ctx, app: &App) {
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.online_list.status = Status::Working;
        shared.online_list.ticket
    };
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_remote_run().await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(error) = &result {
                tracing::warn!("cloud backup failed: {}", error.message);
                crate::router::report_action_error(&ctx, &app, "online", "");
            }
            // Either way the list is no longer working; show what the
            // account holds now.
            if current(&ctx, ticket) {
                lock(&ctx.shared).online_list.status = Status::Loading;
                render(&ctx, &app);
                fetch_backups(&ctx, &app, ticket);
            }
        });
    });
}

/// The user confirmed restoring the snapshot they asked about. Core
/// restarts to finish applying it once it answers.
pub fn restore_confirmed(ctx: &Ctx, app: &App) {
    let pending = lock(&ctx.shared).online_list.pending_restore.take();
    let Some((id, _label)) = pending else { return };
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.online_list.status = Status::Working;
        shared.online_list.ticket
    };
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_remote_restore(&id).await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => close(&ctx, &app),
            Err(error) => {
                tracing::warn!("restoring a cloud backup failed: {}", error.message);
                crate::router::report_action_error(&ctx, &app, "online", "");
                if current(&ctx, ticket) {
                    lock(&ctx.shared).online_list.status = Status::Ready;
                    render(&ctx, &app);
                }
            }
        });
    });
}
