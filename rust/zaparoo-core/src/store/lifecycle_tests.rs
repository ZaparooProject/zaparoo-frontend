#![allow(clippy::expect_used, reason = "bounded in-memory lifecycle fixtures")]

use super::*;
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::{timeout, Duration};

#[derive(Clone)]
struct Args {
    calls: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
    bytes: usize,
    block: bool,
}

impl Args {
    fn new(bytes: usize) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            live: Arc::new(AtomicUsize::new(0)),
            bytes,
            block: false,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl PartialEq for Args {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.calls, &other.calls)
    }
}
impl Eq for Args {}
impl Hash for Args {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.calls).hash(state);
    }
}

struct Live(Arc<AtomicUsize>);
impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Page;
impl Endpoint for Page {
    type Args = Args;
    type Output = String;
    const NAME: &'static str = "LifecyclePage";

    fn fetch(_client: Arc<Client>, args: Args) -> BoxFuture<'static, Result<String, ClientError>> {
        Box::pin(async move {
            let call = args.calls.fetch_add(1, Ordering::SeqCst) + 1;
            args.live.fetch_add(1, Ordering::SeqCst);
            let _live = Live(args.live);
            if args.block {
                std::future::pending::<()>().await;
            }
            Ok(format!("{call}:{}", "x".repeat(args.bytes)))
        })
    }

    fn cache_bytes(output: &String) -> Option<usize> {
        Some(size_of::<String>() + output.capacity())
    }
}

fn store() -> Arc<Store> {
    let runtime = Handle::current();
    let client = Client::waiting(&runtime);
    client.connection.send_replace(ConnectionState::Connected);
    Store::new(client, runtime)
}

async fn ready(rx: &mut ResourceSubscription<String>, call: usize) {
    timeout(Duration::from_secs(5), async {
        loop {
            if matches!(&*rx.borrow_and_update(), ResourceStatus::Ready(value) if value.starts_with(&format!("{call}:"))) {
                return;
            }
            rx.changed().await.expect("resource stays open");
        }
    }).await.expect("Ready before timeout");
}

async fn drain() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

async fn visit(store: &Store, args: &Args) {
    let handle = store.subscribe::<Page>(args.clone());
    let mut rx = handle.subscribe();
    ready(&mut rx, 1).await;
}

#[tokio::test]
async fn receiver_clones_own_lifecycle_independently_of_resource_arcs() {
    let store = store();
    let args = Args::new(32);
    let a = store.subscribe::<Page>(args.clone());
    let b = store.subscribe::<Page>(args.clone());
    assert!(Arc::ptr_eq(&a.resource, &b.resource));
    let resource = Arc::downgrade(&a.resource);
    let mut rx = a.subscribe();
    let clone = rx.clone();
    ready(&mut rx, 1).await;
    drop((a, b, rx));
    assert!(
        resource.upgrade().is_some(),
        "receiver clone retains active subscription"
    );
    drop(clone);
    assert!(
        resource.upgrade().is_none(),
        "cache never retains a live resource"
    );
    let inner = lock(&store.inner);
    let entry = inner
        .cache
        .get(&CacheKey::new::<Page>(&args))
        .expect("recent page");
    assert!(entry.resource.is_none());
    assert!(entry.cached.is_some());
    assert!(entry.refetch.is_none());
}

#[tokio::test]
async fn clean_revisit_is_seeded_without_fetching_or_retaining_a_task() {
    let store = store();
    let args = Args::new(32);
    visit(&store, &args).await;
    drain().await;
    let page = store.subscribe::<Page>(args.clone());
    let rx = page.subscribe();
    assert!(matches!(&*rx.borrow(), ResourceStatus::Ready(_)));
    drain().await;
    assert_eq!(args.calls(), 1);
}

#[tokio::test]
async fn invalidation_refreshes_active_only_and_refetches_stale_revisit() {
    let store = store();
    let inactive = Args::new(32);
    visit(&store, &inactive).await;
    let active = Args::new(32);
    let handle = store.subscribe::<Page>(active.clone());
    let mut rx = handle.subscribe();
    ready(&mut rx, 1).await;
    store.invalidate(&Tag::any(Page::NAME));
    ready(&mut rx, 2).await;
    assert_eq!(inactive.calls(), 1);
    let handle = store.subscribe::<Page>(inactive.clone());
    let mut rx = handle.subscribe();
    assert!(matches!(&*rx.borrow(), ResourceStatus::Loading));
    ready(&mut rx, 2).await;
}

#[tokio::test]
async fn reconnect_leaves_inactive_pages_idle_but_invalidates_their_session() {
    let store = store();
    let inactive = Args::new(32);
    visit(&store, &inactive).await;
    let active = Args::new(32);
    let handle = store.subscribe::<Page>(active.clone());
    let mut rx = handle.subscribe();
    ready(&mut rx, 1).await;
    store
        .client
        .connection
        .send_replace(ConnectionState::Reconnecting);
    drain().await;
    store
        .client
        .connection
        .send_replace(ConnectionState::Connected);
    ready(&mut rx, 2).await;
    assert_eq!(inactive.calls(), 1);
    let mut rx = store.subscribe::<Page>(inactive.clone()).subscribe();
    assert!(matches!(&*rx.borrow(), ResourceStatus::Loading));
    ready(&mut rx, 2).await;
}

#[tokio::test]
async fn dropping_last_consumer_cancels_inflight_fetch_and_removes_unready_slot() {
    let store = store();
    let mut args = Args::new(32);
    args.block = true;
    let handle = store.subscribe::<Page>(args.clone());
    let rx = handle.subscribe();
    drain().await;
    assert_eq!(args.live.load(Ordering::SeqCst), 1);
    drop((handle, rx));
    drain().await;
    assert_eq!(args.live.load(Ordering::SeqCst), 0);
    assert!(!lock(&store.inner)
        .cache
        .contains_key(&CacheKey::new::<Page>(&args)));
    store
        .client
        .connection
        .send_replace(ConnectionState::Connected);
    drain().await;
    assert_eq!(args.calls(), 1);
}

#[tokio::test]
async fn lru_respects_bytes_recency_and_active_consumers() {
    let store = store();
    lock(&store.inner).byte_limit = 4096;
    let a = Args::new(1024);
    let b = Args::new(1024);
    let c = Args::new(1024);
    visit(&store, &a).await;
    visit(&store, &b).await;
    visit(&store, &a).await; // a is now most recent, without refetching.
    visit(&store, &c).await;
    {
        let inner = lock(&store.inner);
        assert!(inner.cache.contains_key(&CacheKey::new::<Page>(&a)));
        assert!(!inner.cache.contains_key(&CacheKey::new::<Page>(&b)));
        assert!(inner.cache.contains_key(&CacheKey::new::<Page>(&c)));
        assert!(inner.cache.values().map(|entry| entry.bytes).sum::<usize>() <= inner.byte_limit);
    }
    let huge = Args::new(8192);
    let handle = store.subscribe::<Page>(huge.clone());
    let mut rx = handle.subscribe();
    ready(&mut rx, 1).await;
    assert!(
        lock(&store.inner)
            .cache
            .contains_key(&CacheKey::new::<Page>(&huge)),
        "active working set stays available"
    );
    drop((handle, rx));
    assert!(
        !lock(&store.inner)
            .cache
            .contains_key(&CacheKey::new::<Page>(&huge)),
        "oversized pages are never retained"
    );
}

#[tokio::test]
async fn entry_limit_also_bounds_tiny_pages() {
    let store = store();
    lock(&store.inner).entry_limit = 3;
    for _ in 0..20 {
        visit(&store, &Args::new(0)).await;
    }
    assert_eq!(lock(&store.inner).cache.len(), 3);
}

#[tokio::test]
async fn dropping_store_cancels_cached_tasks_without_a_reference_cycle() {
    let store = store();
    let weak = Arc::downgrade(&store.inner);
    let args = Args::new(32);
    visit(&store, &args).await;
    drop(store);
    drain().await;
    assert!(weak.upgrade().is_none());
    assert_eq!(args.calls(), 1);
}
