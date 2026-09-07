// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Lean port of the Qt frontend's `media_image_cache` for the Slint
// demo, keeping the load-bearing rules:
// - memory only, never disk: Core is the canonical store and the
//   frontend re-fetches after a cold start;
// - strict bytes cap with LRU eviction (MiSTer shares <512 MB with
//   Core, the wrapper, and the active core);
// - one fetch driver task, one outstanding `media.image` RPC at a
//   time, so a page fill serializes instead of flooding Core;
// - negative results memoized for the process lifetime.
//
// Images are stored decoded (RGBA8) because the LRU cap must account
// for what actually occupies RAM; `slint::Image` construction happens
// on the event loop thread at apply time.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use zaparoo_core::client::Client;
use zaparoo_core::media_types::MediaImageParams;

/// Same cap as the Qt frontend's `CACHE_CAP_BYTES`.
const CACHE_CAP_BYTES: usize = 128 * 1024 * 1024;
const NEGATIVE_CAP: usize = 4096;

/// Cache key, mirroring the Qt `MediaKey`: `media_id` when Core
/// provided one, otherwise the canonical `(system, path)` pair, with
/// the requested bounding-box size baked in.
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

#[derive(Debug, Default)]
struct CacheInner {
    map: HashMap<MediaKey, DecodedImage>,
    /// LRU order: front = oldest. Touched on read and insert.
    order: VecDeque<MediaKey>,
    bytes: usize,
    negatives: HashSet<MediaKey>,
    negative_order: VecDeque<MediaKey>,
    queued: HashSet<MediaKey>,
}

#[derive(Debug)]
pub struct MediaCache {
    inner: Mutex<CacheInner>,
    tx: UnboundedSender<MediaKey>,
    /// User-preferred artwork type ("Preferred artwork" setting):
    /// prepended to the request ladder when set. "auto"/empty keeps
    /// the default order.
    preferred_type: Mutex<String>,
}

fn lock_inner(inner: &Mutex<CacheInner>) -> MutexGuard<'_, CacheInner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

impl MediaCache {
    /// Create the cache and hand back the receiver the fetch driver
    /// consumes (`spawn_driver` owns it).
    pub fn new() -> (Arc<Self>, UnboundedReceiver<MediaKey>) {
        let (tx, rx) = unbounded_channel();
        (
            Arc::new(Self {
                inner: Mutex::new(CacheInner::default()),
                tx,
                preferred_type: Mutex::new(String::new()),
            }),
            rx,
        )
    }

    /// Set the preferred artwork type. Cached images keep their old
    /// art until they age out of the LRU; new fetches prefer the new
    /// type (the Qt cache's behavior on the same setting).
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
        let hit = inner.map.get(key).cloned();
        if hit.is_some() {
            if let Some(pos) = inner.order.iter().position(|k| k == key) {
                inner.order.remove(pos);
                inner.order.push_back(key.clone());
            }
        }
        hit
    }

    /// Core answered "no image" for this key earlier in the process.
    pub fn is_negative(&self, key: &MediaKey) -> bool {
        lock_inner(&self.inner).negatives.contains(key)
    }

    /// Queue a fetch unless the key is cached, memoized-negative, or
    /// already queued.
    pub fn enqueue(&self, key: MediaKey) {
        {
            let mut inner = lock_inner(&self.inner);
            if inner.map.contains_key(&key)
                || inner.negatives.contains(&key)
                || !inner.queued.insert(key.clone())
            {
                return;
            }
        }
        let _ = self.tx.send(key);
    }

    /// True when the key already has an image in memory.
    pub fn is_cached(&self, key: &MediaKey) -> bool {
        lock_inner(&self.inner).map.contains_key(key)
    }

    /// Put an image the cache did not fetch itself into it (the
    /// cold-boot manifest's own seed).
    pub fn seed(&self, key: MediaKey, image: DecodedImage) {
        self.insert(key, image);
    }

    fn insert(&self, key: MediaKey, image: DecodedImage) {
        let mut inner = lock_inner(&self.inner);
        inner.queued.remove(&key);
        inner.bytes += image.byte_size();
        inner.map.insert(key.clone(), image);
        inner.order.push_back(key);
        while inner.bytes > CACHE_CAP_BYTES {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            if let Some(evicted) = inner.map.remove(&oldest) {
                inner.bytes -= evicted.byte_size();
            }
        }
    }

    fn insert_negative(&self, key: MediaKey) {
        let mut inner = lock_inner(&self.inner);
        inner.queued.remove(&key);
        if inner.negatives.insert(key.clone()) {
            inner.negative_order.push_back(key);
            while inner.negative_order.len() > NEGATIVE_CAP {
                if let Some(old) = inner.negative_order.pop_front() {
                    inner.negatives.remove(&old);
                }
            }
        }
    }
}

/// Spawn the serialized fetch driver. `on_ready` runs on the driver
/// task for every successful decode; it is expected to marshal onto
/// the UI event loop itself.
/// Latched once a Core that does not know the delivery field rejects
/// it: the session stops asking rather than paying a failed round trip
/// per cover.
static LOCAL_PATH_DISABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Whether Core runs on this machine, so the paths it names are ours to
/// open. Set once at startup from the configured endpoint.
static CORE_IS_LOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static IS_MISTER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record the runtime facts the local-path fast path depends on.
pub fn configure_local_path(is_mister: bool, core_endpoint: &str) {
    use std::sync::atomic::Ordering;
    IS_MISTER.store(is_mister, Ordering::Release);
    CORE_IS_LOCAL.store(
        zaparoo_app::covers::endpoint_is_loopback(core_endpoint),
        Ordering::Release,
    );
}

/// Whether a request for `max_size` may ask Core for a path.
pub fn should_request_local_path(max_size: u32) -> bool {
    use std::sync::atomic::Ordering;
    zaparoo_app::covers::local_path_request_allowed(
        max_size,
        IS_MISTER.load(Ordering::Acquire),
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

pub fn spawn_driver(
    cache: Arc<MediaCache>,
    client: Arc<Client>,
    handle: &tokio::runtime::Handle,
    mut rx: UnboundedReceiver<MediaKey>,
    on_ready: impl Fn(MediaKey, DecodedImage) + Send + 'static,
) {
    handle.spawn(async move {
        while let Some(key) = rx.recv().await {
            // A key can be enqueued, then satisfied by an earlier
            // in-flight duplicate before we get to it.
            if cache.get(&key).is_some() {
                continue;
            }
            // The media ref is EXCLUSIVE: mediaId when Core provided
            // one, otherwise the (system, path) pair - Core rejects a
            // request mixing both. Image types are preference-first,
            // then the Core default ladder (the Qt cache's
            // `preferred_image_types` shape): a library scraped
            // through `<image>` tags has no `boxart` property at all.
            let mut image_types: Vec<String> =
                ["boxart", "image", "thumbnail", "boxart3d", "screenshot"]
                    .iter()
                    .map(ToString::to_string)
                    .collect();
            let preferred = cache.preferred_image_type();
            if !preferred.is_empty() && preferred != "auto" {
                image_types.retain(|t| *t != preferred);
                image_types.insert(0, preferred);
            }
            if let Some(kind) = &key.image_type {
                image_types = vec![kind.clone()];
            }
            // On a colocated MiSTer the bytes are already on the SD
            // card: ask for the path and read it here rather than
            // making Core base64 a file we can open ourselves.
            let delivery = should_request_local_path(key.max_size)
                .then(|| zaparoo_app::covers::DELIVERY_LOCAL_PATH.to_string());
            let params = if key.media_id.is_some() {
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
            };
            let asked_for_path = params.delivery.is_some();
            let mut outcome = client.media_image(params.clone()).await;
            if let Err(e) = &outcome {
                if asked_for_path && zaparoo_app::covers::is_unsupported_delivery_error(&e.message)
                {
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
                    let image = if result.delivery == zaparoo_app::covers::DELIVERY_LOCAL_PATH {
                        match result.local_path.filter(|p| !p.is_empty()) {
                            Some(path) => read_local(path).await.as_deref().and_then(decode_bytes),
                            None => None,
                        }
                    } else {
                        decode(&result.data)
                    };
                    match image {
                        Some(image) => {
                            cache.insert(key.clone(), image.clone());
                            on_ready(key, image);
                        }
                        None => cache.insert_negative(key),
                    }
                }
                Err(e) => {
                    tracing::debug!(path = %key.path, "media.image failed: {}", e.message);
                    cache.insert_negative(key);
                }
            }
        }
    });
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
        let (cache, _) = MediaCache::new();
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
        let (cache, _rx) = MediaCache::new();
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
        let (cache, _rx) = MediaCache::new();
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
    fn enqueue_dedupes_and_skips_negatives() {
        let (cache, mut rx) = MediaCache::new();
        cache.enqueue(key(1));
        cache.enqueue(key(1));
        assert_eq!(rx.try_recv().ok(), Some(key(1)));
        assert!(rx.try_recv().is_err(), "duplicate must not re-queue");

        cache.insert_negative(key(2));
        cache.enqueue(key(2));
        assert!(rx.try_recv().is_err(), "negative must not queue");
    }

    #[test]
    fn empty_payload_decodes_to_none() {
        assert!(decode("").is_none());
        assert!(decode("bm90IGFuIGltYWdl").is_none());
    }
}
