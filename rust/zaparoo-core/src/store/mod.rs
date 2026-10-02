// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Store` is the root of the data layer. It owns the `Client` and
// runtime, hands out shared `RemoteResource`s keyed by (endpoint, args),
// and routes mutations through to the same client. In RTK-Query terms
// this is the `api` slice's reducer + dispatcher. One `Store` per
// frontend process; UI drivers subscribe through it.
//
// Responsibilities: cache `(endpoint NAME, args hash) → RemoteResource`,
// hand back shared subscriptions, route mutations through to the same
// `Client`, and refetch every cache entry whose `provides` set
// intersects a successful mutation's `invalidates` list.

mod catalog_refresh;
mod endpoint;
#[cfg(test)]
mod lifecycle_tests;
mod media_status;
mod mutation;
mod subscription;
mod tag;

pub use endpoint::Endpoint;
pub use media_status::{MediaStatusResource, MediaStatusState};
pub use mutation::Mutation;
pub use subscription::{ResourceHandle, ResourceSubscription};
pub use tag::Tag;

use crate::client::{Client, ClientError, ConnectionState, Notification};
use crate::remote_resource::{RemoteResource, ResourceStatus};
use std::any::Any;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, Weak};
use subscription::Lease;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

// Active consumers own their working set. Only inactive first pages count
// against this RAM-only LRU; no fetch or per-entry watcher survives retirement.
const INACTIVE_BYTES: usize = 8 * 1024 * 1024;
const INACTIVE_ENTRIES: usize = 64;
// Covers the fixed entry, channel marker, box and hash-table bookkeeping.
const ENTRY_OVERHEAD: usize = 512;

/// Cache lookup key for `Store::subscribe`. Combines the endpoint's
/// `NAME` with a hash of its `Args`. Endpoints are expected to choose
/// unique names, so cross-endpoint collisions are a programmer error;
/// within an endpoint a 64-bit `Args` hash collision is astronomically
/// unlikely for the cardinalities this frontend sees (one catalog,
/// tens of system ids).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct CacheKey {
    name: &'static str,
    args_hash: u64,
}

impl CacheKey {
    fn new<E: Endpoint>(args: &E::Args) -> Self {
        let mut hasher = DefaultHasher::new();
        args.hash(&mut hasher);
        Self {
            name: E::NAME,
            args_hash: hasher.finish(),
        }
    }
}

/// Active resources are shared only by explicit consumer leases. Inactive
/// entries contain a measured Ready payload, never a live resource/task.
struct CacheEntry {
    id: u64,
    resource: Option<Arc<dyn Any + Send + Sync>>,
    lease: Weak<Lease>,
    provides: Vec<Tag>,
    refetch: Option<Arc<dyn Fn() + Send + Sync>>,
    cached: Option<Box<dyn Any + Send + Sync>>,
    connection: watch::Receiver<ConnectionState>,
    stale: bool,
    revision: u64,
    bytes: usize,
    used: u64,
}

impl CacheEntry {
    fn active<E: Endpoint>(&self) -> Option<ResourceHandle<E::Output>> {
        let resource = self
            .resource
            .as_ref()?
            .clone()
            .downcast::<RemoteResource<E::Output>>();
        let Ok(resource) = resource else {
            tracing::error!(
                name = E::NAME,
                "endpoint NAME collision: different Output types"
            );
            debug_assert!(false, "endpoint NAME collision: {}", E::NAME);
            return None;
        };
        let refetch = self.refetch.clone()?;
        let lease = self.lease.upgrade()?;
        Some(ResourceHandle {
            resource,
            lease,
            refetch,
        })
    }
}

async fn fetch_tracked<E: Endpoint>(
    client: Arc<Client>,
    args: E::Args,
    inner: Weak<Mutex<Inner>>,
    key: CacheKey,
    id: u64,
    marker: Arc<Mutex<watch::Receiver<ConnectionState>>>,
) -> Result<E::Output, ClientError> {
    marker
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .borrow_and_update();
    let revision = inner
        .upgrade()
        .and_then(|inner| lock(&inner).cache.get(&key).map(|entry| entry.revision));
    let output = E::fetch(client, args.clone()).await?;
    if let Some(inner) = inner.upgrade() {
        if let Some(entry) = lock(&inner)
            .cache
            .get_mut(&key)
            .filter(|entry| entry.id == id && entry.resource.is_some())
        {
            entry.provides = E::provides(&args, &output);
            if Some(entry.revision) == revision {
                entry.stale = false;
            }
        }
    }
    Ok(output)
}

struct Inner {
    cache: HashMap<CacheKey, CacheEntry>,
    clock: u64,
    byte_limit: usize,
    entry_limit: usize,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            clock: 0,
            byte_limit: INACTIVE_BYTES,
            entry_limit: INACTIVE_ENTRIES,
        }
    }
}

impl Inner {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn trim(&mut self) {
        loop {
            let inactive = || self.cache.values().filter(|e| e.resource.is_none());
            let bytes = inactive().fold(0usize, |sum, e| sum.saturating_add(e.bytes));
            if bytes <= self.byte_limit && inactive().count() <= self.entry_limit {
                break;
            }
            let oldest = self
                .cache
                .iter()
                .filter(|(_, e)| e.resource.is_none())
                .min_by_key(|(_, e)| e.used)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else { break };
            self.cache.remove(&key);
        }
        if self.cache.capacity() > self.cache.len().saturating_mul(4).max(INACTIVE_ENTRIES) {
            self.cache.shrink_to_fit();
        }
    }
}

#[allow(
    clippy::unwrap_used,
    reason = "poisoning means another thread panicked while mutating the store"
)]
fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap()
}

/// Runs exactly once when the last consumer (including cloned receivers) goes
/// away. The lease owns the resource until this callback returns, so eviction
/// also cancels an in-flight request rather than merely forgetting its key.
fn retire<E: Endpoint>(
    inner: &Weak<Mutex<Inner>>,
    key: &CacheKey,
    id: u64,
    resource: &RemoteResource<E::Output>,
    marker: &Mutex<watch::Receiver<ConnectionState>>,
) {
    let Some(inner) = inner.upgrade() else { return };
    let rx = resource.subscribe();
    let status = rx.borrow();
    let mut inner = lock(&inner);
    let used = inner.tick();
    let limit = inner.byte_limit;
    let Some(entry) = inner.cache.get_mut(key).filter(|e| e.id == id) else {
        return;
    };
    let retained = match &*status {
        ResourceStatus::Ready(value) if !entry.stale => E::cache_bytes(value)
            .map(|bytes| {
                bytes
                    .saturating_add(ENTRY_OVERHEAD)
                    .saturating_add(entry.provides.capacity() * size_of::<Tag>())
                    .saturating_add(
                        entry
                            .provides
                            .iter()
                            .map(|tag| tag.id.as_ref().map_or(0, String::capacity))
                            .sum::<usize>(),
                    )
            })
            .filter(|&bytes| bytes <= limit)
            .map(|bytes| (bytes, Box::new(value.clone()) as Box<dyn Any + Send + Sync>)),
        _ => None,
    };
    if let Some((bytes, cached)) = retained {
        entry.resource = None;
        entry.refetch = None;
        entry.cached = Some(cached);
        entry.connection = marker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        entry.bytes = bytes;
        entry.used = used;
    } else {
        inner.cache.remove(key);
    }
    inner.trim();
}

pub struct Store {
    client: Arc<Client>,
    runtime: Handle,
    inner: Arc<Mutex<Inner>>,
    /// Singleton media-status publisher. Eagerly constructed on
    /// `Store::new` so the seeding task starts as soon as the frontend
    /// has a `Client`, even if no UI driver has subscribed yet.
    media_status: Arc<MediaStatusResource>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    pub fn new(client: Arc<Client>, runtime: Handle) -> Arc<Self> {
        let media_status = MediaStatusResource::new(&client, &runtime);
        let store = Arc::new(Self {
            client,
            runtime,
            inner: Arc::new(Mutex::new(Inner::default())),
            media_status: media_status.clone(),
        });
        Self::spawn_media_db_invalidation_watcher(&store, &media_status);
        Self::spawn_catalog_refresh_watcher(&store, &media_status);
        store
    }

    /// Incremental catalog reads run off the UI thread and keep the last good
    /// value if Core is temporarily busy. The resource serializes/coalesces
    /// requests, so a slow query is never cancelled by the next timer tick.
    fn spawn_catalog_refresh_watcher(store: &Arc<Self>, media_status: &Arc<MediaStatusResource>) {
        let weak = Arc::downgrade(store);
        let rx = media_status.subscribe();
        store.runtime.spawn(catalog_refresh::run(
            rx,
            catalog_refresh::PERIOD,
            move || {
                let Some(store) = weak.upgrade() else {
                    return false;
                };
                store
                    .subscribe::<crate::endpoints::catalog::CatalogEndpoint>(())
                    .refresh_in_background();
                true
            },
        ));
    }

    /// Watches the live `MediaStatusResource` for the busy → idle edge
    /// (indexing, optimizing or a metadata import finished) and pulses
    /// `Tag::MEDIA_DB`. Cache entries whose `provides` set contains that
    /// tag — the systems catalog and the game lists — are refetched in
    /// place, so the UI picks up what the run changed.
    ///
    /// Held weakly so the watcher exits when the store is dropped at
    /// process teardown without forming a cycle that pins `Inner`.
    fn spawn_media_db_invalidation_watcher(
        store: &Arc<Self>,
        media_status: &Arc<MediaStatusResource>,
    ) {
        let weak = Arc::downgrade(store);
        let mut rx = media_status.subscribe();
        store.runtime.spawn(async move {
            let mut prev = rx.borrow_and_update().clone();
            while rx.changed().await.is_ok() {
                let curr = rx.borrow_and_update().clone();
                let fire = is_media_db_completion_edge(&prev, &curr);
                prev = curr;
                if !fire {
                    continue;
                }
                let Some(store) = weak.upgrade() else {
                    return;
                };
                store.invalidate(&Tag::MEDIA_DB);
            }
        });
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<Notification> {
        self.client.subscribe_notifications()
    }

    /// Shared `MediaStatusResource`. Singleton: every caller uses
    /// the same publisher, so subscribing late still observes the
    /// current state through the underlying watch channel.
    pub fn media_status(&self) -> Arc<MediaStatusResource> {
        self.media_status.clone()
    }

    /// Direct access to the underlying `Client` for callers that need
    /// one-shot, cursor-driven calls that don't fit the cached
    /// `subscribe::<E>` pattern. The store does not cache the result,
    /// which is the whole point of the bypass, so callers must accept
    /// that mutation invalidation does not reach this call. Used by the
    /// Games driver's `fetch_more` to advance pagination cursors that
    /// would otherwise pollute the endpoint cache key with one entry
    /// per page.
    pub fn client(&self) -> Arc<Client> {
        self.client.clone()
    }

    /// Share one active fetch, or reopen a clean recent first page. Receiver
    /// clones carry the lease too; the Store's own Arcs never count as users.
    pub fn subscribe<E: Endpoint>(&self, args: E::Args) -> ResourceHandle<E::Output> {
        let key = CacheKey::new::<E>(&args);
        let mut inner = lock(&self.inner);
        if let Some(handle) = inner.cache.get(&key).and_then(CacheEntry::active::<E>) {
            return handle;
        }
        let previous = inner.cache.remove(&key);
        let mut provides = Vec::new();
        let cached = previous.and_then(|mut entry| {
            if entry.stale || entry.connection.has_changed().unwrap_or(true) {
                return None;
            }
            provides = entry.provides;
            entry
                .cached
                .take()?
                .downcast::<E::Output>()
                .ok()
                .map(|value| (*value, entry.connection))
        });
        let id = inner.tick();
        let marker = Arc::new(Mutex::new(self.client.connection.subscribe()));
        let marker_for_fetch = marker.clone();
        let inner_weak = Arc::downgrade(&self.inner);
        let inner_for_fetch = inner_weak.clone();
        let key_for_fetch = key.clone();
        let resource = Arc::new(RemoteResource::driven_by_cached(
            self.client.clone(),
            &self.runtime,
            move |client| {
                fetch_tracked::<E>(
                    client,
                    args.clone(),
                    inner_for_fetch.clone(),
                    key_for_fetch.clone(),
                    id,
                    marker_for_fetch.clone(),
                )
            },
            cached,
        ));
        let resource_for_refetch = resource.clone();
        let inner_for_refetch = inner_weak.clone();
        let key_for_refetch = key.clone();
        let refetch: Arc<dyn Fn() + Send + Sync> =
            Arc::new(move || {
                if let Some(inner) = inner_for_refetch.upgrade() {
                    let mut inner = lock(&inner);
                    let Some(entry) = inner.cache.get_mut(&key_for_refetch).filter(|e| {
                        e.id == id && e.resource.is_some() && e.lease.strong_count() > 0
                    }) else {
                        return;
                    };
                    entry.stale = true;
                    entry.revision += 1;
                }
                resource_for_refetch.refetch();
            });
        let resource_for_lease = resource.clone();
        let key_for_lease = key.clone();
        let lease = Arc::new(Lease(Box::new(move || {
            retire::<E>(
                &inner_weak,
                &key_for_lease,
                id,
                &resource_for_lease,
                &marker,
            );
        })));
        inner.cache.insert(
            key,
            CacheEntry {
                id,
                resource: Some(resource.clone()),
                lease: Arc::downgrade(&lease),
                provides,
                refetch: Some(refetch.clone()),
                cached: None,
                connection: self.client.connection.subscribe(),
                stale: false,
                revision: 0,
                bytes: 0,
                used: id,
            },
        );
        ResourceHandle {
            resource,
            lease,
            refetch,
        }
    }

    /// Invoke a mutation. On success, every cache entry whose
    /// `provides` set matches any of `M::invalidates(args, result)` is
    /// refetched in place — the underlying `RemoteResource` keeps the
    /// same `Arc`, so existing subscribers see the new value through
    /// their existing watch channel without re-binding.
    pub async fn run_mutation<M: Mutation>(&self, args: M::Args) -> Result<M::Output, ClientError> {
        let args_for_invalidate = args.clone();
        let result = M::run(self.client.clone(), args).await?;
        for tag in M::invalidates(&args_for_invalidate, &result) {
            self.invalidate(&tag);
        }
        Ok(result)
    }

    /// Active matches refresh; inactive matches become stale without spawning
    /// work. Their next subscriber fetches a fresh page before publishing it.
    pub fn invalidate(&self, tag: &Tag) {
        let to_refetch: Vec<_> = {
            let mut inner = lock(&self.inner);
            inner
                .cache
                .values_mut()
                .filter(|entry| entry.provides.iter().any(|p| tags_match(p, tag)))
                .filter_map(|entry| {
                    entry.stale = true;
                    entry.revision += 1;
                    entry.refetch.clone()
                })
                .collect()
        };
        for refetch in to_refetch {
            refetch();
        }
    }
}

/// True when `curr` represents the busy → idle edge of a run that
/// rewrites the media DB: indexing, optimizing or a metadata import.
/// Core stays in the optimizing phase after the file scan ends and
/// before the DB is queryable, and an import can follow an index
/// directly, so the edge fires only once the whole pipeline has wound
/// down. Pulled out of the watcher so the edge logic is unit-testable
/// without driving a runtime; frontends use it for their own caches.
pub fn is_media_db_completion_edge(prev: &MediaStatusState, curr: &MediaStatusState) -> bool {
    let prev_busy = prev.indexing || prev.optimizing || prev.scraping;
    let curr_busy = curr.indexing || curr.optimizing || curr.scraping;
    prev_busy && !curr_busy
}

/// RTK-Query tag matching. Two tags match iff their kinds agree and at
/// least one side has a `None` id (the "any" wildcard) or both sides
/// share the same specific id. Used for both directions:
/// `provided.matches(invalidating)` and the reverse have the same
/// truth table, so we don't need to track which is which here.
fn tags_match(a: &Tag, b: &Tag) -> bool {
    if a.kind != b.kind {
        return false;
    }
    match (&a.id, &b.id) {
        (None, _) | (_, None) => true,
        (Some(left), Some(right)) => left == right,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests should fail-fast on setup errors")]
mod tests {
    use super::*;
    use futures_util::future::BoxFuture;

    /// Endpoint stand-in that doesn't touch the `Client`. Used by the
    /// cache-key and subscribe tests below where end-to-end fetch
    /// behavior is out of scope; real endpoints have their own
    /// integration tests.
    struct DummyEndpoint;
    impl Endpoint for DummyEndpoint {
        type Args = String;
        type Output = i32;
        const NAME: &'static str = "Dummy";

        fn fetch(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(0) })
        }
    }

    struct OtherEndpoint;
    impl Endpoint for OtherEndpoint {
        type Args = String;
        type Output = i32;
        const NAME: &'static str = "Other";

        fn fetch(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(0) })
        }
    }

    struct UnitEndpoint;
    impl Endpoint for UnitEndpoint {
        type Args = ();
        type Output = i32;
        const NAME: &'static str = "Unit";

        fn fetch(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(0) })
        }
    }

    #[test]
    fn cache_key_equal_for_equal_args() {
        let a = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let b = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn cache_key_differs_for_different_args() {
        let a = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let b = CacheKey::new::<DummyEndpoint>(&"beta".to_string());
        assert_ne!(a, b);
    }

    #[test]
    fn cache_key_differs_for_different_endpoint_names() {
        let a = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let b = CacheKey::new::<OtherEndpoint>(&"alpha".to_string());
        // Same args produce the same hash — the NAME is the
        // distinguishing field.
        assert_eq!(a.args_hash, b.args_hash);
        assert_ne!(a, b);
    }

    #[test]
    fn unit_args_collapse_to_a_single_key() {
        let a = CacheKey::new::<UnitEndpoint>(&());
        let b = CacheKey::new::<UnitEndpoint>(&());
        assert_eq!(a, b);
    }

    fn test_store() -> Arc<Store> {
        // Leak the test runtime so the Handles stored in Client/Store
        // remain valid for the test's lifetime — Handle does not keep
        // the Runtime alive on its own.
        let runtime: &'static tokio::runtime::Runtime = Box::leak(Box::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build runtime"),
        ));
        // The Client spawns a reconnect task against this endpoint; the
        // test never lets the task connect, so the URL just needs to
        // parse. `subscribe` itself doesn't await anything network-y.
        let client = Client::new("ws://127.0.0.1:1/never".to_string(), runtime.handle());
        Store::new(client, runtime.handle().clone())
    }

    #[test]
    fn subscribe_with_equal_args_returns_same_arc() {
        let store = test_store();
        let a = store.subscribe::<DummyEndpoint>("alpha".to_string());
        let b = store.subscribe::<DummyEndpoint>("alpha".to_string());
        // Pointer equality — the cache must hand back the same
        // resource Arc, not a fresh `RemoteResource` per call.
        assert!(Arc::ptr_eq(&a.resource, &b.resource));
    }

    #[test]
    fn subscribe_with_different_args_returns_different_arcs() {
        let store = test_store();
        let a = store.subscribe::<DummyEndpoint>("alpha".to_string());
        let b = store.subscribe::<DummyEndpoint>("beta".to_string());
        assert!(!Arc::ptr_eq(&a.resource, &b.resource));

        // Each args value should occupy its own cache slot — two
        // entries with the expected keys means the cache really did
        // distinguish them, not just hand back fresh Arcs from one
        // shared slot.
        let cache = &store.inner.lock().expect("lock store inner").cache;
        assert_eq!(cache.len(), 2);
        let key_a = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let key_b = CacheKey::new::<DummyEndpoint>(&"beta".to_string());
        assert!(cache.contains_key(&key_a));
        assert!(cache.contains_key(&key_b));

        // Independent watch channels: each resource has its own
        // status sender, so subscribing to one yields a receiver
        // that is not aliased to the other's sender. Both seed to
        // Idle (the connection is never established by the test
        // client).
        let mut rx_a = a.subscribe();
        let mut rx_b = b.subscribe();
        assert!(matches!(*rx_a.borrow_and_update(), ResourceStatus::Idle));
        assert!(matches!(*rx_b.borrow_and_update(), ResourceStatus::Idle));
    }

    // Media-DB completion edge detection. The watcher in
    // `spawn_media_db_invalidation_watcher` calls `Store::invalidate`
    // exactly once per busy → idle transition; these tests pin down
    // the truth table of the underlying helper.

    fn busy() -> MediaStatusState {
        MediaStatusState {
            indexing: true,
            ..MediaStatusState::default()
        }
    }

    fn optimizing() -> MediaStatusState {
        MediaStatusState {
            optimizing: true,
            ..MediaStatusState::default()
        }
    }

    fn idle() -> MediaStatusState {
        MediaStatusState::default()
    }

    #[test]
    fn completion_edge_fires_when_indexing_finishes() {
        assert!(is_media_db_completion_edge(&busy(), &idle()));
    }

    #[test]
    fn completion_edge_fires_when_optimizing_finishes() {
        // Core's pipeline can land in optimizing without first flipping
        // indexing on for our subscriber (e.g. a partial-resume path);
        // the helper still treats the transition out of optimizing as
        // a completion.
        assert!(is_media_db_completion_edge(&optimizing(), &idle()));
    }

    #[test]
    fn completion_edge_skips_indexing_to_optimizing_handoff() {
        // The DB isn't queryable yet — we wait for the optimizing phase
        // to drain before refetching.
        let mut prev = idle();
        prev.indexing = true;
        let mut curr = idle();
        curr.optimizing = true;
        assert!(!is_media_db_completion_edge(&prev, &curr));
    }

    #[test]
    fn completion_edge_fires_when_a_metadata_import_finishes() {
        let scraping = MediaStatusState {
            scraping: true,
            ..idle()
        };
        assert!(is_media_db_completion_edge(&scraping, &idle()));
        // An index handing straight over to an import has not finished.
        assert!(!is_media_db_completion_edge(&busy(), &scraping));
    }

    #[test]
    fn completion_edge_skips_idle_to_busy_transitions() {
        assert!(!is_media_db_completion_edge(&idle(), &busy()));
        assert!(!is_media_db_completion_edge(&idle(), &optimizing()));
    }

    #[test]
    fn completion_edge_skips_idle_to_idle() {
        // Notifications can repeat the same idle snapshot when only
        // metadata fields change (totals, step display); no spurious
        // invalidation should fire on those.
        assert!(!is_media_db_completion_edge(&idle(), &idle()));
    }

    // RTK-Query tag matching parity. See `tags_match` in this module.

    #[test]
    fn any_matches_any_of_same_kind() {
        assert!(tags_match(&Tag::any("X"), &Tag::any("X")));
    }

    #[test]
    fn any_matches_specific_of_same_kind() {
        // A mutation invalidating Tag::any("X") refetches both entries
        // tagged Tag::any("X") and Tag::specific("X", id) — the broad
        // tag invalidates everything in the namespace.
        assert!(tags_match(&Tag::any("X"), &Tag::specific("X", "a")));
        assert!(tags_match(&Tag::specific("X", "a"), &Tag::any("X")));
    }

    #[test]
    fn specific_matches_specific_only_for_same_id() {
        assert!(tags_match(
            &Tag::specific("X", "a"),
            &Tag::specific("X", "a"),
        ));
        assert!(!tags_match(
            &Tag::specific("X", "a"),
            &Tag::specific("X", "b"),
        ));
    }

    #[test]
    fn cross_kind_never_matches() {
        assert!(!tags_match(&Tag::any("X"), &Tag::any("Y")));
        assert!(!tags_match(
            &Tag::specific("X", "a"),
            &Tag::specific("Y", "a"),
        ));
        assert!(!tags_match(&Tag::any("X"), &Tag::specific("Y", "a")));
    }

    // run_mutation / invalidate end-to-end behavior. These bypass
    // `Store::subscribe`'s connection-driven resource lifecycle by
    // populating cache entries directly, so the tests stay deterministic
    // regardless of the `Client`'s reconnect task.

    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    fn install_entry_with_provides(
        store: &Arc<Store>,
        key: CacheKey,
        provides: Vec<Tag>,
    ) -> Arc<AtomicUsize> {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_closure = counter.clone();
        let refetch: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter_for_closure.fetch_add(1, AtomicOrdering::SeqCst);
        });
        let dummy_resource: Arc<dyn Any + Send + Sync> = Arc::new(());
        let entry = CacheEntry {
            id: 0,
            resource: Some(dummy_resource),
            lease: Weak::new(),
            provides,
            refetch: Some(refetch),
            cached: None,
            connection: store.client.connection.subscribe(),
            stale: false,
            revision: 0,
            bytes: 0,
            used: 0,
        };
        store
            .inner
            .lock()
            .expect("lock store inner")
            .cache
            .insert(key, entry);
        counter
    }

    /// Mutation whose `invalidates` matches `Tag::any("Dummy")` — used to
    /// drive the refetch path under test.
    struct InvalidatingMutation;
    impl Mutation for InvalidatingMutation {
        type Args = ();
        type Output = ();
        fn run(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(()) })
        }
        fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
            vec![Tag::any("Dummy")]
        }
    }

    /// Mutation with no invalidates — the no-op default. Confirms that
    /// `run_mutation` does not refetch entries when nothing is asked.
    struct InertMutation;
    impl Mutation for InertMutation {
        type Args = ();
        type Output = ();
        fn run(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// Mutation whose `invalidates` targets one specific id rather than
    /// the whole kind. Used to drive the discriminating-match path.
    struct SpecificInvalidatingMutation;
    impl Mutation for SpecificInvalidatingMutation {
        type Args = ();
        type Output = ();
        fn run(
            _client: Arc<Client>,
            _args: Self::Args,
        ) -> BoxFuture<'static, Result<Self::Output, ClientError>> {
            Box::pin(async { Ok(()) })
        }
        fn invalidates(_args: &Self::Args, _result: &Self::Output) -> Vec<Tag> {
            vec![Tag::specific("Dummy", "id1")]
        }
    }

    #[test]
    fn run_mutation_refetches_entry_with_matching_provides() {
        let store = test_store();
        let key = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let counter = install_entry_with_provides(&store, key, vec![Tag::any("Dummy")]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<InvalidatingMutation>(())
                .await
                .expect("mutation runs");
        });

        assert_eq!(counter.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn run_mutation_skips_entry_with_unrelated_provides() {
        let store = test_store();
        let key = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        // Provides a tag from a different kind — `InvalidatingMutation`
        // invalidates `Tag::any("Dummy")` so this entry must not match.
        let counter = install_entry_with_provides(&store, key, vec![Tag::any("Other")]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<InvalidatingMutation>(())
                .await
                .expect("mutation runs");
        });

        assert_eq!(counter.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn run_mutation_succeeds_when_provides_not_yet_populated() {
        let store = test_store();
        let key = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        // Empty provides simulates a cache entry whose resource has not
        // yet emitted `Ready`. The mutation still runs, and the entry
        // is correctly skipped because no provides match.
        let counter = install_entry_with_provides(&store, key, Vec::new());

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<InvalidatingMutation>(())
                .await
                .expect("mutation runs");
        });

        assert_eq!(counter.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn run_mutation_refetches_every_matching_entry_at_once() {
        // Per-entry matching is unit-tested above; this asserts the
        // dispatch loop in `Store::invalidate` invokes *every* matching
        // entry's `refetch` closure (across endpoints) for a single
        // mutation, and leaves non-matching entries alone. The closure
        // is the test fixture's counter bump, not the real
        // `RemoteResource::refetch` notify pulse — covered separately.
        let store = test_store();

        let key_match_any = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let key_match_specific = CacheKey::new::<OtherEndpoint>(&"alpha".to_string());
        let key_unrelated = CacheKey::new::<DummyEndpoint>(&"beta".to_string());

        let counter_match_any =
            install_entry_with_provides(&store, key_match_any, vec![Tag::any("Dummy")]);
        let counter_match_specific = install_entry_with_provides(
            &store,
            key_match_specific,
            vec![Tag::specific("Dummy", "id1")],
        );
        let counter_unrelated =
            install_entry_with_provides(&store, key_unrelated, vec![Tag::any("Other")]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<InvalidatingMutation>(())
                .await
                .expect("mutation runs");
        });

        // `InvalidatingMutation::invalidates` returns `Tag::any("Dummy")`,
        // which matches both `Tag::any("Dummy")` and
        // `Tag::specific("Dummy", _)` via the wildcard rule in
        // `tags_match`.
        assert_eq!(counter_match_any.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(counter_match_specific.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(counter_unrelated.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn specific_mutation_refetches_only_same_id_and_any_of_kind() {
        // Companion to `run_mutation_refetches_every_matching_entry_at_once`:
        // exercises the discriminating-match side of `tags_match` through
        // the dispatch loop. A `Tag::specific("Dummy","id1")` mutation
        // should hit the same-id entry and any `Tag::any("Dummy")` entry,
        // but skip a sibling `Tag::specific("Dummy","id2")` entry.
        let store = test_store();

        let key_same_id = CacheKey::new::<DummyEndpoint>(&"id1".to_string());
        let key_other_id = CacheKey::new::<DummyEndpoint>(&"id2".to_string());
        let key_any_of_kind = CacheKey::new::<OtherEndpoint>(&"alpha".to_string());

        let counter_same_id =
            install_entry_with_provides(&store, key_same_id, vec![Tag::specific("Dummy", "id1")]);
        let counter_other_id =
            install_entry_with_provides(&store, key_other_id, vec![Tag::specific("Dummy", "id2")]);
        let counter_any_of_kind =
            install_entry_with_provides(&store, key_any_of_kind, vec![Tag::any("Dummy")]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<SpecificInvalidatingMutation>(())
                .await
                .expect("mutation runs");
        });

        assert_eq!(counter_same_id.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(counter_any_of_kind.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(counter_other_id.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn run_mutation_with_no_invalidates_never_refetches() {
        let store = test_store();
        let key = CacheKey::new::<DummyEndpoint>(&"alpha".to_string());
        let counter = install_entry_with_provides(&store, key, vec![Tag::any("Dummy")]);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        runtime.block_on(async {
            store
                .run_mutation::<InertMutation>(())
                .await
                .expect("mutation runs");
        });

        assert_eq!(counter.load(AtomicOrdering::SeqCst), 0);
    }
}
