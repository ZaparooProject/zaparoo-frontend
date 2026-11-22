// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Who is playing?": the root screen a fresh start opens on when Settings
// > Profiles asks for it. The curtain stays up until Core has said who
// there is, then lifts onto the picker. Picking a profile switches to it
// through the PIN panel's flows (`pin::switch_then`, `pin::leave_then`);
// once Core has switched, or on Back, the start carries on to the screen
// it would have shown. The picker is never restored after a kill and
// never written as the active screen. Rules live in
// `zaparoo_app::profile_picker`.

use slint::{ComponentHandle, SharedString};
use zaparoo_app::media_list as list_rules;
use zaparoo_app::profile_picker::{self as rules, Entry, Kiosk, Row};
use zaparoo_core::input_actions::actions;
use zaparoo_core::profile_types::{KioskState, Profile};

use crate::router::{lock, Ctx, Shared};
use crate::{App, GridCell, ProfilePickerInput, ProfilePickerView};

/// Where this start is with the picker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Start {
    /// Nothing owed: the picker has gone, or was never asked for.
    #[default]
    Done,
    /// The start opens on the picker once Core has its catalog in.
    Owed,
    /// The picker was asked for; the Hub comes after it.
    ThenHub,
    /// The picker was asked for; the saved screen comes after it.
    ThenRestore,
}

#[derive(Debug, Default)]
pub struct Model {
    start: Start,
    profiles: Vec<Profile>,
    /// Core will not launch without a profile.
    require_for_launch: bool,
    rows: Vec<Row>,
    index: usize,
    top: usize,
    /// A switch without a PIN panel is in flight: input waits for it.
    busy: bool,
    /// Bumped by every fetch, so a late answer is dropped.
    ticket: u64,
}

/// The picker is the active screen.
pub(crate) fn is_active(app: &App) -> bool {
    app.global::<crate::Shell>().get_active_screen() == crate::Screen::ProfilePicker
}

/// This start is a fresh one with the setting on: hold the curtain for
/// the picker.
pub(crate) fn owe(shared: &mut Shared) {
    shared.profile_picker.start = Start::Owed;
}

/// The picker is still owed to this start.
pub(crate) fn owed(shared: &Shared) -> bool {
    shared.profile_picker.start == Start::Owed
}

/// The start cannot reach Core after all: it shows the Hub, never the
/// picker over it later.
pub(crate) fn drop_owed(shared: &mut Shared) {
    let model = &mut shared.profile_picker;
    if model.start == Start::Owed {
        model.start = Start::Done;
    }
    model.ticket = model.ticket.wrapping_add(1);
}

fn kiosk_of(state: &KioskState) -> Kiosk {
    Kiosk {
        locked: state.is_locked(),
        profile_switching: state.profile_switching(),
    }
}

fn entries(profiles: &[Profile]) -> Vec<Entry<'_>> {
    profiles
        .iter()
        .map(|p| Entry {
            id: &p.profile_id,
            admin: p.is_admin(),
        })
        .collect()
}

/// The catalog is in under the curtain: ask Core who there is, then open
/// on the picker, or carry on as the start would have when it does not
/// show. `restore` says whether the saved screen comes back afterwards.
pub(crate) fn begin(ctx: &Ctx, app: &App, restore: bool) {
    let ticket = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.profile_picker;
        model.start = if restore {
            Start::ThenRestore
        } else {
            Start::ThenHub
        };
        model.ticket = model.ticket.wrapping_add(1);
        model.ticket
    };
    // A Core that never answers lifts the curtain onto the Hub.
    crate::router::arm_restore_timeout(ctx, app);
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let (profiles, active, kiosk, settings) = tokio::join!(
            client.profiles(),
            client.profiles_active(),
            client.kiosk(),
            client.settings(),
        );
        let profiles = match profiles {
            Ok(result) => Some(result.profiles),
            Err(error) => {
                tracing::warn!("profiles for the picker failed: {}", error.message);
                None
            }
        };
        // A Core without kiosk mode answers with an error: off.
        let kiosk = kiosk.unwrap_or_default();
        let require = settings
            .ok()
            .and_then(|s| s.profiles_require_for_launch)
            .unwrap_or(false);
        let active = active.ok().flatten().map(|p| p.profile_id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            landed(
                &ctx2,
                &app,
                ticket,
                profiles,
                active.as_deref(),
                kiosk,
                require,
            );
        });
    });
}

fn landed(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    profiles: Option<Vec<Profile>>,
    active: Option<&str>,
    kiosk: KioskState,
    require: bool,
) {
    // The restore timed out or Core went away meanwhile: the Hub is up.
    if lock(&ctx.shared).profile_picker.ticket != ticket
        || !app.global::<crate::Shell>().get_boot_curtain()
    {
        return;
    }
    let shows = {
        let mut shared = lock(&ctx.shared);
        let shared = &mut *shared;
        let profiles = profiles.unwrap_or_default();
        if shared.core.profiles.is_none() {
            shared.core.profiles = Some(profiles.clone());
        }
        let list = entries(&profiles);
        let rule_kiosk = kiosk_of(&kiosk);
        shared.kiosk = kiosk;
        let kiosk = rule_kiosk;
        let rows = rules::rows(&list, require, kiosk);
        let shows = rules::shows(&rows, kiosk);
        let index = rules::initial(&rows, &list, active);
        let model = &mut shared.profile_picker;
        if shows {
            model.rows = rows;
            model.index = index;
            model.top = 0;
            model.busy = false;
            model.require_for_launch = require;
            model.profiles = profiles;
        }
        shows
    };
    if !shows {
        carry_on(ctx, app);
        return;
    }
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::ProfilePicker, 1);
}

/// The fetch `begin` started last, for tests that answer it themselves.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn ticket_for_test(shared: &Shared) -> u64 {
    shared.profile_picker.ticket
}

/// Core's answer to a `begin` fetch, for tests.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn landed_for_test(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    profiles: Vec<Profile>,
    active: Option<&str>,
    kiosk: KioskState,
) {
    landed(ctx, app, ticket, Some(profiles), active, kiosk, false);
}

/// The kiosk lock changed: the open picker lists what the lock allows now.
pub(crate) fn kiosk_changed(ctx: &Ctx, app: &App) {
    if !is_active(app) {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let shared = &mut *shared;
        let kiosk = kiosk_of(&shared.kiosk);
        let model = &mut shared.profile_picker;
        let focused = model.rows.get(model.index).copied();
        let list = entries(&model.profiles);
        model.rows = rules::rows(&list, model.require_for_launch, kiosk);
        model.index = focused
            .and_then(|row| model.rows.iter().position(|r| *r == row))
            .unwrap_or(0);
    }
    render(ctx, app);
}

/// Go on to the screen the start would have shown: the saved one where
/// there is one, otherwise the Hub. A restore that cannot route, or has
/// nothing left to restore, lands on the Hub.
fn carry_on(ctx: &Ctx, app: &App) {
    let restore = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.profile_picker;
        model.busy = false;
        std::mem::take(&mut model.start) == Start::ThenRestore
    };
    if restore {
        crate::restore_target(ctx, app);
    }
    let shell = app.global::<crate::Shell>();
    if shell.get_transitioning() {
        return;
    }
    if is_active(app) || shell.get_boot_curtain() {
        crate::router::return_to_hub(ctx, app);
    }
}

// ---------- Paint ----------

fn rotated(shared: &Shared) -> bool {
    matches!(shared.persist.settings.orientation.as_str(), "cw" | "ccw")
}

fn geometry(ctx: &Ctx, app: &App) -> list_rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let rotated = rotated(&lock(&ctx.shared));
    crate::games::list_geometry_for(
        &inputs,
        rotated,
        0,
        list_rules::list_visible_rows(ctx.crt_enabled, rotated),
    )
}

fn visible(ctx: &Ctx, app: &App) -> usize {
    geometry(ctx, app).visible_rows.max(1)
}

fn cell(app: &App, shared: &Shared, row: Row) -> GridCell {
    let labels = app.global::<crate::Labels>();
    let active = shared
        .active_profile
        .as_ref()
        .map(|p| p.profile_id.as_str());
    let (key, name, is_active) = match row {
        Row::Profile(i) => {
            let profile = shared.profile_picker.profiles.get(i);
            let name = profile.map(|p| p.name.as_str()).unwrap_or_default();
            let id = profile.map(|p| p.profile_id.as_str());
            ("", name, id.is_some() && id == active)
        }
        Row::Shared => ("shared", "", active.is_none()),
    };
    GridCell {
        name: labels.invoke_profile_row(SharedString::from(key), SharedString::from(name)),
        tags: if is_active {
            labels.invoke_profile_active()
        } else {
            SharedString::default()
        },
        ..Default::default()
    }
}

/// Paint the picker from its model.
pub(crate) fn render(ctx: &Ctx, app: &App) {
    let g = geometry(ctx, app);
    let visible = g.visible_rows.max(1);
    let view = app.global::<ProfilePickerView>();
    view.set_card_x(g.card_x as f32);
    view.set_card_y(g.card_y as f32);
    view.set_card_width(g.card_width as f32);
    view.set_card_height(g.card_height as f32);
    view.set_row_height(g.row_height as f32);
    let mut shared = lock(&ctx.shared);
    let cells: Vec<GridCell> = shared
        .profile_picker
        .rows
        .iter()
        .map(|row| cell(app, &shared, *row))
        .collect();
    let shared = &mut *shared;
    let model = &mut shared.profile_picker;
    let count = cells.len();
    let current = model.index.min(count.saturating_sub(1));
    model.top = crate::browse_motion::publish_list(
        &view,
        &shared.input,
        current,
        count,
        visible,
        model.top,
        &cells,
        Clone::clone,
    );
}

/// The scene changed size: refit and repaint.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    if is_active(app) {
        render(ctx, app);
    }
}

/// The active profile changed: its row's marker moves with it.
pub(crate) fn active_changed(ctx: &Ctx, app: &App) {
    relayout(ctx, app);
}

// ---------- Input ----------

fn move_by(ctx: &Ctx, app: &App, delta: i64) {
    let held = list_rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.profile_picker;
        let count = model.rows.len();
        match list_rules::linear_move(model.index, count, delta, false, held) {
            list_rules::LinearMove::To { index, .. } => {
                model.index = index;
                true
            }
            list_rules::LinearMove::WrapToTail => {
                model.index = count.saturating_sub(1);
                true
            }
            list_rules::LinearMove::FetchMore | list_rules::LinearMove::Stay { .. } => false,
        }
    };
    if moved {
        render(ctx, app);
    }
}

/// What picking the focused row asks Core for.
enum Pick {
    /// It is the active profile already: nothing to switch.
    Stay,
    Switch(Profile),
    Leave,
}

fn pick_focused(ctx: &Ctx, app: &App) {
    let pick = {
        let mut shared = lock(&ctx.shared);
        let active = shared.active_profile.as_ref().map(|p| p.profile_id.clone());
        let model = &mut shared.profile_picker;
        let pick = match model.rows.get(model.index) {
            Some(Row::Profile(i)) => match model.profiles.get(*i) {
                Some(p) if active.as_deref() == Some(p.profile_id.as_str()) => Pick::Stay,
                Some(p) => Pick::Switch(p.clone()),
                None => return,
            },
            Some(Row::Shared) if active.is_none() => Pick::Stay,
            Some(Row::Shared) => Pick::Leave,
            None => return,
        };
        // A PIN panel waits on the user, and holds input itself.
        model.busy = match &pick {
            Pick::Switch(p) => !p.has_pin,
            Pick::Leave => true,
            Pick::Stay => false,
        };
        pick
    };
    let done = |ctx: &Ctx, app: &App, ok: bool| {
        lock(&ctx.shared).profile_picker.busy = false;
        if ok && is_active(app) {
            carry_on(ctx, app);
        }
    };
    match pick {
        Pick::Stay => carry_on(ctx, app),
        Pick::Switch(profile) => crate::pin::switch_then(ctx, app, &profile, done),
        Pick::Leave => crate::pin::leave_then(ctx, app, done),
    }
}

pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if lock(&ctx.shared).profile_picker.busy {
        return;
    }
    let page = i64::try_from(visible(ctx, app)).unwrap_or(10);
    let dir: i64 = if matches!(action, actions::UP | actions::LEFT | actions::PAGE_PREV) {
        -1
    } else {
        1
    };
    let step = if crate::input::rapid_page(ctx) {
        page
    } else {
        1
    };
    match action {
        actions::UP | actions::DOWN => move_by(ctx, app, dir * step),
        actions::PAGE_PREV | actions::PAGE_NEXT | actions::LEFT | actions::RIGHT => {
            move_by(ctx, app, dir * page);
        }
        actions::ACCEPT => pick_focused(ctx, app),
        // Back keeps whoever is active and carries on.
        actions::CANCEL => carry_on(ctx, app),
        _ => {}
    }
}

// ---------- Pointer ----------

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    if !is_active(app) || shell.get_transitioning() || crate::input::modal_open(app) {
        return false;
    }
    let shared = lock(&ctx.shared);
    shared.persist.settings.mouse_enabled && !shared.profile_picker.busy
}

/// Point the cursor at a row from the view, in the list's own order.
fn point_at(ctx: &Ctx, app: &App, index: i32) -> bool {
    let top = app.global::<ProfilePickerView>().get_list_view_top();
    let Ok(row) = usize::try_from(top + index) else {
        return false;
    };
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.profile_picker;
    if row >= model.rows.len() {
        return false;
    }
    model.index = row;
    true
}

/// Wire the view's pointer callbacks.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<ProfilePickerInput>();
    let on = |f: fn(&Ctx, &App, i32)| {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        move |index: i32| {
            if let Some(app) = weak.upgrade() {
                if pointer_allowed(&ctx, &app) {
                    f(&ctx, &app, index);
                }
            }
        }
    };
    input.on_row_hovered(on(|ctx, app, index| {
        let before = lock(&ctx.shared).profile_picker.index;
        if point_at(ctx, app, index) && lock(&ctx.shared).profile_picker.index != before {
            render(ctx, app);
        }
    }));
    input.on_row_clicked(on(|ctx, app, index| {
        if point_at(ctx, app, index) {
            pick_focused(ctx, app);
        }
    }));
    input.on_wheel(on(|ctx, app, delta| {
        move_by(ctx, app, i64::from(delta.signum()));
    }));
}
