// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `SystemsUserListEndpoint`: standard `systems` scoped to one built-in
// list's media, for a list grouped by system.

use crate::client::{Client, ClientError};
use crate::collections::UserList;
use crate::media_types::{SystemsParams, SystemsResult};
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct SystemsUserListEndpoint;

fn list_systems_params(list: UserList) -> SystemsParams {
    SystemsParams {
        tags: vec![list.tag().into()],
        ..SystemsParams::default()
    }
}

impl Endpoint for SystemsUserListEndpoint {
    type Args = UserList;
    type Output = SystemsResult;
    const NAME: &'static str = "SystemsUserList";

    fn cache_bytes(output: &Self::Output) -> Option<usize> {
        Some(super::retained::bytes(output))
    }

    fn fetch(
        client: Arc<Client>,
        list: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.systems(list_systems_params(list)).await })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME), Tag::MEDIA_DB]
    }
}

#[cfg(test)]
mod tests {
    use super::list_systems_params;
    use crate::collections::UserList;

    #[test]
    fn scopes_standard_systems_to_the_list_tag() {
        let params = list_systems_params(UserList::PlayLater);
        assert_eq!(params.tags, vec!["user:playlater"]);
        assert_eq!(params.all, None);
    }
}
