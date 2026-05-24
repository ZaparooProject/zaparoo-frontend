// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::client::{Client, ClientError};
use crate::media_types::SettingsResult;
use crate::store::{Endpoint, Tag};
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct SettingsEndpoint;

impl Endpoint for SettingsEndpoint {
    type Args = ();
    type Output = SettingsResult;
    const NAME: &'static str = "Settings";

    fn fetch(
        client: Arc<Client>,
        _args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.settings().await })
    }

    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}
