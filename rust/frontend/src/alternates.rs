// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Discover alt. versions": find the other builds of an Arcade game and
// offer them as a page of the context menu the user opened it from. The
// menu stays open, its rows become the alternates, and Back returns to
// the menu. The rules live in `zaparoo_app::alternate_versions`.

use std::collections::HashSet;

use zaparoo_app::alternate_versions as rules;
use zaparoo_core::client::Client;
use zaparoo_core::media_types::{MediaBrowseParams, MediaMetaParams, MediaSearchParams};

use crate::context_page::{Page, PageRow};
use crate::router::Ctx;
use crate::App;

/// Menu row accepted: start discovery, leaving the menu open with its
/// "Discover alt. versions" row relabelled to say it is searching.
pub fn begin(ctx: &Ctx, app: &App, system_id: &str, name: &str, path: &str) {
    if !rules::can_discover(system_id, name, path) {
        return;
    }
    let client = ctx.store.client();
    let system_id = system_id.to_string();
    let name = name.to_string();
    let path = path.to_string();
    crate::context_page::begin(ctx, app, Page::Alternates, async move {
        discover(&client, &system_id, &name, &path).await
    });
}

/// The Core side: resolve the game's canonical title, search the
/// Arcade system for the same title under an alternates folder, then
/// browse each folder those hits name and collect the files.
async fn discover(
    client: &Client,
    system_id: &str,
    name: &str,
    selected_path: &str,
) -> Result<Vec<PageRow>, String> {
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
            // The builds are still alternates of this game when their
            // folder is hidden from the list, which takes them out of an
            // ordinary search. The folder itself still browses.
            include_hidden: Some(true),
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
            // The filename is what tells two builds apart.
            let name = zaparoo_app::media_list::file_stem_or_name(&entry.path, &entry.name);
            rows.push(PageRow {
                label_key: "",
                label: name.clone(),
                name,
                launch_text,
            });
        }
    }
    Ok(rows)
}
