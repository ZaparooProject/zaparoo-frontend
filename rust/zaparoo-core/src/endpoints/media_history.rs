// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaHistoryEndpoint` — paginated play-history listing for the
// Recently Played screen. Cache key is `(sorted systems, limit)` so two
// singletons asking for the same scoped history share one fetch task.
// Mirrors `MediaBrowseEndpoint`: only the *initial* page hits the cache;
// cursor-driven follow-up pages bypass it and call `Client::media_history`
// directly because each follow-up has a different cursor.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaHistoryParams, MediaHistoryResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct HistoryArgs {
    /// Sorted on construction so cache keys are deterministic across
    /// callers that build the list in different orders.
    pub systems: Vec<String>,
    /// Initial page size. Part of the cache key so two singletons
    /// asking for the same systems list with different page sizes don't
    /// share a fetch (in practice each screen has a fixed page size, so
    /// duplicates inside one process are rare).
    pub limit: u32,
}

impl HistoryArgs {
    pub fn new(mut systems: Vec<String>, limit: u32) -> Self {
        systems.sort();
        systems.dedup();
        Self { systems, limit }
    }
}

#[derive(Debug)]
pub struct MediaHistoryEndpoint;

impl Endpoint for MediaHistoryEndpoint {
    type Args = HistoryArgs;
    type Output = MediaHistoryResult;
    const NAME: &'static str = "MediaHistory";

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_history(MediaHistoryParams {
                    limit: Some(args.limit),
                    cursor: None,
                    systems: args.systems,
                })
                .await
        })
    }

    fn provides(args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::specific(Self::NAME, args.systems.join(","))]
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryArgs;

    #[test]
    fn history_args_sorts_systems() {
        let args = HistoryArgs::new(vec!["NES".into(), "SNES".into(), "GBC".into()], 25);
        assert_eq!(args.systems, vec!["GBC", "NES", "SNES"]);
    }

    #[test]
    fn history_args_dedups_systems() {
        let args = HistoryArgs::new(vec!["SNES".into(), "SNES".into()], 25);
        assert_eq!(args.systems, vec!["SNES"]);
    }

    #[test]
    fn history_args_equal_for_equivalent_inputs_in_any_order() {
        let a = HistoryArgs::new(vec!["SNES".into(), "NES".into()], 25);
        let b = HistoryArgs::new(vec!["NES".into(), "SNES".into()], 25);
        assert_eq!(a, b);
    }

    #[test]
    fn history_args_distinct_for_different_limits() {
        let a = HistoryArgs::new(vec!["NES".into()], 25);
        let b = HistoryArgs::new(vec!["NES".into()], 50);
        assert_ne!(a, b);
    }
}
