// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `RunMutation` — fire the upstream `run` RPC. Today it invalidates
// nothing, but the wiring is in place for the future
// `NowPlayingEndpoint` (and any other endpoint whose value depends on
// what's currently launched). Adding the dependency later is a one-line
// `invalidates` impl rather than a re-plumbing exercise.

use crate::client::{Client, ClientError};
use crate::media_types::RunParams;
use crate::store::Mutation;
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct RunMutation;

impl Mutation for RunMutation {
    type Args = RunParams;
    type Output = ();

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.run(args).await })
    }
}
