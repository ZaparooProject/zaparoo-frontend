// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaFavoritesEndpoint` — first page of media tagged `user:favorite`.

use crate::client::{Client, ClientError};
use crate::media_types::{MediaSearchParams, MediaSearchResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct FavoritesArgs {
    pub max_results: u32,
}

impl FavoritesArgs {
    #[must_use]
    pub const fn new(max_results: u32) -> Self {
        Self { max_results }
    }
}

#[derive(Debug)]
pub struct MediaFavoritesEndpoint;

impl Endpoint for MediaFavoritesEndpoint {
    type Args = FavoritesArgs;
    type Output = MediaSearchResult;
    const NAME: &'static str = "MediaFavorites";

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_search(MediaSearchParams {
                    max_results: Some(args.max_results),
                    tags: vec!["user:favorite".into()],
                    ..MediaSearchParams::default()
                })
                .await
        })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}
