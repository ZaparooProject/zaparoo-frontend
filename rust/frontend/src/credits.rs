// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Credits screen driver, opened from Settings > About: every component
// the frontend is built from (its own license, the fonts, artwork and data
// it includes, and the software it links) as a list, and one component's
// full license text on a reading page of the same screen. The text is the
// `LICENSE.txt` the release bundles ship, embedded compressed by `build.rs`
// and inflated only while the screen is open. The list moves as the file
// browser's does (`browse_motion`, the selection debounce); the reading
// page scrolls as About does. Rules live in `zaparoo_app::credits`.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::credits::{Block, Credits};
use zaparoo_app::media_list::{self as list_rules, SelectionPersist};
use zaparoo_core::input_actions::actions;

use crate::navigation::EntryMode;
use crate::router::{lock, Ctx, Shared};
use crate::{App, CreditsBlock, CreditsInput, CreditsPage, CreditsView, GridCell};

/// `rust/frontend/LICENSE.txt`, deflated by `build.rs`.
static LICENSE_DEFLATED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/LICENSE.txt.deflate"));

/// One step of the reading page, in thousandths of the screen's height:
/// About's.
const READ_STEP: i32 = 80;

#[derive(Debug, Default)]
pub(crate) struct Model {
    /// The parsed file, held only while the screen is open.
    credits: Option<Arc<Credits>>,
    index: usize,
    list_top: usize,
    reading: bool,
    persist: SelectionPersist,
    persist_seq: u64,
}

/// Inflate and read the embedded file. It is checked against the parser
/// by `zaparoo_app`'s tests, so a failure here is a broken build.
fn load() -> Option<Arc<Credits>> {
    let bytes = match miniz_oxide::inflate::decompress_to_vec(LICENSE_DEFLATED) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!("credits: embedded license text does not inflate: {err:?}");
            return None;
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            tracing::error!("credits: embedded license text is not UTF-8: {err}");
            return None;
        }
    };
    match Credits::parse(text) {
        Ok(credits) => Some(Arc::new(credits)),
        Err(err) => {
            tracing::error!("credits: embedded license text does not parse: {err:?}");
            None
        }
    }
}

fn credits(shared: &mut Shared) -> Option<Arc<Credits>> {
    if shared.credits.credits.is_none() {
        shared.credits.credits = load();
    }
    shared.credits.credits.clone()
}

/// Open Credits from its Settings row (`Fresh`, at the top of the list) or
/// on a cold start (`Restore`, where the last run left it).
pub(crate) fn enter(ctx: &Ctx, app: &App, entry: EntryMode) {
    {
        let mut shared = lock(&ctx.shared);
        let Some(credits) = credits(&mut shared) else {
            return;
        };
        shared.persist.active_screen = crate::Screen::Credits.token().to_string();
        if entry == EntryMode::Fresh {
            shared.persist.credits = zaparoo_core::persist::CreditsState::default();
        }
        let saved = shared.persist.credits.clone();
        let index = credits.position(&saved.selected).unwrap_or(0);
        let model = &mut shared.credits;
        model.index = index;
        model.list_top = saved.list_top.unwrap_or(0);
        model.reading = saved.reading && !credits.components().is_empty();
        model.persist = SelectionPersist::new();
    }
    let scroll = lock(&ctx.shared).persist.credits.scroll_milli.min(1000);
    publish_reading(ctx, app);
    app.global::<CreditsView>()
        .set_scroll_milli(i32::try_from(scroll).unwrap_or(0));
    render(ctx, app);
    crate::router::save_persist(&ctx.shared);
    crate::router::transition_to_screen(app, crate::Screen::Credits, 1);
}

/// Back onto the Settings row that opened the screen, letting go of the
/// text.
fn leave(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    {
        let mut shared = lock(&ctx.shared);
        shared.credits.credits = None;
        shared.persist.active_screen = crate::Screen::Settings.token().to_string();
    }
    let view = app.global::<CreditsView>();
    view.set_blocks(ModelRc::new(VecModel::from(Vec::<CreditsBlock>::new())));
    crate::router::save_persist(&ctx.shared);
    crate::settings::show_return(ctx, app, crate::SettingsPage::About, "credits");
}

// ---------- Persistence ----------

fn write_selection(shared: &mut Shared) {
    let model = &shared.credits;
    let selected = model
        .credits
        .as_ref()
        .and_then(|credits| {
            credits
                .components()
                .get(model.index)
                .map(|c| credits.name(c).to_string())
        })
        .unwrap_or_default();
    let list_top = model.list_top;
    let state = &mut shared.persist.credits;
    state.selected = selected;
    state.list_top = Some(list_top);
}

/// A move landed: write the selection once it rests, as the browse lists
/// do (`PERSIST_DEBOUNCE_MS`).
fn schedule_persist(ctx: &Ctx) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.credits;
        let key = model.index.to_string();
        if !model.persist.schedule(&key) {
            return;
        }
        model.persist_seq += 1;
        model.persist_seq
    };
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(list_rules::PERSIST_DEBOUNCE_MS),
        move || {
            if lock(&ctx.shared).credits.persist_seq == seq {
                flush_persist(&ctx);
            }
        },
    );
}

/// Commit a scheduled selection write now (Accept, Back, hold release).
fn flush_persist(ctx: &Ctx) {
    let pending = {
        let mut shared = lock(&ctx.shared);
        shared.credits.persist_seq += 1;
        let pending = shared.credits.persist.flush().is_some();
        if pending {
            write_selection(&mut shared);
        }
        pending
    };
    if pending {
        crate::router::save_persist(&ctx.shared);
    }
}

// ---------- Geometry and rendering ----------

fn rotated(ctx: &Ctx) -> bool {
    matches!(
        lock(&ctx.shared).persist.settings.orientation.as_str(),
        "cw" | "ccw"
    )
}

fn list_geometry(ctx: &Ctx, app: &App) -> list_rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let rotated = rotated(ctx);
    crate::games::list_geometry_for(
        &inputs,
        rotated,
        0,
        list_rules::list_visible_rows(ctx.crt_enabled, rotated),
    )
}

fn list_visible(ctx: &Ctx, app: &App) -> usize {
    list_geometry(ctx, app).visible_rows.max(1)
}

fn cell_for(credits: &Credits, index: usize) -> GridCell {
    let component = &credits.components()[index];
    GridCell {
        name: SharedString::from(credits.name(component)),
        tags: SharedString::from(credits.licenses(component).join(" / ")),
        opens: true,
        ..Default::default()
    }
}

/// Paint the page on show: the list's window and cues, and which page it is.
pub(crate) fn render(ctx: &Ctx, app: &App) {
    let visible = list_visible(ctx, app);
    let geometry = list_geometry(ctx, app);
    let mut shared = lock(&ctx.shared);
    let Some(credits) = shared.credits.credits.clone() else {
        return;
    };
    let view = app.global::<CreditsView>();
    let model = &shared.credits;
    let count = credits.components().len();
    let current = model.index.min(count.saturating_sub(1));
    view.set_page(if model.reading {
        CreditsPage::Reading
    } else {
        CreditsPage::List
    });
    view.set_focus_ready(true);
    view.set_count(i32::try_from(count).unwrap_or(i32::MAX));
    view.set_current_index(i32::try_from(current).unwrap_or(0));
    let paging = list_rules::list_paging(current, count, Some(count), true, visible, false);
    view.set_list_page(i32::try_from(paging.current_page).unwrap_or(0));
    view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
    view.set_has_items_above(paging.has_items_above);
    view.set_has_items_below(paging.has_items_below);
    view.set_list_x(geometry.card_x as f32);
    view.set_list_y(geometry.card_y as f32);
    view.set_list_width(geometry.card_width as f32);
    view.set_list_height(geometry.card_height as f32);
    view.set_list_row_height(geometry.row_height as f32);
    let indices: Vec<usize> = (0..count).collect();
    let top = crate::browse_motion::publish_list(
        &view,
        &shared.input,
        current,
        count,
        visible,
        model.list_top,
        &indices,
        |&index| cell_for(&credits, index),
    );
    shared.credits.list_top = top;
}

/// The focused component's text, for the reading page.
fn publish_reading(ctx: &Ctx, app: &App) {
    let (credits, index) = {
        let shared = lock(&ctx.shared);
        (shared.credits.credits.clone(), shared.credits.index)
    };
    let Some(credits) = credits else {
        return;
    };
    let Some(component) = credits.components().get(index) else {
        return;
    };
    let blocks: Vec<CreditsBlock> = credits
        .blocks(component)
        .into_iter()
        .map(|block| match block {
            Block::Heading(text) => CreditsBlock {
                text: SharedString::from(text),
                heading: true,
            },
            Block::Paragraph(text) => CreditsBlock {
                text: SharedString::from(text.as_ref()),
                heading: false,
            },
        })
        .collect();
    let view = app.global::<CreditsView>();
    view.set_title(SharedString::from(credits.name(component)));
    view.set_blocks(ModelRc::new(VecModel::from(blocks)));
}

/// The scene changed size: refit the list and repaint.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_active_screen() == crate::Screen::Credits {
        render(ctx, app);
    }
}

// ---------- Input ----------

/// A list move: a page step lands on the end before wrapping, and a held
/// one stops there.
fn list_move(ctx: &Ctx, app: &App, delta: i64) {
    let held = list_rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let moved = {
        let mut shared = lock(&ctx.shared);
        let count = shared
            .credits
            .credits
            .as_ref()
            .map_or(0, |credits| credits.components().len());
        let model = &mut shared.credits;
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
        schedule_persist(ctx);
        render(ctx, app);
    }
}

/// Open the focused component's text at its top.
fn open_reading(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    {
        let mut shared = lock(&ctx.shared);
        shared.credits.reading = true;
        shared.persist.credits.reading = true;
        shared.persist.credits.scroll_milli = 0;
    }
    publish_reading(ctx, app);
    app.global::<CreditsView>().set_scroll_milli(0);
    render(ctx, app);
    crate::router::save_persist(&ctx.shared);
}

/// Back from the text onto its row.
fn close_reading(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.credits.reading = false;
        shared.persist.credits.reading = false;
        shared.persist.credits.scroll_milli = 0;
    }
    let view = app.global::<CreditsView>();
    view.set_blocks(ModelRc::new(VecModel::from(Vec::<CreditsBlock>::new())));
    view.set_scroll_milli(0);
    render(ctx, app);
    crate::router::save_persist(&ctx.shared);
}

fn reading_action(ctx: &Ctx, app: &App, action: &str) {
    let view = app.global::<CreditsView>();
    let page = view.get_page_milli().max(READ_STEP);
    match action {
        actions::UP | actions::DOWN => {
            let dir = if action == actions::UP { -1 } else { 1 };
            let step = if crate::input::rapid_page(ctx) {
                page
            } else {
                READ_STEP
            };
            view.invoke_move(dir * step);
        }
        actions::LEFT | actions::PAGE_PREV => view.invoke_move(-page),
        actions::RIGHT | actions::PAGE_NEXT => view.invoke_move(page),
        actions::CANCEL => close_reading(ctx, app),
        _ => {}
    }
}

pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if lock(&ctx.shared).credits.credits.is_none() {
        if action == actions::CANCEL {
            leave(ctx, app);
        }
        return;
    }
    if lock(&ctx.shared).credits.reading {
        reading_action(ctx, app, action);
        return;
    }
    let page = i64::try_from(list_visible(ctx, app)).unwrap_or(10);
    match action {
        actions::LEFT | actions::RIGHT | actions::PAGE_PREV | actions::PAGE_NEXT => {
            let dir: i64 = if matches!(action, actions::LEFT | actions::PAGE_PREV) {
                -1
            } else {
                1
            };
            list_move(ctx, app, dir * page);
        }
        actions::UP | actions::DOWN => {
            let dir: i64 = if action == actions::UP { -1 } else { 1 };
            if crate::input::rapid_page(ctx) {
                list_move(ctx, app, dir * page);
            } else {
                list_move(ctx, app, dir);
            }
        }
        actions::ACCEPT => open_reading(ctx, app),
        actions::CANCEL => leave(ctx, app),
        _ => {}
    }
}

// ---------- Pointer ----------

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    shell.get_active_screen() == crate::Screen::Credits
        && !shell.get_transitioning()
        && !crate::input::modal_open(app)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Move focus to the visible row `local`; false when there is none.
fn point_at(ctx: &Ctx, app: &App, local: i32) -> bool {
    let Ok(local) = usize::try_from(local) else {
        return false;
    };
    // The list reports rows from the start of its published window.
    let base = usize::try_from(app.global::<CreditsView>().get_list_view_top()).unwrap_or(0);
    let mut shared = lock(&ctx.shared);
    let count = shared
        .credits
        .credits
        .as_ref()
        .map_or(0, |credits| credits.components().len());
    let index = base + local;
    if index >= count {
        return false;
    }
    if shared.credits.index != index {
        shared.credits.index = index;
        drop(shared);
        schedule_persist(ctx);
    }
    true
}

/// Wire the view's pointer and scroll callbacks.
pub(crate) fn bind(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<CreditsInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_hovered(move |local| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, local) {
                render(&ctx, &app);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_clicked(move |local| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, local) {
                render(&ctx, &app);
                crate::router::handle_action(&ctx, &app, actions::ACCEPT);
            }
        });
    }
    // A wheel notch or a page chevron turns a page, as on the browse lists.
    let page = |ctx: Arc<Ctx>, weak: slint::Weak<App>| {
        move |delta: i32| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if delta == 0 || !pointer_allowed(&ctx, &app) {
                return;
            }
            let action = if delta > 0 {
                actions::PAGE_NEXT
            } else {
                actions::PAGE_PREV
            };
            handle_action(&ctx, &app, action);
        }
    };
    input.on_wheel(page(ctx.clone(), app.as_weak()));
    input.on_page_requested(page(ctx.clone(), app.as_weak()));
    let view = app.global::<CreditsView>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        view.on_scroll_requested(move |position| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let position = position.clamp(0, 1000);
            let changed = {
                let mut shared = lock(&ctx.shared);
                let previous = shared.persist.credits.scroll_milli;
                shared.persist.credits.scroll_milli = position.unsigned_abs();
                previous != shared.persist.credits.scroll_milli
            };
            app.global::<CreditsView>().set_scroll_milli(position);
            if changed {
                crate::router::save_persist(&ctx.shared);
            }
        });
    }
}
