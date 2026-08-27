// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaBrowseEndpoint` — directory listing for the games view. Cache key
// is `(path, sorted systems, sorted tags)` so two singletons asking for same
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
    /// Sorted tag scope. Empty means unfiltered browsing.
    pub tags: Vec<String>,
    /// Initial page size. Part of the cache key so two singletons
    /// asking for the same path with different page sizes don't share
    /// a fetch (in practice each screen has a fixed page size, so
    /// duplicates inside one process are rare).
    pub max_results: u32,
    /// `media.browse`'s pathless `rootView` (PR #1312) -- `Some("contents")`
    /// whenever `path` is empty and `systems` names exactly one system,
    /// `None` otherwise. Derived here rather than accepted as a
    /// constructor parameter so no caller can forget it; a field on the
    /// struct rather than computed inline at the call site so it
    /// participates in the derived `Eq`/`Hash` the store's cache keys on,
    /// even though in practice it can never actually vary independently
    /// of `path`/`systems` for a given cache entry.
    pub root_view: Option<String>,
}

impl BrowseArgs {
    pub fn new(
        path: String,
        mut systems: Vec<String>,
        max_results: u32,
        mut tags: Vec<String>,
    ) -> Self {
        systems.sort();
        systems.dedup();
        tags.sort();
        tags.dedup();
        let root_view = root_view_for(&path, &systems);
        Self {
            path,
            systems,
            tags,
            max_results,
            root_view,
        }
    }
}

/// The one rule that decides `media.browse`/`media.browse.index`'s
/// pathless `rootView` (PR #1312): `Some("contents")` for a pathless
/// browse scoped to exactly one system, `None` otherwise (multi-system,
/// no system, or already below a path -- Core ignores the field there
/// anyway). `BrowseArgs::new` is the only caller that goes through the
/// cached `Endpoint` path; every direct `Client::media_browse`/
/// `media_browse_index` call elsewhere (cursor follow-ups, the letter
/// index) must derive the same value the initial page did, since Core
/// embeds the root view in the browse cursor and validates follow-ups
/// against it -- exported so those call sites share this exact function
/// rather than re-deriving the rule and risking drift.
pub fn root_view_for(path: &str, systems: &[String]) -> Option<String> {
    (path.is_empty() && systems.len() == 1).then(|| "contents".to_string())
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
                    root_view: args.root_view,
                    max_results: Some(args.max_results),
                    cursor: None,
                    tags: args.tags,
                    letter: None,
                    sort: None,
                })
                .await
        })
    }

    fn provides(args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::specific(
            Self::NAME,
            format!(
                "{}::{}::{}",
                args.path,
                args.systems.join(","),
                args.tags.join(",")
            ),
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
            Vec::new(),
        );
        assert_eq!(args.systems, vec!["GBC", "NES", "SNES"]);
    }

    #[test]
    fn browse_args_dedups_systems() {
        let args = BrowseArgs::new(
            String::new(),
            vec!["SNES".into(), "SNES".into()],
            15,
            Vec::new(),
        );
        assert_eq!(args.systems, vec!["SNES"]);
    }

    #[test]
    fn browse_args_equal_for_equivalent_inputs_in_any_order() {
        let a = BrowseArgs::new(
            "/roms/shared".into(),
            vec!["SNES".into(), "NES".into()],
            15,
            Vec::new(),
        );
        let b = BrowseArgs::new(
            "/roms/shared".into(),
            vec!["NES".into(), "SNES".into()],
            15,
            Vec::new(),
        );
        assert_eq!(a, b);
    }

    #[test]
    fn browse_args_distinct_for_different_page_sizes() {
        let a = BrowseArgs::new(String::new(), vec!["NES".into()], 15, Vec::new());
        let b = BrowseArgs::new(String::new(), vec!["NES".into()], 30, Vec::new());
        assert_ne!(a, b);
    }

    #[test]
    fn browse_args_normalize_and_distinguish_tag_scope() {
        let unfiltered = BrowseArgs::new(String::new(), vec!["NES".into()], 15, Vec::new());
        let filtered = BrowseArgs::new(
            String::new(),
            vec!["NES".into()],
            15,
            vec!["user:favorite".into(), "user:favorite".into()],
        );
        assert_ne!(unfiltered, filtered);
        assert_eq!(filtered.tags, vec!["user:favorite"]);
    }

    #[test]
    fn browse_args_requests_root_contents_only_for_a_pathless_single_system() {
        let args = BrowseArgs::new(String::new(), vec!["SNES".into()], 15, Vec::new());
        assert_eq!(args.root_view.as_deref(), Some("contents"));
    }

    #[test]
    fn browse_args_leaves_root_view_unset_below_the_system_root() {
        let args = BrowseArgs::new("/roms/SNES".into(), vec!["SNES".into()], 15, Vec::new());
        assert_eq!(args.root_view, None);
    }

    #[test]
    fn browse_args_leaves_root_view_unset_for_multiple_systems() {
        let args = BrowseArgs::new(
            String::new(),
            vec!["SNES".into(), "NES".into()],
            15,
            Vec::new(),
        );
        assert_eq!(args.root_view, None);
    }

    #[test]
    fn browse_args_leaves_root_view_unset_with_no_system_scope() {
        // The category-browse "Systems" screen root: no system selected yet,
        // Core returns launcher routes for every configured system.
        let args = BrowseArgs::new(String::new(), Vec::new(), 15, Vec::new());
        assert_eq!(args.root_view, None);
    }
}
