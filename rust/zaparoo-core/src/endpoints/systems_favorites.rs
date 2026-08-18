// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `SystemsFavoritesEndpoint` — standard `systems` scoped to favorite media.

use crate::client::{Client, ClientError};
use crate::media_types::{SystemsParams, SystemsResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

const FAVORITE_TAG: &str = "user:favorite";

#[derive(Debug)]
pub struct SystemsFavoritesEndpoint;

fn favorite_systems_params() -> SystemsParams {
    SystemsParams {
        tags: vec![FAVORITE_TAG.into()],
        ..SystemsParams::default()
    }
}

impl Endpoint for SystemsFavoritesEndpoint {
    type Args = ();
    type Output = SystemsResult;
    const NAME: &'static str = "SystemsFavorites";

    fn fetch(
        client: Arc<Client>,
        _args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.systems(favorite_systems_params()).await })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME), Tag::MEDIA_DB]
    }
}

#[cfg(test)]
mod tests {
    use super::{favorite_systems_params, FAVORITE_TAG};

    #[test]
    fn uses_standard_systems_favorite_tag_scope() {
        let params = favorite_systems_params();
        assert_eq!(params.tags, vec![FAVORITE_TAG]);
        assert_eq!(params.all, None);
    }
}
