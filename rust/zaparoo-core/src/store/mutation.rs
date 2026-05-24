// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Mutation` — a write-side RPC. Mirrors RTK Query's
// `endpoints.builder.mutation(...)`. The store invokes `run` and, on
// success, refetches every cache entry whose `provides` set intersects
// `invalidates(args, result)`.

use crate::client::{Client, ClientError};
use crate::store::tag::Tag;
use futures_util::future::BoxFuture;
use std::sync::Arc;

pub trait Mutation: 'static {
    /// `Clone` is required so `Store::run_mutation` can hand the args to
    /// both `run` (which consumes them into the `BoxFuture`) and
    /// `invalidates` (which inspects them after a successful run).
    type Args: Clone + Send + 'static;
    type Output: Send + 'static;

    fn run(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>>;

    /// Tags whose cache entries should be refetched after this mutation
    /// succeeds. Default: nothing — invalidation is opt-in per
    /// mutation. The tag list may depend on both the input args and the
    /// server's reply (e.g. a "create" mutation might tag with the new
    /// id from the response).
    fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
        Vec::new()
    }
}
