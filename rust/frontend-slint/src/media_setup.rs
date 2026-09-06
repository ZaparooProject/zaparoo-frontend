// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The media-job setup modals: the Update media database and Update
// metadata forms the Library settings rows open, their in-panel scope
// and source pickers, and the calls that start the job. Mirrors
// IndexSetupModal.qml and ScrapeSetupModal.qml, with the rules in
// `zaparoo_app::media_setup`.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::media_setup::{self as rules, FormRow, Kind};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{MediaIndexParams, MediaScrapeParams, ScraperInfo};

use crate::router::{lock, Ctx, Shared};
use crate::{App, SettingsRow, SetupInput, SetupModalView, SetupPickerRow};

/// Rows the picker page shows at once before it scrolls.
const PICKER_WINDOW: usize = 7;

/// The open form's state.
#[derive(Debug, Clone)]
pub struct SetupModel {
    pub open: bool,
    pub kind: Kind,
    pub index: usize,
    /// The scope token the Systems row holds.
    pub scope: String,
    /// The scraper id the Source row holds (Scrape only).
    pub scraper: String,
    pub rescrape: bool,
    /// Which page the panel shows: None is the form.
    pub picker: Option<FormRow>,
    pub picker_index: usize,
    /// Scrapers Core reported, once the list has answered.
    pub scrapers: Vec<ScraperInfo>,
}

impl SetupModel {
    pub fn new() -> Self {
        Self {
            open: false,
            kind: Kind::Index,
            index: 0,
            scope: "*".to_string(),
            scraper: String::new(),
            rescrape: false,
            picker: None,
            picker_index: 0,
            scrapers: Vec::new(),
        }
    }

    fn rows(&self) -> &'static [FormRow] {
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

/// The scope rows the picker offers: All systems, the categories that
/// have indexable systems, then every system by display name.
fn scope_entries(shared: &Shared) -> Vec<rules::ScopeEntry> {
    let systems: Vec<(String, String)> = shared
        .systems
        .iter()
        .map(|s| (s.id.clone(), crate::systems::display_name(shared, &s.id)))
        .collect();
    rules::scope_entries(&shared.categories, &systems)
}

fn system_name(shared: &Shared, id: &str) -> String {
    crate::systems::display_name(shared, id)
}

/// The label pair a picker row (or the form's own row) renders with.
fn scope_pair(shared: &Shared, token: &str) -> (&'static str, String) {
    rules::scope_label(&rules::parse_scope(token), &|id| system_name(shared, id))
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
    view.set_kind(SharedString::from(match model.kind {
        Kind::Index => "index",
        Kind::Scrape => "scrape",
    }));
    view.set_index(i32::try_from(model.index).unwrap_or(0));

    let rows: Vec<SettingsRow> = model
        .rows()
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut out = SettingsRow {
                kind: SharedString::from("field"),
                id: SharedString::from(match row {
                    // The start row names the job it starts.
                    FormRow::Start => match model.kind {
                        Kind::Index => "startIndex",
                        Kind::Scrape => "startImport",
                    },
                    other => other.id(),
                }),
                control: SharedString::from(row.control()),
                enabled: true,
                y_offset: (i as i32 * row_h) as f32,
                height: row_h as f32,
                ..Default::default()
            };
            match row {
                FormRow::Systems => {
                    let (kind, name) = scope_pair(&shared, &model.scope);
                    out.value = SharedString::from(kind);
                    out.value_name = SharedString::from(name.as_str());
                }
                FormRow::Source => {
                    out.value = SharedString::from("source");
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
    view.set_rows(ModelRc::new(VecModel::from(rows)));

    let Some(page) = model.picker else {
        view.set_picker_page(false);
        view.set_picker_rows(ModelRc::new(VecModel::from(Vec::<SetupPickerRow>::new())));
        return;
    };
    view.set_picker_page(true);
    view.set_picker_title(SharedString::from(page.id()));
    let entries: Vec<(&'static str, String)> = match page {
        FormRow::Source => model
            .scrapers
            .iter()
            .map(|s| {
                (
                    "source",
                    if s.name.is_empty() {
                        s.id.clone()
                    } else {
                        s.name.clone()
                    },
                )
            })
            .collect(),
        _ => scope_entries(&shared)
            .into_iter()
            .map(|entry| (entry.kind, entry.name))
            .collect(),
    };
    // Window the rows around the cursor, like the browse list does.
    let visible = PICKER_WINDOW.min(entries.len().max(1));
    let top =
        zaparoo_app::media_list::list_view_top(model.picker_index, entries.len(), visible, None);
    let rows: Vec<SetupPickerRow> = entries
        .iter()
        .skip(top)
        .take(visible)
        .map(|(kind, name)| SetupPickerRow {
            kind: SharedString::from(*kind),
            name: SharedString::from(name.as_str()),
        })
        .collect();
    view.set_picker_rows(ModelRc::new(VecModel::from(rows)));
    view.set_picker_sel(i32::try_from(model.picker_index.saturating_sub(top)).unwrap_or(0));
    view.set_has_above(top > 0);
    view.set_has_below(top + visible < entries.len());
}

// ---------- Opening ----------

/// Open one of the forms. Scrape seeds its source from the persisted
/// scraper and refreshes the list from Core.
pub fn open(ctx: &Ctx, app: &App, kind: Kind) {
    {
        let mut shared = lock(&ctx.shared);
        let persisted = shared.persist.settings.metadata_scraper.clone();
        let model = &mut shared.setup;
        model.open = true;
        model.kind = kind;
        model.index = 0;
        model.scope = "*".to_string();
        model.rescrape = false;
        model.picker = None;
        model.picker_index = 0;
        if model.scraper.is_empty() {
            model.scraper = persisted;
        }
    }
    render(ctx, app);
    if kind == Kind::Scrape {
        fetch_scrapers(ctx, app);
    }
}

pub fn close(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).setup.open = false;
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

/// Core's scraper inventory, for the Source row.
fn fetch_scrapers(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let result = match client.scrapers().await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("scraper list unavailable: {}", e.message);
                let _ = weak.upgrade_in_event_loop(move |app| {
                    crate::router::report_action_error(&ctx2, &app, "media_scrapers", "");
                });
                return;
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx2.shared);
                let persisted = shared.persist.settings.metadata_scraper.clone();
                let model = &mut shared.setup;
                model.scrapers = result.scrapers;
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
            render(&ctx2, &app);
        });
    });
}

// ---------- Input ----------

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let (page, len, picker_len) = {
        let shared = lock(&ctx.shared);
        let model = &shared.setup;
        let picker_len = model.picker.map_or(0, |page| match page {
            FormRow::Source => model.scrapers.len(),
            _ => scope_entries(&shared).len(),
        });
        (model.picker, model.rows().len(), picker_len)
    };

    if page.is_some() {
        match action {
            actions::UP | actions::DOWN => {
                let delta = if action == actions::UP { -1 } else { 1 };
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.setup;
                model.picker_index = rules::move_index(model.picker_index, picker_len, delta);
                drop(shared);
                render(ctx, app);
            }
            actions::ACCEPT => pick(ctx, app),
            actions::CANCEL => {
                lock(&ctx.shared).setup.picker = None;
                render(ctx, app);
            }
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
        } else {
            let scope = shared.setup.scope.clone();
            scope_entries(&shared)
                .iter()
                .position(|entry| entry.token == scope)
        };
        let model = &mut shared.setup;
        model.picker = Some(page);
        model.picker_index = seat.unwrap_or(0);
    }
    render(ctx, app);
}

/// Take the highlighted picker row back to the form.
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
        } else {
            let picked = scope_entries(&shared).get(index).map(|e| e.token.clone());
            if let Some(token) = picked {
                shared.setup.scope = token;
            }
        }
        shared.setup.picker = None;
    }
    render(ctx, app);
}

/// Start the job over the chosen scope and close the panel.
fn start(ctx: &Ctx, app: &App) {
    let (kind, systems, scraper, rescrape) = {
        let shared = lock(&ctx.shared);
        let scope = rules::parse_scope(&shared.setup.scope);
        let catalog = zaparoo_app::systems::indexable_ids;
        let systems = rules::resolved_systems(&scope, &|category| {
            catalog(&crate::systems::catalog_systems(&shared.systems), category)
        });
        (
            shared.setup.kind,
            systems,
            shared.setup.scraper.clone(),
            shared.setup.rescrape,
        )
    };
    match kind {
        Kind::Index => {
            let client = ctx.store.client();
            let weak = app.as_weak();
            let params = MediaIndexParams {
                systems: (!systems.is_empty()).then_some(systems),
            };
            let ctx2 = ctx.clone();
            ctx.handle.spawn(async move {
                if let Err(e) = client.media_generate(params).await {
                    tracing::warn!("media update failed to start: {}", e.message);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        crate::router::report_action_error(&ctx2, &app, "media_index", "");
                    });
                }
            });
        }
        Kind::Scrape => {
            if scraper.is_empty() {
                return;
            }
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
                systems,
                force: rescrape,
            };
            let ctx2 = ctx.clone();
            ctx.handle.spawn(async move {
                if let Err(e) = client.media_scrape(params).await {
                    tracing::warn!("metadata update failed to start: {}", e.message);
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        crate::router::report_action_error(&ctx2, &app, "media_scrape", "");
                    });
                }
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
            if let Ok(index) = usize::try_from(i) {
                lock(&ctx.shared).setup.index = index;
                accept(&ctx, &app);
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
        let index = top + local;
        if index >= len {
            return false;
        }
        shared.setup.picker_index = index;
    }
    render(ctx, app);
    true
}
