// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaBrowseEndpoint` — directory listing for the games view. Cache key
// is `(path, sorted systems)` so two singletons asking for the same
// scoped path share one fetch task. The frontend only uses this Endpoint
// for the *initial* page of a browse target; cursor-driven follow-up
// pages bypass the cache and call `Client::media_browse` directly,
// because each follow-up has a different cursor.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaBrowseParams, MediaBrowseResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BrowseArgs {
    /// Empty string means "system roots" — caller pairs it with a
    /// non-empty `systems` list and Core returns launcher routes for
    /// those systems.
    pub path: String,
    /// Sorted on construction so cache keys are deterministic across
    /// callers that build the list in different orders.
    pub systems: Vec<String>,
    /// Initial page size. Part of the cache key so two singletons
    /// asking for the same path with different page sizes don't share
    /// a fetch (in practice each screen has a fixed page size, so
    /// duplicates inside one process are rare).
    pub max_results: u32,
}

impl BrowseArgs {
    pub fn new(path: String, mut systems: Vec<String>, max_results: u32) -> Self {
        systems.sort();
        systems.dedup();
        Self {
            path,
            systems,
            max_results,
        }
    }
}

#[derive(Debug)]
pub struct MediaBrowseEndpoint;

impl Endpoint for MediaBrowseEndpoint {
    type Args = BrowseArgs;
    type Output = MediaBrowseResult;
    const NAME: &'static str = "MediaBrowse";

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_browse(MediaBrowseParams {
                    path: args.path,
                    systems: args.systems,
                    max_results: Some(args.max_results),
                    cursor: None,
                    letter: None,
                    sort: None,
                })
                .await
        })
    }

    fn provides(args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::specific(
            Self::NAME,
            format!("{}::{}", args.path, args.systems.join(",")),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::BrowseArgs;

    #[test]
    fn browse_args_sorts_systems() {
        let args = BrowseArgs::new(
            String::new(),
            vec!["NES".into(), "SNES".into(), "GBC".into()],
            15,
        );
        assert_eq!(args.systems, vec!["GBC", "NES", "SNES"]);
    }

    #[test]
    fn browse_args_dedups_systems() {
        let args = BrowseArgs::new(String::new(), vec!["SNES".into(), "SNES".into()], 15);
        assert_eq!(args.systems, vec!["SNES"]);
    }

    #[test]
    fn browse_args_equal_for_equivalent_inputs_in_any_order() {
        let a = BrowseArgs::new("/roms/shared".into(), vec!["SNES".into(), "NES".into()], 15);
        let b = BrowseArgs::new("/roms/shared".into(), vec!["NES".into(), "SNES".into()], 15);
        assert_eq!(a, b);
    }

    #[test]
    fn browse_args_distinct_for_different_page_sizes() {
        let a = BrowseArgs::new(String::new(), vec!["NES".into()], 15);
        let b = BrowseArgs::new(String::new(), vec!["NES".into()], 30);
        assert_ne!(a, b);
    }
}
