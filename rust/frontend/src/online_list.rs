// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Online: the lists behind it. Cloud backups (back up now,
// restore a snapshot), local backups (a backup's details, delete it) and
// the remote-control activity log share one modal; a local backup's
// details are a page of it.
// Core owns every snapshot and every upload; this module asks, shows what
// came back through `zaparoo_app::online_lists`, and forwards the user's
// choice. Every answer from Core lands on the UI thread through an
// `observe_*` function that ignores an answer for a list the user has
// already left.

use slint::{ComponentHandle, SharedString};
use zaparoo_app::backup_status::Activity;
use zaparoo_app::online_lists::{self as lists, BackupCategory, ListRow, RowKind};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    BackupRemoteListResult, LocalBackup, LocalBackupListItem, RemoteActivityResult,
};

use crate::router::{lock, Ctx};
use crate::{
    App, BackupActivity, DialogButton, DialogKind, OnlineListFailure, OnlineListKind,
    OnlineListRow, OnlineListRowKind, OnlineListStatus, Overlays,
};

/// The action row that leads the backup list.
const RUN_ROW: &str = "run";

#[derive(Debug, Clone, Default)]
pub struct State {
    kind: Option<Kind>,
    rows: Vec<ListRow>,
    index: usize,
    status: Status,
    /// Why the last request was refused, while `status` is `Failed`.
    failure: lists::Failure,
    note: String,
    /// Retires an answer for a list the user closed or reopened.
    ticket: u64,
    /// The snapshot a restore was asked about, and its label, while the
    /// confirmation is up.
    pending_restore: Option<(String, String)>,
    /// The local backup a delete was asked about, while the confirmation
    /// is up.
    pending_delete: Option<String>,
    /// The details page's title: the backup's time, as its list row read.
    title: String,
    /// Where the cursor was on the local list, for Back from its details.
    list_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Backups,
    Activity,
    Local,
    /// One local backup's details, a page of the local list.
    LocalDetails,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Status {
    #[default]
    Ready,
    Loading,
    Failed,
    Empty,
    BackingUp,
    Restoring,
    Deleting,
}

impl Status {
    /// A backup, a restore or a delete is running: the list takes no
    /// input.
    fn working(self) -> bool {
        matches!(self, Self::BackingUp | Self::Restoring | Self::Deleting)
    }
}

impl From<Status> for OnlineListStatus {
    fn from(value: Status) -> Self {
        match value {
            Status::Ready => Self::Ready,
            Status::Loading => Self::Loading,
            Status::Failed => Self::Failed,
            Status::Empty => Self::Empty,
            Status::BackingUp => Self::BackingUp,
            Status::Restoring => Self::Restoring,
            Status::Deleting => Self::Deleting,
        }
    }
}

fn row_kind(kind: RowKind) -> OnlineListRowKind {
    match kind {
        RowKind::Entry => OnlineListRowKind::Entry,
        RowKind::Run => OnlineListRowKind::Run,
        RowKind::Created => OnlineListRowKind::Created,
        RowKind::Size => OnlineListRowKind::Size,
        RowKind::Skipped => OnlineListRowKind::Skipped,
        RowKind::Error => OnlineListRowKind::Error,
        RowKind::Category(BackupCategory::Zaparoo) => OnlineListRowKind::Zaparoo,
        RowKind::Category(BackupCategory::Settings) => OnlineListRowKind::Settings,
        RowKind::Category(BackupCategory::Inputs) => OnlineListRowKind::Inputs,
        RowKind::Category(BackupCategory::Saves) => OnlineListRowKind::Saves,
        RowKind::Category(BackupCategory::Savestates) => OnlineListRowKind::Savestates,
        RowKind::Delete => OnlineListRowKind::Delete,
    }
}

/// The pace of the backup or restore running in Core, for the working
/// status. Published whether or not the list is up, so it is right the
/// moment one opens.
pub fn publish_activity(ctx: &Ctx, app: &App) {
    let activity = lock(&ctx.shared).online.activity;
    app.global::<Overlays>()
        .set_backup_activity(match activity {
            Activity::Idle => BackupActivity::Idle,
            Activity::Running => BackupActivity::Running,
            Activity::Paused => BackupActivity::Paused,
            Activity::Slowed => BackupActivity::Slowed,
        });
}

fn render(ctx: &Ctx, app: &App) {
    let overlays = app.global::<Overlays>();
    let shared = lock(&ctx.shared);
    let state = &shared.online_list;
    overlays.set_online_list_kind(match state.kind {
        Some(Kind::Backups) => OnlineListKind::Backups,
        Some(Kind::Activity) => OnlineListKind::Activity,
        Some(Kind::Local) => OnlineListKind::Local,
        Some(Kind::LocalDetails) => OnlineListKind::LocalDetails,
        None => OnlineListKind::None,
    });
    overlays.set_online_list_title(SharedString::from(state.title.as_str()));
    overlays.set_online_list_status(state.status.into());
    overlays.set_online_list_failure(match state.failure {
        lists::Failure::Unlinked => OnlineListFailure::Unlinked,
        lists::Failure::NeedsWarp => OnlineListFailure::NeedsWarp,
        // The rest only arise from running a backup or a restore, which
        // report through the error dialog.
        _ => OnlineListFailure::Other,
    });
    overlays.set_online_list_note(SharedString::from(state.note.as_str()));
    // The whole list: the view windows and scrolls it, like the pickers.
    let shown: Vec<OnlineListRow> = state
        .rows
        .iter()
        .map(|row| OnlineListRow {
            kind: row_kind(row.kind),
            id: SharedString::from(row.id.as_str()),
            label: SharedString::from(row.label.as_str()),
            detail: SharedString::from(row.detail.as_str()),
            count: i32::try_from(row.count).unwrap_or(i32::MAX),
            enabled: row.enabled,
            this_device: row.this_device,
        })
        .collect();
    overlays.set_online_list_index(i32::try_from(state.index).unwrap_or(0));
    crate::view_model::publish(&overlays.get_online_list_rows(), shown, |rows| {
        overlays.set_online_list_rows(rows);
    });
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
                observe_failed(&ctx, &app, ticket, lists::failure(&error.message));
            }
        });
    });
}

fn backup_rows(list: &BackupRemoteListResult) -> Vec<ListRow> {
    let mut snapshots: Vec<_> = list.items.iter().collect();
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut rows = vec![ListRow {
        kind: RowKind::Run,
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

/// Open the list of this device's local backups.
pub fn open_local(ctx: &Ctx, app: &App) {
    let ticket = begin(ctx, app, Kind::Local);
    fetch_local(ctx, app, ticket);
}

fn fetch_local(ctx: &Ctx, app: &App, ticket: u64) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_list().await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(list) => observe_local(&ctx, &app, ticket, &list),
            Err(error) => {
                tracing::warn!("listing local backups failed: {}", error.message);
                observe_failed(&ctx, &app, ticket, lists::failure(&error.message));
            }
        });
    });
}

pub(crate) fn observe_local(ctx: &Ctx, app: &App, ticket: u64, list: &[LocalBackupListItem]) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        let backups: Vec<lists::LocalBackup<'_>> = list
            .iter()
            .map(|item| lists::LocalBackup {
                name: &item.name,
                created_at: &item.created_at,
                size: item.size,
            })
            .collect();
        state.rows = lists::local_backup_rows(&backups);
        state.index = state.index.min(state.rows.len().saturating_sub(1));
        state.status = if state.rows.is_empty() {
            Status::Empty
        } else {
            Status::Ready
        };
    }
    render(ctx, app);
}

/// Accepted a local backup: its details replace the list, as a page of
/// the same panel.
fn open_details(ctx: &Ctx, app: &App, index: usize) {
    let row = lock(&ctx.shared).online_list.rows.get(index).cloned();
    let Some(row) = row else { return };
    let ticket = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.ticket = state.ticket.wrapping_add(1);
        state.kind = Some(Kind::LocalDetails);
        state.list_index = index;
        state.title = row.label;
        state.rows.clear();
        state.index = 0;
        state.status = Status::Loading;
        state.ticket
    };
    crate::press_feedback::cancel(app);
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    let name = row.id;
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_inspect(&name).await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(backup) => observe_details(&ctx, &app, ticket, &backup),
            Err(error) => {
                tracing::warn!("reading a local backup failed: {}", error.message);
                observe_failed(&ctx, &app, ticket, lists::failure(&error.message));
            }
        });
    });
}

pub(crate) fn observe_details(ctx: &Ctx, app: &App, ticket: u64, backup: &LocalBackup) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let categories: Vec<(&str, lists::Category)> = backup
            .categories
            .iter()
            .map(|(token, entry)| {
                (
                    token.as_str(),
                    lists::Category {
                        files: entry.files,
                        bytes: entry.bytes,
                        enabled: entry.enabled,
                    },
                )
            })
            .collect();
        let rows = lists::local_detail_rows(&lists::LocalDetails {
            name: &backup.name,
            created_at: &backup.created_at,
            size: backup.size,
            status: &backup.status,
            warnings: backup.warnings.len(),
            error: backup.error.as_deref(),
            categories: &categories,
        });
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        // Seated on Delete, the one row that acts.
        state.index = rows.len().saturating_sub(1);
        state.rows = rows;
        state.status = Status::Ready;
    }
    render(ctx, app);
}

/// Back from a local backup's details, to the list seated where it was.
fn back_to_local(ctx: &Ctx, app: &App) {
    let ticket = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.ticket = state.ticket.wrapping_add(1);
        state.kind = Some(Kind::Local);
        state.index = state.list_index;
        state.title.clear();
        state.pending_delete = None;
        state.rows.clear();
        state.status = Status::Loading;
        state.ticket
    };
    crate::press_feedback::cancel(app);
    render(ctx, app);
    fetch_local(ctx, app, ticket);
}

fn accept_detail_row(ctx: &Ctx, app: &App, index: usize) {
    let (row, title) = {
        let shared = lock(&ctx.shared);
        let state = &shared.online_list;
        (state.rows.get(index).cloned(), state.title.clone())
    };
    let Some(row) = row.filter(|row| row.kind == RowKind::Delete) else {
        return;
    };
    lock(&ctx.shared).online_list.pending_delete = Some(row.id);
    crate::router::open_dialog(
        app,
        DialogKind::DeleteLocalBackup,
        &title,
        "",
        &[DialogButton::No, DialogButton::Yes],
        0,
    );
}

/// The user confirmed deleting the local backup they asked about. The
/// list comes back without it.
pub fn delete_confirmed(ctx: &Ctx, app: &App) {
    let pending = lock(&ctx.shared).online_list.pending_delete.take();
    let Some(name) = pending else { return };
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.online_list.status = Status::Deleting;
        shared.online_list.ticket
    };
    render(ctx, app);
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = client.settings_backup_delete(&name).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !current(&ctx, ticket) {
                return;
            }
            match result {
                Ok(()) => back_to_local(&ctx, &app),
                Err(error) => {
                    tracing::warn!("deleting a local backup failed: {}", error.message);
                    crate::router::report_action_error(&ctx, &app, "backup", "");
                    lock(&ctx.shared).online_list.status = Status::Ready;
                    render(&ctx, &app);
                }
            }
        });
    });
}

/// The user said no to deleting: the details stay.
pub fn delete_declined(ctx: &Ctx) {
    lock(&ctx.shared).online_list.pending_delete = None;
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
                observe_failed(&ctx, &app, ticket, lists::failure(&error.message));
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

fn observe_failed(ctx: &Ctx, app: &App, ticket: u64, failure: lists::Failure) {
    if !current(ctx, ticket) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        state.rows.clear();
        state.status = Status::Failed;
        state.failure = failure;
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
        // Back from a backup's details is a page back, not out, unless a
        // delete is running.
        actions::CANCEL if kind == Some(Kind::LocalDetails) && !status.working() => {
            back_to_local(ctx, app);
        }
        actions::CANCEL | actions::PAGE_MENU => close(ctx, app),
        actions::UP if len > 0 => move_to(ctx, app, (index + len - 1) % len),
        actions::DOWN if len > 0 => move_to(ctx, app, (index + 1) % len),
        actions::ACCEPT if status.working() => {}
        actions::ACCEPT if kind == Some(Kind::Backups) => accept_backup_row(ctx, app, index),
        actions::ACCEPT if kind == Some(Kind::Local) && index < len => {
            open_details(ctx, app, index);
        }
        actions::ACCEPT if kind == Some(Kind::LocalDetails) => {
            accept_detail_row(ctx, app, index);
        }
        _ => {}
    }
}

fn move_to(ctx: &Ctx, app: &App, index: usize) {
    lock(&ctx.shared).online_list.index = index;
    render(ctx, app);
}

/// The pointer is over a row: put the cursor there.
pub(crate) fn focus(ctx: &Ctx, app: &App, index: usize) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.online_list;
        if state.kind.is_none() || index >= state.rows.len() {
            return false;
        }
        state.index = index;
    }
    render(ctx, app);
    true
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
        shared.online_list.status = Status::BackingUp;
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
                let cause = lists::failure(&error.message).token();
                crate::router::report_action_error(&ctx, &app, "backup", cause);
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
        shared.online_list.status = Status::Restoring;
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
                let cause = lists::failure(&error.message).token();
                crate::router::report_action_error(&ctx, &app, "backup", cause);
                if current(&ctx, ticket) {
                    lock(&ctx.shared).online_list.status = Status::Ready;
                    render(&ctx, &app);
                }
            }
        });
    });
}
