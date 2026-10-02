// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Endpoint` — a typed, cacheable RPC. Implementing types are unit
// structs (e.g. `pub struct CatalogEndpoint;`) used as type-level keys;
// `fetch` is dispatched statically through `Store::subscribe::<E>`.
//
// The trait surface mirrors RTK Query's `endpoints.builder.query(...)`:
// `NAME` is the cache namespace and the default tag kind, `Args` is the
// cache key, and `provides()` declares which tags the resulting data
// carries for invalidation matching.

use crate::client::{Client, ClientError};
use crate::store::tag::Tag;
use futures_util::future::BoxFuture;
use std::hash::Hash;
use std::sync::Arc;

pub trait Endpoint: 'static {
    /// Cache key for this endpoint. Two subscribers with equal `Args`
    /// share a `RemoteResource`. `()` is the right choice for endpoints
    /// whose data is global to the connection (e.g. the systems
    /// catalog); use a richer type when the fetch parameters vary.
    type Args: Clone + Eq + Hash + Send + Sync + 'static;

    /// The endpoint's deserialized payload, exactly as the UI consumes
    /// it. Must be `Clone` because every status-watch update sends a
    /// fresh `ResourceStatus<Output>`.
    type Output: Clone + Send + Sync + 'static;

    /// Stable identifier used in cache keys and as the default tag
    /// kind. Two endpoints with the same `NAME` share a cache namespace
    /// and become indistinguishable to the invalidation matcher; pick a
    /// unique string per endpoint.
    const NAME: &'static str;

    fn fetch(
        client: Arc<Client>,
        args: Self::Args,
    ) -> BoxFuture<'static, Result<Self::Output, ClientError>>;

    /// Retained allocation size of a Ready payload, including nested strings
    /// and vector capacity. Opt out unless every heap-owning field is counted;
    /// unmeasured outputs are released with their last consumer. Store adds
    /// entry/tag overhead and enforces its inactive-byte and entry limits.
    fn cache_bytes(_output: &Self::Output) -> Option<usize> {
        None
    }

    /// Tags this endpoint's data provides. The default — a single
    /// `Tag::any(NAME)` — means any mutation invalidating
    /// `Tag::any(NAME)` *or* `Tag::specific(NAME, _)` will refetch this
    /// entry. Override for finer-grained tags (e.g. per-system search
    /// results that should only invalidate when *that* system's data
    /// changes).
    ///
    /// Recomputed from every successful fetch before Ready is published, not
    /// through a lossy status watcher. Inactive cached pages keep these tags
    /// so invalidation can mark them stale without waking old folders.
    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}
