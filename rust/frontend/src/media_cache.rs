// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The in-process cover cache. Its load-bearing rules:
// - memory only, never disk: Core is the canonical store and the
//   frontend re-fetches after a cold start;
// - strict bytes cap with LRU eviction (MiSTer shares <512 MB with
//   Core, the wrapper, and the active core);
// - one fetch driver with at most `PARALLEL_FETCHES` `media.image` RPCs
//   in flight, so a page fill cannot flood Core;
// - the queue is most recent first and deduplicated: what the user is
//   looking at now is fetched before what scrolled past, and a browse
//   view can drop queued covers it no longer shows;
// - negative results memoized for the process lifetime.
//
// Images are stored decoded (RGBA8) because the LRU cap must account
// for what actually occupies RAM; `slint::Image` construction happens
// on the event loop thread at apply time.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::sync::Notify;
use zaparoo_core::client::Client;
use zaparoo_core::media_types::MediaImageParams;

/// Hard ceiling on the decoded bytes the cache holds.
const CACHE_CAP_BYTES: usize = 128 * 1024 * 1024;
const NEGATIVE_CAP: usize = 4096;

/// Cache key: `media_id` when Core provided one, otherwise the canonical
/// `(system, path)` pair, with the requested bounding-box size baked in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaKey {
    pub media_id: Option<i64>,
    pub system: String,
    pub path: String,
    pub max_size: u32,
    /// Explicit carousel slot; None retains the browse artwork preference ladder.
    pub image_type: Option<String>,
}

/// Decoded pixels stored directly as a Slint pixel buffer: it is
/// refcounted and Send, so the one unavoidable memcpy happens here on
/// the fetch-driver thread and every UI-side `Image` wrap is a cheap
/// refcount clone - patching a cover mid-animation costs no copy on
/// the render thread.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub buffer: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
}

impl DecodedImage {
    fn byte_size(&self) -> usize {
        self.buffer.width() as usize * self.buffer.height() as usize * 4
    }
}

#[derive(Debug)]
struct CachedImage {
    image: DecodedImage,
    /// Recency stamp: the cache's tick at the last read or insert.
    used: u64,
}

#[derive(Debug, Default)]
struct CacheInner {
    map: HashMap<MediaKey, CachedImage>,
    /// Advances on every read and insert; the entry with the lowest
    /// stamp is the least recently used.
    tick: u64,
    bytes: usize,
    negatives: HashSet<MediaKey>,
    negative_order: VecDeque<MediaKey>,
    /// Every key waiting in `pending` or in flight.
    queued: HashSet<MediaKey>,
    /// Keys waiting for a fetch slot, most recent first.
    pending: VecDeque<MediaKey>,
    /// The covers the browse view last asked for (`request_wanted`).
    /// Only these can be dropped from `pending` by its next request.
    wanted: HashSet<MediaKey>,
    /// Bumped by `clear`. A fetch that started under an older generation
    /// may carry art from before the run that cleared the cache, so its
    /// result is dropped and the key fetched again.
    generation: u64,
}

impl CacheInner {
    fn stamp(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// Put `key` at the front of the queue. False when there is nothing
    /// to fetch: it is cached, known missing, or already in flight.
    fn push_front(&mut self, key: MediaKey) -> bool {
        if self.map.contains_key(&key) || self.negatives.contains(&key) {
            return false;
        }
        if self.queued.contains(&key) {
            // Waiting: move it up. In flight: nothing to do.
            let Some(at) = self.pending.iter().position(|k| *k == key) else {
                return false;
            };
            if at > 0 {
                if let Some(moved) = self.pending.remove(at) {
                    self.pending.push_front(moved);
                }
            }
            return true;
        }
        self.queued.insert(key.clone());
        self.pending.push_front(key);
        true
    }

    fn insert(&mut self, key: MediaKey, image: DecodedImage) {
        self.queued.remove(&key);
        let used = self.stamp();
        self.bytes += image.byte_size();
        if let Some(replaced) = self.map.insert(key, CachedImage { image, used }) {
            self.bytes -= replaced.image.byte_size();
        }
        self.evict_to_cap();
    }

    fn insert_negative(&mut self, key: MediaKey) {
        self.queued.remove(&key);
        if self.negatives.insert(key.clone()) {
            self.negative_order.push_back(key);
            while self.negative_order.len() > NEGATIVE_CAP {
                if let Some(old) = self.negative_order.pop_front() {
                    self.negatives.remove(&old);
                }
            }
        }
    }

    /// Evict least recently used images until the cap holds. The scan is
    /// linear, but only runs when an insert overflows the cap.
    fn evict_to_cap(&mut self) {
        while self.bytes > CACHE_CAP_BYTES {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(evicted) = self.map.remove(&oldest) {
                self.bytes -= evicted.image.byte_size();
            }
        }
    }
}

#[derive(Debug)]
pub struct MediaCache {
    inner: Mutex<CacheInner>,
    /// Wakes the fetch driver when `pending` gains a key.
    wake: Notify,
    /// User-preferred artwork type ("Preferred artwork" setting):
    /// prepended to the request ladder when set. "auto"/empty keeps
    /// the default order.
    preferred_type: Mutex<String>,
}

fn lock_inner(inner: &Mutex<CacheInner>) -> MutexGuard<'_, CacheInner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

impl MediaCache {
    /// Create the cache; `spawn_driver` then fetches what it queues.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CacheInner::default()),
            wake: Notify::new(),
            preferred_type: Mutex::new(String::new()),
        })
    }

    /// Set the preferred artwork type. Cached images keep their old
    /// art until they age out of the LRU; new fetches prefer the new
    /// type.
    pub fn set_preferred_image_type(&self, value: &str) {
        let mut preferred = self
            .preferred_type
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *preferred = value.to_string();
    }

    fn preferred_image_type(&self) -> String {
        self.preferred_type
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Cached image for `key`, touching LRU recency.
    pub fn get(&self, key: &MediaKey) -> Option<DecodedImage> {
        let mut inner = lock_inner(&self.inner);
        let stamp = inner.stamp();
        let entry = inner.map.get_mut(key)?;
        entry.used = stamp;
        Some(entry.image.clone())
    }

    /// Core answered "no image" for this key earlier in the process.
    pub fn is_negative(&self, key: &MediaKey) -> bool {
        lock_inner(&self.inner).negatives.contains(key)
    }

    /// Queue a fetch ahead of everything already waiting, unless the key
    /// is cached, memoized-negative, or in flight. A key that is already
    /// waiting moves to the front.
    pub fn enqueue(&self, key: MediaKey) {
        let queued = lock_inner(&self.inner).push_front(key);
        if queued {
            self.wake.notify_one();
        }
    }

    /// The browse view's covers, in priority order. They go to the front
    /// of the queue in that order, and whatever its previous request
    /// queued that is not in this one is dropped before it is fetched:
    /// covers that scrolled past are not worth a round trip. Keys queued
    /// through `enqueue` alone are never dropped here.
    pub fn request_wanted(&self, keys: Vec<MediaKey>) {
        let queued = {
            let mut inner = lock_inner(&self.inner);
            let wanted: HashSet<MediaKey> = keys.iter().cloned().collect();
            let stale: HashSet<MediaKey> = inner.wanted.difference(&wanted).cloned().collect();
            if !stale.is_empty() {
                let CacheInner {
                    pending, queued, ..
                } = &mut *inner;
                pending.retain(|key| {
                    let keep = !stale.contains(key);
                    if !keep {
                        queued.remove(key);
                    }
                    keep
                });
            }
            inner.wanted = wanted;
            let mut any = false;
            for key in keys.into_iter().rev() {
                any |= inner.push_front(key);
            }
            any
        };
        if queued {
            self.wake.notify_one();
        }
    }

    /// The next key to fetch, waiting until there is one.
    async fn next_pending(&self) -> MediaKey {
        loop {
            let woken = self.wake.notified();
            if let Some(key) = lock_inner(&self.inner).pending.pop_front() {
                return key;
            }
            woken.await;
        }
    }

    /// True when the key already has an image in memory.
    pub fn is_cached(&self, key: &MediaKey) -> bool {
        lock_inner(&self.inner).map.contains_key(key)
    }

    /// Put an in-flight request back on the queue, behind the dormancy
    /// gate or a reconnect. Its queued identity remains set, so no
    /// duplicate can join it while it waits; it waits behind newer work.
    fn requeue(&self, key: MediaKey) {
        lock_inner(&self.inner).pending.push_back(key);
        self.wake.notify_one();
    }

    /// A request ended without a result to store; its key may queue again.
    fn forget_queued(&self, key: &MediaKey) {
        lock_inner(&self.inner).queued.remove(key);
    }

    /// Release decoded image storage before `MiSTer` hands RAM to a
    /// launched core. Negative results and queued identities remain so
    /// a frontend that survives the handoff can continue cleanly.
    pub fn clear_decoded(&self) {
        let mut inner = lock_inner(&self.inner);
        inner.map.clear();
        inner.bytes = 0;
    }

    /// Forget every image and every "no image" answer. An index or a
    /// metadata import can add, replace or remove art for any game, and a
    /// remembered answer would otherwise hold for the whole session.
    /// Queued requests stay queued; one already in flight is fetched
    /// again when it lands (see `store_fetched`).
    pub fn clear(&self) {
        let mut inner = lock_inner(&self.inner);
        inner.map.clear();
        inner.bytes = 0;
        inner.negatives.clear();
        inner.negative_order.clear();
        inner.generation = inner.generation.wrapping_add(1);
    }

    fn generation(&self) -> u64 {
        lock_inner(&self.inner).generation
    }

    /// Store what a fetch that began under `generation` found: an image,
    /// or `None` for no image. When `clear` ran since, the answer may
    /// predate it, so it is dropped and the still-queued key goes back on
    /// the queue. Returns whether the result was stored.
    fn store_fetched(&self, key: MediaKey, generation: u64, image: Option<DecodedImage>) -> bool {
        {
            let mut inner = lock_inner(&self.inner);
            if inner.generation == generation {
                match image {
                    Some(image) => inner.insert(key, image),
                    None => inner.insert_negative(key),
                }
                return true;
            }
        }
        self.requeue(key);
        false
    }

    /// Put an image the cache did not fetch itself into it (the
    /// cold-boot manifest's own seed).
    pub fn seed(&self, key: MediaKey, image: DecodedImage) {
        self.insert(key, image);
    }

    fn insert(&self, key: MediaKey, image: DecodedImage) {
        lock_inner(&self.inner).insert(key, image);
    }

    #[cfg(test)]
    fn insert_negative(&self, key: MediaKey) {
        lock_inner(&self.inner).insert_negative(key);
    }
}

/// Latched once a Core that does not know the delivery field rejects
/// it: the session stops asking rather than paying a failed round trip
/// per cover.
static LOCAL_PATH_DISABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Whether Core runs on this machine, so the paths it names are ours to
/// open. Updated when the host replaces the active transport.
static CORE_IS_LOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static READS_CORE_FILES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record the runtime facts the local-path fast path depends on:
/// whether this frontend can open files Core names, and whether Core is
/// local.
pub fn configure_local_path(reads_core_files: bool, is_local: bool) {
    use std::sync::atomic::Ordering;
    READS_CORE_FILES.store(reads_core_files, Ordering::Release);
    CORE_IS_LOCAL.store(is_local, Ordering::Release);
}

/// Whether a request for `max_size` may ask Core for a path.
pub fn should_request_local_path(max_size: u32) -> bool {
    use std::sync::atomic::Ordering;
    zaparoo_app::covers::local_path_request_allowed(
        max_size,
        READS_CORE_FILES.load(Ordering::Acquire),
        CORE_IS_LOCAL.load(Ordering::Acquire),
        LOCAL_PATH_DISABLED.load(Ordering::Acquire),
    )
}

/// Read a thumbnail Core named, with the byte cap that keeps a
/// mis-sized file from taking the machine down with it.
pub fn read_local_image_file(path: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("thumbnail path is not a regular file".to_string());
    }
    let length = usize::try_from(metadata.len())
        .map_err(|_| "thumbnail file size does not fit memory limits".to_string())?;
    if length == 0 {
        return Err("thumbnail file was empty".to_string());
    }
    if length > max_bytes {
        return Err(format!(
            "thumbnail file exceeds {max_bytes}-byte read limit"
        ));
    }
    let mut bytes = Vec::with_capacity(length);
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("thumbnail file was empty".to_string());
    }
    if bytes.len() > max_bytes {
        return Err(format!(
            "thumbnail file exceeds {max_bytes}-byte read limit"
        ));
    }
    Ok(bytes)
}

/// Decode bytes already in hand (a local read, or the manifest's own
/// seed) into the cache's image form.
pub fn decode_bytes(bytes: &[u8]) -> Option<DecodedImage> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(DecodedImage {
        buffer: slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &rgba, width, height,
        ),
    })
}

/// Build one Core request. Media identity fields are exclusive, and artwork
/// preference leads the fallback ladder unless a carousel slot names one type.
fn request_params(cache: &MediaCache, key: &MediaKey) -> MediaImageParams {
    let mut image_types: Vec<String> = ["boxart", "image", "thumbnail", "boxart3d", "screenshot"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let preferred = cache.preferred_image_type();
    if !preferred.is_empty() && preferred != "auto" {
        image_types.retain(|kind| *kind != preferred);
        image_types.insert(0, preferred);
    }
    if let Some(kind) = &key.image_type {
        image_types = vec![kind.clone()];
    }
    let delivery = should_request_local_path(key.max_size)
        .then(|| zaparoo_core::media_types::MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string());
    if key.media_id.is_some() {
        MediaImageParams {
            media_id: key.media_id,
            system: String::new(),
            path: String::new(),
            image_types,
            max_size: (key.max_size > 0).then_some(key.max_size),
            delivery,
        }
    } else {
        MediaImageParams {
            media_id: None,
            system: key.system.clone(),
            path: key.path.clone(),
            image_types,
            max_size: (key.max_size > 0).then_some(key.max_size),
            delivery,
        }
    }
}

/// Requests allowed in flight at once. Core serves them concurrently, so a
/// page of tiles fills in about this many times faster than one at a time,
/// while a long list still cannot flood the connection.
const PARALLEL_FETCHES: usize = 4;

/// Spawn the fetch driver. `on_settled` runs on a driver task for every
/// key that got an image or a remembered "no image"; it is expected to
/// marshal onto the UI event loop itself (see `Landings`).
pub fn spawn_driver(
    cache: Arc<MediaCache>,
    client: Arc<Client>,
    handle: &tokio::runtime::Handle,
    dormant: tokio::sync::watch::Receiver<bool>,
    on_settled: impl Fn(MediaKey) + Send + 'static,
) {
    // Fetches finish in any order; one task hands them to `on_settled`,
    // so the callback keeps its single caller.
    let (done_tx, mut done_rx) = unbounded_channel::<MediaKey>();
    handle.spawn(async move {
        while let Some(key) = done_rx.recv().await {
            on_settled(key);
        }
    });
    let permits = Arc::new(tokio::sync::Semaphore::new(PARALLEL_FETCHES));
    let spawner = handle.clone();
    handle.spawn(async move {
        loop {
            // Take the slot first, then the key: the queue's order is
            // decided when a fetch can actually start, so anything queued
            // while every slot was busy still goes first.
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return;
            };
            let key = cache.next_pending().await;
            let cache = cache.clone();
            let client = client.clone();
            let dormant = dormant.clone();
            let done = done_tx.clone();
            spawner.spawn(async move {
                let _permit = permit;
                fetch_one(&cache, &client, key, dormant, &done).await;
            });
        }
    });
}

/// Keys settled on the driver side and not yet delivered to the UI. The
/// first key of a batch schedules one delivery; keys that settle before
/// it runs join that same delivery, so a page of covers landing together
/// repaints once, not once per cover.
#[derive(Debug, Default)]
pub struct Landings {
    inner: Mutex<(Vec<MediaKey>, bool)>,
}

impl Landings {
    /// Record a settled key. True when the caller must schedule the
    /// delivery that will `take` it.
    pub fn push(&self, key: MediaKey) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if !inner.0.contains(&key) {
            inner.0.push(key);
        }
        !std::mem::replace(&mut inner.1, true)
    }

    /// Everything settled since the last delivery; the next key schedules
    /// a new one.
    pub fn take(&self) -> Vec<MediaKey> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.1 = false;
        std::mem::take(&mut inner.0)
    }
}

/// One cover request, from the dormancy gate to a decoded image or a
/// remembered "no image".
async fn fetch_one(
    cache: &Arc<MediaCache>,
    client: &Client,
    key: MediaKey,
    mut dormant: tokio::sync::watch::Receiver<bool>,
    done: &UnboundedSender<MediaKey>,
) {
    // Preserve queued work while a local game owns the screen.
    // One RPC already in flight may finish; the next one waits.
    while *dormant.borrow_and_update() {
        if dormant.changed().await.is_err() {
            return;
        }
    }
    // A key can be enqueued, then satisfied by a seed before we get to
    // it.
    if cache.is_cached(&key) {
        cache.forget_queued(&key);
        return;
    }
    // When this frontend can open Core's files (a colocated MiSTer, or an
    // embedding host running Core as the same app), request the path
    // instead of making Core base64 a local file.
    let params = request_params(cache, &key);
    let generation = cache.generation();
    let asked_for_path = params.delivery.is_some();
    let mut outcome = client.media_image(params.clone()).await;
    if let Err(e) = &outcome {
        if asked_for_path && zaparoo_app::covers::is_unsupported_delivery_error(&e.message) {
            // This Core predates the delivery field; stop
            // asking for the rest of the session and retry the
            // ordinary way.
            LOCAL_PATH_DISABLED.store(true, std::sync::atomic::Ordering::Release);
            tracing::info!("core does not support localPath delivery; using inline images");
            outcome = client
                .media_image(MediaImageParams {
                    delivery: None,
                    ..params
                })
                .await;
        }
    }
    match outcome {
        Ok(result) => {
            // Finish any asynchronous local read before taking the
            // dormancy guard; no decoded pixels exist yet.
            let local_bytes =
                if result.delivery == zaparoo_core::media_types::MEDIA_IMAGE_DELIVERY_LOCAL_PATH {
                    match result.local_path.as_deref().filter(|p| !p.is_empty()) {
                        Some(path) => read_local(path.to_string()).await,
                        None => None,
                    }
                } else {
                    None
                };
            // Holding the watch read guard makes this result atomic
            // with set_dormant's send_replace. Either this finishes
            // first and clear_decoded removes it, or dormancy wins and
            // the undecoded request goes back through the gate.
            let dormant_guard = dormant.borrow_and_update();
            if *dormant_guard {
                drop(dormant_guard);
                cache.requeue(key);
                return;
            }
            let image =
                if result.delivery == zaparoo_core::media_types::MEDIA_IMAGE_DELIVERY_LOCAL_PATH {
                    local_bytes.as_deref().and_then(decode_bytes)
                } else {
                    decode(&result.data)
                };
            if cache.store_fetched(key.clone(), generation, image) {
                let _ = done.send(key);
            }
        }
        Err(e) => {
            // Scoped so the read guard is not held across the
            // reconnect wait below.
            let is_dormant = *dormant.borrow_and_update();
            if is_dormant {
                cache.requeue(key);
                return;
            }
            // No link is not Core saying there is no image: a
            // tile asked for before Core was reachable (every Hub
            // tile on a cold Android start) would stay blank for
            // the whole session. Ask again once Core is connected.
            if e.is_transport() {
                tracing::debug!(path = %key.path, "media.image waits for Core: {}", e.message);
                wait_until_connected(client).await;
                cache.requeue(key);
                return;
            }
            tracing::debug!(path = %key.path, "media.image failed: {}", e.message);
            if cache.store_fetched(key.clone(), generation, None) {
                let _ = done.send(key);
            }
        }
    }
}

/// Resolve once the client has a live link. A session that just ended can
/// still read as connected for a moment, so a short pause keeps a request
/// that failed on it from spinning.
async fn wait_until_connected(client: &Client) {
    let mut state = client.connection.subscribe();
    let _ = state
        .wait_for(|s| *s == zaparoo_core::client::ConnectionState::Connected)
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

/// Read a Core-named thumbnail off the event loop.
async fn read_local(path: String) -> Option<Vec<u8>> {
    let read = tokio::task::spawn_blocking(move || {
        read_local_image_file(&path, zaparoo_app::covers::MAX_LOCAL_IMAGE_BYTES)
    })
    .await;
    match read {
        Ok(Ok(bytes)) => Some(bytes),
        Ok(Err(e)) => {
            tracing::debug!("local thumbnail read failed: {e}");
            None
        }
        Err(e) => {
            tracing::debug!("local thumbnail read task failed: {e}");
            None
        }
    }
}

/// Base64 payload -> decoded RGBA8. Returns None for empty payloads
/// or undecodable data (both memoized as negatives by the caller).
fn decode(data_b64: &str) -> Option<DecodedImage> {
    use base64::Engine as _;
    if data_b64.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(DecodedImage {
        buffer: slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            rgba.as_raw(),
            width,
            height,
        ),
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test fixtures fail fast")]
mod tests {
    use super::*;

    fn key(id: i64) -> MediaKey {
        MediaKey {
            media_id: Some(id),
            system: String::new(),
            path: format!("/g/{id}"),
            max_size: 256,
            image_type: None,
        }
    }

    #[test]
    fn carousel_types_do_not_alias_each_other_or_browse_art() {
        let cache = MediaCache::new();
        let browse = key(1);
        let boxart = MediaKey {
            image_type: Some("boxart".into()),
            ..browse.clone()
        };
        let screenshot = MediaKey {
            image_type: Some("screenshot".into()),
            ..browse.clone()
        };
        cache.seed(boxart.clone(), img(16));
        assert!(cache.get(&boxart).is_some());
        assert!(cache.get(&browse).is_none());
        assert!(cache.get(&screenshot).is_none());
        cache.insert_negative(screenshot.clone());
        assert!(cache.is_negative(&screenshot));
        assert!(!cache.is_negative(&boxart));
        assert!(!cache.is_negative(&browse));
    }

    fn img(bytes: usize) -> DecodedImage {
        let px = (bytes / 4).max(1) as u32;
        DecodedImage {
            buffer: slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(px, 1),
        }
    }

    #[test]
    fn lru_evicts_oldest_when_over_cap() {
        let cache = MediaCache::new();
        // Three entries of ~half the cap: inserting the third must
        // evict the first (oldest).
        let half = CACHE_CAP_BYTES / 2;
        cache.insert(key(1), img(half));
        cache.insert(key(2), img(half));
        cache.insert(key(3), img(half));
        assert!(cache.get(&key(1)).is_none());
        assert!(cache.get(&key(2)).is_some());
        assert!(cache.get(&key(3)).is_some());
    }

    #[test]
    fn read_recency_protects_from_eviction() {
        let cache = MediaCache::new();
        let half = CACHE_CAP_BYTES / 2;
        cache.insert(key(1), img(half));
        cache.insert(key(2), img(half));
        // Touch 1 so 2 becomes the LRU victim.
        assert!(cache.get(&key(1)).is_some());
        cache.insert(key(3), img(half));
        assert!(cache.get(&key(1)).is_some());
        assert!(cache.get(&key(2)).is_none());
    }

    #[test]
    fn clear_decoded_releases_images_but_keeps_negative_memo() {
        let cache = MediaCache::new();
        cache.seed(key(1), img(64));
        cache.insert_negative(key(2));

        cache.clear_decoded();

        assert!(!cache.is_cached(&key(1)));
        assert!(cache.is_negative(&key(2)));
        let inner = lock_inner(&cache.inner);
        assert!(inner.map.is_empty());
        assert_eq!(inner.bytes, 0);
    }

    fn pending(cache: &MediaCache) -> Vec<i64> {
        lock_inner(&cache.inner)
            .pending
            .iter()
            .filter_map(|k| k.media_id)
            .collect()
    }

    fn pop(cache: &MediaCache) -> Option<MediaKey> {
        lock_inner(&cache.inner).pending.pop_front()
    }

    #[test]
    fn a_fetch_that_lands_after_clear_is_fetched_again() {
        let cache = MediaCache::new();
        cache.enqueue(key(1));
        cache.enqueue(key(2));
        // Both are in flight.
        assert!(pop(&cache).is_some());
        assert!(pop(&cache).is_some());
        let before = cache.generation();

        cache.clear();

        // Both answers predate the clear: neither is stored, and each key
        // goes back on the queue instead of staying stuck queued.
        assert!(!cache.store_fetched(key(1), before, Some(img(64))));
        assert!(!cache.store_fetched(key(2), before, None));
        assert!(!cache.is_cached(&key(1)));
        assert!(!cache.is_negative(&key(2)));
        let mut again = pending(&cache);
        again.sort_unstable();
        assert_eq!(again, vec![1, 2]);

        // The retry runs under the new generation and lands.
        let current = cache.generation();
        assert!(cache.store_fetched(key(1), current, Some(img(64))));
        assert!(cache.store_fetched(key(2), current, None));
        assert!(cache.is_cached(&key(1)));
        assert!(cache.is_negative(&key(2)));
    }

    #[test]
    fn enqueue_dedupes_and_skips_negatives() {
        let cache = MediaCache::new();
        cache.enqueue(key(1));
        cache.enqueue(key(1));
        assert_eq!(pending(&cache), vec![1], "duplicate must not re-queue");

        cache.insert_negative(key(2));
        cache.enqueue(key(2));
        assert_eq!(pending(&cache), vec![1], "negative must not queue");

        // In flight: taken off the queue but not finished. Asking again
        // must not queue a second request for it.
        assert_eq!(pop(&cache), Some(key(1)));
        cache.enqueue(key(1));
        assert!(pending(&cache).is_empty());
    }

    #[test]
    fn the_most_recent_request_is_fetched_first() {
        let cache = MediaCache::new();
        cache.enqueue(key(1));
        cache.enqueue(key(2));
        cache.enqueue(key(3));
        assert_eq!(pending(&cache), vec![3, 2, 1]);
        // Asking again for a waiting key moves it up.
        cache.enqueue(key(1));
        assert_eq!(pending(&cache), vec![1, 3, 2]);
        // A requeued request waits behind newer work.
        let first = pop(&cache).expect("queued");
        cache.requeue(first);
        assert_eq!(pending(&cache), vec![3, 2, 1]);
    }

    #[test]
    fn a_browse_request_keeps_its_order_and_drops_what_scrolled_past() {
        let cache = MediaCache::new();
        // Queued outside the browse view (the detail pane, the Hub).
        cache.enqueue(key(99));
        cache.request_wanted(vec![key(1), key(2), key(3)]);
        assert_eq!(pending(&cache), vec![1, 2, 3, 99]);
        // One page on: 1 and 2 left the view before they were fetched.
        cache.request_wanted(vec![key(3), key(4)]);
        assert_eq!(pending(&cache), vec![3, 4, 99]);
        // A dropped key can be asked for again later.
        cache.request_wanted(vec![key(1)]);
        assert_eq!(pending(&cache), vec![1, 99]);
        // An empty request (a fast scroll) drops the whole browse queue
        // but never what was queued without it.
        cache.request_wanted(Vec::new());
        assert_eq!(pending(&cache), vec![99]);
    }

    #[test]
    fn the_driver_takes_the_newest_key() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let cache = MediaCache::new();
        cache.enqueue(key(1));
        cache.enqueue(key(2));
        let next = runtime.block_on(cache.next_pending());
        assert_eq!(next, key(2));
        // An empty queue waits for the next enqueue.
        let _ = runtime.block_on(cache.next_pending());
        let waiter = cache.clone();
        let late = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            waiter.enqueue(key(7));
        });
        assert_eq!(runtime.block_on(cache.next_pending()), key(7));
        late.join().expect("enqueue thread");
    }

    #[test]
    fn eviction_follows_recency_across_many_entries() {
        let cache = MediaCache::new();
        // Ten entries of a tenth of the cap each fill it exactly.
        let tenth = CACHE_CAP_BYTES / 10;
        for id in 0..10 {
            cache.insert(key(id), img(tenth));
        }
        // Touch the even ones; the odd ones are now the oldest.
        for id in (0..10).step_by(2) {
            assert!(cache.get(&key(id)).is_some());
        }
        for id in 10..13 {
            cache.insert(key(id), img(tenth));
        }
        for id in [1, 3, 5] {
            assert!(!cache.is_cached(&key(id)), "{id} was least recent");
        }
        for id in [0, 2, 4, 6, 7, 8, 9, 10, 11, 12] {
            assert!(cache.is_cached(&key(id)), "{id} must survive");
        }
        assert!(lock_inner(&cache.inner).bytes <= CACHE_CAP_BYTES);
        // Replacing an entry does not count its bytes twice.
        cache.insert(key(12), img(tenth));
        assert!(cache.is_cached(&key(0)));
    }

    #[test]
    fn landings_schedule_one_delivery_per_batch() {
        let landings = Landings::default();
        assert!(landings.push(key(1)), "the first key schedules");
        assert!(!landings.push(key(2)), "later keys join the batch");
        assert!(!landings.push(key(1)), "a repeat joins without a copy");
        assert_eq!(landings.take(), vec![key(1), key(2)]);
        assert!(landings.take().is_empty());
        assert!(
            landings.push(key(3)),
            "after a delivery the next key schedules again"
        );
    }

    #[test]
    fn empty_payload_decodes_to_none() {
        assert!(decode("").is_none());
        assert!(decode("bm90IGFuIGltYWdl").is_none());
    }
}
