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

/// Tiny-preview tier: Core's small-thumbnail API size. A whole page
/// of these costs less disk time than one full cover, so they paint
/// first and the full cover fades in over them.
pub const THUMB_TIER: u32 = 32;

/// Cache key, mirroring the Qt `MediaKey`: `media_id` when Core
/// provided one, otherwise the canonical `(system, path)` pair, with
/// the requested bounding-box size baked in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaKey {
    pub media_id: Option<i64>,
    pub system: String,
    pub path: String,
    pub max_size: u32,
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
            let params = if key.media_id.is_some() {
                MediaImageParams {
                    media_id: key.media_id,
                    system: String::new(),
                    path: String::new(),
                    image_types,
                    max_size: (key.max_size > 0).then_some(key.max_size),
                    delivery: None,
                }
            } else {
                MediaImageParams {
                    media_id: None,
                    system: key.system.clone(),
                    path: key.path.clone(),
                    image_types,
                    max_size: (key.max_size > 0).then_some(key.max_size),
                    delivery: None,
                }
            };
            match client.media_image(params).await {
                Ok(result) => match decode(&result.data) {
                    Some(image) => {
                        cache.insert(key.clone(), image.clone());
                        on_ready(key, image);
                    }
                    None => cache.insert_negative(key),
                },
                Err(e) => {
                    tracing::debug!(path = %key.path, "media.image failed: {}", e.message);
                    cache.insert_negative(key);
                }
            }
        }
    });
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
        }
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
