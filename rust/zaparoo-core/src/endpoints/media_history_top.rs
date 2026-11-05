// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaHistoryTopEndpoint`: the most played games (`media.history.top`),
// for the Top played screen and its systems. It is play history, so
// whatever makes history stale (`Tag::any("MediaHistory")`, a finished
// launch, `media.history.changed`) refreshes it too.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaHistoryTopParams, MediaHistoryTopResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

/// Core returns at most this many games.
pub const TOP_LIMIT: u32 = 100;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Default)]
pub struct TopArgs {
    /// One system's games, or every system's when empty.
    pub system: String,
}

impl TopArgs {
    pub fn new(system: &str) -> Self {
        Self {
            system: system.trim().to_string(),
        }
    }
}

#[derive(Debug)]
pub struct MediaHistoryTopEndpoint;

impl Endpoint for MediaHistoryTopEndpoint {
    type Args = TopArgs;
    type Output = MediaHistoryTopResult;
    const NAME: &'static str = "MediaHistoryTop";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_history_top(MediaHistoryTopParams {
                    limit: Some(TOP_LIMIT),
                    systems: if args.system.is_empty() {
                        Vec::new()
                    } else {
                        vec![args.system]
                    },
                })
                .await
        })
    }

    fn provides(args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![
            Tag::specific("MediaHistory", format!("top:{}", args.system)),
            Tag::MEDIA_DB,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_changes_refresh_the_top_list() {
        let args = TopArgs::new(" SNES ");
        assert_eq!(args.system, "SNES");
        let tags = MediaHistoryTopEndpoint::provides(&args, &MediaHistoryTopResult::default());
        // Invalidating `Tag::any("MediaHistory")` matches every id of it.
        assert!(tags.iter().any(|tag| tag.kind == "MediaHistory"));
    }
}
