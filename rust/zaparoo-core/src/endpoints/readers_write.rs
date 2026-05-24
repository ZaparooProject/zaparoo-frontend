// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `ReadersWriteMutation` — ask Core to write ZapScript to the first
// available write-capable reader.

use crate::client::{Client, ClientError};
use crate::media_types::ReadersWriteParams;
use crate::store::Mutation;
use futures_util::future::BoxFuture;
use std::sync::Arc;

#[derive(Debug)]
pub struct ReadersWriteMutation;

impl Mutation for ReadersWriteMutation {
    type Args = ReadersWriteParams;
    type Output = ();

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
        Box::pin(async move { client.readers_write(args).await })
    }
}
