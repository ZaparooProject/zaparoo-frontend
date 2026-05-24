// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaTagsUpdateMutation` — add/remove mutable user tags for a media row.

use crate::client::{Client, ClientError};
use crate::endpoints::{
    media_browse::MediaBrowseEndpoint, media_favorites::MediaFavoritesEndpoint,
    media_search::MediaSearchEndpoint,
};
use crate::media_types::{MediaTagsUpdateParams, MediaTagsUpdateResult};
use crate::store::{Endpoint, Mutation, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct MediaTagsUpdateMutation;

impl Mutation for MediaTagsUpdateMutation {
    type Args = MediaTagsUpdateParams;
    type Output = MediaTagsUpdateResult;

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.media_tags_update(args).await })
    }

    fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        vec![
            Tag::any(MediaBrowseEndpoint::NAME),
            Tag::any(MediaFavoritesEndpoint::NAME),
            Tag::any(MediaSearchEndpoint::NAME),
        ]
    }
}
