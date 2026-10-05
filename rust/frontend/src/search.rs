// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Search screen driver: the query being typed, its system and tag
// filters, the pane of recent searches or live matches, and the routes in
// (the Hub tile, a folder's View menu) and out (results, back). The search
// itself is held in `persist.search`, so the results list and a cold
// start read the same one. Rules live in `zaparoo_app::search`.

use std::time::Duration;

use slint::{ComponentHandle, SharedString};
use zaparoo_app::keyboard::{Key, Layer, Layers};
use zaparoo_app::search::{self as rules, Count, Focus, Landing, Shape, Zone};
use zaparoo_core::endpoints::media_search::{MediaSearchEndpoint, SearchArgs};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::MediaSearchResult;
use zaparoo_core::remote_resource::ResourceStatus;

use crate::browse_filter::Target;
use crate::keyboard::{Keyboard, Press};
use crate::navigation::EntryMode;
use crate::router::{lock, menu_header, menu_row_keyed, menu_value, Ctx, ListContext, Shared};
use crate::{App, SearchInput, SearchPane, SearchPaneRow, SearchView, SearchZone};

/// The held West button or Backspace key: delete, repeating while held.
pub(crate) const DELETE: &str = zaparoo_app::input::TEXT_DELETE;
/// A held Accept on a key that types or deletes.
const KEY: &str = zaparoo_app::input::TEXT_KEY;

/// One live match in the pane.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Preview {
    name: String,
    system: String,
    path: String,
}

#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about the preview and the route, not one state"
)]
pub struct Model {
    keyboard: Keyboard,
    focus: Focus,
    preview: Vec<Preview>,
    count: Option<Count>,
    searching: bool,
    failed: bool,
    /// Bumped per edit so a late preview cannot fill a newer search.
    seq: u64,
    task: crate::scoped_task::ScopedTask,
    /// First pane row on show.
    pane_top: usize,
    /// The key held down for its press cue, and the ticket that lifts it.
    pressed: Option<usize>,
    press_seq: u64,
    /// The browse folder a scoped search opened on is still loaded behind
    /// it, so Back can return without refetching.
    browse_intact: bool,
    /// The system changed: once its tag list lands, drop the tags it lacks.
    prune_pending: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            keyboard: Keyboard::new(Layers::Basic, rules::QUERY_MAX_CHARS),
            focus: Focus::default(),
            preview: Vec::new(),
            count: None,
            searching: false,
            failed: false,
            seq: 0,
            task: crate::scoped_task::ScopedTask::default(),
            pane_top: 0,
            pressed: None,
            press_seq: 0,
            browse_intact: false,
            prune_pending: false,
        }
    }
}

/// The search as Core's request, shared by the preview and the results so
/// both read one cached first page.
pub(crate) fn args(shared: &Shared) -> SearchArgs {
    let search = &shared.persist.search;
    SearchArgs {
        query: rules::query_text(&search.query).to_string(),
        system_id: search.system_id.clone(),
        tags: zaparoo_app::browse_filter::normalize(&search.tags),
        path_prefix: search.scope_path.clone(),
        max_results: rules::PREVIEW_LIMIT,
        sort: rules::RESULT_SORT.to_string(),
    }
}

fn can_search(shared: &Shared) -> bool {
    let search = &shared.persist.search;
    rules::can_search(
        &search.query,
        &search.system_id,
        &search.tags,
        &search.scope_path,
    )
}

/// The system's name as every other screen words it.
fn system_name(shared: &Shared, system_id: &str) -> String {
    crate::systems::display_name(shared, system_id)
}

fn tag_labels(shared: &Shared, tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|tag| crate::browse_filter::search_tag_label(shared, tag))
        .collect()
}

/// The results screen's title: the query in quotes, then whatever narrows
/// it. Empty when a search has nothing to name, which the view words.
pub(crate) fn results_title(shared: &Shared) -> String {
    let search = &shared.persist.search;
    let mut parts = Vec::new();
    let query = rules::query_text(&search.query);
    if !query.is_empty() {
        parts.push(format!("\u{201c}{query}\u{201d}"));
    }
    if !search.system_id.is_empty() {
        parts.push(system_name(shared, &search.system_id));
    }
    if !search.scope_name.is_empty() {
        parts.push(search.scope_name.clone());
    }
    parts.extend(tag_labels(shared, &search.tags));
    parts.join(" \u{b7} ")
}

/// Whether the pane has room: not at 240p, and not on a rotated scene or
/// one narrower than 4:3, where the keyboard needs the whole width.
fn pane_collapsed(app: &App) -> bool {
    let sizing = app.global::<crate::Sizing>();
    sizing.get_tier_240()
        || sizing.get_swap_axes()
        || sizing.get_screen_width() < sizing.get_screen_height() * 1.3
}

/// What the pane lists: recent searches while there is nothing to search
/// on, matches once there is.
fn pane_kind(shared: &Shared) -> SearchPane {
    if can_search(shared) {
        SearchPane::Preview
    } else if shared.persist.search.scoped {
        SearchPane::None
    } else {
        SearchPane::Recents
    }
}

fn pane_rows(shared: &Shared) -> Vec<SearchPaneRow> {
    let row = |title: &str, detail: &str| SearchPaneRow {
        title: SharedString::from(title),
        detail: SharedString::from(detail),
    };
    match pane_kind(shared) {
        SearchPane::None => Vec::new(),
        SearchPane::Preview => shared
            .search
            .preview
            .iter()
            .take(rules::PANE_ROWS)
            .map(|p| {
                // The system is worth naming only when several are searched.
                let detail = if shared.persist.search.system_id.is_empty() {
                    p.system.as_str()
                } else {
                    ""
                };
                row(&p.name, detail)
            })
            .collect(),
        SearchPane::Recents => shared
            .persist
            .search
            .recent
            .iter()
            .map(|recent| {
                let mut filters = Vec::new();
                if !recent.system_id.is_empty() {
                    filters.push(system_name(shared, &recent.system_id));
                }
                filters.extend(recent.tags.iter().map(|tag| {
                    let value = tag.split_once(':').map_or(tag.as_str(), |(_, v)| v);
                    zaparoo_app::browse_filter::display_label("", value)
                }));
                let filters = filters.join(" \u{b7} ");
                if recent.query.is_empty() {
                    row(&filters, "")
                } else {
                    row(&recent.query, &filters)
                }
            })
            .collect(),
    }
}

/// The recent searches end in a Clear row, one more stop past the rows.
fn pane_has_clear(shared: &Shared) -> bool {
    pane_kind(shared) == SearchPane::Recents && !shared.persist.search.recent.is_empty()
}

fn shape(app: &App, shared: &Shared) -> Shape {
    Shape {
        scoped: shared.persist.search.scoped,
        pane_rows: if pane_collapsed(app) {
            0
        } else {
            pane_rows(shared).len() + usize::from(pane_has_clear(shared))
        },
    }
}

fn zone(zone: Zone) -> SearchZone {
    match zone {
        Zone::System => SearchZone::System,
        Zone::Filter => SearchZone::Filter,
        Zone::Keys => SearchZone::Keys,
        Zone::Pane => SearchZone::Pane,
    }
}

/// Paint the screen from the model. Cheap enough to run on every press.
pub(crate) fn render(ctx: &Ctx, app: &App) {
    let collapsed = pane_collapsed(app);
    let mut shared = lock(&ctx.shared);
    let shape = shape(app, &shared);
    shared.search.focus = rules::settle(shared.search.focus, shape);
    let focus = shared.search.focus;
    // Keep the focused pane row inside the window; the Clear row sits
    // below the rows and scrolls nothing.
    let rows = pane_rows(&shared);
    let visible = usize::try_from(app.global::<SearchView>().get_pane_fit())
        .unwrap_or(1)
        .max(1);
    let on_clear = focus.zone == Zone::Pane && focus.pane_index >= rows.len();
    let listed = focus.pane_index.min(rows.len().saturating_sub(1));
    shared.search.pane_top = if focus.zone == Zone::Pane {
        rules::window_top(shared.search.pane_top, listed, rows.len(), visible)
    } else {
        0
    };
    let shared = &*shared;
    let search = &shared.persist.search;
    let model = &shared.search;
    let view = app.global::<SearchView>();
    let (before, at, after) = model.keyboard.display();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
    view.set_scoped(search.scoped);
    view.set_system_name(SharedString::from(if search.system_id.is_empty() {
        String::new()
    } else {
        system_name(shared, &search.system_id)
    }));
    view.set_scope_name(SharedString::from(search.scope_name.as_str()));
    view.set_filter_text(SharedString::from(
        crate::browse_filter::search_summary(shared).unwrap_or_default(),
    ));
    crate::view_model::publish(&view.get_keys(), model.keyboard.cells(), |rows| {
        view.set_keys(rows);
    });
    view.set_key_rows(i32::try_from(model.keyboard.row_count()).unwrap_or(4));
    view.set_shift_active(model.keyboard.layer() == Layer::Upper);
    view.set_symbols_active(model.keyboard.layer() == Layer::Symbols);
    view.set_key_index(i32::try_from(model.keyboard.index()).unwrap_or(0));
    view.set_key_kind(match model.keyboard.key() {
        Some(Key::Submit) => crate::KeyKind::Submit,
        Some(Key::Space) => crate::KeyKind::Space,
        Some(Key::Backspace) => crate::KeyKind::Backspace,
        Some(Key::Shift) => crate::KeyKind::Shift,
        Some(Key::Symbols) => crate::KeyKind::Symbols,
        Some(Key::Char(_)) | None => crate::KeyKind::Char,
    });
    view.set_pressed_key(
        model
            .pressed
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1),
    );
    view.set_zone(zone(focus.zone));
    view.set_pane_index(i32::try_from(focus.pane_index).unwrap_or(0));
    view.set_pane_top(i32::try_from(model.pane_top).unwrap_or(0));
    view.set_pane_clear(pane_has_clear(shared));
    view.set_pane_on_clear(on_clear);
    view.set_pane(pane_kind(shared));
    crate::view_model::publish(&view.get_pane_rows(), rows, |rows| {
        view.set_pane_rows(rows);
    });
    view.set_pane_collapsed(collapsed);
    let (known, count, more) = match model.count {
        Some(Count::Exact(n)) => (true, n, false),
        Some(Count::AtLeast(n)) => (true, n, true),
        None => (false, 0, false),
    };
    view.set_count_known(known);
    view.set_count(i32::try_from(count).unwrap_or(i32::MAX));
    view.set_count_more(more);
    view.set_searching(model.searching);
    view.set_failed(model.failed);
    view.set_can_search(can_search(shared));
}

/// Show the screen with the model as it stands.
fn show(ctx: &Ctx, app: &App, direction: i32) {
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.active_screen = crate::Screen::Search.token().to_string();
        let query = shared.persist.search.query.clone();
        shared.search.keyboard.set_text(&query);
        shared.search.pressed = None;
    }
    crate::router::save_persist(&ctx.shared);
    crate::browse_filter::begin_for(ctx, app, Target::Search);
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::Search, direction);
    refresh_preview(ctx, app, false);
}

fn reset_model(shared: &mut Shared) {
    shared.search.task.cancel();
    let seq = shared.search.seq.wrapping_add(1);
    shared.search = Model {
        seq,
        ..Model::default()
    };
}

/// The Hub's Search tile. A fresh visit starts empty; a restore keeps the
/// search a cold start found on disk.
pub fn enter(ctx: &Ctx, app: &App, entry: EntryMode) {
    {
        let mut shared = lock(&ctx.shared);
        reset_model(&mut shared);
        if entry == EntryMode::Fresh {
            shared.persist.search.reset();
        }
    }
    show(ctx, app, 1);
}

/// "Search here…" from a browse folder's View menu: the same screen,
/// limited to the system on screen and, inside a folder, to that folder.
pub(crate) fn enter_scoped(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.games.mode != crate::GamesMode::Browse || shared.games.system_id.is_empty() {
            return;
        }
        reset_model(&mut shared);
        shared.search.browse_intact = true;
        let system_id = shared.games.system_id.clone();
        let path = shared.games.browse_path.clone();
        let in_folder =
            zaparoo_app::media_list::at_folder_level(shared.persist.games.path_stack.len())
                && !path.is_empty();
        let search = &mut shared.persist.search;
        search.reset();
        search.scoped = true;
        search.system_id = system_id;
        if in_folder {
            search.scope_name = zaparoo_app::media_list::folder_name_for_path(&path);
            search.scope_path = path;
        }
    }
    crate::games::flush_persist(ctx);
    show(ctx, app, 1);
}

/// Cold start on the results screen: rebuild the search behind it, then
/// fill the results where they were left.
pub fn restore_results(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        reset_model(&mut shared);
        let query = shared.persist.search.query.clone();
        shared.search.keyboard.set_text(&query);
        shared.search.keyboard.focus(Key::Submit);
    }
    crate::games::enter_search(ctx, app, EntryMode::Restore);
}

/// Back from the results: the search as it was, focus on the Search key.
pub(crate) fn return_from_results(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).search.keyboard.focus(Key::Submit);
    show(ctx, app, -1);
}

/// The source screen came back after a cancelled route into the results.
pub(crate) fn resume(ctx: &Ctx, app: &App) {
    render(ctx, app);
}

/// Whether typed text belongs to the query right now: the screen is up
/// with nothing over it.
pub(crate) fn owns_input(app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    shell.get_active_screen() == crate::Screen::Search
        && !shell.get_transitioning()
        && !shell.get_saver_armed()
        && !shell.get_boot_curtain()
        && !crate::input::modal_open(app)
}

/// The query changed: keep the persisted copy in step and refetch.
fn edited(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let text = shared.search.keyboard.text().to_string();
        shared.persist.search.query = text;
    }
    render(ctx, app);
    refresh_preview(ctx, app, true);
}

/// A character from a physical keyboard. Focus moves to the Search key so
/// Enter runs the search.
pub(crate) fn type_char(ctx: &Ctx, app: &App, c: char) {
    let press = {
        let mut shared = lock(&ctx.shared);
        let press = shared.search.keyboard.insert(c);
        shared.search.focus.zone = Zone::Keys;
        shared.search.keyboard.focus(Key::Submit);
        press
    };
    if press == Press::Edited {
        edited(ctx, app);
    } else {
        render(ctx, app);
    }
}

fn apply_press(ctx: &Ctx, app: &App, press: Press) {
    match press {
        Press::Edited => edited(ctx, app),
        Press::Layer | Press::None => render(ctx, app),
        Press::Submit => submit(ctx, app),
    }
}

/// Hold the pressed key down for the press cue, then lift it.
fn flash_key(ctx: &Ctx, app: &App) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        shared.search.pressed = Some(shared.search.keyboard.index());
        shared.search.press_seq = shared.search.press_seq.wrapping_add(1);
        shared.search.press_seq
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(
        Duration::from_millis(zaparoo_app::input::PRESS_FEEDBACK_MS),
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            {
                let mut shared = lock(&ctx.shared);
                if shared.search.press_seq != seq {
                    return;
                }
                shared.search.pressed = None;
            }
            app.global::<SearchView>().set_pressed_key(-1);
        },
    );
}

/// Run the search: record it and open the results.
fn submit(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if !can_search(&shared) {
            drop(shared);
            render(ctx, app);
            return;
        }
        shared.search.task.cancel();
        shared.search.seq = shared.search.seq.wrapping_add(1);
        shared.search.browse_intact = false;
        let query = rules::query_text(&shared.persist.search.query).to_string();
        shared.persist.search.query = query;
        shared.persist.search.remember();
    }
    render(ctx, app);
    crate::games::enter_search(ctx, app, EntryMode::Fresh);
}

/// Open the results focused on one previewed match.
fn open_match(ctx: &Ctx, app: &App, index: usize) {
    let path = {
        let mut shared = lock(&ctx.shared);
        let Some(path) = shared.search.preview.get(index).map(|p| p.path.clone()) else {
            return;
        };
        shared.search.task.cancel();
        shared.search.seq = shared.search.seq.wrapping_add(1);
        shared.search.browse_intact = false;
        shared.persist.search.remember();
        path
    };
    crate::games::enter_search_at(ctx, app, &path);
}

/// Accept on a pane row.
fn accept_pane(ctx: &Ctx, app: &App, index: usize) {
    let kind = pane_kind(&lock(&ctx.shared));
    match kind {
        SearchPane::Preview => open_match(ctx, app, index),
        SearchPane::Recents => {
            let picked = {
                let mut shared = lock(&ctx.shared);
                let Some(recent) = shared.persist.search.recent.get(index).cloned() else {
                    // The stop past the last search is the Clear row.
                    shared.persist.search.recent.clear();
                    shared.search.focus = Focus::default();
                    drop(shared);
                    crate::router::save_persist(&ctx.shared);
                    render(ctx, app);
                    return;
                };
                let search = &mut shared.persist.search;
                search.query.clone_from(&recent.query);
                search.system_id.clone_from(&recent.system_id);
                search.tags.clone_from(&recent.tags);
                shared.search.keyboard.set_text(&recent.query);
                shared.search.focus = Focus::default();
                shared.search.keyboard.focus(Key::Submit);
                recent
            };
            tracing::debug!(query = %picked.query, "recent search picked");
            submit(ctx, app);
        }
        SearchPane::None => {}
    }
}

/// Accept on the Filter field: open the picker, where a category set to
/// Any drops its value and Clear filter drops them all.
fn open_filter(ctx: &Ctx, app: &App) {
    crate::browse_filter::begin_for(ctx, app, Target::Search);
    crate::browse_filter::open(ctx, app);
}

/// Systems used lately, most recent first: the recent searches' own, then
/// the ones last browsed.
fn recent_systems(shared: &Shared) -> Vec<String> {
    shared
        .persist
        .search
        .recent
        .iter()
        .map(|r| r.system_id.clone())
        .chain(
            shared
                .persist
                .games
                .system_focus
                .iter()
                .map(|f| f.system_id.clone()),
        )
        .filter(|id| !id.is_empty())
        .collect()
}

/// The system picker: every system with games under its manufacturer,
/// behind All systems and the ones used lately.
fn open_system_picker(ctx: &Ctx, app: &App) {
    use zaparoo_app::system_picker::{self as picker, Row, Section};
    let (rows, index) = {
        let shared = lock(&ctx.shared);
        let current = shared.persist.search.system_id.clone();
        let systems: Vec<picker::System> = shared
            .systems
            .iter()
            .filter(|s| s.zap_script.trim().is_empty() && s.media_count != Some(0))
            .map(|s| picker::System {
                id: s.id.clone(),
                name: system_name(&shared, &s.id),
                manufacturer: s.manufacturer.clone().unwrap_or_default(),
                games: s.media_count,
            })
            .collect();
        let mut rows = vec![menu_row_keyed("all", "search:all_systems", "")];
        let mut index = 0;
        for row in picker::rows(&systems, &recent_systems(&shared)) {
            rows.push(match row {
                Row::Header(Section::Recent) => menu_header("section:recent", ""),
                Row::Header(Section::Other) => menu_header("section:other", ""),
                Row::Header(Section::Manufacturer(name)) => menu_header("", &name),
                Row::System(system) => {
                    // Focus the system's own row, not its Recent copy.
                    if system.id == current {
                        index = rows.len();
                    }
                    menu_value(
                        &format!("sys:{}", system.id),
                        "",
                        &system.name,
                        &system.games.map(|n| n.to_string()).unwrap_or_default(),
                        "",
                    )
                }
            });
        }
        (rows, index)
    };
    crate::router::present_form_list(
        ctx,
        app,
        ListContext::SearchSystem,
        "title:search_system",
        rows,
        index,
    );
}

/// A row of the system picker was chosen.
pub(crate) fn system_picked(ctx: &Ctx, app: &App, id: &str) {
    let changed = {
        let mut shared = lock(&ctx.shared);
        let next = id.strip_prefix("sys:").unwrap_or_default().to_string();
        let changed = shared.persist.search.system_id != next;
        shared.persist.search.system_id = next;
        shared.search.prune_pending = changed;
        changed
    };
    if !changed {
        return;
    }
    // The new system's tag list prunes the chips once it lands.
    crate::browse_filter::begin_for(ctx, app, Target::Search);
    render(ctx, app);
    refresh_preview(ctx, app, false);
}

/// The tag list for the search's system landed. It names the chosen tags;
/// after a change of system it also retires the ones that system lacks.
pub(crate) fn tags_loaded(ctx: &Ctx, app: &App) {
    let changed = {
        let mut shared = lock(&ctx.shared);
        let prune = std::mem::take(&mut shared.search.prune_pending);
        let pruned = match crate::browse_filter::search_groups(&shared) {
            Some(groups) if prune => rules::prune_tags(&shared.persist.search.tags, groups),
            Some(_) => shared.persist.search.tags.clone(),
            None => {
                // Not this system's list yet; keep waiting for it.
                shared.search.prune_pending = prune;
                return;
            }
        };
        let changed = pruned != shared.persist.search.tags;
        shared.persist.search.tags = pruned;
        changed
    };
    if app.global::<crate::Shell>().get_active_screen() != crate::Screen::Search {
        return;
    }
    render(ctx, app);
    if changed {
        refresh_preview(ctx, app, false);
    }
}

/// The filter changed in the picker.
pub(crate) fn tags_changed(ctx: &Ctx, app: &App) {
    render(ctx, app);
    refresh_preview(ctx, app, false);
}

/// Fetch the pane's matches for the search as it stands. `debounce` waits
/// out a burst of typing; a filter change fetches at once.
fn refresh_preview(ctx: &Ctx, app: &App, debounce: bool) {
    let (seq, searchable) = {
        let mut shared = lock(&ctx.shared);
        shared.search.task.cancel();
        shared.search.seq = shared.search.seq.wrapping_add(1);
        let searchable = can_search(&shared);
        shared.search.failed = false;
        shared.search.searching = searchable;
        if !searchable {
            shared.search.preview.clear();
            shared.search.count = None;
        }
        (shared.search.seq, searchable)
    };
    render(ctx, app);
    if !searchable {
        return;
    }
    let delay = if debounce {
        rules::PREVIEW_DEBOUNCE_MS
    } else {
        0
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(delay), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        fetch_preview(&ctx, &app, seq);
    });
}

fn fetch_preview(ctx: &Ctx, app: &App, seq: u64) {
    let args = {
        let shared = lock(&ctx.shared);
        if shared.search.seq != seq {
            return;
        }
        args(&shared)
    };
    let resource = ctx.store.subscribe::<MediaSearchEndpoint>(args);
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        preview_landed(&ctx2, &app, seq, Ok(&result));
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        preview_landed(&ctx2, &app, seq, Err(&message));
                    });
                    return;
                }
                ResourceStatus::Idle | ResourceStatus::Loading => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    lock(&ctx.shared).search.task.replace(task);
}

/// Seat keyboard focus on a key, for a test.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn focus_key_for_test(ctx: &Ctx, key: Key) {
    let mut shared = lock(&ctx.shared);
    shared.search.focus.zone = Zone::Keys;
    shared.search.keyboard.focus(key);
}

/// The ticket of the preview in flight, for a test to answer or outrun.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn preview_seq(shared: &Shared) -> u64 {
    shared.search.seq
}

pub(crate) fn preview_landed(
    ctx: &Ctx,
    app: &App,
    seq: u64,
    outcome: Result<&MediaSearchResult, &str>,
) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.search.seq != seq {
            return;
        }
        let model = &mut shared.search;
        model.searching = false;
        match outcome {
            Ok(result) => {
                let more = result.pagination.as_ref().is_some_and(|p| p.has_next_page);
                model.count = Some(rules::count(result.results.len(), more));
                model.preview = result
                    .results
                    .iter()
                    .take(rules::PANE_ROWS)
                    .map(|item| Preview {
                        name: item.name.clone(),
                        system: item.system.name.clone(),
                        path: item.path.clone(),
                    })
                    .collect();
                model.failed = false;
            }
            Err(message) => {
                tracing::warn!(error = %message, "search preview failed");
                model.preview.clear();
                model.count = None;
                model.failed = true;
            }
        }
    }
    if app.global::<crate::Shell>().get_active_screen() == crate::Screen::Search {
        render(ctx, app);
    }
}

/// Back: to the folder a scoped search opened on, otherwise to the Hub.
fn leave(ctx: &Ctx, app: &App) {
    let (scoped, intact, system) = {
        let mut shared = lock(&ctx.shared);
        shared.search.task.cancel();
        shared.search.seq = shared.search.seq.wrapping_add(1);
        let scoped = shared.persist.search.scoped;
        let system = shared
            .systems
            .iter()
            .find(|s| s.id == shared.persist.search.system_id)
            .cloned();
        let intact = shared.search.browse_intact
            && shared.games.mode == crate::GamesMode::Browse
            && shared.games.system_id == shared.persist.search.system_id;
        (scoped, intact, system)
    };
    if scoped {
        if intact {
            lock(&ctx.shared).persist.active_screen = crate::Screen::Games.token().to_string();
            crate::router::save_persist(&ctx.shared);
            crate::router::transition_to_screen(app, crate::Screen::Games, -1);
            crate::games::render(ctx, app);
            return;
        }
        if let Some(system) = system {
            crate::navigation::stage(ctx, app);
            crate::games::enter_restored(ctx, app, &system);
            return;
        }
    }
    lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
    crate::router::save_persist(&ctx.shared);
    crate::router::return_to_hub(ctx, app);
}

/// Delete the character before the cursor. A delete held long enough
/// stops stepping and empties the field.
fn delete(ctx: &Ctx, app: &App) {
    let clear = crate::input::repeat_held_ms(ctx) >= zaparoo_app::input::TEXT_CLEAR_HOLD_MS;
    let press = {
        let mut shared = lock(&ctx.shared);
        if clear {
            shared.search.keyboard.clear()
        } else {
            shared.search.keyboard.backspace()
        }
    };
    apply_press(ctx, app, press);
}

/// Whether Accept on the focused key should repeat while held: a key that
/// types or deletes, never Search or a layer key.
pub(crate) fn key_repeats(ctx: &Ctx) -> bool {
    let shared = lock(&ctx.shared);
    shared.search.focus.zone == Zone::Keys
        && matches!(
            shared.search.keyboard.key(),
            Some(Key::Char(_) | Key::Space | Key::Backspace)
        )
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::UP | actions::DOWN | actions::LEFT | actions::RIGHT => {
            let mut shared = lock(&ctx.shared);
            let shape = shape(app, &shared);
            let focus = rules::settle(shared.search.focus, shape);
            shared.search.focus = if focus.zone == Zone::Keys {
                match shared.search.keyboard.step(action, shape.keys_wrap()) {
                    Some(edge) => {
                        let unit = shared.search.keyboard.unit();
                        rules::leave_keys(focus, edge, unit, shape)
                    }
                    None => focus,
                }
            } else {
                let (next, landing) = rules::step(focus, action, shape);
                match landing {
                    Landing::Unchanged => {}
                    Landing::TopRow => shared.search.keyboard.enter_row(false),
                    Landing::BottomRow => shared.search.keyboard.enter_row(true),
                    Landing::RowStart => shared.search.keyboard.row_start(),
                }
                next
            };
            drop(shared);
            render(ctx, app);
        }
        actions::ACCEPT => {
            let focus = lock(&ctx.shared).search.focus;
            match focus.zone {
                Zone::Keys => {
                    let press = lock(&ctx.shared).search.keyboard.press();
                    flash_key(ctx, app);
                    apply_press(ctx, app, press);
                }
                Zone::System => open_system_picker(ctx, app),
                Zone::Filter => open_filter(ctx, app),
                Zone::Pane => accept_pane(ctx, app, focus.pane_index),
            }
        }
        // A held Accept on a key that types or deletes.
        KEY => {
            // Mark the key pressed first, so the paint that follows the
            // edit shows it down.
            flash_key(ctx, app);
            let key = lock(&ctx.shared).search.keyboard.key();
            if key == Some(Key::Backspace) {
                delete(ctx, app);
            } else {
                let press = lock(&ctx.shared).search.keyboard.press();
                apply_press(ctx, app, press);
            }
        }
        // West deletes and North spaces from anywhere on the screen, so a
        // typo is one press away whichever key has focus.
        DELETE | actions::PAGE_MENU => delete(ctx, app),
        actions::CONTEXT_MENU => {
            let press = lock(&ctx.shared).search.keyboard.insert(' ');
            apply_press(ctx, app, press);
        }
        actions::PAGE_PREV | actions::PAGE_NEXT => {
            let moved = lock(&ctx.shared)
                .search
                .keyboard
                .move_caret(action == actions::PAGE_NEXT);
            if moved {
                render(ctx, app);
            }
        }
        actions::CANCEL => leave(ctx, app),
        _ => {}
    }
}

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    owns_input(app) && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Move focus to what the pointer is over; false when it names nothing.
fn point_at(ctx: &Ctx, app: &App, at: SearchZone, index: i32) -> bool {
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    let mut shared = lock(&ctx.shared);
    let shape = shape(app, &shared);
    let ok = match at {
        SearchZone::Keys => shared.search.keyboard.set_index(index),
        SearchZone::System => !shape.scoped,
        SearchZone::Filter => true,
        SearchZone::Pane => index < shape.pane_rows,
    };
    if !ok {
        return false;
    }
    let focus = &mut shared.search.focus;
    match at {
        SearchZone::Keys => focus.zone = Zone::Keys,
        SearchZone::System => focus.zone = Zone::System,
        SearchZone::Filter => focus.zone = Zone::Filter,
        SearchZone::Pane => {
            focus.zone = Zone::Pane;
            focus.pane_index = index;
        }
    }
    true
}

/// Wire the view's pointer callbacks.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<SearchInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_hovered(move |at, index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, at, index) {
                render(&ctx, &app);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_layout_changed(move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.global::<crate::Shell>().get_active_screen() == crate::Screen::Search {
                render(&ctx, &app);
            }
        });
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    input.on_clicked(move |at, index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, at, index) {
            crate::router::handle_action(&ctx, &app, actions::ACCEPT);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::persist::{PersistedState, RecentSearch};

    fn shared() -> Shared {
        Shared::new(
            PersistedState::default(),
            false,
            vec![],
            vec![],
            String::new(),
            std::path::PathBuf::new(),
        )
    }

    #[test]
    fn the_request_trims_the_query_and_carries_every_filter() {
        let mut shared = shared();
        let search = &mut shared.persist.search;
        search.query = "  mario kart ".into();
        search.system_id = "SNES".into();
        search.tags = vec!["year:1992".into(), "bogus:x".into(), "genre:racing".into()];
        search.scope_path = "/roms/SNES/Racing".into();
        let args = args(&shared);
        assert_eq!(args.query, "mario kart");
        assert_eq!(args.system_id, "SNES");
        assert_eq!(args.tags, ["genre:racing", "year:1992"]);
        assert_eq!(args.path_prefix, "/roms/SNES/Racing");
        assert_eq!(args.max_results, rules::PREVIEW_LIMIT);
        assert_eq!(args.sort, "name-asc");
    }

    #[test]
    fn the_pane_lists_recents_until_there_is_something_to_search_on() {
        let mut shared = shared();
        assert_eq!(pane_kind(&shared), SearchPane::Recents);
        assert!(pane_rows(&shared).is_empty());

        shared.persist.search.recent = vec![
            RecentSearch {
                query: "zelda".into(),
                system_id: "SNES".into(),
                tags: vec!["genre:action-rpg".into()],
            },
            RecentSearch {
                query: String::new(),
                system_id: String::new(),
                tags: vec!["year:1994".into()],
            },
        ];
        let rows = pane_rows(&shared);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "zelda");
        assert_eq!(rows[0].detail, "SNES \u{b7} action-rpg");
        // A search with no query is named by its filters.
        assert_eq!(rows[1].title, "1994");
        // The Clear row is a stop of its own under the rows.
        assert!(pane_has_clear(&shared));

        shared.persist.search.query = "m".into();
        assert_eq!(pane_kind(&shared), SearchPane::Preview);
        shared.search.preview = vec![Preview {
            name: "Mario Kart 64".into(),
            system: "Nintendo 64".into(),
            path: "/roms/n64/mk64.z64".into(),
        }];
        let rows = pane_rows(&shared);
        assert!(!pane_has_clear(&shared));
        assert_eq!(rows[0].title, "Mario Kart 64");
        assert_eq!(rows[0].detail, "Nintendo 64");
        // One system searched: naming it on every row says nothing.
        shared.persist.search.system_id = "N64".into();
        assert_eq!(pane_rows(&shared)[0].detail, "");
    }

    #[test]
    fn recent_systems_come_from_searches_first_then_browsing() {
        let mut shared = shared();
        shared.persist.search.recent = vec![
            RecentSearch {
                query: "zelda".into(),
                system_id: "SNES".into(),
                tags: Vec::new(),
            },
            RecentSearch {
                query: "mario".into(),
                ..RecentSearch::default()
            },
        ];
        shared.persist.games.system_focus = vec![zaparoo_core::persist::SystemFocus {
            system_id: "Genesis".into(),
            ..Default::default()
        }];
        // A search of every system names none.
        assert_eq!(recent_systems(&shared), ["SNES", "Genesis"]);
    }

    #[test]
    fn a_folder_search_with_nothing_typed_has_no_recents() {
        let mut shared = shared();
        shared.persist.search.scoped = true;
        shared.persist.search.recent = vec![RecentSearch {
            query: "zelda".into(),
            ..RecentSearch::default()
        }];
        assert_eq!(pane_kind(&shared), SearchPane::None);
        assert!(pane_rows(&shared).is_empty());
        // At a system's top level the system alone is enough to search on.
        shared.persist.search.system_id = "SNES".into();
        assert_eq!(pane_kind(&shared), SearchPane::Preview);
    }

    #[test]
    fn the_results_title_names_the_query_then_what_narrows_it() {
        let mut shared = shared();
        assert_eq!(results_title(&shared), "");
        let search = &mut shared.persist.search;
        search.query = " mario ".into();
        search.system_id = "SNES".into();
        search.scope_name = "Racing".into();
        search.tags = vec!["genre:racing".into()];
        assert_eq!(
            results_title(&shared),
            "\u{201c}mario\u{201d} \u{b7} SNES \u{b7} Racing \u{b7} racing"
        );
    }
}
