// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Settings > Library > Launcher preference: Core's global launcher
// preference, an ordered list of launchers and launcher groups tried first
// on every system. One modal with three pages of its own panel: the list
// (with Add… at its end), an entry's Move and Remove, and the launchers and
// groups that can be added. Move holds an entry and walks it through the
// list the way the Hub's Move walks a tile, rebuilt from the list as it was
// when the move began; Back puts it back.
//
// Core owns the list. Every change is shown at once and written through
// one writer that always sends the newest list; a refusal puts back what
// Core last accepted and says so. The RetroAchievements switch on the same
// page writes through here too. Rules live in
// `zaparoo_app::launcher_preference`.

use slint::{ComponentHandle, SharedString};
use zaparoo_app::launcher_preference::{self as rules, Choice, EntryKind, LauncherRef};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{LauncherInfo, UpdateSettingsParams};

use crate::router::{lock, Ctx};
use crate::{App, LauncherPrefsPage, LauncherPrefsRow, LauncherPrefsRowKind, Overlays};

#[derive(Debug, Default)]
pub struct State {
    open: bool,
    page: Page,
    /// The list on show: what Core accepted, or a change still on its way.
    list: Vec<String>,
    /// The List page cursor: an entry, or `list.len()` for Add….
    index: usize,
    /// The Entry page cursor: Move, then Remove.
    entry_index: usize,
    choices: Vec<Choice>,
    choice_index: usize,
    /// A move in progress: the list as it was when it began, and where the
    /// held entry came from.
    moving: Option<(Vec<String>, usize)>,
    /// The newest list sent, with its sequence; the writer always sends
    /// the newest one and skips any it was handed in between.
    writer: Option<tokio::sync::watch::Sender<(u64, Vec<String>)>>,
    seq: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    List,
    Entry,
    Add,
}

fn refs(launchers: &[LauncherInfo]) -> Vec<LauncherRef<'_>> {
    launchers
        .iter()
        .map(|l| LauncherRef {
            id: &l.id,
            system_id: &l.system_id,
            system_name: &l.system_name,
            groups: &l.groups,
            available: l.available,
        })
        .collect()
}

/// The list Core holds, with any change still on its way: what the page and
/// the `RetroAchievements` switch read.
pub(crate) fn current(ctx: &Ctx) -> Option<Vec<String>> {
    lock(&ctx.shared).launcher_preference.clone()
}

fn entry_row(entry: &str, launchers: &[LauncherRef<'_>], held: bool) -> LauncherPrefsRow {
    let kind = rules::entry_kind(entry, launchers);
    let available = match &kind {
        EntryKind::Group => launchers
            .iter()
            .any(|l| l.available && l.groups.iter().any(|g| g.eq_ignore_ascii_case(entry))),
        EntryKind::Launcher(_) => launchers
            .iter()
            .any(|l| l.available && l.id.eq_ignore_ascii_case(entry)),
        EntryKind::Unknown => false,
    };
    LauncherPrefsRow {
        kind: match kind {
            EntryKind::Group => LauncherPrefsRowKind::Group,
            EntryKind::Launcher(_) | EntryKind::Unknown => LauncherPrefsRowKind::Launcher,
        },
        id: SharedString::from(entry),
        detail: SharedString::from(match &kind {
            EntryKind::Launcher(system) => system.as_str(),
            _ => "",
        }),
        enabled: available,
        held,
    }
}

fn action_row(kind: LauncherPrefsRowKind) -> LauncherPrefsRow {
    LauncherPrefsRow {
        kind,
        enabled: true,
        ..LauncherPrefsRow::default()
    }
}

fn render(ctx: &Ctx, app: &App) {
    let overlays = app.global::<Overlays>();
    let shared = lock(&ctx.shared);
    let state = &shared.launcher_prefs;
    let launchers = refs(&shared.launchers);
    let (page, rows, index) = match state.page {
        Page::List => {
            let held = state.moving.as_ref().map(|_| state.index);
            let mut rows: Vec<LauncherPrefsRow> = state
                .list
                .iter()
                .enumerate()
                .map(|(i, entry)| entry_row(entry, &launchers, held == Some(i)))
                .collect();
            if state.moving.is_none() {
                rows.push(action_row(LauncherPrefsRowKind::Add));
            }
            (LauncherPrefsPage::List, rows, state.index)
        }
        Page::Entry => (
            LauncherPrefsPage::Entry,
            vec![
                action_row(LauncherPrefsRowKind::Move),
                action_row(LauncherPrefsRowKind::Remove),
            ],
            state.entry_index,
        ),
        Page::Add => (
            LauncherPrefsPage::Add,
            state
                .choices
                .iter()
                .map(|choice| LauncherPrefsRow {
                    kind: if choice.group {
                        LauncherPrefsRowKind::Group
                    } else {
                        LauncherPrefsRowKind::Launcher
                    },
                    id: SharedString::from(choice.id.as_str()),
                    detail: SharedString::from(choice.system_name.as_str()),
                    enabled: choice.available,
                    held: false,
                })
                .collect(),
            state.choice_index,
        ),
    };
    let entry = state.list.get(state.index).map_or_else(
        || action_row(LauncherPrefsRowKind::Add),
        |entry| entry_row(entry, &launchers, false),
    );
    drop(shared);
    overlays.set_launcher_prefs_page(page);
    overlays.set_launcher_prefs_entry(entry);
    overlays.set_launcher_prefs_index(i32::try_from(index).unwrap_or(0));
    crate::view_model::publish(&overlays.get_launcher_prefs_rows(), rows, |rows| {
        overlays.set_launcher_prefs_rows(rows);
    });
    let (open, moving) = {
        let shared = lock(&ctx.shared);
        (
            shared.launcher_prefs.open,
            shared.launcher_prefs.moving.is_some(),
        )
    };
    overlays.set_launcher_prefs_moving(moving);
    overlays.set_launcher_prefs_open(open);
}

/// Open the list from its Settings row.
pub(crate) fn open(ctx: &Ctx, app: &App) {
    crate::press_feedback::cancel(app);
    {
        let mut shared = lock(&ctx.shared);
        let list = shared.launcher_preference.clone().unwrap_or_default();
        let state = &mut shared.launcher_prefs;
        state.open = true;
        state.page = Page::List;
        state.list = list;
        state.index = 0;
        state.moving = None;
    }
    render(ctx, app);
}

fn close(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        state.open = false;
        state.moving = None;
        state.page = Page::List;
    }
    crate::press_feedback::cancel(app);
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let (page, moving) = {
        let shared = lock(&ctx.shared);
        (
            shared.launcher_prefs.page,
            shared.launcher_prefs.moving.is_some(),
        )
    };
    if moving {
        move_action(ctx, app, action);
        return;
    }
    match (page, action) {
        (Page::List, actions::CANCEL | actions::PAGE_MENU) => close(ctx, app),
        (Page::Entry | Page::Add, actions::CANCEL) => to_list(ctx, app),
        (_, actions::UP | actions::DOWN) => walk(ctx, app, action == actions::DOWN),
        (Page::List, actions::ACCEPT) => accept_list(ctx, app),
        (Page::Entry, actions::ACCEPT) => accept_entry(ctx, app),
        (Page::Add, actions::ACCEPT) => accept_add(ctx, app),
        _ => {}
    }
}

/// Up and Down on whichever page is up, wrapping.
fn walk(ctx: &Ctx, app: &App, down: bool) {
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        let (cursor, len) = match state.page {
            Page::List => (&mut state.index, state.list.len() + 1),
            Page::Entry => (&mut state.entry_index, 2),
            Page::Add => (&mut state.choice_index, state.choices.len()),
        };
        if len == 0 {
            return;
        }
        *cursor = if down {
            (*cursor + 1) % len
        } else {
            (*cursor + len - 1) % len
        };
    }
    render(ctx, app);
}

fn to_list(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).launcher_prefs.page = Page::List;
    render(ctx, app);
}

fn accept_list(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let launchers = shared.launchers.clone();
        let state = &mut shared.launcher_prefs;
        if state.index < state.list.len() {
            state.page = Page::Entry;
            state.entry_index = 0;
        } else {
            state.choices = rules::choices(&refs(&launchers), &state.list);
            state.choice_index = 0;
            if state.choices.is_empty() {
                return;
            }
            state.page = Page::Add;
        }
    }
    render(ctx, app);
}

fn accept_entry(ctx: &Ctx, app: &App) {
    let list = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        state.page = Page::List;
        if state.entry_index == 0 {
            state.moving = Some((state.list.clone(), state.index));
            None
        } else {
            state.list = rules::remove(&state.list, state.index);
            state.index = state.index.min(state.list.len());
            Some(state.list.clone())
        }
    };
    if let Some(list) = list {
        write(ctx, app, list);
    }
    render(ctx, app);
}

/// Add the chosen launcher or group at the end, then hold it so the user
/// can put it where it belongs.
fn accept_add(ctx: &Ctx, app: &App) {
    let list = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        let Some(choice) = state.choices.get(state.choice_index) else {
            return;
        };
        state.list = rules::add(&state.list, &choice.id);
        state.index = state.list.len() - 1;
        state.page = Page::List;
        state.moving = Some((state.list.clone(), state.index));
        state.list.clone()
    };
    write(ctx, app, list);
    render(ctx, app);
}

fn move_action(ctx: &Ctx, app: &App, action: &str) {
    let write_list = {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        let Some((base, origin)) = state.moving.clone() else {
            return;
        };
        match action {
            actions::UP | actions::DOWN => {
                let Some(to) = rules::step(state.index, base.len(), action == actions::DOWN) else {
                    return;
                };
                state.index = to;
                state.list = rules::reseat(&base, origin, to);
                None
            }
            actions::ACCEPT => {
                state.moving = None;
                (state.list != base).then(|| state.list.clone())
            }
            actions::CANCEL => {
                state.moving = None;
                state.list = base;
                state.index = origin;
                None
            }
            _ => return,
        }
    };
    if let Some(list) = write_list {
        write(ctx, app, list);
    }
    render(ctx, app);
}

/// The pointer is over a row: put the cursor there.
pub(crate) fn focus(ctx: &Ctx, app: &App, index: usize) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let state = &mut shared.launcher_prefs;
        if !state.open || state.moving.is_some() {
            return false;
        }
        let (cursor, len) = match state.page {
            Page::List => (&mut state.index, state.list.len() + 1),
            Page::Entry => (&mut state.entry_index, 2),
            Page::Add => (&mut state.choice_index, state.choices.len()),
        };
        if index >= len {
            return false;
        }
        *cursor = index;
    }
    render(ctx, app);
    true
}

/// The `RetroAchievements` switch: put the group first, or take it out.
pub(crate) fn set_retro_achievements(ctx: &Ctx, app: &App, on: bool) {
    let Some(list) = current(ctx) else {
        return;
    };
    let next = rules::set(&list, rules::RETRO_ACHIEVEMENTS, on);
    if next != list {
        write(ctx, app, next);
    }
}

/// Show `list` as Core's at once and send it. Each send replaces any older
/// one still waiting, so a quick run of changes costs one request each way.
fn write(ctx: &Ctx, app: &App, list: Vec<String>) {
    let spawn = {
        let mut shared = lock(&ctx.shared);
        shared.launcher_preference = Some(list.clone());
        let state = &mut shared.launcher_prefs;
        state.seq = state.seq.wrapping_add(1);
        let seq = state.seq;
        if let Some(tx) = &state.writer {
            tx.send_replace((seq, list));
            None
        } else {
            let (tx, rx) = tokio::sync::watch::channel((seq, list));
            state.writer = Some(tx);
            Some(rx)
        }
    };
    crate::settings::refresh(ctx, app);
    if let Some(rx) = spawn {
        start_writer(ctx, app, rx);
    }
}

fn start_writer(ctx: &Ctx, app: &App, mut rx: tokio::sync::watch::Receiver<(u64, Vec<String>)>) {
    let client = ctx.store.client();
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        // The first list is already waiting when the writer starts.
        let mut first = true;
        loop {
            if !first && rx.changed().await.is_err() {
                return;
            }
            first = false;
            let (seq, list) = rx.borrow_and_update().clone();
            let params = UpdateSettingsParams {
                launcher_preference: Some(list.clone()),
                ..UpdateSettingsParams::default()
            };
            let result = client.settings_update(params).await;
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok(()) => {
                    lock(&ctx.shared).launcher_pref_confirmed = Some(list);
                }
                Err(error) => {
                    tracing::warn!("saving the launcher preference failed: {}", error.message);
                    refused(&ctx, &app, seq);
                }
            });
        }
    });
}

/// Core refused the newest list: show what it last accepted again.
fn refused(ctx: &Ctx, app: &App, seq: u64) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.launcher_prefs.seq != seq {
            return;
        }
        let confirmed = shared.launcher_pref_confirmed.clone();
        shared.launcher_preference.clone_from(&confirmed);
        let state = &mut shared.launcher_prefs;
        if state.moving.is_none() {
            state.list = confirmed.unwrap_or_default();
            state.index = state.index.min(state.list.len());
        }
    }
    if lock(&ctx.shared).launcher_prefs.open {
        render(ctx, app);
    }
    crate::settings::refresh(ctx, app);
    crate::router::report_action_error(ctx, app, "setting", "");
}

/// Core's answer to `settings`: the list it holds, unless a change of ours
/// is still on its way.
pub(crate) fn observe(ctx: &Ctx, list: Option<Vec<String>>) {
    let mut shared = lock(&ctx.shared);
    let pending = shared.launcher_preference != shared.launcher_pref_confirmed;
    shared.launcher_pref_confirmed.clone_from(&list);
    if !pending || list.is_none() {
        shared.launcher_preference = list;
    }
}

#[cfg(all(test, feature = "mister"))]
pub(crate) fn shown_for_test(ctx: &Ctx) -> Vec<String> {
    lock(&ctx.shared).launcher_prefs.list.clone()
}
