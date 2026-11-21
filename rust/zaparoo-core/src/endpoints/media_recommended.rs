// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaRecommendedEndpoint`: what the active profile may want to play
// next (`media.recommended`), as Core's one ranked `queue` list, for the
// Recommended screen. Core builds it from play history, favorites, likes
// and decks, so whatever makes any of those stale refreshes it too.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaRecommendedParams, MediaRecommendedResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

/// Core returns at most this many games in one list.
pub const RECOMMENDED_LIMIT: u32 = 30;

#[derive(Debug)]
pub struct MediaRecommendedEndpoint;

impl Endpoint for MediaRecommendedEndpoint {
    type Args = ();
    type Output = MediaRecommendedResult;
    const NAME: &'static str = "MediaRecommended";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        _args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_recommended(MediaRecommendedParams {
                    max_results: Some(RECOMMENDED_LIMIT),
                    layout: Some("queue".to_string()),
                })
                .await
        })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![
            Tag::any(Self::NAME),
            Tag::specific("MediaHistory", "recommended"),
            Tag::specific("MediaUserList", "recommended"),
            Tag::specific("Decks", "recommended"),
            Tag::MEDIA_DB,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_collections_and_decks_refresh_the_list() {
        let tags = MediaRecommendedEndpoint::provides(&(), &MediaRecommendedResult::default());
        for kind in ["MediaHistory", "MediaUserList", "Decks"] {
            assert!(tags.iter().any(|tag| tag.kind == kind), "{kind}");
        }
    }
}
