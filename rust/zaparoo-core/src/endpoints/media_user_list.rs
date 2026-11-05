// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaUserListEndpoint`: first page of one built-in list (Favorites,
// Liked, Disliked, Play later), the media tagged with its `user:*` flag.

use crate::client::{Client, ClientError};
use crate::collections::UserList;
use crate::media_types::{MediaSearchParams, MediaSearchResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct UserListArgs {
    pub list: UserList,
    pub max_results: u32,
    pub sort: Option<String>,
    pub systems: Vec<String>,
}

impl UserListArgs {
    #[must_use]
    pub fn new(
        list: UserList,
        max_results: u32,
        sort: Option<String>,
        mut systems: Vec<String>,
    ) -> Self {
        systems.retain(|system| !system.is_empty());
        systems.sort();
        systems.dedup();
        Self {
            list,
            max_results,
            sort,
            systems,
        }
    }
}

#[derive(Debug)]
pub struct MediaUserListEndpoint;

impl Endpoint for MediaUserListEndpoint {
    type Args = UserListArgs;
    type Output = MediaSearchResult;
    const NAME: &'static str = "MediaUserList";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move {
            client
                .media_search(MediaSearchParams {
                    systems: args.systems,
                    max_results: Some(args.max_results),
                    tags: vec![args.list.tag().into()],
                    sort: args.sort,
                    ..MediaSearchParams::default()
                })
                .await
        })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME), Tag::MEDIA_DB]
    }
}

#[cfg(test)]
mod tests {
    use super::UserListArgs;
    use crate::collections::UserList;

    #[test]
    fn cache_identity_includes_list_sort_and_system_scope() {
        let fav = UserList::Favorite;
        let default = UserListArgs::new(fav, 25, None, Vec::new());
        let sorted = UserListArgs::new(fav, 25, Some("name-asc".into()), Vec::new());
        let scoped = UserListArgs::new(fav, 25, None, vec!["SNES".into()]);
        let liked = UserListArgs::new(UserList::Liked, 25, None, Vec::new());
        assert_ne!(default, sorted);
        assert_ne!(default, scoped);
        assert_ne!(default, liked);
        assert_eq!(
            UserListArgs::new(
                fav,
                25,
                None,
                vec!["SNES".into(), "NES".into(), "SNES".into()]
            ),
            UserListArgs::new(fav, 25, None, vec!["NES".into(), "SNES".into()]),
        );
    }
}
