// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaSearchEndpoint` — first page of a text search, shared by the Search
// screen's preview and its results.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaSearchParams, MediaSearchResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash)]
pub struct SearchArgs {
    pub query: String,
    /// Empty searches every system.
    pub system_id: String,
    pub tags: Vec<String>,
    /// Empty searches every folder.
    pub path_prefix: String,
    pub max_results: u32,
    pub sort: String,
    /// Part of the key: the two visibilities are different result sets.
    pub include_hidden: bool,
}

impl SearchArgs {
    /// The request these arguments stand for, at `cursor` or its first page.
    #[must_use]
    pub fn params(&self, cursor: Option<String>) -> MediaSearchParams {
        let some = |text: &str| (!text.is_empty()).then(|| text.to_string());
        MediaSearchParams {
            query: some(self.query.trim()),
            systems: some(&self.system_id).into_iter().collect(),
            max_results: Some(self.max_results),
            cursor,
            tags: self.tags.clone(),
            letter: None,
            sort: some(&self.sort),
            path_prefix: some(&self.path_prefix),
            include_hidden: self.include_hidden.then_some(true),
        }
    }
}

#[derive(Debug)]
pub struct MediaSearchEndpoint;

impl Endpoint for MediaSearchEndpoint {
    type Args = SearchArgs;
    type Output = MediaSearchResult;
    const NAME: &'static str = "MediaSearch";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.media_search(args.params(None)).await })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME), Tag::MEDIA_DB]
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "a fixed request always serialises")]

    use super::SearchArgs;

    #[test]
    fn empty_fields_are_left_out_of_the_request() {
        let args = SearchArgs {
            query: "  mario ".into(),
            max_results: 100,
            sort: "name-asc".into(),
            ..SearchArgs::default()
        };
        assert_eq!(
            serde_json::to_value(args.params(None)).expect("serialise"),
            serde_json::json!({"query": "mario", "maxResults": 100, "sort": "name-asc"})
        );
        let scoped = SearchArgs {
            system_id: "SNES".into(),
            tags: vec!["genre:rpg".into()],
            path_prefix: "/roms/SNES/RPG".into(),
            max_results: 50,
            ..SearchArgs::default()
        };
        assert_eq!(
            serde_json::to_value(scoped.params(Some("abc".into()))).expect("serialise"),
            serde_json::json!({
                "systems": ["SNES"],
                "maxResults": 50,
                "cursor": "abc",
                "tags": ["genre:rpg"],
                "pathPrefix": "/roms/SNES/RPG",
            })
        );
    }

    #[test]
    fn hidden_matches_are_asked_for_on_every_page() {
        let args = SearchArgs {
            max_results: 100,
            include_hidden: true,
            ..SearchArgs::default()
        };
        for cursor in [None, Some("abc".to_string())] {
            let params = serde_json::to_value(args.params(cursor)).expect("serialise");
            assert_eq!(params["includeHidden"], true);
        }
        assert_ne!(
            args,
            SearchArgs {
                include_hidden: false,
                ..args.clone()
            }
        );
    }
}
