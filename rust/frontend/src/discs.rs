// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Choose disc": a folder of one game's discs is a single row that launches
// the disc Core picked. Its menu lists them all as a page of the context
// menu, by browsing the folder. The rules live in `zaparoo_app::multi_disc`.

use zaparoo_app::multi_disc::{self as rules, Label};
use zaparoo_core::client::Client;
use zaparoo_core::media_types::{BrowseEntry, MediaBrowseParams};

use crate::context_page::{Page, PageRow};
use crate::router::Ctx;
use crate::App;

/// Menu row accepted: list the folder's discs, leaving the menu open.
/// `name` is the game's, for the launch cue.
pub fn begin(ctx: &Ctx, app: &App, system_id: &str, path: &str, name: &str, include_hidden: bool) {
    if system_id.is_empty() || path.is_empty() {
        return;
    }
    let client = ctx.store.client();
    let system_id = system_id.to_string();
    let path = path.to_string();
    let name = name.to_string();
    crate::context_page::begin(ctx, app, Page::Discs, async move {
        list(&client, &system_id, &path, &name, include_hidden).await
    });
}

async fn list(
    client: &Client,
    system_id: &str,
    path: &str,
    name: &str,
    include_hidden: bool,
) -> Result<Vec<PageRow>, String> {
    let page = client
        .media_browse(MediaBrowseParams {
            path: path.to_string(),
            systems: vec![system_id.to_string()],
            max_results: Some(rules::MAX_DISCS),
            include_hidden: Some(include_hidden),
            ..MediaBrowseParams::default()
        })
        .await
        .map_err(|e| e.message)?;
    Ok(rows(&page.entries, name))
}

/// The discs in Core's order. A disc launches as any game row does: by its
/// path, or its script when Core sent no path.
fn rows(entries: &[BrowseEntry], name: &str) -> Vec<PageRow> {
    entries
        .iter()
        .filter(|entry| !entry.is_folder())
        .filter_map(|entry| {
            let launch_text = [&entry.path, &entry.zap_script]
                .into_iter()
                .find(|text| !text.trim().is_empty())?
                .clone();
            let tags = entry
                .tags
                .iter()
                .map(|tag| (tag.tag_type.as_str(), tag.tag.as_str()));
            let (label_key, label) = match rules::label(tags, &entry.path, &entry.name) {
                Label::Disc(number) => ("disc", number),
                Label::File(file) => ("", file),
            };
            Some(PageRow {
                label_key,
                label,
                name: name.to_string(),
                launch_text,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::media_types::TagInfo;

    fn disc(path: &str, number: &str) -> BrowseEntry {
        BrowseEntry {
            name: "Chrono Cross".into(),
            path: path.into(),
            entry_type: "media".into(),
            tags: vec![TagInfo {
                tag: number.into(),
                tag_type: "disc".into(),
                ..TagInfo::default()
            }],
            ..BrowseEntry::default()
        }
    }

    #[test]
    fn each_disc_is_a_row_that_launches_its_own_file() {
        let entries = [
            disc("/g/CC/CC (Disc 1).chd", "1"),
            disc("/g/CC/CC (Disc 2).chd", "2"),
            BrowseEntry {
                entry_type: "directory".into(),
                path: "/g/CC/extras".into(),
                ..BrowseEntry::default()
            },
            // Core tagged no disc: the file name tells it apart.
            disc("/g/CC/CC (Bonus).chd", ""),
        ];
        let rows = rows(&entries, "Chrono Cross");
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[1].label_key, rows[1].label.as_str()), ("disc", "2"));
        assert_eq!(rows[1].launch_text, "/g/CC/CC (Disc 2).chd");
        assert_eq!(rows[1].name, "Chrono Cross");
        assert_eq!(
            (rows[2].label_key, rows[2].label.as_str()),
            ("", "CC (Bonus)")
        );
    }
}
