// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaSearchEndpoint` — per-system search results.
// `Args = String` is the system id; the store hashes it into the cache
// key so two singletons asking for the same system share one fetch task
// and two singletons asking for different systems run independently.
// `provides` returns `Tag::specific("MediaSearch", id)` so a future
// `RunEndpoint` mutation that affects one system's library can
// invalidate just that entry without touching siblings.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaSearchParams, MediaSearchResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct MediaSearchEndpoint;

impl Endpoint for MediaSearchEndpoint {
    type Args = String;
    type Output = MediaSearchResult;
    const NAME: &'static str = "MediaSearch";

    fn fetch(
        client: Arc<Client>,
        system_id: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_search(MediaSearchParams {
                    systems: vec![system_id],
                    max_results: Some(100),
                    ..MediaSearchParams::default()
                })
                .await
        })
    }

    fn provides(args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::specific(Self::NAME, args.clone())]
    }
}
