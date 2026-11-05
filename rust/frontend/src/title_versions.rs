// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Choose version": a search result stands for one title, launching the
// version Core picked. Its menu lists every version the search matched as
// a page of the context menu, by asking Core for the title's versions
// under the same filters. The rules live in `zaparoo_app::title_versions`.

use zaparoo_app::title_versions::{self as rules, Version};
use zaparoo_core::client::Client;
use zaparoo_core::endpoints::media_search::SearchArgs;
use zaparoo_core::media_types::{MediaItem, MediaSearchParams};

use crate::context_page::{Page, PageRow};
use crate::router::Ctx;
use crate::App;

/// Menu row accepted: list the title's versions, leaving the menu open.
/// `name` is the game's, for the launch cue; `search` is the search that
/// counted them.
pub fn begin(ctx: &Ctx, app: &App, title_id: i64, name: &str, search: &SearchArgs) {
    let client = ctx.store.client();
    let params = params(title_id, search);
    let name = name.to_string();
    crate::context_page::begin(ctx, app, Page::Versions, async move {
        list(&client, params, &name).await
    });
}

/// Core lists the versions matching the search's tags, folder and
/// visibility, so the page holds the ones the result counted. It ignores
/// the query and systems for a title.
fn params(title_id: i64, search: &SearchArgs) -> MediaSearchParams {
    MediaSearchParams {
        title_id: Some(title_id),
        tags: search.tags.clone(),
        path_prefix: (!search.path_prefix.is_empty()).then(|| search.path_prefix.clone()),
        include_hidden: search.include_hidden.then_some(true),
        max_results: Some(rules::MAX_VERSIONS),
        ..MediaSearchParams::default()
    }
}

async fn list(
    client: &Client,
    params: MediaSearchParams,
    name: &str,
) -> Result<Vec<PageRow>, String> {
    let page = client.media_search(params).await.map_err(|e| e.message)?;
    Ok(rows(&page.results, name))
}

/// The versions in Core's order. A version launches as any game row does:
/// by its path, or its script when Core sent no path.
fn rows(items: &[MediaItem], name: &str) -> Vec<PageRow> {
    let items: Vec<(&MediaItem, String)> = items
        .iter()
        .filter_map(|item| {
            let launch_text = [&item.path, &item.zap_script]
                .into_iter()
                .find(|text| !text.trim().is_empty())?
                .clone();
            Some((item, launch_text))
        })
        .collect();
    let tags: Vec<String> = items
        .iter()
        .map(|(item, _)| {
            crate::tag_utils::disambiguating_tag_labels(&item.disambiguating_tags).join(" ")
        })
        .collect();
    let versions: Vec<Version<'_>> = items
        .iter()
        .zip(&tags)
        .map(|((item, _), tags)| Version {
            name: &item.name,
            path: &item.path,
            tags,
        })
        .collect();
    rules::labels(&versions)
        .into_iter()
        .zip(items)
        .map(|(label, (_, launch_text))| PageRow {
            label_key: "",
            label,
            name: name.to_string(),
            launch_text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "fixed fixtures always parse")]

    use super::*;

    fn item(path: &str, region: &str) -> MediaItem {
        serde_json::from_value(serde_json::json!({
            "name": "Super Mario World",
            "path": path,
            "zapScript": format!("@SNES/Super Mario World (region:{region})"),
            "disambiguatingTags": [{ "type": "region", "tag": region }],
        }))
        .expect("version")
    }

    #[test]
    fn the_list_keeps_the_search_filters_and_drops_the_query() {
        let search = SearchArgs {
            query: "mario".into(),
            systems: vec!["SNES".into()],
            tags: vec!["genre:platformer".into()],
            path_prefix: "/media/fat/games/SNES".into(),
            max_results: 30,
            sort: "name-asc".into(),
            include_hidden: true,
        };
        assert_eq!(
            serde_json::to_value(params(7, &search)).expect("serialise"),
            serde_json::json!({
                "titleId": 7,
                "tags": ["genre:platformer"],
                "pathPrefix": "/media/fat/games/SNES",
                "includeHidden": true,
                "maxResults": rules::MAX_VERSIONS,
            })
        );
    }

    #[test]
    fn versions_read_as_their_tags_and_launch_by_path() {
        let rows = rows(
            &[
                item("/g/SNES/Super Mario World (USA).sfc", "us"),
                item("/g/SNES/Super Mario World (Europe).sfc", "eu"),
                MediaItem {
                    path: String::new(),
                    zap_script: String::new(),
                    ..item("", "jp")
                },
            ],
            "Super Mario World",
        );
        assert_eq!(
            rows.len(),
            2,
            "a version with nothing to launch is left out"
        );
        assert_eq!(rows[0].label, "US");
        assert_eq!(rows[1].label, "EU");
        assert_eq!(rows[0].launch_text, "/g/SNES/Super Mario World (USA).sfc");
        assert_eq!(rows[1].name, "Super Mario World");
    }
}
