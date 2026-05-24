// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::client::{Client, ClientError};
use crate::media_types::LaunchersResult;
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct LaunchersEndpoint;

impl Endpoint for LaunchersEndpoint {
    type Args = ();
    type Output = LaunchersResult;
    const NAME: &'static str = "Launchers";

    fn fetch(
        client: Arc<Client>,
        _args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.launchers().await })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}
