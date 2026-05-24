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

    /// Tags this endpoint's data provides. The default — a single
    /// `Tag::any(NAME)` — means any mutation invalidating
    /// `Tag::any(NAME)` *or* `Tag::specific(NAME, _)` will refetch this
    /// entry. Override for finer-grained tags (e.g. per-system search
    /// results that should only invalidate when *that* system's data
    /// changes).
    ///
    /// The store's per-entry watcher recomputes `provides` only on
    /// transitions through `Ready`, and `tokio::sync::watch` is
    /// intentionally lossy: a rapid `Ready → Loading → Ready` sequence
    /// can collapse into a single watcher wake-up that observes the
    /// later state. Today every implementation derives `provides`
    /// purely from `args`, so the value is invariant across successive
    /// fetches and the lossiness is harmless. If you derive provides
    /// from `output` (e.g. a server-issued id), the tags must be
    /// stable across fetches with the same args — otherwise an
    /// invalidation matched against stale provides will miss.
    fn provides(_args: &Self::Args, _output: &Self::Output) -> Vec<Tag> {
        vec![Tag::any(Self::NAME)]
    }
}
