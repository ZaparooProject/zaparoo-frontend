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
    pub sort: Option<String>,
    pub systems: Vec<String>,
}

impl FavoritesArgs {
    #[must_use]
    pub fn new(max_results: u32, sort: Option<String>, mut systems: Vec<String>) -> Self {
        systems.retain(|system| !system.is_empty());
        systems.sort();
        systems.dedup();
        Self {
            max_results,
            sort,
            systems,
        }
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
                    systems: args.systems,
                    max_results: Some(args.max_results),
                    tags: vec!["user:favorite".into()],
                    sort: args.sort,
                    ..MediaSearchParams::default()
                })
                .await
        })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}

#[cfg(test)]
mod tests {
    use super::FavoritesArgs;

    #[test]
    fn cache_identity_includes_sort_and_system_scope() {
        let default = FavoritesArgs::new(25, None, Vec::new());
        let sorted = FavoritesArgs::new(25, Some("name-asc".into()), Vec::new());
        let scoped = FavoritesArgs::new(25, None, vec!["SNES".into()]);
        assert_ne!(default, sorted);
        assert_ne!(default, scoped);
        assert_eq!(
            FavoritesArgs::new(25, None, vec!["SNES".into(), "NES".into(), "SNES".into()]),
            FavoritesArgs::new(25, None, vec!["NES".into(), "SNES".into()]),
        );
    }
}
