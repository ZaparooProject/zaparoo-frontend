// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Discover alt. versions": find the other builds of an Arcade game and
// offer them as a page of the context menu the user opened it from. The
// menu stays open, its rows become the alternates, and Back returns to
// the menu. The rules live in `zaparoo_app::alternate_versions`.

use std::collections::HashSet;

use slint::{ComponentHandle, ModelRc, VecModel};
use zaparoo_app::alternate_versions as rules;
use zaparoo_core::client::Client;
use zaparoo_core::media_types::{MediaBrowseParams, MediaMetaParams, MediaSearchParams};

use crate::router::{lock, Ctx};
use crate::App;

/// One discovered build: what to show, and what to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternateRow {
    pub name: String,
    pub launch_text: String,
}

/// The context menu's alternates page.
#[derive(Debug, Clone, Default)]
pub struct AlternatesModel {
    pub rows: Vec<AlternateRow>,
    /// True while the menu is showing the alternates rather than its
    /// own entries.
    pub showing: bool,
    /// Bumped per run so a late discovery cannot land on a menu the
    /// user has moved on from.
    pub seq: u64,
    /// The "Searching…" row's wait while a discovery is unanswered.
    pub wait: Option<crate::cue::LocalWait>,
}

/// The row id for an entry on the alternates page.
fn row_id(index: usize) -> String {
    format!("alternate_version:{index}")
}

/// Menu row accepted: start discovery, leaving the menu open with its
/// "Discover alt. versions" row relabelled to say it is searching.
pub fn begin(ctx: &Ctx, app: &App, system_id: &str, name: &str, path: &str) {
    if !rules::can_discover(system_id, name, path) {
        return;
    }
    let (ticket, stale) = {
        let mut shared = lock(&ctx.shared);
        shared.alternates.seq += 1;
        shared.alternates.rows.clear();
        shared.alternates.showing = false;
        (shared.alternates.seq, shared.alternates.wait.take())
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    // The row says it is searching only once the search is a wait.
    let shared = ctx.shared.clone();
    let wait = crate::cue::begin_local(app, move |app| {
        if lock(&shared).alternates.seq == ticket
            && app.global::<crate::Overlays>().get_context_open()
        {
            relabel_discover(app, "discover:searching");
        }
    });
    lock(&ctx.shared).alternates.wait = Some(wait);
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let system_id = system_id.to_string();
    let name = name.to_string();
    let path = path.to_string();
    ctx.handle.spawn(async move {
        let found = discover(&client, &system_id, &name, &path).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let wait = {
                let mut shared = lock(&ctx2.shared);
                if shared.alternates.seq != ticket {
                    return;
                }
                shared.alternates.wait.take()
            };
            let finish = move |app: &App| {
                // The menu the discovery belongs to has to still be open.
                if lock(&ctx2.shared).alternates.seq != ticket
                    || !app.global::<crate::Overlays>().get_context_open()
                {
                    return;
                }
                match found {
                    Ok(rows) if rows.is_empty() => relabel_discover(app, "discover:none"),
                    Ok(rows) => {
                        lock(&ctx2.shared).alternates.rows = rows;
                        present(&ctx2, app);
                    }
                    Err(message) => {
                        tracing::warn!("discover alternate versions failed: {message}");
                        // The search is over: the row must not go on
                        // saying it is still running.
                        restore_discover(app);
                        crate::router::report_action_error(&ctx2, app, "alternate_discovery", "");
                    }
                }
            };
            match wait {
                Some(wait) => crate::cue::end_local(&app, wait, finish),
                None => finish(&app),
            }
        });
    });
}

/// Swap the open menu's "Discover alt. versions" row for one that says
/// what happened. The replacement carries no id, so pressing it does
/// nothing.
fn relabel_discover(app: &App, key: &str) {
    use slint::Model as _;
    let entries = app.global::<crate::Overlays>().get_context_entries();
    let rows: Vec<crate::MenuEntry> = entries
        .iter()
        .map(|entry| {
            if entry.id.as_str() == "discover" || entry.label_key.as_str().starts_with("discover:")
            {
                crate::router::menu_row_keyed("", key, "")
            } else {
                entry
            }
        })
        .collect();
    app.global::<crate::Overlays>()
        .set_context_entries(ModelRc::new(VecModel::from(rows)));
}

/// Put the "Discover alt. versions" row back as it was, pressable again.
fn restore_discover(app: &App) {
    use slint::Model as _;
    let entries = app.global::<crate::Overlays>().get_context_entries();
    let rows: Vec<crate::MenuEntry> = entries
        .iter()
        .map(|entry| {
            if entry.label_key.as_str().starts_with("discover:") {
                crate::router::menu_row("discover")
            } else {
                entry
            }
        })
        .collect();
    app.global::<crate::Overlays>()
        .set_context_entries(ModelRc::new(VecModel::from(rows)));
}

/// Replace the menu's rows with the alternates.
fn present(ctx: &Ctx, app: &App) {
    let entries: Vec<crate::MenuEntry> = {
        let mut shared = lock(&ctx.shared);
        shared.alternates.showing = true;
        shared
            .alternates
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| crate::router::menu_entry(&row_id(index), &row.name))
            .collect()
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_index(0);
}

/// True when the menu is showing the alternates page.
pub fn showing(ctx: &Ctx) -> bool {
    lock(&ctx.shared).alternates.showing
}

/// Back on the alternates page returns to the menu it came from
/// instead of closing it.
pub fn leave(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.alternates.showing = false;
        shared.alternates.rows.clear();
        shared.alternates.seq += 1;
    }
    crate::games::reopen_context_menu(ctx, app);
}

/// A row on the alternates page was accepted: run it and close.
pub fn accept(ctx: &Ctx, app: &App, id: &str) {
    let index = id
        .strip_prefix("alternate_version:")
        .and_then(|index| index.parse::<usize>().ok());
    let row = index.and_then(|index| lock(&ctx.shared).alternates.rows.get(index).cloned());
    {
        let mut shared = lock(&ctx.shared);
        shared.alternates.showing = false;
        shared.alternates.rows.clear();
        shared.alternates.seq += 1;
    }
    app.global::<crate::Overlays>().set_context_open(false);
    if let Some(row) = row {
        crate::router::launch(ctx, app, row.launch_text, &row.name);
    }
}

/// The Core side: resolve the game's canonical title, search the
/// Arcade system for the same title under an alternates folder, then
/// browse each folder those hits name and collect the files.
async fn discover(
    client: &Client,
    system_id: &str,
    name: &str,
    selected_path: &str,
) -> Result<Vec<AlternateRow>, String> {
    // The scraped title is the better seed; the row's own name stands
    // in when Core has no metadata for it.
    let canonical_title = client
        .media_meta(MediaMetaParams {
            media_id: None,
            system: system_id.to_string(),
            path: selected_path.to_string(),
        })
        .await
        .ok()
        .map(|result| result.media.title.name.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| name.trim().to_string());
    let result = client
        .media_search(MediaSearchParams {
            query: Some(canonical_title.clone()),
            systems: vec![rules::ARCADE_SYSTEM_ID.to_string()],
            max_results: Some(rules::MAX_ALT_RESULTS),
            ..MediaSearchParams::default()
        })
        .await
        .map_err(|e| e.message)?;
    let selected_norm = rules::normalize_seed_title(&canonical_title);
    let mut folders = HashSet::new();
    for entry in result.results {
        let relative = entry.relative_path.unwrap_or_default();
        if !rules::is_alternate_candidate(&entry.path, &relative, selected_path) {
            continue;
        }
        if rules::normalize_seed_title(&entry.name) != selected_norm {
            continue;
        }
        if let Some(folder) = rules::alternate_folder_path(&entry.path) {
            folders.insert(folder);
        }
    }
    if folders.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for folder in folders {
        let page = client
            .media_browse(MediaBrowseParams {
                path: folder,
                systems: vec![rules::ARCADE_SYSTEM_ID.to_string()],
                max_results: Some(rules::MAX_FOLDER_RESULTS),
                ..MediaBrowseParams::default()
            })
            .await
            .map_err(|e| e.message)?;
        for entry in page.entries {
            if !rules::is_discovered_entry(entry.is_folder(), &entry.path, selected_path) {
                continue;
            }
            if !seen.insert(entry.path.clone()) {
                continue;
            }
            let launch_text = if entry.path.trim().is_empty() {
                entry.zap_script.clone()
            } else {
                entry.path.clone()
            };
            if launch_text.is_empty() {
                continue;
            }
            rows.push(AlternateRow {
                // The filename is what tells two builds apart.
                name: zaparoo_app::media_list::file_stem_or_name(&entry.path, &entry.name),
                launch_text,
            });
        }
    }
    Ok(rows)
}
