// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The media-job setup modals: the Update media database and Update
// metadata forms the Library settings rows open, their in-panel scope
// and source pickers, and the calls that start the job. The rules live
// in `zaparoo_app::media_setup`.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::media_setup::{self as rules, FormRow, Kind};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    MediaIndexParams, MediaScrapeParams, MediaScrapeScope, ScraperInfo,
};

use crate::router::{lock, Ctx, Shared};
use crate::{App, SettingsRow, SetupInput, SetupModalView, SetupPickerRow};

/// Rows the picker page shows at once before it scrolls.
const PICKER_WINDOW: usize = 7;
/// Slots kept either side of the picker's viewport, so a held scroll that
/// runs ahead of the glide still has rows to draw.
const PICKER_OVERSCAN: usize = 4;

/// The open form's state.
#[derive(Debug, Clone)]
pub struct SetupModel {
    pub open: bool,
    pub kind: Kind,
    pub index: usize,
    /// The Systems row's selected scope, separate from Core's token encoding.
    pub scope: rules::Scope,
    /// The game the form was opened on, which its Systems page keeps
    /// offering after the scope is widened.
    pub game: Option<rules::GameTarget>,
    /// The systems checked on the open Systems page.
    pub checked: Vec<String>,
    /// The ones that were checked when that page opened: they lead its
    /// list, and stay put while it is open.
    pub pinned: Vec<String>,
    /// The scraper id the Source row holds (Scrape only).
    pub scraper: String,
    pub rescrape: bool,
    /// Which page the panel shows: None is the form.
    pub picker: Option<FormRow>,
    pub picker_index: usize,
    /// Scrapers Core reported, once the list has answered.
    pub scrapers: Vec<ScraperInfo>,
    /// The Source row is waiting on that list, and whether it says so yet.
    pub sources_wait: Option<crate::cue::LocalWait>,
    pub sources_loading: bool,
    /// Which scraper-list request the form is waiting on; an answer to
    /// any other is dropped.
    pub sources_seq: u64,
}

impl SetupModel {
    pub fn new() -> Self {
        Self {
            open: false,
            kind: Kind::Index,
            index: 0,
            scope: rules::Scope::All,
            game: None,
            checked: Vec::new(),
            pinned: Vec::new(),
            scraper: String::new(),
            rescrape: false,
            picker: None,
            picker_index: 0,
            scrapers: Vec::new(),
            sources_wait: None,
            sources_loading: false,
            sources_seq: 0,
        }
    }

    /// Stop waiting on the scraper list: its answer no longer applies.
    fn retire_sources(&mut self) -> Option<crate::cue::LocalWait> {
        self.sources_seq = self.sources_seq.wrapping_add(1);
        self.sources_loading = false;
        self.sources_wait.take()
    }

    pub(crate) fn rows(&self) -> &'static [FormRow] {
        rules::rows(self.kind)
    }

    fn focused(&self) -> Option<FormRow> {
        self.rows().get(self.index).copied()
    }
}

impl Default for SetupModel {
    fn default() -> Self {
        Self::new()
    }
}

/// The scope rows the picker offers: the game the form was opened on, All
/// systems, the categories that have indexable systems, the systems already
/// checked, then every system under its manufacturer.
fn scope_entries(shared: &Shared) -> Vec<rules::ScopeEntry> {
    let systems: Vec<zaparoo_app::system_picker::System> = shared
        .systems
        .iter()
        .map(|s| zaparoo_app::system_picker::System {
            id: s.id.clone(),
            name: crate::systems::display_name(shared, &s.id),
            manufacturer: s.manufacturer.clone().unwrap_or_default(),
            games: s.media_count,
        })
        .collect();
    rules::scope_entries(
        &shared.categories,
        &systems,
        &rules::Picks {
            checked: &shared.setup.checked,
            pinned: &shared.setup.pinned,
            game: shared.setup.game.as_ref(),
        },
    )
}

/// What each row of the open picker page is to the list cursor: the
/// systems page has manufacturer headers, the source page is all options.
fn picker_roles(shared: &Shared, page: FormRow) -> Vec<zaparoo_app::form_list::Role> {
    match page {
        FormRow::Source => {
            vec![zaparoo_app::form_list::Role::Option; shared.setup.scrapers.len()]
        }
        _ => rules::scope_roles(&scope_entries(shared)),
    }
}

fn system_name(shared: &Shared, id: &str) -> String {
    crate::systems::display_name(shared, id)
}

/// The label pair a picker row (or the form's own row) renders with.
fn scope_pair(shared: &Shared, scope: &rules::Scope) -> (crate::ScopeKind, String) {
    let (kind, name) = rules::scope_label(scope, &|id| system_name(shared, id));
    (kind.into(), name)
}

fn scraper_name(model: &SetupModel, id: &str) -> String {
    model.scrapers.iter().find(|s| s.id == id).map_or_else(
        || id.to_string(),
        |s| {
            if s.name.is_empty() {
                s.id.clone()
            } else {
                s.name.clone()
            }
        },
    )
}

// ---------- Rendering ----------

fn row_metrics(app: &App) -> (i32, i32) {
    let inputs = crate::router::output_scene(app).inputs();
    (inputs.pct_h(8.0), inputs.pct_h(1.5))
}

/// Push the form rows, or the windowed picker page.
pub fn render(ctx: &Ctx, app: &App) {
    let view = app.global::<SetupModalView>();
    let (row_h, _) = row_metrics(app);
    let shared = lock(&ctx.shared);
    let model = &shared.setup;
    view.set_open(model.open);
    view.set_kind(model.kind.into());
    view.set_index(i32::try_from(model.index).unwrap_or(0));

    let rows: Vec<SettingsRow> = model
        .rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut out = SettingsRow {
                kind: crate::RowKind::Field,
                id: SharedString::from(match row {
                    // The start row names the job it starts.
                    FormRow::Start => match model.kind {
                        Kind::Index => "startIndex",
                        Kind::Scrape => "startImport",
                    },
                    other => other.id(),
                }),
                control: row.control().into(),
                enabled: true,
                y_offset: (i as i32 * row_h) as f32,
                height: row_h as f32,
                ..Default::default()
            };
            match row {
                FormRow::Systems => {
                    let (kind, name) = scope_pair(&shared, &model.scope);
                    out.scope_kind = kind;
                    out.value_name = SharedString::from(name.as_str());
                }
                FormRow::Source => {
                    out.scope_kind = if model.sources_loading {
                        crate::ScopeKind::SourceLoading
                    } else {
                        crate::ScopeKind::Source
                    };
                    out.value_name =
                        SharedString::from(scraper_name(model, &model.scraper).as_str());
                    out.enabled = !model.scrapers.is_empty();
                }
                FormRow::Rescrape => out.checked = model.rescrape,
                // The Start row is its own button: its centered label
                // says what it does, so it carries no action text.
                FormRow::Start => {}
            }
            out
        })
        .collect();
    crate::view_model::publish(&view.get_rows(), rows, |rows| view.set_rows(rows));

    let Some(page) = model.picker else {
        view.set_picker_page(false);
        view.set_picker_toggle(false);
        view.set_picker_checked(0);
        view.set_picker_rows(ModelRc::new(VecModel::from(Vec::<SetupPickerRow>::new())));
        return;
    };
    view.set_picker_page(true);
    view.set_picker_title(if page == FormRow::Source {
        crate::SetupPicker::Source
    } else {
        crate::SetupPicker::Systems
    });
    let entries: Vec<SetupPickerRow> = match page {
        FormRow::Source => model
            .scrapers
            .iter()
            .map(|s| SetupPickerRow {
                kind: crate::ScopeKind::Source,
                name: SharedString::from(if s.name.is_empty() {
                    s.id.as_str()
                } else {
                    s.name.as_str()
                }),
                checked: false,
            })
            .collect(),
        _ => scope_entries(&shared)
            .into_iter()
            .map(|entry| SetupPickerRow {
                kind: entry.kind.into(),
                name: SharedString::from(entry.name.as_str()),
                checked: entry.checked,
            })
            .collect(),
    };
    // A system row is checked, not picked, and the title counts the checks.
    let toggles = entries
        .get(model.picker_index)
        .is_some_and(|row| row.kind == crate::ScopeKind::System);
    view.set_picker_toggle(toggles);
    view.set_picker_checked(if page == FormRow::Systems {
        i32::try_from(model.checked.len()).unwrap_or(i32::MAX)
    } else {
        0
    });
    publish_picker_window(&view, &entries, model.picker_index);
}

/// Publish the picker's window of rows around `picker_index`.
fn publish_picker_window(
    view: &SetupModalView<'_>,
    entries: &[SetupPickerRow],
    picker_index: usize,
) {
    // Window the rows around the cursor, like the browse list does, with a
    // few slots either side of the viewport so the window can glide to its
    // next position. A slot past either end of the list stays empty, which
    // keeps the slot count fixed.
    let visible = PICKER_WINDOW.min(entries.len().max(1));
    let top = zaparoo_app::media_list::list_view_top(picker_index, entries.len(), visible, None);
    let rows: Vec<SetupPickerRow> = (0..visible + 2 * PICKER_OVERSCAN)
        .map(|slot| {
            (top + slot)
                .checked_sub(PICKER_OVERSCAN)
                .and_then(|index| entries.get(index))
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    // These are fixed slots, not animated item identities. Update their
    // labels in place when the window scrolls instead of rebuilding every
    // delegate on every held repeat.
    crate::view_model::publish_keyed(
        &view.get_picker_rows(),
        rows,
        PartialEq::eq,
        |_, _| true,
        |rows| view.set_picker_rows(rows),
    );
    let int = |value: usize| i32::try_from(value).unwrap_or(0);
    view.set_picker_first(int(top) - int(PICKER_OVERSCAN));
    view.set_picker_top(int(top));
    view.set_picker_visible(int(visible));
    view.set_picker_count(int(entries.len()));
    view.set_picker_sel(int(picker_index.saturating_sub(top)));
    view.set_has_above(top > 0);
    view.set_has_below(top + visible < entries.len());
}

// ---------- Opening ----------

/// Open one of the forms. Scrape seeds its source from the persisted
/// scraper and refreshes the list from Core.
pub fn open(ctx: &Ctx, app: &App, kind: Kind, scope: rules::Scope) {
    let stale = {
        let mut shared = lock(&ctx.shared);
        let persisted = shared.persist.settings.metadata_scraper.clone();
        let model = &mut shared.setup;
        model.open = true;
        model.kind = kind;
        model.index = 0;
        model.game = match &scope {
            rules::Scope::Game(game) => Some(game.clone()),
            _ => None,
        };
        model.scope = scope;
        model.checked.clear();
        model.pinned.clear();
        model.rescrape = false;
        model.picker = None;
        model.picker_index = 0;
        if model.scraper.is_empty() {
            model.scraper = persisted;
        }
        model.retire_sources()
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    render(ctx, app);
    if kind == Kind::Scrape {
        fetch_scrapers(ctx, app);
    }
}

pub fn close(ctx: &Ctx, app: &App) {
    let stale = {
        let mut shared = lock(&ctx.shared);
        shared.setup.open = false;
        shared.setup.retire_sources()
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// Core's scraper inventory, for the Source row. Until it answers the row
/// cannot be used; if that lasts, its value says why (`crate::cue`'s timing).
fn fetch_scrapers(ctx: &Ctx, app: &App) {
    let (stale, seq) = {
        let mut shared = lock(&ctx.shared);
        let stale = shared.setup.retire_sources();
        (stale, shared.setup.sources_seq)
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    let shown = ctx.clone();
    let wait = crate::cue::begin_local(app, move |app| {
        {
            let mut shared = lock(&shown.shared);
            if shared.setup.sources_seq != seq || !shared.setup.scrapers.is_empty() {
                return;
            }
            shared.setup.sources_loading = true;
        }
        render(&shown, app);
    });
    lock(&ctx.shared).setup.sources_wait = Some(wait);

    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let result = client.scrapers().await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            scrapers_answered(&ctx2, &app, seq, result);
        });
    });
}

/// The scraper list answered request `seq`. A form closed or reopened
/// since is waiting on something else, and so is one closed while the
/// hold puts the answer off.
pub(crate) fn scrapers_answered(
    ctx: &Ctx,
    app: &App,
    seq: u64,
    result: Result<zaparoo_core::media_types::ScrapersResult, zaparoo_core::client::ClientError>,
) {
    let wait = {
        let mut shared = lock(&ctx.shared);
        if shared.setup.sources_seq != seq {
            return;
        }
        shared.setup.sources_wait.take()
    };
    let ctx = ctx.clone();
    let finish = move |app: &App| {
        if lock(&ctx.shared).setup.sources_seq == seq {
            scrapers_landed(&ctx, app, result);
        }
    };
    match wait {
        Some(wait) => crate::cue::end_local(app, wait, finish),
        None => finish(app),
    }
}

fn scrapers_landed(
    ctx: &Ctx,
    app: &App,
    result: Result<zaparoo_core::media_types::ScrapersResult, zaparoo_core::client::ClientError>,
) {
    lock(&ctx.shared).setup.sources_loading = false;
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            tracing::warn!("scraper list unavailable: {}", e.message);
            render(ctx, app);
            crate::router::report_action_error(ctx, app, "media_scrapers", "");
            return;
        }
    };
    {
        let mut shared = lock(&ctx.shared);
        let persisted = shared.persist.settings.metadata_scraper.clone();
        // A panel opened on a system or category starts on a source
        // that covers it, since each platform registers its own.
        let scoped = (shared.setup.scope != rules::Scope::All).then(|| {
            let systems = scope_systems(&shared);
            let offered: Vec<(&str, &[String])> = result
                .scrapers
                .iter()
                .map(|s| (s.id.as_str(), s.supported_systems.as_slice()))
                .collect();
            rules::scraper_for(&offered, &shared.setup.scraper, &systems).map(str::to_string)
        });
        let model = &mut shared.setup;
        model.scrapers = result.scrapers;
        if let Some(Some(scraper)) = scoped {
            model.scraper = scraper;
        }
        // Keep the stored choice when Core still offers it.
        let known = model.scrapers.iter().any(|s| s.id == model.scraper);
        if !known {
            model.scraper = if model.scrapers.iter().any(|s| s.id == persisted) {
                persisted
            } else {
                model
                    .scrapers
                    .first()
                    .map(|s| s.id.clone())
                    .unwrap_or_default()
            };
        }
    }
    render(ctx, app);
}

// ---------- Input ----------

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let (page, len) = {
        let shared = lock(&ctx.shared);
        (shared.setup.picker, shared.setup.rows().len())
    };

    if let Some(page) = page {
        match action {
            // Up and Down pass over the manufacturer headers; sideways and
            // the shoulder buttons jump a manufacturer at a time.
            actions::UP
            | actions::DOWN
            | actions::LEFT
            | actions::RIGHT
            | actions::PAGE_PREV
            | actions::PAGE_NEXT => {
                use zaparoo_app::form_list;
                let mut shared = lock(&ctx.shared);
                let roles = picker_roles(&shared, page);
                let index = shared.setup.picker_index;
                shared.setup.picker_index = match action {
                    actions::UP => form_list::step(&roles, index, false),
                    actions::DOWN => form_list::step(&roles, index, true),
                    actions::LEFT | actions::PAGE_PREV => form_list::jump(&roles, index, false),
                    _ => form_list::jump(&roles, index, true),
                };
                drop(shared);
                render(ctx, app);
            }
            actions::ACCEPT => pick(ctx, app),
            actions::CANCEL => leave_picker(ctx, app),
            _ => {}
        }
        return;
    }

    match action {
        actions::UP | actions::DOWN => {
            let delta = if action == actions::UP { -1 } else { 1 };
            let mut shared = lock(&ctx.shared);
            shared.setup.index = rules::move_index(shared.setup.index, len, delta);
            drop(shared);
            render(ctx, app);
        }
        // Left and Right flip the one toggle the scrape form carries.
        actions::LEFT | actions::RIGHT => {
            let toggled = {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.setup;
                if model.focused() == Some(FormRow::Rescrape) {
                    model.rescrape = !model.rescrape;
                    true
                } else {
                    false
                }
            };
            if toggled {
                render(ctx, app);
            }
        }
        actions::ACCEPT => accept(ctx, app),
        actions::CANCEL => close(ctx, app),
        _ => {}
    }
}

fn accept(ctx: &Ctx, app: &App) {
    let row = {
        let shared = lock(&ctx.shared);
        shared.setup.focused()
    };
    match row {
        Some(FormRow::Systems) => open_picker(ctx, app, FormRow::Systems),
        Some(FormRow::Source) => {
            if !lock(&ctx.shared).setup.scrapers.is_empty() {
                open_picker(ctx, app, FormRow::Source);
            }
        }
        Some(FormRow::Rescrape) => {
            {
                let mut shared = lock(&ctx.shared);
                shared.setup.rescrape = !shared.setup.rescrape;
            }
            render(ctx, app);
        }
        Some(FormRow::Start) => start(ctx, app),
        None => {}
    }
}

/// Open a picker page seated on the value the form holds.
fn open_picker(ctx: &Ctx, app: &App, page: FormRow) {
    {
        let mut shared = lock(&ctx.shared);
        let seat = if page == FormRow::Source {
            shared
                .setup
                .scrapers
                .iter()
                .position(|s| s.id == shared.setup.scraper)
                .unwrap_or(0)
        } else {
            // The scope's own systems start checked, and lead the list.
            let checked = rules::checked_systems(&shared.setup.scope);
            shared.setup.pinned.clone_from(&checked);
            shared.setup.checked = checked;
            rules::scope_seat(&scope_entries(&shared), &shared.setup.scope)
        };
        let model = &mut shared.setup;
        model.picker = Some(page);
        model.picker_index = seat;
    }
    render(ctx, app);
}

/// Accept on a picker row. A source, the game, All systems or a category
/// goes back to the form as its value; a system is checked or unchecked
/// and the page stays.
fn pick(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let Some(page) = shared.setup.picker else {
            return;
        };
        let index = shared.setup.picker_index;
        if page == FormRow::Source {
            let picked = shared.setup.scrapers.get(index).map(|s| s.id.clone());
            if let Some(id) = picked {
                shared.setup.scraper = id;
            }
            shared.setup.picker = None;
        } else {
            let Some(entry) = scope_entries(&shared).into_iter().nth(index) else {
                return;
            };
            let model = &mut shared.setup;
            if let Some(scope) = rules::picked_scope(&entry, model.game.as_ref()) {
                model.scope = scope;
                model.checked.clear();
                model.picker = None;
            } else if entry.kind == rules::ScopeKind::System {
                match model.checked.iter().position(|id| *id == entry.token) {
                    Some(at) => {
                        model.checked.remove(at);
                    }
                    None => model.checked.push(entry.token),
                }
            } else {
                return;
            }
        }
    }
    render(ctx, app);
}

/// Back on a picker page returns to the form. The Systems page has no
/// confirm of its own: what is checked becomes the form's scope, and
/// nothing checked leaves the scope as it was.
fn leave_picker(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.setup.picker == Some(FormRow::Systems) {
            let scope = rules::scope_after_checks(&scope_entries(&shared), &shared.setup.scope);
            shared.setup.scope = scope;
        }
        shared.setup.picker = None;
    }
    render(ctx, app);
}

/// The systems Core is given for the panel's scope; empty means all.
fn scope_systems(shared: &Shared) -> Vec<String> {
    let catalog = zaparoo_app::systems::indexable_ids;
    rules::resolved_systems(&shared.setup.scope, &|category| {
        catalog(&crate::systems::catalog_systems(&shared.systems), category)
    })
}

/// The scope Core is given for a game; None for a job over systems. An
/// error when the game cannot be asked for on its own, which must not
/// widen into its system or the whole library.
fn scrape_scope(
    game: Option<&rules::GameTarget>,
    core_version: &str,
) -> Result<Option<MediaScrapeScope>, ()> {
    let Some(game) = game else {
        return Ok(None);
    };
    let Some(item) = rules::scrape_item(game) else {
        tracing::warn!(game = game.name, "the game has no media ID or path");
        return Err(());
    };
    // A Core below the supported floor ignores the scope, and with no
    // systems beside it would run over every system. The startup warning
    // can be dismissed, so this cannot rest on it.
    if !crate::router::version_supported(core_version) {
        tracing::warn!(core_version, "this Core cannot scrape a single game");
        return Err(());
    }
    Ok(Some(match item {
        rules::ScrapeItem::Media(id) => MediaScrapeScope::MediaId(id),
        rules::ScrapeItem::File { system, path } => MediaScrapeScope::File { system, path },
    }))
}

/// Start the job over the chosen scope and close the panel.
fn start(ctx: &Ctx, app: &App) {
    let (kind, scoped, systems, scraper, rescrape, covered, game, core_version) = {
        let shared = lock(&ctx.shared);
        let systems = scope_systems(&shared);
        let game = match &shared.setup.scope {
            rules::Scope::Game(game) => Some(game.clone()),
            _ => None,
        };
        // Whether the chosen source handles every system in the scope. A
        // source Core no longer lists is left for Core to judge.
        let covered = shared
            .setup
            .scrapers
            .iter()
            .find(|s| s.id == shared.setup.scraper)
            .is_none_or(|s| {
                let offered = [(s.id.as_str(), s.supported_systems.as_slice())];
                rules::scraper_for(&offered, &s.id, &systems).is_some()
            });
        (
            shared.setup.kind,
            shared.setup.scope != rules::Scope::All,
            systems,
            shared.setup.scraper.clone(),
            shared.setup.rescrape,
            covered,
            game,
            shared.core_version.clone(),
        )
    };
    // Core reads an empty list as every system, so a scope that resolved
    // to nothing (its category emptied since the panel opened) must not
    // widen into a full run.
    if scoped && systems.is_empty() {
        close(ctx, app);
        return;
    }
    match kind {
        Kind::Index => {
            let client = ctx.store.client();
            let weak = app.as_weak();
            let params = MediaIndexParams {
                systems: (!systems.is_empty()).then_some(systems),
            };
            let ctx2 = ctx.clone();
            let wait = crate::cue::begin(ctx, app, crate::AppCue::Starting, "", "");
            ctx.handle.spawn(async move {
                let result = client.media_generate(params).await;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    crate::cue::end(&ctx2, &app, wait);
                    if let Err(e) = result {
                        tracing::warn!("media update failed to start: {}", e.message);
                        crate::router::report_action_error(&ctx2, &app, "media_index", "");
                    }
                });
            });
        }
        Kind::Scrape => {
            if scraper.is_empty() {
                return;
            }
            // Core would accept the pair and import nothing: say so, and
            // leave the panel open for another source or scope.
            if scoped && !covered {
                tracing::warn!(?systems, scraper, "the source does not cover these systems");
                crate::router::report_action_error(ctx, app, "media_scrape", "");
                return;
            }
            // A game is named to Core on its own, in place of the systems.
            let Ok(scope) = scrape_scope(game.as_ref(), &core_version) else {
                crate::router::report_action_error(ctx, app, "media_scrape", "");
                return;
            };
            // The chosen source becomes the persisted default.
            {
                let mut shared = lock(&ctx.shared);
                shared
                    .persist
                    .settings
                    .metadata_scraper
                    .clone_from(&scraper);
            }
            crate::settings::save(ctx, app);
            let client = ctx.store.client();
            let weak = app.as_weak();
            let params = MediaScrapeParams {
                scraper_id: scraper,
                // Core refuses a scope beside systems, an empty list too.
                systems: if scope.is_some() { Vec::new() } else { systems },
                scope,
                force: rescrape,
            };
            let ctx2 = ctx.clone();
            let wait = crate::cue::begin(ctx, app, crate::AppCue::Starting, "", "");
            ctx.handle.spawn(async move {
                let result = client.media_scrape(params).await;
                let _ = weak.upgrade_in_event_loop(move |app| {
                    crate::cue::end(&ctx2, &app, wait);
                    if let Err(e) = result {
                        tracing::warn!("metadata update failed to start: {}", e.message);
                        crate::router::report_action_error(&ctx2, &app, "media_scrape", "");
                    }
                });
            });
        }
    }
    close(ctx, app);
}

// ---------- Pointer ----------

pub fn bind_input(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<SetupInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_row_hovered(move |i| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if crate::press_feedback::pending(&app) {
                return;
            }
            if let Ok(index) = usize::try_from(i) {
                let len = {
                    let shared = lock(&ctx.shared);
                    shared.setup.rows().len()
                };
                if index < len {
                    lock(&ctx.shared).setup.index = index;
                    render(&ctx, &app);
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_row_clicked(move |i| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if crate::press_feedback::pending(&app) {
                return;
            }
            if let Ok(index) = usize::try_from(i) {
                lock(&ctx.shared).setup.index = index;
                render(&ctx, &app);
                crate::router::handle_action(&ctx, &app, actions::ACCEPT);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_picker_hovered(move |i| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            focus_picker(&ctx, &app, i);
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_picker_clicked(move |i| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if focus_picker(&ctx, &app, i) {
                pick(&ctx, &app);
            }
        });
    }
}

/// A pointer landed on a windowed picker row: the index is local to the
/// window Rust sent.
fn focus_picker(ctx: &Ctx, app: &App, local: i32) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let Some(page) = shared.setup.picker else {
            return false;
        };
        let Ok(local) = usize::try_from(local) else {
            return false;
        };
        let len = match page {
            FormRow::Source => shared.setup.scrapers.len(),
            _ => scope_entries(&shared).len(),
        };
        let visible = PICKER_WINDOW.min(len.max(1));
        let top =
            zaparoo_app::media_list::list_view_top(shared.setup.picker_index, len, visible, None);
        // `local` is a published slot; the first holds the row a few
        // above the viewport's top.
        let Some(index) = (top + local).checked_sub(PICKER_OVERSCAN) else {
            return false;
        };
        if index >= len
            || picker_roles(&shared, page).get(index) == Some(&zaparoo_app::form_list::Role::Header)
        {
            return false;
        }
        shared.setup.picker_index = index;
    }
    render(ctx, app);
    true
}
