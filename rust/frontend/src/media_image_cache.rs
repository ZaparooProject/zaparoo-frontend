// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// In-memory cache of media images (boxart, screenshot, wheel, titleshot,
// map, marquee, fanart, generic image) keyed by `mediaId` when Core
// provides one, otherwise by the canonical `(systemId, path)` pair used
// across `media.search`/`media.browse`/`media.image`/`media.meta`.
//
// Owns a single fetch driver task so concurrent enqueues (e.g. a
// freshly-loaded games page with 30 tiles) serialise into one
// outstanding `media.image` RPC at a time — Core's WebSocket has the
// same rate limit `apply_append_page` already calls out, and overlapping
// scrape requests are not the bottleneck we want to hit first.
//
// **Memory only — never disk.** Zaparoo Core is the canonical
// persistent store for media images and metadata; the frontend caches
// in process memory only and re-fetches what it needs after a cold
// start. MiSTer has under 512 MB of shared system RAM with the frontend
// competing against Core, the FPGA wrapper, and the active core for it,
// so the cache enforces a strict bytes cap (`CACHE_CAP_BYTES`) with LRU
// eviction that prefers read entries over still-unread prefetches.
//
// Negative results (Core returned "no image" or any client error) are
// memoised in a FIFO ring capped at 4096 entries — process-lifetime
// only, so a subsequently scraped game shows up after the next frontend
// restart without any eviction dance.
//
// QML reaches the cache through a `QQuickImageProvider` registered on
// the QML engine under the `media-image` scheme: a `coverKey` of
// `media-image/<base64url-no-pad>` becomes the URL
// `image://media-image/<...>`, which `requestImage` decodes back to a
// `MediaKey` and looks up in the in-memory map.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard, Notify};
use tracing::{debug, info, warn};

use zaparoo_core::client::ClientError;
use zaparoo_core::media_types::{
    MediaImageParams, MediaImageResult, MEDIA_IMAGE_DELIVERY_LOCAL_PATH,
};
use zaparoo_core::runtime;
use zaparoo_core::store::Store;

/// Field separator used inside the encoded key. Unit Separator (US,
/// 0x1F) — never appears in valid system ids or filesystem paths so the
/// split back to `(system_id, path)` is unambiguous.
const KEY_SEPARATOR: u8 = 0x1F;

/// Cap on the negative memo ring. Sized so a typical browse session
/// never trims it under normal flow (one page is ~30 entries; 4096
/// covers ~130 pages of misses), while the bytes cost stays bounded:
/// each entry is two `Arc<str>` so ~32 B header + the average
/// `system_id/path` pair (~64 B) → roughly 400 KiB worst-case.
const NEGATIVE_MEMO_CAP: usize = 4096;

/// Hard cap on cached image bytes. Sized to hold several pages of
/// full-resolution tiles while leaving headroom for Core, the FPGA
/// wrapper, and a loaded game core. On `MiSTer` (492 MiB total, no swap)
/// measured free RAM with the frontend running was ~367 MiB, so 128 MiB
/// leaves ~239 MiB for the rest of the system. When `max_cover_size` is
/// set (resized covers average ~30 KB), this cap holds thousands of
/// tiles rather than the ~110 full-resolution SNES covers that fit at
/// 64 MiB.
const CACHE_CAP_BYTES: usize = 128 * 1024 * 1024;
/// Core's `localPath` response is a resized thumbnail, never an arbitrary
/// source image. Bound one file well below total cache capacity so a corrupt,
/// replaced, or remote-host path cannot allocate `MiSTer`'s remaining RAM.
/// `pub(crate)` — also the read-size bound `hub_cover_manifest` uses for
/// its own local-path opens at startup.
pub(crate) const MAX_LOCAL_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Maximum retries for a single key after a transient fetch failure
/// (RPC error, base64 decode error). Generous enough to ride through
/// one bad reconnect, small enough that a key genuinely broken on
/// Core's side stops thrashing the wire. The counter resets on the
/// next user-driven re-enqueue (a page revisit clears `pending` →
/// re-enters `enqueue` → resets `attempts`), so giving up is a
/// session-local "stop retrying right now" rather than a permanent
/// memo.
const MAX_FETCH_ATTEMPTS: u8 = 3;

/// Number of fetch worker tasks pulling from the shared LIFO queue.
/// Two workers let visible pages fill multiple covers at a time
/// while keeping Core/WebSocket pressure low on `MiSTer`. If runtime
/// logs show stalls, resets, or media.image rate-limit errors, tune
/// this before trying any broader queue changes.
const FETCH_DRIVER_WORKERS: usize = 2;

/// Set after a connected older Core explicitly rejects the additive delivery
/// parameter. The frontend then stays inline for the process lifetime instead
/// of doubling every cover request with a known-unsupported probe.
static LOCAL_PATH_REQUESTS_DISABLED: AtomicBool = AtomicBool::new(false);
/// Set after Core accepts one local-path request. Before confirmation, workers
/// share a gate so only one can probe the additive delivery parameter.
static LOCAL_PATH_REQUESTS_CONFIRMED: AtomicBool = AtomicBool::new(false);
static LOCAL_PATH_CAPABILITY_PROBE: AsyncMutex<()> = AsyncMutex::const_new(());

enum LocalPathRequestPermit {
    Inline,
    Request(Option<AsyncMutexGuard<'static, ()>>),
}

async fn acquire_local_path_request_permit() -> LocalPathRequestPermit {
    if LOCAL_PATH_REQUESTS_DISABLED.load(Ordering::Acquire) {
        return LocalPathRequestPermit::Inline;
    }
    if LOCAL_PATH_REQUESTS_CONFIRMED.load(Ordering::Acquire) {
        return LocalPathRequestPermit::Request(None);
    }

    let probe = LOCAL_PATH_CAPABILITY_PROBE.lock().await;
    if LOCAL_PATH_REQUESTS_DISABLED.load(Ordering::Acquire) {
        LocalPathRequestPermit::Inline
    } else if LOCAL_PATH_REQUESTS_CONFIRMED.load(Ordering::Acquire) {
        LocalPathRequestPermit::Request(None)
    } else {
        LocalPathRequestPermit::Request(Some(probe))
    }
}

/// Hard cap on pending enqueues in the fetch queue. Sized for a few
/// dense visual pages (current, lookahead, previous) plus margin, so
/// an explicit page-window rebuild does not drop the previous-page
/// warm on a 30-tile layout while still bounding stale queue memory.
const MAX_QUEUE_LEN: usize = 96;

/// MIME content-type → on-disk extension for the formats we are willing
/// to cache. Falls back to inspecting `MediaImageResult.extension` when
/// `content_type` is missing or unknown — Core started populating the
/// `extension` field directly for exactly this reason.
const SUPPORTED_EXTS: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/jpg", "jpg"),
    ("image/webp", "webp"),
];

const SUPPORTED_PLAIN_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const CORE_DEFAULT_IMAGE_TYPES: &[&str] = &[
    "image",
    "thumbnail",
    "boxart",
    "boxart3d",
    "screenshot",
    "wheel",
    "titleshot",
    "map",
    "marquee",
    "fanart",
];
const COVER_PREF_PREFIX: &str = "__pref:";

fn current_cover_preference_marker() -> Option<Arc<str>> {
    let value = crate::models::try_with_persist_read(|s| s.settings.media_image_type.clone())?;
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "auto" {
        return None;
    }
    Some(Arc::from(format!("{COVER_PREF_PREFIX}{trimmed}")))
}

// `pub(crate)` — also used by `hub_cover_manifest`'s one-off local-path
// lookup, which builds the same `image_types` list the ordinary fetch
// driver does so a manifest-refreshed path matches what a live fetch
// would have requested.
pub(crate) fn preferred_image_types(preference: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(CORE_DEFAULT_IMAGE_TYPES.len() + 1);
    out.push(preference.to_string());
    for image_type in CORE_DEFAULT_IMAGE_TYPES {
        if *image_type != preference {
            out.push((*image_type).to_string());
        }
    }
    out
}

fn ext_for_content_type(content_type: &str) -> Option<&'static str> {
    let head = content_type.split(';').next()?.trim().to_ascii_lowercase();
    SUPPORTED_EXTS
        .iter()
        .find_map(|(ct, ext)| (*ct == head).then_some(*ext))
}

/// Normalise a Core-supplied extension (e.g. `"jpeg"`, `".PNG"`) to
/// our canonical lowercase no-dot form (`"jpg"`, `"png"`). Returns
/// `None` for anything outside the supported set so callers fall
/// back to `content_type` resolution or the negative memo.
fn ext_from_extension_field(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim_start_matches('.').trim().to_ascii_lowercase();
    if !SUPPORTED_PLAIN_EXTS.iter().any(|e| *e == trimmed) {
        return None;
    }
    Some(match trimmed.as_str() {
        "jpeg" | "jpg" => "jpg",
        "png" => "png",
        "webp" => "webp",
        // Unreachable: filtered above.
        _ => return None,
    })
}

/// Canonical media identifier: `(systemId, path)` pair used everywhere
/// downstream. `Arc<str>` so cloning into broadcast frames /
/// `MediaImageUpdate` is cheap and the encoded URL key keeps a single
/// allocation.
#[derive(Clone, Debug)]
pub struct MediaKey {
    pub system_id: Arc<str>,
    pub path: Arc<str>,
    pub media_id: Option<i64>,
    pub image_type: Option<Arc<str>>,
    // Requested `max_size` (bounding-box px) baked into the key so views
    // that want a different resolution (e.g. the larger detail pane vs the
    // grid) get independent cache entries instead of sharing one image.
    // `0` means "unsized" — the fetch falls back to the global default and
    // the key encodes/compares exactly as it did before this field existed.
    pub max_size: u32,
}

impl MediaKey {
    pub fn new(system_id: impl Into<Arc<str>>, path: impl Into<Arc<str>>) -> Self {
        Self {
            system_id: system_id.into(),
            path: path.into(),
            media_id: None,
            image_type: None,
            max_size: 0,
        }
    }

    pub fn with_media_id(
        system_id: impl Into<Arc<str>>,
        path: impl Into<Arc<str>>,
        media_id: i64,
    ) -> Self {
        Self {
            system_id: system_id.into(),
            path: path.into(),
            media_id: Some(media_id),
            image_type: None,
            max_size: 0,
        }
    }

    pub fn with_current_cover_preference(mut self) -> Self {
        if let Some(pref) = current_cover_preference_marker() {
            self.image_type = Some(pref);
        }
        self
    }

    /// Bake a requested bounding-box size into the key. `0` clears it.
    pub fn with_max_size(mut self, max_size: u32) -> Self {
        self.max_size = max_size;
        self
    }

    fn cover_preference(&self) -> Option<&str> {
        self.image_type
            .as_deref()
            .and_then(|t| t.strip_prefix(COVER_PREF_PREFIX))
            .filter(|t| !t.is_empty())
    }

    pub fn is_cover_key(&self) -> bool {
        self.image_type.is_none() || self.cover_preference().is_some()
    }

    pub fn with_image_type(
        system_id: impl Into<Arc<str>>,
        path: impl Into<Arc<str>>,
        image_type: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            system_id: system_id.into(),
            path: path.into(),
            media_id: None,
            image_type: Some(image_type.into()),
            max_size: 0,
        }
    }

    #[cfg(test)]
    pub fn with_media_id_and_image_type(
        system_id: impl Into<Arc<str>>,
        path: impl Into<Arc<str>>,
        media_id: i64,
        image_type: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            system_id: system_id.into(),
            path: path.into(),
            media_id: Some(media_id),
            image_type: Some(image_type.into()),
            max_size: 0,
        }
    }

    /// Encode this key as a single URL path segment using
    /// base64url-no-pad. Sized keys use `v4`; otherwise new keys include
    /// `media_id` when available (`v3`); legacy `(system_id, path)` and v2
    /// typed keys still decode.
    pub fn encode(&self) -> String {
        if self.max_size > 0 {
            let sys = self.system_id.as_bytes();
            let typ = self.image_type.as_deref().unwrap_or("").as_bytes();
            let path = self.path.as_bytes();
            let sys_len = sys.len().to_string();
            let typ_len = typ.len().to_string();
            let size = self.max_size.to_string();
            // Empty id field == `media_id: None`.
            let id = self.media_id.map(|i| i.to_string()).unwrap_or_default();
            let mut buf = Vec::with_capacity(
                7 + size.len()
                    + id.len()
                    + sys_len.len()
                    + typ_len.len()
                    + sys.len()
                    + typ.len()
                    + path.len(),
            );
            buf.extend_from_slice(b"v4");
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(size.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(id.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(path);
            return URL_SAFE_NO_PAD.encode(&buf);
        }
        if let Some(media_id) = self.media_id {
            let sys = self.system_id.as_bytes();
            let typ = self.image_type.as_deref().unwrap_or("").as_bytes();
            let path = self.path.as_bytes();
            let sys_len = sys.len().to_string();
            let typ_len = typ.len().to_string();
            let id = media_id.to_string();
            let mut buf = Vec::with_capacity(
                6 + sys_len.len() + typ_len.len() + id.len() + sys.len() + typ.len() + path.len(),
            );
            buf.extend_from_slice(b"v3");
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(id.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(path);
            return URL_SAFE_NO_PAD.encode(&buf);
        }
        if let Some(image_type) = self.image_type.as_ref() {
            let sys = self.system_id.as_bytes();
            let typ = image_type.as_bytes();
            let path = self.path.as_bytes();
            let sys_len = sys.len().to_string();
            let typ_len = typ.len().to_string();
            let mut buf = Vec::with_capacity(
                4 + sys_len.len() + typ_len.len() + sys.len() + typ.len() + path.len(),
            );
            buf.extend_from_slice(b"v2");
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(sys);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ_len.as_bytes());
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(typ);
            buf.push(KEY_SEPARATOR);
            buf.extend_from_slice(path);
            return URL_SAFE_NO_PAD.encode(&buf);
        }
        let mut buf = Vec::with_capacity(self.system_id.len() + 1 + self.path.len());
        buf.extend_from_slice(self.system_id.as_bytes());
        buf.push(KEY_SEPARATOR);
        buf.extend_from_slice(self.path.as_bytes());
        URL_SAFE_NO_PAD.encode(&buf)
    }

    pub fn decode(encoded: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).ok()?;
        if bytes.starts_with(b"v4") && bytes.get(2) == Some(&KEY_SEPARATOR) {
            return Self::decode_v4(&bytes);
        }
        if bytes.starts_with(b"v3") && bytes.get(2) == Some(&KEY_SEPARATOR) {
            return Self::decode_v3(&bytes);
        }
        if bytes.starts_with(b"v2") && bytes.get(2) == Some(&KEY_SEPARATOR) {
            return Self::decode_v2(&bytes);
        }
        let sep = bytes.iter().position(|b| *b == KEY_SEPARATOR)?;
        let (sys, rest) = bytes.split_at(sep);
        let path = &rest[1..]; // skip the separator
        let system_id = std::str::from_utf8(sys).ok()?;
        let path = std::str::from_utf8(path).ok()?;
        Some(Self::new(system_id.to_string(), path.to_string()))
    }

    fn decode_v2(bytes: &[u8]) -> Option<Self> {
        let mut pos = 3; // "v2" + separator
        let sys_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let sys_len = std::str::from_utf8(&bytes[pos..sys_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = sys_len_end + 1;
        let sys_end = pos.checked_add(sys_len)?;
        let system_id = std::str::from_utf8(bytes.get(pos..sys_end)?).ok()?;
        if bytes.get(sys_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        pos = sys_end + 1;
        let type_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let type_len = std::str::from_utf8(&bytes[pos..type_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = type_len_end + 1;
        let type_end = pos.checked_add(type_len)?;
        let image_type = std::str::from_utf8(bytes.get(pos..type_end)?).ok()?;
        if bytes.get(type_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        let path = std::str::from_utf8(bytes.get(type_end + 1..)?).ok()?;
        Some(Self::with_image_type(
            system_id.to_string(),
            path.to_string(),
            image_type.to_string(),
        ))
    }

    fn decode_v3(bytes: &[u8]) -> Option<Self> {
        let mut pos = 3; // "v3" + separator
        let id_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let media_id = std::str::from_utf8(&bytes[pos..id_end])
            .ok()?
            .parse::<i64>()
            .ok()?;
        pos = id_end + 1;
        let sys_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let sys_len = std::str::from_utf8(&bytes[pos..sys_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = sys_len_end + 1;
        let sys_end = pos.checked_add(sys_len)?;
        let system_id = std::str::from_utf8(bytes.get(pos..sys_end)?).ok()?;
        if bytes.get(sys_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        pos = sys_end + 1;
        let type_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let type_len = std::str::from_utf8(&bytes[pos..type_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = type_len_end + 1;
        let type_end = pos.checked_add(type_len)?;
        let image_type = std::str::from_utf8(bytes.get(pos..type_end)?).ok()?;
        if bytes.get(type_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        let path = std::str::from_utf8(bytes.get(type_end + 1..)?).ok()?;
        let image_type = (!image_type.is_empty()).then(|| Arc::<str>::from(image_type));
        Some(Self {
            system_id: Arc::from(system_id),
            path: Arc::from(path),
            media_id: Some(media_id),
            image_type,
            max_size: 0,
        })
    }

    fn decode_v4(bytes: &[u8]) -> Option<Self> {
        let mut pos = 3; // "v4" + separator
        let size_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let max_size = std::str::from_utf8(&bytes[pos..size_end])
            .ok()?
            .parse::<u32>()
            .ok()?;
        pos = size_end + 1;
        let id_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let id_field = std::str::from_utf8(&bytes[pos..id_end]).ok()?;
        // Empty id field == no media_id.
        let media_id = if id_field.is_empty() {
            None
        } else {
            Some(id_field.parse::<i64>().ok()?)
        };
        pos = id_end + 1;
        let sys_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let sys_len = std::str::from_utf8(&bytes[pos..sys_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = sys_len_end + 1;
        let sys_end = pos.checked_add(sys_len)?;
        let system_id = std::str::from_utf8(bytes.get(pos..sys_end)?).ok()?;
        if bytes.get(sys_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        pos = sys_end + 1;
        let type_len_end = bytes[pos..].iter().position(|b| *b == KEY_SEPARATOR)? + pos;
        let type_len = std::str::from_utf8(&bytes[pos..type_len_end])
            .ok()?
            .parse::<usize>()
            .ok()?;
        pos = type_len_end + 1;
        let type_end = pos.checked_add(type_len)?;
        let image_type = std::str::from_utf8(bytes.get(pos..type_end)?).ok()?;
        if bytes.get(type_end) != Some(&KEY_SEPARATOR) {
            return None;
        }
        let path = std::str::from_utf8(bytes.get(type_end + 1..)?).ok()?;
        let image_type = (!image_type.is_empty()).then(|| Arc::<str>::from(image_type));
        Some(Self {
            system_id: Arc::from(system_id),
            path: Arc::from(path),
            media_id,
            image_type,
            max_size,
        })
    }
}

impl PartialEq for MediaKey {
    fn eq(&self, other: &Self) -> bool {
        // Distinct requested sizes are distinct cache entries.
        if self.max_size != other.max_size {
            return false;
        }
        match (self.media_id, other.media_id) {
            (Some(a), Some(b)) => a == b && self.image_type == other.image_type,
            (None, None) => {
                *self.system_id == *other.system_id
                    && *self.path == *other.path
                    && self.image_type == other.image_type
            }
            _ => false,
        }
    }
}
impl Eq for MediaKey {}

impl std::hash::Hash for MediaKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.max_size.hash(state);
        if let Some(media_id) = self.media_id {
            media_id.hash(state);
            self.image_type.hash(state);
            return;
        }
        (*self.system_id).hash(state);
        (*self.path).hash(state);
        self.image_type.hash(state);
    }
}

/// Update event published when the cache state changes for one media
/// key. `ext` is `Some` after a successful fetch and `None` after a
/// negative resolution; subscribers use this to invalidate row
/// `dataChanged(coverKey)` on the Qt thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaImageUpdate {
    pub key: MediaKey,
    pub ext: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoImagePolicy {
    Memoize,
    SoftMiss,
}

/// One slot in the LIFO fetch queue. `page_size` is retained as
/// caller metadata for logging/backward-compatible enqueue signatures;
/// Core now receives one `media.image` request per queue entry.
#[derive(Clone, Debug)]
struct QueueEntry {
    key: MediaKey,
    page_size: u32,
    no_image_policy: NoImagePolicy,
    enqueued_at: Instant,
}

#[derive(Debug)]
struct MediaImageEntry {
    // `Arc`, not `Vec<u8>` directly: `get_bytes` used to clone the full
    // 30-80 KiB buffer while holding CacheState's write lock, serialising
    // every cover paint against every other reader/writer for the
    // duration of a real memcpy. An `Arc` clone under the lock is an
    // atomic refcount bump instead; the actual byte copy (`to_vec()`)
    // happens after the lock is released. See `get_bytes`.
    bytes: Arc<Vec<u8>>,
    #[allow(dead_code, reason = "ext is informational; provider only needs bytes")]
    ext: &'static str,
    /// Monotonically increasing usage counter used as the LRU clock.
    /// `u64` is overkill but guarantees no wraparound across any
    /// realistic process lifetime.
    last_used: u64,
    /// `true` once `get_bytes` has handed these bytes to `QtQuick` at
    /// least once. Eviction prefers read entries — unread entries are
    /// the prefetcher's bet that QML is about to ask for them, and
    /// dropping them under pressure causes the tile to render as
    /// fallback text even though Core already returned the bytes.
    read: bool,
}

#[derive(Default, Debug)]
struct NegativeMemo {
    order: VecDeque<MediaKey>,
    set: HashSet<MediaKey>,
}

impl NegativeMemo {
    fn contains(&self, key: &MediaKey) -> bool {
        self.set.contains(key)
    }

    fn insert(&mut self, key: MediaKey) {
        if !self.set.insert(key.clone()) {
            return;
        }
        self.order.push_back(key);
        while self.order.len() > NEGATIVE_MEMO_CAP {
            if let Some(dropped) = self.order.pop_front() {
                self.set.remove(&dropped);
            }
        }
    }

    fn remove(&mut self, key: &MediaKey) {
        if !self.set.remove(key) {
            return;
        }
        self.order.retain(|memo_key| memo_key != key);
    }

    /// Drop every entry for `system_id`. One retain pass each over `set`
    /// and `order` rather than a collect-then-remove-per-key loop (the
    /// latter is O(K * `order.len()`) for K stale keys, since each `remove`
    /// call does its own `order.retain` scan) — this only runs on an
    /// index/scrape *completion* edge (see
    /// `MediaImageCache::invalidate_negative_memo_for_system`), a rare,
    /// user-paced event, not the read/write hot path the rest of this
    /// struct serves, but a scrape that invalidates a large slice of one
    /// system's entries is exactly the case the old approach scaled
    /// worst for.
    fn remove_system(&mut self, system_id: &str) {
        self.set.retain(|key| &*key.system_id != system_id);
        self.order.retain(|key| self.set.contains(key));
    }

    fn clear(&mut self) {
        self.set.clear();
        self.order.clear();
    }
}

#[derive(Debug)]
struct CacheState {
    map: HashMap<MediaKey, MediaImageEntry>,
    total_bytes: usize,
    negative: NegativeMemo,
    soft_no_image: NegativeMemo,
    search_seen: NegativeMemo,
    pending: HashSet<MediaKey>,
    /// Per-key retry counter for transient fetch failures. Bumped in
    /// the fetch driver before each re-enqueue; cleared on Success,
    /// `NoImage`, or final give-up after `MAX_FETCH_ATTEMPTS`. Lives
    /// inside the locked state so the read/bump/decision happens
    /// atomically with the `pending` mutation that drives the retry.
    attempts: HashMap<MediaKey, u8>,
    /// Sidecar of latest-known `media_id` hints per `MediaKey`. Used
    /// only to populate the `mediaId` field on outgoing batched
    /// requests when the model has one — the cache keeps identifying
    /// rows by `(systemId, path)` because Core treats `media_id` as
    /// session-ephemeral and the frontend must continue to work after
    /// a Core restart that invalidates the integers. Entries get
    /// cleaned up alongside the row in `evict_until_fits` so the
    /// sidecar can't outgrow the rest of the cache.
    media_ids: HashMap<MediaKey, i64>,
    /// Short image type that Core resolved for each cached key.
    /// Populated from `MediaImageResult.type_tag` (stripped of the
    /// `property:image-` prefix) on every successful fetch. Used by the
    /// carousel dedup: when the preference is "auto" (empty `imageTypes`),
    /// Core may return e.g. `boxart` — the carousel tail can then drop
    /// the concrete `boxart` key so left/right shows a genuinely different
    /// image. Cleaned up alongside `map` in `evict_until_fits`.
    resolved_types: HashMap<MediaKey, String>,
    /// Strictly increasing LRU clock. Bumped on every successful read
    /// or insert; the entry with the smallest value is the LRU.
    clock: u64,
    /// LRU index into `map`, split into two independent trees (read vs
    /// unread) rather than one composite-keyed tree — eviction always
    /// prefers a read entry over an unread one regardless of how stale
    /// the unread entry is (see `evict_until_fits`'s doc comment), so the
    /// two populations must be independently orderable, not just sorted
    /// within one shared key. Keyed on `last_used`, which `next_clock`
    /// guarantees is unique across the cache's entire lifetime — that
    /// uniqueness is what makes removing an entry from its tree by
    /// `last_used` alone (no key comparison needed) correct. Replaces the
    /// old O(N) linear scan over `map` that `evict_until_fits` used to do
    /// per evicted entry; kept in sync exclusively through
    /// `insert_entry`/`remove_entry`/`touch_entry` below — nothing else
    /// may touch `map`, `read_lru`, or `unread_lru` directly, or the
    /// index silently drifts out of sync with the data it's meant to
    /// describe.
    read_lru: BTreeMap<u64, MediaKey>,
    unread_lru: BTreeMap<u64, MediaKey>,
}

impl CacheState {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            total_bytes: 0,
            negative: NegativeMemo::default(),
            soft_no_image: NegativeMemo::default(),
            search_seen: NegativeMemo::default(),
            pending: HashSet::new(),
            attempts: HashMap::new(),
            media_ids: HashMap::new(),
            resolved_types: HashMap::new(),
            clock: 0,
            read_lru: BTreeMap::new(),
            unread_lru: BTreeMap::new(),
        }
    }

    fn next_clock(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn index_insert(&mut self, last_used: u64, read: bool, key: MediaKey) {
        if read {
            self.read_lru.insert(last_used, key);
        } else {
            self.unread_lru.insert(last_used, key);
        }
    }

    fn index_remove(&mut self, last_used: u64, read: bool) {
        if read {
            self.read_lru.remove(&last_used);
        } else {
            self.unread_lru.remove(&last_used);
        }
    }

    /// Insert a fresh entry, or replace an existing one, keeping
    /// `read_lru`/`unread_lru` in sync. The only path that may write into
    /// `map` — see the field doc comment on `read_lru` for why.
    fn insert_entry(&mut self, key: MediaKey, entry: MediaImageEntry) -> Option<MediaImageEntry> {
        let last_used = entry.last_used;
        let read = entry.read;
        let prev = self.map.insert(key.clone(), entry);
        if let Some(ref prev) = prev {
            self.index_remove(prev.last_used, prev.read);
        }
        self.index_insert(last_used, read, key);
        prev
    }

    /// Remove `key`, keeping `read_lru`/`unread_lru` in sync. The only
    /// path that may remove from `map` — see the field doc comment on
    /// `read_lru` for why.
    fn remove_entry(&mut self, key: &MediaKey) -> Option<MediaImageEntry> {
        let removed = self.map.remove(key);
        if let Some(ref removed) = removed {
            self.index_remove(removed.last_used, removed.read);
        }
        removed
    }

    /// Bump `key`'s `last_used` to `next_clock` and mark it read, keeping
    /// `read_lru`/`unread_lru` in sync. Returns the entry so the caller
    /// can read its bytes without a second lookup. The only path that may
    /// mutate an existing `map` entry in place — see the field doc
    /// comment on `read_lru` for why.
    fn touch_entry(&mut self, key: &MediaKey, next_clock: u64) -> Option<&MediaImageEntry> {
        let entry = self.map.get_mut(key)?;
        let old_last_used = entry.last_used;
        let old_read = entry.read;
        entry.last_used = next_clock;
        entry.read = true;
        self.index_remove(old_last_used, old_read);
        self.index_insert(next_clock, true, key.clone());
        self.map.get(key)
    }

    /// Drop entries until `total_bytes` fits under `cap_bytes`. Prefers
    /// the LRU among **read** entries; falls back to the LRU among
    /// unread entries only when nothing has been read yet. This means
    /// QML-consumed entries are eligible for eviction before
    /// prefetched-but-not-yet-painted ones — without that ordering, a
    /// page-fill burst that overshoots the cap can drop entries before
    /// the `QtQuick` provider's first paint pass reads them.
    ///
    /// O(log N) per evicted entry via `read_lru`/`unread_lru` (a
    /// `BTreeMap`'s first element is its smallest key, i.e. the oldest
    /// `last_used`). Replaces an old O(N) linear scan over `map` per
    /// evicted entry — fine at the "a few hundred entries" the cache was
    /// originally sized for, but the cap comment on `CACHE_CAP_BYTES`
    /// says 128 MiB holds "thousands of tiles" once `max_cover_size` is
    /// set, and `get_bytes` takes the cache's write lock too, so every
    /// cover paint was serialising against an eviction pass whose cost
    /// scaled with total cache size once the cache was actually full.
    fn evict_until_fits(&mut self, cap_bytes: usize) {
        while self.total_bytes > cap_bytes {
            let victim = self
                .read_lru
                .values()
                .next()
                .or_else(|| self.unread_lru.values().next())
                .cloned();
            let Some(victim) = victim else {
                break;
            };
            if let Some(entry) = self.remove_entry(&victim) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes.len());
                // Keep the sidecars in lock-step with `map` — without
                // this the hint tables grow unboundedly past `cap_bytes`
                // because the eviction pass only touches the primary cache.
                self.media_ids.remove(&victim);
                self.resolved_types.remove(&victim);
                debug!(
                    system_id = %victim.system_id,
                    path = %victim.path,
                    bytes = entry.bytes.len(),
                    read = entry.read,
                    total_bytes = self.total_bytes,
                    "media_image_cache: evicted entry"
                );
            }
        }
    }
}

pub struct MediaImageCache {
    state: Arc<RwLock<CacheState>>,
    /// LIFO queue of pending fetches. `enqueue` pushes to the back,
    /// the fetch driver pops from the back: newest ad-hoc enqueues
    /// drain first. Games page prefetch uses
    /// `replace_pending_requests_ordered`, which clears queued stale
    /// work and pushes the supplied page window in reverse so public
    /// order is still drain order. Plain `std::sync::Mutex` because
    /// every critical section is a bounded queue mutation with no
    /// awaits in between.
    queue: Arc<Mutex<VecDeque<QueueEntry>>>,
    /// Single-permit signal that wakes the driver when a fresh key
    /// hits the queue. Drained by `notified().await` and rearmed by
    /// `notify_one()` per enqueue.
    queue_notify: Arc<Notify>,
    updates_tx: broadcast::Sender<MediaImageUpdate>,
    /// Maximum cover dimension requested from Core. 0 = full resolution.
    /// Set by QML via `set_max_cover_size` when the grid shape changes.
    max_cover_size: Arc<AtomicU32>,
}

impl MediaImageCache {
    fn new<F>(cap_bytes: usize, runtime: &Handle, store_factory: F) -> Self
    where
        F: Fn() -> Arc<Store> + Send + Sync + 'static,
    {
        info!(cap_bytes, "media_image_cache: initialised (in-memory)");
        let state = Arc::new(RwLock::new(CacheState::new()));
        let queue: Arc<Mutex<VecDeque<QueueEntry>>> = Arc::new(Mutex::new(VecDeque::new()));
        let queue_notify = Arc::new(Notify::new());
        let (updates_tx, _) = broadcast::channel::<MediaImageUpdate>(64);
        let max_cover_size = Arc::new(AtomicU32::new(0));

        spawn_fetch_driver(
            runtime,
            cap_bytes,
            &state,
            &updates_tx,
            &queue,
            &queue_notify,
            &max_cover_size,
            store_factory,
        );

        Self {
            state,
            queue,
            queue_notify,
            updates_tx,
            max_cover_size,
        }
    }

    /// Set the maximum cover dimension (pixels) sent to Core with every
    /// `media.image` request. Core resizes the image to fit within a
    /// `size × size` bounding box before returning it, so the cached
    /// bytes are smaller and more covers fit in the fixed-size cache.
    /// Pass `0` to request full-resolution images (default).
    pub fn set_max_cover_size(&self, size: u32) {
        self.max_cover_size.store(size, Ordering::Relaxed);
    }

    /// Bytes for `key`, if cached. Bumps `last_used` so the entry's LRU
    /// position reflects the read.
    ///
    /// The `Arc` clone happens under the write lock (cheap — a refcount
    /// bump, not a copy); the real byte copy (`to_vec()`, 30-80 KiB) is
    /// deferred until after the guard is dropped, so a cover paint no
    /// longer holds every other cache reader/writer for the duration of
    /// a memcpy. See `MediaImageEntry.bytes`'s doc comment.
    pub fn get_bytes(&self, key: &MediaKey) -> Option<Vec<u8>> {
        let bytes = {
            #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
            let mut guard = self.state.write().unwrap();
            let next = guard.next_clock();
            guard.touch_entry(key, next)?.bytes.clone()
        };
        Some(bytes.to_vec())
    }

    /// Seed the cache with bytes read directly from disk rather than
    /// fetched from Core — used once at startup by the Hub/Resume
    /// cold-boot cover manifest (see `hub_layout.rs`'s seed-manifest
    /// module doc) to open Core's own already-on-disk thumbnails
    /// directly, ahead of the first `media.image` round trip.
    ///
    /// Bypasses the fetch queue/retry machinery entirely: this isn't
    /// answering a request anything is waiting on, it's front-running
    /// one, so there's no `FetchOutcome`, no broadcast, and nothing to
    /// retry. A no-op if `key` is already cached (a real fetch that
    /// happened to land first wins) or if `bytes` is empty or exceeds
    /// the cache's own byte cap — the manifest never overrides a fresher
    /// answer or a corrupt read.
    pub fn seed_local(&self, key: MediaKey, bytes: Vec<u8>) {
        if bytes.is_empty() || bytes.len() > CACHE_CAP_BYTES {
            return;
        }
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let mut guard = self.state.write().unwrap();
        if guard.map.contains_key(&key) {
            return;
        }
        let clock = guard.next_clock();
        guard.total_bytes = guard.total_bytes.saturating_add(bytes.len());
        guard.insert_entry(
            key,
            MediaImageEntry {
                bytes: Arc::new(bytes),
                // Core's own thumbnail cache always encodes WebP (see
                // media_image.go's resize path) — informational only,
                // the provider decodes by sniffing the byte content.
                ext: "webp",
                last_used: clock,
                read: false,
            },
        );
        guard.evict_until_fits(CACHE_CAP_BYTES);
    }

    /// True iff `key` has bytes in the cache. Unlike `get_bytes`,
    /// this does **not** bump `last_used` or flip `read` — it's a
    /// pure existence query for callers (e.g. role-data lookups in
    /// `GamesModel`) that need to choose a URL without their lookup
    /// contaminating the LRU clock. The clock should track actual
    /// paints (provider calls from
    /// `QQuickImageProvider::requestImage`), not role-data lookups,
    /// so read-pinning eviction stays meaningful.
    pub fn is_cached(&self, key: &MediaKey) -> bool {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let guard = self.state.read().unwrap();
        guard.map.contains_key(key)
    }

    /// True iff `key` is in the negative memo (Core said "no image",
    /// or returned an unsupported format / oversize payload). Used
    /// by callers that drive miss-recovery enqueues to suppress
    /// re-fetch attempts for keys we've already learned have nothing
    /// to fetch.
    pub fn is_negative(&self, key: &MediaKey) -> bool {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let guard = self.state.read().unwrap();
        guard.negative.contains(key)
    }

    pub fn is_soft_no_image(&self, key: &MediaKey) -> bool {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let guard = self.state.read().unwrap();
        guard.soft_no_image.contains(key)
    }

    /// Drop every negative-memo entry (all three memos — `negative`,
    /// `soft_no_image`, `search_seen`) for `system_id`. No-op for an empty
    /// `system_id`.
    ///
    /// The negative memo is deliberately process-lifetime-only (see this
    /// file's module doc comment) — a "no image" answer that was actually
    /// a transient race (a NAS mount not yet up when the initial boot
    /// index ran, say) stayed memoized for the rest of the process's life
    /// with nothing to clear it, so a subsequently fixed system's covers
    /// never came back without a full frontend restart. Called on that
    /// system's index/scrape completion edge (`media_status.rs::apply`)
    /// so a rescrape's answer actually gets a chance to land.
    pub fn invalidate_negative_memo_for_system(&self, system_id: &str) {
        if system_id.is_empty() {
            return;
        }
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let mut guard = self.state.write().unwrap();
        guard.negative.remove_system(system_id);
        guard.soft_no_image.remove_system(system_id);
        guard.search_seen.remove_system(system_id);
        debug!(
            system_id,
            "media_image_cache: invalidated negative memo for system"
        );
    }

    /// Drop every negative-memo entry across all systems (all three
    /// memos). Called on a whole-library index completion — indexing
    /// carries no per-system scope (unlike a targeted scrape), so a
    /// system-scoped call can't distinguish "this system's covers are
    /// still genuinely missing" from "this system's covers failed during
    /// the same race that just got fixed."
    pub fn invalidate_negative_memo_all(&self) {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let mut guard = self.state.write().unwrap();
        guard.negative.clear();
        guard.soft_no_image.clear();
        guard.search_seen.clear();
        debug!("media_image_cache: invalidated negative memo for all systems");
    }

    /// Return the short image type that Core resolved for `key` on its last
    /// successful fetch (e.g. `"boxart"`). `None` when the key hasn't been
    /// fetched successfully yet, when Core returned an empty `type_tag`
    /// (older Core versions), or when the entry was evicted.
    ///
    /// Used by the carousel dedup: when the user's preference is "auto",
    /// `ordered_detail_image_keys` needs Core's answer to know which
    /// concrete type key in the carousel tail is a duplicate of index 0.
    pub fn resolved_image_type(&self, key: &MediaKey) -> Option<String> {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let guard = self.state.read().unwrap();
        guard.resolved_types.get(key).cloned()
    }

    /// Subscribe to cache updates. Used by `GamesModel` to bridge image
    /// completions onto `dataChanged(coverKey)` on the Qt thread.
    pub fn subscribe(&self) -> broadcast::Receiver<MediaImageUpdate> {
        self.updates_tx.subscribe()
    }

    /// Schedule a fetch for `key` if it isn't already cached, in the
    /// negative memo, or already pending. Idempotent: callers can spam
    /// this from `apply_initial_page`/`apply_append_page` without
    /// filtering, the cache deduplicates internally.
    ///
    /// When the queue exceeds `MAX_QUEUE_LEN`, the oldest entries at
    /// the front are dropped and released from `pending` — they were
    /// most likely enqueued for a page the user has already navigated
    /// past, and a future role-data lookup (or an explicit re-enqueue)
    /// can re-add them if they become relevant again.
    ///
    /// The optional `media_id` is preferred for Core requests because
    /// it bypasses path resolution. `(system, path)` stays on the key
    /// as fallback when a stale ID is rejected. Safe to call
    /// repeatedly: the latest non-`None` hint wins.
    ///
    /// `page_size` is retained only for call-site compatibility and
    /// logging. Core receives one `media.image` request per queue entry.
    pub fn enqueue_with_media_id(&self, key: MediaKey, media_id: Option<i64>, page_size: u32) {
        self.enqueue_with_policy(key, media_id, page_size, NoImagePolicy::Memoize);
    }

    /// Schedule a search/history cover fetch whose "no image" result
    /// should not poison the global negative memo. Favorites and
    /// Recents are backed by `media.search`/`media.history`; their row
    /// paths can fail `media.image` fallback even when the same game
    /// has a valid cover through browse/detail paths. They still need
    /// a broadcast so QML leaves the loading state, but the miss must
    /// remain local to this fetch attempt.
    pub fn enqueue_search_cover_with_media_id(
        &self,
        key: MediaKey,
        media_id: Option<i64>,
        page_size: u32,
    ) {
        self.enqueue_with_policy(key, media_id, page_size, NoImagePolicy::SoftMiss);
    }

    /// Drop queued-but-not-in-flight cover requests. Cached bytes,
    /// negative memos, and requests currently being fetched stay untouched.
    /// Drained keys leave `pending` so final-page
    /// prefetch after rapid navigation can re-enqueue them.
    pub fn clear_pending_requests(&self) {
        let drained = self.drain_queue();
        if drained.is_empty() {
            return;
        }
        self.release_drained_pending(&drained);
        debug!(
            dropped = drained.len(),
            "media_image_cache: cleared pending cover fetches"
        );
    }

    /// Replace queued-but-not-in-flight cover requests with an explicit
    /// ordered page window. `entries` is public drain order: the first
    /// key supplied is the next key the driver should fetch after any
    /// current in-flight request finishes.
    pub fn replace_pending_requests_ordered(
        &self,
        entries: Vec<(MediaKey, Option<i64>)>,
        page_size: u32,
    ) {
        let drained = self.drain_queue();
        if !drained.is_empty() {
            self.release_drained_pending(&drained);
        }

        let page_size = page_size.max(1);
        let mut queued = Vec::new();
        let mut seen = HashSet::new();
        {
            #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
            let mut guard = self.state.write().unwrap();
            for (mut key, media_id) in entries.into_iter().take(MAX_QUEUE_LEN) {
                if key.media_id.is_none() {
                    if let Some(id) = media_id {
                        key.media_id = Some(id);
                    }
                }
                if key.system_id.is_empty() || key.path.is_empty() || !seen.insert(key.clone()) {
                    continue;
                }
                if let Some(id) = media_id.or(key.media_id) {
                    guard.media_ids.insert(key.clone(), id);
                }
                if guard.map.contains_key(&key)
                    || guard.negative.contains(&key)
                    || guard.pending.contains(&key)
                {
                    continue;
                }
                guard.attempts.remove(&key);
                guard.pending.insert(key.clone());
                queued.push(QueueEntry {
                    key,
                    page_size,
                    no_image_policy: NoImagePolicy::Memoize,
                    enqueued_at: Instant::now(),
                });
            }
        }
        if queued.is_empty() {
            return;
        }
        let queued_len = queued.len();
        {
            #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
            let mut q = self.queue.lock().unwrap();
            for entry in queued.into_iter().rev() {
                q.push_back(entry);
            }
        }
        debug!(
            dropped = drained.len(),
            queued = queued_len,
            "media_image_cache: replaced pending cover fetches"
        );
        for _ in 0..FETCH_DRIVER_WORKERS {
            self.queue_notify.notify_one();
        }
    }

    fn drain_queue(&self) -> Vec<MediaKey> {
        #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
        let mut q = self.queue.lock().unwrap();
        q.drain(..).map(|entry| entry.key).collect::<Vec<_>>()
    }

    fn release_drained_pending(&self, drained: &[MediaKey]) {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let mut guard = self.state.write().unwrap();
        for key in drained {
            guard.pending.remove(key);
            guard.attempts.remove(key);
        }
    }

    fn enqueue_with_policy(
        &self,
        mut key: MediaKey,
        media_id: Option<i64>,
        page_size: u32,
        no_image_policy: NoImagePolicy,
    ) {
        if key.media_id.is_none() {
            if let Some(id) = media_id {
                key.media_id = Some(id);
            }
        }
        if key.system_id.is_empty() || key.path.is_empty() {
            return;
        }
        // A `page_size` of zero would let a single key dominate a
        // round (cap = 0 * 1 = 0, but the first item is always taken)
        // and breaks the "K * page_size" invariant. Clamp to 1 so the
        // batch is at least the single triggering entry.
        let page_size = page_size.max(1);
        let should_send = {
            #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
            let mut guard = self.state.write().unwrap();
            if let Some(id) = media_id.or(key.media_id) {
                guard.media_ids.insert(key.clone(), id);
            }
            if no_image_policy == NoImagePolicy::SoftMiss {
                guard.search_seen.insert(key.clone());
            }
            let cached = guard.map.contains_key(&key);
            let negative = guard.negative.contains(&key);
            let pending = guard.pending.contains(&key);
            let soft_no_image = guard.soft_no_image.contains(&key);
            let search_seen = guard.search_seen.contains(&key);
            let blocked_by_soft_miss = soft_no_image && no_image_policy == NoImagePolicy::SoftMiss;
            debug!(
                system_id = %key.system_id,
                path = %key.path,
                media_id = ?media_id,
                page_size,
                policy = ?no_image_policy,
                cached,
                negative,
                pending,
                soft_no_image,
                search_seen,
                blocked_by_soft_miss,
                "media_image_cache: enqueue cover request"
            );
            if cached || negative || pending || blocked_by_soft_miss {
                false
            } else {
                // Reset the retry counter — a fresh user-driven
                // enqueue (e.g. a page revisit after a previous
                // give-up) deserves another bounded run of attempts.
                guard.attempts.remove(&key);
                guard.pending.insert(key.clone());
                true
            }
        };
        if !should_send {
            return;
        }
        let dropped = {
            #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
            let mut q = self.queue.lock().unwrap();
            q.push_back(QueueEntry {
                key,
                page_size,
                no_image_policy,
                enqueued_at: Instant::now(),
            });
            // Keep only the freshest MAX_QUEUE_LEN entries; the rest
            // (oldest enqueues at the front) get dropped. The dropped
            // keys must also leave `pending` so a later `enqueue` can
            // re-add them — otherwise the `pending` short-circuit
            // would silently suppress them forever.
            let mut dropped: Vec<MediaKey> = Vec::new();
            while q.len() > MAX_QUEUE_LEN {
                let Some(stale) = q.pop_front() else { break };
                dropped.push(stale.key);
            }
            dropped
        };
        if !dropped.is_empty() {
            #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
            let mut guard = self.state.write().unwrap();
            for stale in &dropped {
                guard.pending.remove(stale);
            }
            debug!(
                dropped = dropped.len(),
                queue_cap = MAX_QUEUE_LEN,
                "media_image_cache: queue cap hit, dropped stale enqueues"
            );
        }
        self.queue_notify.notify_one();
    }

    /// `coverKey` value for QML: `"media-image/<encoded>"`. The
    /// `Resources.qml` helper rewrites this to
    /// `image://media-image/<encoded>` so the `QQuickImageProvider`
    /// resolves the cached bytes.
    pub fn image_key_for(key: &MediaKey) -> String {
        format!("media-image/{}", key.encode())
    }

    /// Returns the user's configured preferred image type (e.g. `"boxart"`)
    /// if one is set, or `None` when the preference is `"auto"` or unset.
    /// Used by callers that need to deduplicate the cover-preference key
    /// against a list of specific image-type keys.
    pub fn current_cover_preference_type() -> Option<String> {
        let value = crate::models::try_with_persist_read(|s| s.settings.media_image_type.clone())?;
        let trimmed = value.trim().to_owned();
        if trimmed.is_empty() || trimmed == "auto" {
            return None;
        }
        Some(trimmed)
    }
}

/// Pop one entry from the LIFO queue. Core now accepts only one
/// `media.image` item per JSON-RPC call, so batching stays out of the
/// frontend fetch driver entirely.
fn pop_one(queue: &Arc<Mutex<VecDeque<QueueEntry>>>) -> Option<QueueEntry> {
    #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
    queue.lock().unwrap().pop_back()
}

#[allow(
    clippy::too_many_arguments,
    reason = "private constructor; adding max_cover_size pushed it to 8"
)]
fn spawn_fetch_driver<F>(
    runtime: &Handle,
    cap_bytes: usize,
    state: &Arc<RwLock<CacheState>>,
    updates_tx: &broadcast::Sender<MediaImageUpdate>,
    queue: &Arc<Mutex<VecDeque<QueueEntry>>>,
    queue_notify: &Arc<Notify>,
    max_cover_size: &Arc<AtomicU32>,
    store_factory: F,
) where
    F: Fn() -> Arc<Store> + Send + Sync + 'static,
{
    // One Arc'd factory shared across the worker pool — `F` is only
    // `Fn`, not `Clone`, so wrapping it once and cloning the Arc gives
    // every worker a cheap handle into the same underlying closure.
    let store_factory: Arc<F> = Arc::new(store_factory);
    for _ in 0..FETCH_DRIVER_WORKERS {
        let state = state.clone();
        let updates_tx = updates_tx.clone();
        let queue = queue.clone();
        let queue_notify = queue_notify.clone();
        let store_factory = store_factory.clone();
        let max_cover_size = max_cover_size.clone();
        runtime.spawn(async move {
            loop {
                let Some(entry) = pop_one(&queue) else {
                    queue_notify.notified().await;
                    continue;
                };
                let store = store_factory();
                let outcome = fetch_one(&store, &state, &max_cover_size, entry).await;
                process_batch_outcomes(
                    &state,
                    cap_bytes,
                    &updates_tx,
                    &queue,
                    &queue_notify,
                    vec![outcome],
                );
            }
        });
    }
}

/// Apply outcomes for a finished batch: write each into the cache via
/// `finish_fetch`, broadcast updates, and re-enqueue keys that
/// failed transiently. Pulled out of the worker loop so the driver
/// stays at one screen of code and the retry/give-up branches read
/// the same as before.
fn process_batch_outcomes(
    state: &Arc<RwLock<CacheState>>,
    cap_bytes: usize,
    updates_tx: &broadcast::Sender<MediaImageUpdate>,
    queue: &Arc<Mutex<VecDeque<QueueEntry>>>,
    queue_notify: &Arc<Notify>,
    outcomes: Vec<(QueueEntry, FetchOutcome)>,
) {
    for (mut entry, outcome) in outcomes {
        let key = entry.key.clone();
        let is_transient = matches!(outcome, FetchOutcome::Transient);
        let is_connection_down = matches!(outcome, FetchOutcome::ConnectionDown);
        let update = finish_fetch(state, cap_bytes, &key, outcome, entry.no_image_policy);
        if is_connection_down {
            continue;
        }
        if is_transient {
            let attempts = {
                #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                let mut s = state.write().unwrap();
                let counter = s.attempts.entry(key.clone()).or_insert(0);
                *counter = counter.saturating_add(1);
                *counter
            };
            if attempts < MAX_FETCH_ATTEMPTS {
                // Re-enter `pending` and re-enqueue at the front,
                // preserving the original `page_size` while keeping
                // retries behind any freshly-installed ordered page
                // window. The driver pops from the back, so front-side
                // retries cannot jump ahead of current-page work.
                {
                    #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                    let mut s = state.write().unwrap();
                    s.pending.insert(key.clone());
                }
                let dropped = {
                    #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
                    let mut q = queue.lock().unwrap();
                    entry.enqueued_at = Instant::now();
                    q.push_front(entry);
                    // Mirror the `enqueue_with_media_id` cap: a
                    // long-running burst of transient failures can
                    // push the queue past `MAX_QUEUE_LEN` if we
                    // re-enter without trimming. Drop the oldest
                    // fronts and clear them from `pending` so a
                    // future `enqueue` for the same key can re-add
                    // it instead of being short-circuited.
                    let mut dropped: Vec<MediaKey> = Vec::new();
                    while q.len() > MAX_QUEUE_LEN {
                        let Some(stale) = q.pop_front() else { break };
                        dropped.push(stale.key);
                    }
                    dropped
                };
                if !dropped.is_empty() {
                    #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                    let mut guard = state.write().unwrap();
                    for stale in &dropped {
                        guard.pending.remove(stale);
                    }
                    debug!(
                        dropped = dropped.len(),
                        queue_cap = MAX_QUEUE_LEN,
                        "media_image_cache: retry queue cap hit, dropped stale enqueues"
                    );
                }
                queue_notify.notify_one();
            } else {
                // Bounded give-up: clear the counter, no negative
                // memo. The next user-driven enqueue (page revisit)
                // gets a fresh `MAX_FETCH_ATTEMPTS` budget via
                // `enqueue`'s `attempts.remove`.
                #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                state.write().unwrap().attempts.remove(&key);
                info!(
                    system_id = %key.system_id,
                    path = %key.path,
                    attempts,
                    "media_image_cache: giving up after transient failures",
                );
            }
            continue;
        }
        // Success or NoImage: clear the attempts counter and
        // broadcast the resolved state.
        {
            #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
            let mut s = state.write().unwrap();
            s.attempts.remove(&key);
        }
        if let Some(update) = update {
            if let Some(ext) = update.ext {
                debug!(
                    system_id = %key.system_id,
                    path = %key.path,
                    ext,
                    "media_image_cache: cached image",
                );
            } else {
                #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                let guard = state.read().unwrap();
                let negative = guard.negative.contains(&key);
                let soft_no_image = guard.soft_no_image.contains(&key);
                let search_seen = guard.search_seen.contains(&key);
                if negative {
                    debug!(
                        system_id = %key.system_id,
                        path = %key.path,
                        search_seen,
                        "media_image_cache: no image (negative memo)",
                    );
                } else {
                    debug!(
                        system_id = %key.system_id,
                        path = %key.path,
                        policy = ?entry.no_image_policy,
                        soft_no_image,
                        search_seen,
                        "media_image_cache: no image (soft/protected miss)",
                    );
                }
            }
            let _ = updates_tx.send(update);
        }
    }
}

#[derive(Debug)]
enum FetchOutcome {
    Success {
        bytes: Vec<u8>,
        ext: &'static str,
        /// Short image type resolved by Core (e.g. `"boxart"`), stripped of
        /// the `property:image-` prefix. Empty for older Core versions that
        /// do not populate `type_tag`, or when the prefix is absent.
        type_tag: String,
    },
    /// Core gave a "no image" answer for this `(system_id, path)` —
    /// empty payload, unsupported format, or per-item miss. The queue
    /// entry's policy decides whether that answer is strong enough to
    /// enter the shared negative memo.
    NoImage,
    /// Local or RPC-level failure that may not repeat while Core is
    /// still connected: stale `media_id`, base64 wire corruption, or a
    /// generic `media.image` error. Caller retries within the bounded
    /// attempt budget.
    Transient,
    /// Batch-level connection failure (`disconnected`, `not connected`,
    /// reset/refused). Caller clears `pending` but does not immediately
    /// retry every key, avoiding a retry storm while Core is down.
    ConnectionDown,
}

fn fetch_outcome_label(outcome: &FetchOutcome) -> &'static str {
    match outcome {
        FetchOutcome::Success { .. } => "success",
        FetchOutcome::NoImage => "no_image",
        FetchOutcome::Transient => "transient",
        FetchOutcome::ConnectionDown => "connection_down",
    }
}

struct FetchedMediaImage {
    image: MediaImageResult,
    local_bytes: Option<Vec<u8>>,
    rpc_duration: Duration,
    path_read_duration: Duration,
}

// `pub(crate)` — also used by `hub_cover_manifest`'s one-off local-path
// lookup, so it shares the exact same gate the ordinary fetch driver
// uses rather than a duplicated, potentially-drifting copy.
pub(crate) fn should_request_local_path(max_size: u32) -> bool {
    local_path_request_allowed(
        max_size,
        runtime::current().is_mister(),
        crate::models::core_is_local(),
        LOCAL_PATH_REQUESTS_DISABLED.load(Ordering::Acquire),
    )
}

fn local_path_request_allowed(
    max_size: u32,
    frontend_is_mister: bool,
    core_is_local: bool,
    disabled: bool,
) -> bool {
    max_size > 0 && frontend_is_mister && core_is_local && !disabled
}

fn is_unsupported_local_path_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("delivery")
        && (message.contains("unknown")
            || message.contains("unsupported")
            || message.contains("invalid params"))
}

async fn read_local_image(path: String) -> Result<Vec<u8>, String> {
    match tokio::task::spawn_blocking(move || read_local_image_file(&path, MAX_LOCAL_IMAGE_BYTES))
        .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("blocking thumbnail read failed: {error}")),
    }
}

// `pub(crate)` — also used synchronously by `hub_cover_manifest::
// seed_from_manifest` at startup (small, local, sequential reads before
// the first frame; no need for `spawn_blocking` there the way the async
// fetch path needs it).
pub(crate) fn read_local_image_file(path: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
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
        .map_err(|error| error.to_string())?;
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

fn inline_fallback_params(mut params: MediaImageParams) -> MediaImageParams {
    // Omit the additive field entirely. A legacy Core that rejected
    // `delivery: "localPath"` rejects `delivery: "inline"` too.
    params.delivery = None;
    params
}

async fn fetch_inline_media_image(
    store: &Arc<Store>,
    params: MediaImageParams,
) -> (Result<MediaImageResult, ClientError>, Duration) {
    let started = Instant::now();
    let result = store
        .client()
        .media_image(inline_fallback_params(params))
        .await;
    (result, started.elapsed())
}

async fn fetch_media_image_payload(
    store: &Arc<Store>,
    key: &MediaKey,
    mut params: MediaImageParams,
    request_local_path: bool,
) -> Result<FetchedMediaImage, ClientError> {
    let (request_local_path, probe_guard) = if request_local_path {
        match acquire_local_path_request_permit().await {
            LocalPathRequestPermit::Inline => (false, None),
            LocalPathRequestPermit::Request(probe) => (true, probe),
        }
    } else {
        (false, None)
    };
    if request_local_path {
        params.delivery = Some(MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string());
    }

    let rpc_started = Instant::now();
    let first = store.client().media_image(params.clone()).await;
    let mut rpc_duration = rpc_started.elapsed();
    let mut path_read_duration = Duration::ZERO;

    let image = match first {
        Ok(image) => {
            if probe_guard.is_some() {
                LOCAL_PATH_REQUESTS_CONFIRMED.store(true, Ordering::Release);
            }
            drop(probe_guard);
            image
        }
        Err(error) if request_local_path && is_unsupported_local_path_error(&error.message) => {
            // Publish rejection before releasing the probe gate so every
            // waiting worker switches directly to legacy inline delivery.
            LOCAL_PATH_REQUESTS_DISABLED.store(true, Ordering::Release);
            drop(probe_guard);
            warn!(
                system_id = %key.system_id,
                path = %key.path,
                "media_image_cache: Core rejected local-path delivery; using inline for this session"
            );
            let (fallback, fallback_duration) =
                fetch_inline_media_image(store, params.clone()).await;
            rpc_duration += fallback_duration;
            fallback?
        }
        Err(error) => {
            drop(probe_guard);
            return Err(error);
        }
    };

    if image.delivery != MEDIA_IMAGE_DELIVERY_LOCAL_PATH {
        return Ok(FetchedMediaImage {
            image,
            local_bytes: None,
            rpc_duration,
            path_read_duration,
        });
    }

    let path = image.local_path.clone().filter(|path| !path.is_empty());
    if let Some(path) = path {
        let read_started = Instant::now();
        let read_result = read_local_image(path.clone()).await;
        path_read_duration = read_started.elapsed();
        match read_result {
            Ok(bytes) => {
                return Ok(FetchedMediaImage {
                    image,
                    local_bytes: Some(bytes),
                    rpc_duration,
                    path_read_duration,
                });
            }
            Err(error) => warn!(
                system_id = %key.system_id,
                media_path = %key.path,
                local_path = %path,
                "media_image_cache: local thumbnail read failed, retrying inline: {error}"
            ),
        }
    } else {
        warn!(
            system_id = %key.system_id,
            path = %key.path,
            "media_image_cache: local-path response omitted localPath, retrying inline"
        );
    }

    let (fallback, fallback_duration) = fetch_inline_media_image(store, params).await;
    rpc_duration += fallback_duration;
    Ok(FetchedMediaImage {
        image: fallback?,
        local_bytes: None,
        rpc_duration,
        path_read_duration,
    })
}

/// Fetch one media image. Queue fan-out happens entirely in this driver;
/// local-path read failures may issue one documented inline fallback request.
async fn fetch_one(
    store: &Arc<Store>,
    state: &Arc<RwLock<CacheState>>,
    max_cover_size: &Arc<AtomicU32>,
    entry: QueueEntry,
) -> (QueueEntry, FetchOutcome) {
    let key = entry.key.clone();
    let queue_wait = entry.enqueued_at.elapsed();
    let (mut params, had_id_hint) = {
        #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
        let guard = state.read().unwrap();
        match guard.media_ids.get(&key).copied() {
            Some(id) => (MediaImageParams::for_media_id(id), true),
            None => (
                MediaImageParams::for_media(key.system_id.as_ref(), key.path.as_ref()),
                false,
            ),
        }
    };
    if let Some(preference) = key.cover_preference() {
        params.image_types = preferred_image_types(preference);
    } else if let Some(image_type) = key.image_type.as_ref() {
        params.image_types.push(image_type.to_string());
    }
    // A size baked into the key (e.g. the detail pane's larger request)
    // wins; otherwise fall back to the global default the grid drives.
    let max_size = if key.max_size > 0 {
        key.max_size
    } else {
        max_cover_size.load(Ordering::Relaxed)
    };
    if max_size > 0 {
        params.max_size = Some(max_size);
    }
    let request_local_path = should_request_local_path(max_size);
    debug!(
        system_id = %key.system_id,
        path = %key.path,
        media_id = ?params.media_id,
        had_id_hint,
        policy = ?entry.no_image_policy,
        page_size = entry.page_size,
        max_size,
        request_local_path,
        "media_image_cache: media.image request"
    );
    let fetch_started = Instant::now();
    let result = fetch_media_image_payload(store, &key, params, request_local_path).await;
    let fetch_duration = fetch_started.elapsed();
    let (outcome, decode_duration, rpc_duration, path_read_duration) = match result {
        Ok(payload) => {
            let (outcome, decode_duration) = match payload.local_bytes {
                Some(bytes) => (
                    classify_media_image_bytes(&key, &payload.image, bytes),
                    Duration::ZERO,
                ),
                None => classify_media_image_result(&key, &payload.image),
            };
            (
                outcome,
                decode_duration,
                payload.rpc_duration,
                payload.path_read_duration,
            )
        }
        Err(e) => {
            let outcome = classify_single_media_image_error(&key, &e.message, had_id_hint);
            if matches!(outcome, FetchOutcome::Transient) && had_id_hint {
                #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
                state.write().unwrap().media_ids.remove(&key);
            }
            (outcome, Duration::ZERO, fetch_duration, Duration::ZERO)
        }
    };
    debug!(
        system_id = %key.system_id,
        path = %key.path,
        outcome = fetch_outcome_label(&outcome),
        queue_wait_ms = queue_wait.as_millis(),
        fetch_ms = fetch_duration.as_millis(),
        rpc_ms = rpc_duration.as_millis(),
        path_read_ms = path_read_duration.as_millis(),
        decode_ms = decode_duration.as_millis(),
        "media_image_cache: cover timing",
    );
    (entry, outcome)
}

fn classify_single_media_image_error(
    key: &MediaKey,
    message: &str,
    had_id_hint: bool,
) -> FetchOutcome {
    if is_connection_down_error(message) {
        info!(
            system_id = %key.system_id,
            path = %key.path,
            "media_image_cache: media.image failed while connection down: {message}",
        );
        return FetchOutcome::ConnectionDown;
    }
    if is_stable_media_image_miss(message) {
        debug!(
            system_id = %key.system_id,
            path = %key.path,
            had_id_hint,
            "media_image_cache: media.image stable miss: {message}",
        );
        return FetchOutcome::NoImage;
    }
    if had_id_hint && !key.system_id.is_empty() && !key.path.is_empty() {
        debug!(
            system_id = %key.system_id,
            path = %key.path,
            media_id = ?key.media_id,
            "media_image_cache: media.image error with media_id hint: {message} (transient, will retry with system/path)",
        );
        return FetchOutcome::Transient;
    }
    info!(
        system_id = %key.system_id,
        path = %key.path,
        "media_image_cache: media.image failed: {message} (transient, will retry within attempt budget)",
    );
    FetchOutcome::Transient
}

fn is_stable_media_image_miss(message: &str) -> bool {
    message.contains("no image found for media")
        || message.contains("system not found:")
        || message.contains("media not found:")
        || message.contains("media.image: image file too large")
        || message.contains("media.image: image blob too large")
        || (message.contains("media binary") && message.contains("is too large"))
}

fn classify_media_image_result(
    key: &MediaKey,
    image: &MediaImageResult,
) -> (FetchOutcome, Duration) {
    let decode_started = Instant::now();
    let bytes = match BASE64_STANDARD.decode(image.data.as_bytes()) {
        Ok(b) => b,
        Err(e) => {
            let decode_duration = decode_started.elapsed();
            warn!(
                system_id = %key.system_id,
                path = %key.path,
                "media_image_cache: base64 decode failed: {e} (transient, will retry on next enqueue)",
            );
            return (FetchOutcome::Transient, decode_duration);
        }
    };
    let decode_duration = decode_started.elapsed();
    (
        classify_media_image_bytes(key, image, bytes),
        decode_duration,
    )
}

fn classify_media_image_bytes(
    key: &MediaKey,
    image: &MediaImageResult,
    bytes: Vec<u8>,
) -> FetchOutcome {
    if bytes.is_empty() {
        warn!(
            system_id = %key.system_id,
            path = %key.path,
            "media_image_cache: media.image returned 0 bytes, treating as no image",
        );
        return FetchOutcome::NoImage;
    }
    let ext = image
        .extension
        .as_deref()
        .and_then(ext_from_extension_field)
        .or_else(|| ext_for_content_type(&image.content_type));
    let Some(ext) = ext else {
        warn!(
            system_id = %key.system_id,
            path = %key.path,
            extension = ?image.extension,
            content_type = image.content_type,
            bytes_len = bytes.len(),
            "media_image_cache: unsupported extension/content_type, skipping cache",
        );
        return FetchOutcome::NoImage;
    };
    // Strip the canonical prefix so the stored value is the bare type
    // name (e.g. "boxart"), matching MediaKey::image_type values used by
    // the carousel. Older Core versions return "" for type_tag.
    let type_tag = image
        .type_tag
        .strip_prefix("property:image-")
        .unwrap_or("")
        .to_string();
    FetchOutcome::Success {
        bytes,
        ext,
        type_tag,
    }
}

fn finish_fetch(
    state: &Arc<RwLock<CacheState>>,
    cap_bytes: usize,
    key: &MediaKey,
    outcome: FetchOutcome,
    no_image_policy: NoImagePolicy,
) -> Option<MediaImageUpdate> {
    #[allow(clippy::unwrap_used, reason = "RwLock poisoning is unrecoverable")]
    let mut guard = state.write().unwrap();
    guard.pending.remove(key);
    let search_seen = guard.search_seen.contains(key);
    match outcome {
        FetchOutcome::Success {
            bytes,
            ext,
            type_tag,
        } => {
            if bytes.len() > cap_bytes {
                warn!(
                    system_id = %key.system_id,
                    path = %key.path,
                    bytes = bytes.len(),
                    cap_bytes,
                    "media_image_cache: payload exceeds cache cap, recording as negative",
                );
                if no_image_policy == NoImagePolicy::Memoize && !search_seen {
                    if let Some(prev) = guard.remove_entry(key) {
                        guard.total_bytes = guard.total_bytes.saturating_sub(prev.bytes.len());
                    }
                    // Remove unconditionally: media_ids is written in
                    // enqueue_with_policy before the fetch result arrives, so it
                    // may exist even when map had no entry.
                    guard.media_ids.remove(key);
                    guard.resolved_types.remove(key);
                    guard.soft_no_image.remove(key);
                    guard.negative.insert(key.clone());
                } else if !guard.map.contains_key(key) {
                    guard.soft_no_image.insert(key.clone());
                }
                return Some(MediaImageUpdate {
                    key: key.clone(),
                    ext: None,
                });
            }
            let bytes_len = bytes.len();
            let next = guard.next_clock();
            let entry = MediaImageEntry {
                bytes: Arc::new(bytes),
                ext,
                last_used: next,
                read: false,
            };
            if let Some(prev) = guard.insert_entry(key.clone(), entry) {
                guard.total_bytes = guard.total_bytes.saturating_sub(prev.bytes.len());
            }
            // Record the resolved type for carousel dedup. Non-empty only
            // when Core populates type_tag (>=v0.7). Old Core versions get
            // no dedup — the carousel may still show a duplicate in that
            // case, which is no worse than today.
            if type_tag.is_empty() {
                // Empty type_tag means old Core; clear any stale entry from a
                // previous fetch so resolved_image_type() doesn't deduplicate
                // against outdated type information.
                guard.resolved_types.remove(key);
            } else {
                guard.resolved_types.insert(key.clone(), type_tag);
            }
            guard.soft_no_image.remove(key);
            guard.total_bytes = guard.total_bytes.saturating_add(bytes_len);
            guard.evict_until_fits(cap_bytes);
            Some(MediaImageUpdate {
                key: key.clone(),
                ext: Some(ext),
            })
        }
        FetchOutcome::NoImage => {
            if no_image_policy == NoImagePolicy::Memoize && !search_seen {
                if let Some(prev) = guard.remove_entry(key) {
                    guard.total_bytes = guard.total_bytes.saturating_sub(prev.bytes.len());
                }
                guard.media_ids.remove(key);
                guard.resolved_types.remove(key);
                guard.soft_no_image.remove(key);
                guard.negative.insert(key.clone());
            } else if !guard.map.contains_key(key) {
                guard.soft_no_image.insert(key.clone());
            }
            Some(MediaImageUpdate {
                key: key.clone(),
                ext: None,
            })
        }
        // Transient: drop the pending guard and let the bounded retry
        // path decide whether to requeue. No map insert, no negative
        // memo, no broadcast — the row's `coverKey` is unchanged so
        // subscribers have nothing to act on.
        FetchOutcome::Transient | FetchOutcome::ConnectionDown => None,
    }
}

fn is_connection_down_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("disconnected")
        || lower.contains("not connected")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("channel closed")
}

static GLOBAL_MEDIA_IMAGE_CACHE: OnceLock<Arc<MediaImageCache>> = OnceLock::new();

/// Lazily initialise the process-wide media image cache and return a
/// handle.
/// Constructed on first call from any thread; subsequent calls return
/// the same `Arc` so subscribers see the same broadcast channel.
pub fn global_media_image_cache() -> Arc<MediaImageCache> {
    GLOBAL_MEDIA_IMAGE_CACHE
        .get_or_init(|| {
            let runtime = crate::models::global_handle();
            let cache =
                MediaImageCache::new(CACHE_CAP_BYTES, &runtime, crate::models::global_store);
            Arc::new(cache)
        })
        .clone()
}

/// C ABI bridge to the `QQuickImageProvider` on the C++ side. The
/// provider passes the URL id (the bit after `image://media-image/`) and a
/// callback that copies bytes into a `QByteArray`. The callback is
/// invoked exactly once with the bytes (or with an empty slice when the
/// key has no cached entry).
///
/// # Safety
///
/// `encoded` must point to `encoded_len` bytes that remain live
/// for the duration of this call. UTF-8 validity is checked
/// internally — non-UTF-8 input is reported via the warn log and
/// returns no bytes. `callback` is invoked exactly once before
/// this function returns; the `data` pointer it receives is valid
/// for the duration of the callback only.
#[no_mangle]
pub unsafe extern "C" fn zaparoo_media_image_bytes_for(
    encoded: *const c_char,
    encoded_len: usize,
    callback: extern "C" fn(user_data: *mut c_void, data: *const u8, len: usize),
    user_data: *mut c_void,
) {
    if encoded.is_null() {
        callback(user_data, std::ptr::null(), 0);
        return;
    }
    // SAFETY: caller guarantees `encoded` points to `encoded_len`
    // bytes live for this call. Qt's `QString::toUtf8()` is documented
    // to produce valid UTF-8, so `from_utf8` should always succeed —
    // but validate anyway to keep this FFI seam free of UB if a future
    // caller or a Qt regression ever sends bad bytes.
    let encoded_bytes = unsafe { std::slice::from_raw_parts(encoded.cast::<u8>(), encoded_len) };
    let Ok(encoded_str) = std::str::from_utf8(encoded_bytes) else {
        warn!(
            encoded_len,
            "media_image_cache: provider id is not valid UTF-8 (Qt invariant violated)"
        );
        callback(user_data, std::ptr::null(), 0);
        return;
    };
    let Some(key) = MediaKey::decode(encoded_str) else {
        warn!(
            encoded_len,
            "media_image_cache: MediaKey::decode failed (malformed image://media-image/ id)"
        );
        callback(user_data, std::ptr::null(), 0);
        return;
    };
    let cache = global_media_image_cache();
    if let Some(bytes) = cache.get_bytes(&key) {
        debug!(
            system_id = %key.system_id,
            path = %key.path,
            cache_hit = true,
            bytes_len = bytes.len(),
            "media_image_cache: provider lookup",
        );
        callback(user_data, bytes.as_ptr(), bytes.len());
    } else {
        debug!(
            system_id = %key.system_id,
            path = %key.path,
            cache_hit = false,
            "media_image_cache: provider lookup",
        );
        callback(user_data, std::ptr::null(), 0);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        classify_media_image_bytes, classify_single_media_image_error, ext_for_content_type,
        ext_from_extension_field, finish_fetch, inline_fallback_params, is_connection_down_error,
        is_unsupported_local_path_error, local_path_request_allowed, pop_one,
        process_batch_outcomes, read_local_image, read_local_image_file, CacheState, FetchOutcome,
        MediaImageCache, MediaImageUpdate, MediaKey, NegativeMemo, NoImagePolicy, QueueEntry,
        MAX_QUEUE_LEN, NEGATIVE_MEMO_CAP,
    };
    use std::collections::VecDeque;
    use std::io::Write as _;
    use std::sync::atomic::AtomicU32;
    use std::sync::{Arc, Mutex, RwLock};
    use std::time::Instant;
    use tokio::runtime::Builder;
    use tokio::sync::{broadcast, Notify};
    use zaparoo_core::media_types::{
        MediaImageParams, MediaImageResult, MEDIA_IMAGE_DELIVERY_LOCAL_PATH,
    };

    /// Build a `MediaImageCache` without spawning the fetch driver.
    /// Lets tests exercise `enqueue` / `is_cached` / `is_negative`
    /// against the public surface without needing a tokio runtime or
    /// a live `Store`. The driver-less queue accumulates indefinitely
    /// (no consumer), which is exactly what these tests want.
    fn cache_for_test() -> MediaImageCache {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let queue: Arc<Mutex<VecDeque<QueueEntry>>> = Arc::new(Mutex::new(VecDeque::new()));
        let queue_notify = Arc::new(Notify::new());
        let (updates_tx, _) = broadcast::channel::<MediaImageUpdate>(64);
        MediaImageCache {
            state,
            queue,
            queue_notify,
            updates_tx,
            max_cover_size: Arc::new(AtomicU32::new(0)),
        }
    }

    #[test]
    fn local_path_delivery_requires_colocated_mister_core() {
        assert!(local_path_request_allowed(256, true, true, false));
        assert!(!local_path_request_allowed(0, true, true, false));
        assert!(!local_path_request_allowed(256, false, true, false));
        assert!(!local_path_request_allowed(256, true, false, false));
        assert!(!local_path_request_allowed(256, true, true, true));
    }

    #[test]
    fn clear_pending_requests_drops_queue_and_releases_pending_keys() {
        let cache = cache_for_test();
        let first = key("SNES", "/old");
        let second = key("NES", "/new");
        cache.enqueue_with_media_id(first.clone(), None, 1);
        cache.enqueue_with_media_id(second.clone(), None, 1);

        cache.clear_pending_requests();

        assert!(cache.queue.lock().unwrap().is_empty());
        let guard = cache.state.read().unwrap();
        assert!(!guard.pending.contains(&first));
        assert!(!guard.pending.contains(&second));
        drop(guard);
        cache.enqueue_with_media_id(first.clone(), None, 1);
        assert_eq!(cache.queue.lock().unwrap().len(), 1);
    }

    #[test]
    fn replace_pending_requests_ordered_drops_stale_and_preserves_public_order() {
        let cache = cache_for_test();
        let stale = key("NES", "/stale");
        let first = key("NES", "/first");
        let second = key("NES", "/second");
        let third = key("NES", "/third");
        cache.enqueue_with_media_id(stale.clone(), None, 1);

        cache.replace_pending_requests_ordered(
            vec![
                (first.clone(), None),
                (second.clone(), None),
                (third.clone(), None),
            ],
            1,
        );

        let guard = cache.state.read().unwrap();
        assert!(!guard.pending.contains(&stale));
        assert!(guard.pending.contains(&first));
        assert!(guard.pending.contains(&second));
        assert!(guard.pending.contains(&third));
        drop(guard);

        assert_eq!(pop_one(&cache.queue).expect("first").key, first);
        assert_eq!(pop_one(&cache.queue).expect("second").key, second);
        assert_eq!(pop_one(&cache.queue).expect("third").key, third);
        assert!(pop_one(&cache.queue).is_none());
    }

    #[test]
    fn invalidate_negative_memo_for_system_clears_only_the_matching_system() {
        let cache = cache_for_test();
        let stale_snes = key("SNES", "/a");
        let stale_snes2 = key("SNES", "/b");
        let unrelated_nes = key("NES", "/c");
        {
            let mut guard = cache.state.write().unwrap();
            guard.negative.insert(stale_snes.clone());
            guard.negative.insert(stale_snes2.clone());
            guard.negative.insert(unrelated_nes.clone());
            guard.soft_no_image.insert(stale_snes.clone());
            guard.search_seen.insert(stale_snes.clone());
        }

        cache.invalidate_negative_memo_for_system("SNES");

        let guard = cache.state.read().unwrap();
        assert!(!guard.negative.contains(&stale_snes));
        assert!(!guard.negative.contains(&stale_snes2));
        assert!(!guard.soft_no_image.contains(&stale_snes));
        assert!(!guard.search_seen.contains(&stale_snes));
        // A different system's memoized "no image" answer is untouched --
        // this is not a blunt full clear.
        assert!(guard.negative.contains(&unrelated_nes));
    }

    #[test]
    fn invalidate_negative_memo_for_system_is_a_noop_for_empty_system_id() {
        let cache = cache_for_test();
        let k = key("SNES", "/a");
        cache.state.write().unwrap().negative.insert(k.clone());

        cache.invalidate_negative_memo_for_system("");

        assert!(cache.state.read().unwrap().negative.contains(&k));
    }

    #[test]
    fn invalidate_negative_memo_all_clears_every_system() {
        let cache = cache_for_test();
        let snes = key("SNES", "/a");
        let nes = key("NES", "/b");
        {
            let mut guard = cache.state.write().unwrap();
            guard.negative.insert(snes.clone());
            guard.negative.insert(nes.clone());
            guard.soft_no_image.insert(snes.clone());
            guard.search_seen.insert(nes.clone());
        }

        cache.invalidate_negative_memo_all();

        let guard = cache.state.read().unwrap();
        assert!(!guard.negative.contains(&snes));
        assert!(!guard.negative.contains(&nes));
        assert!(!guard.soft_no_image.contains(&snes));
        assert!(!guard.search_seen.contains(&nes));
    }

    #[test]
    fn ext_for_content_type_handles_known_mimes() {
        assert_eq!(ext_for_content_type("image/png"), Some("png"));
        assert_eq!(ext_for_content_type("image/PNG"), Some("png"));
        assert_eq!(ext_for_content_type("image/jpeg"), Some("jpg"));
        assert_eq!(ext_for_content_type("image/jpg"), Some("jpg"));
        assert_eq!(ext_for_content_type("image/webp"), Some("webp"));
    }

    #[test]
    fn ext_for_content_type_strips_charset_suffix() {
        assert_eq!(
            ext_for_content_type("image/png; charset=binary"),
            Some("png")
        );
    }

    #[test]
    fn ext_for_content_type_rejects_unsupported() {
        assert_eq!(ext_for_content_type("image/gif"), None);
        assert_eq!(ext_for_content_type("application/octet-stream"), None);
        assert_eq!(ext_for_content_type(""), None);
    }

    #[test]
    fn ext_from_extension_field_normalises_aliases() {
        assert_eq!(ext_from_extension_field("png"), Some("png"));
        assert_eq!(ext_from_extension_field("PNG"), Some("png"));
        assert_eq!(ext_from_extension_field(".png"), Some("png"));
        assert_eq!(ext_from_extension_field("jpg"), Some("jpg"));
        assert_eq!(ext_from_extension_field("jpeg"), Some("jpg"));
        assert_eq!(ext_from_extension_field("JPEG"), Some("jpg"));
        assert_eq!(ext_from_extension_field("webp"), Some("webp"));
    }

    #[test]
    fn ext_from_extension_field_rejects_unsupported() {
        assert_eq!(ext_from_extension_field("gif"), None);
        assert_eq!(ext_from_extension_field("bmp"), None);
        assert_eq!(ext_from_extension_field(""), None);
        assert_eq!(ext_from_extension_field("."), None);
    }

    #[test]
    fn media_key_round_trips_through_url_encoding() {
        // Path with slashes, punctuation, and unicode — all of which
        // would corrupt a naive `system|path` encoding without proper
        // base64.
        let key = MediaKey::new("SNES", "/roms/snes/Super Mario World (USA).sfc");
        let encoded = key.encode();
        // No padding, only URL-safe chars, no separators that would
        // confuse a single-segment URL path.
        assert!(!encoded.contains('='), "no padding: {encoded}");
        assert!(!encoded.contains('+'), "url-safe: {encoded}");
        assert!(!encoded.contains('/'), "url-safe: {encoded}");
        let decoded = MediaKey::decode(&encoded).expect("round-trip");
        assert_eq!(decoded, key);
    }

    #[test]
    fn media_key_handles_paths_with_separator_byte() {
        // Defence in depth: a path containing 0x1F should not corrupt
        // the split — base64 decodes the original bytes back exactly,
        // and we split on the *first* separator (the one we inserted).
        let path_with_us = format!("/x/{}/y", char::from(0x1F));
        let key = MediaKey::new("SNES", path_with_us.as_str());
        let decoded = MediaKey::decode(&key.encode()).expect("round-trip");
        assert_eq!(decoded.system_id.as_ref(), "SNES");
        assert_eq!(decoded.path.as_ref(), path_with_us.as_str());
    }

    #[test]
    fn image_key_for_returns_media_image_prefix() {
        let key = MediaKey::new("SNES", "/p");
        let s = MediaImageCache::image_key_for(&key);
        assert!(s.starts_with("media-image/"), "got {s}");
        let encoded = &s["media-image/".len()..];
        let back = MediaKey::decode(encoded).expect("decode");
        assert_eq!(back, key);
    }

    #[test]
    fn media_key_round_trips_image_type() {
        let key = MediaKey::with_image_type("SNES", "/p", "screenshot");
        let decoded = MediaKey::decode(&key.encode()).expect("round-trip");
        assert_eq!(decoded, key);
        assert_eq!(decoded.image_type.as_deref(), Some("screenshot"));
    }

    #[test]
    fn media_key_round_trips_media_id_and_image_type() {
        let key = MediaKey::with_media_id_and_image_type("SNES", "/p", 42, "boxart");
        let decoded = MediaKey::decode(&key.encode()).expect("round-trip");
        assert_eq!(decoded, key);
        assert_eq!(decoded.media_id, Some(42));
        assert_eq!(decoded.system_id.as_ref(), "SNES");
        assert_eq!(decoded.path.as_ref(), "/p");
        assert_eq!(decoded.image_type.as_deref(), Some("boxart"));
    }

    #[test]
    fn media_key_round_trips_max_size() {
        // Sized key with a media_id, image_type, and a slashed path.
        let key = MediaKey::with_media_id("SNES", "/roms/snes/Super Mario.sfc", 42)
            .with_current_cover_preference()
            .with_max_size(768);
        let encoded = key.encode();
        assert!(!encoded.contains('='), "no padding: {encoded}");
        let decoded = MediaKey::decode(&encoded).expect("round-trip");
        assert_eq!(decoded, key);
        assert_eq!(decoded.max_size, 768);
        assert_eq!(decoded.media_id, Some(42));
    }

    #[test]
    fn media_key_round_trips_max_size_without_media_id() {
        // v4 must carry the empty-id form for keys that have no media_id.
        let key = MediaKey::new("Arcade", "/media/fat/_Arcade/game.mra").with_max_size(256);
        let decoded = MediaKey::decode(&key.encode()).expect("round-trip");
        assert_eq!(decoded, key);
        assert_eq!(decoded.max_size, 256);
        assert_eq!(decoded.media_id, None);
        assert_eq!(decoded.system_id.as_ref(), "Arcade");
        assert_eq!(decoded.path.as_ref(), "/media/fat/_Arcade/game.mra");
    }

    #[test]
    fn distinct_max_size_are_distinct_keys() {
        // The grid (unsized / one size) and detail pane (larger size) must not
        // collide in the cache map: differing max_size => not equal, and the
        // hashes must agree with eq for the HashMap contract.
        let grid = MediaKey::with_media_id("SNES", "/p", 7).with_max_size(256);
        let detail = MediaKey::with_media_id("SNES", "/p", 7).with_max_size(768);
        assert_ne!(grid, detail);

        let mut map = std::collections::HashMap::new();
        map.insert(grid.clone(), "grid");
        map.insert(detail.clone(), "detail");
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&grid), Some(&"grid"));
        assert_eq!(map.get(&detail), Some(&"detail"));

        // Same identity + same size still collapses to one entry.
        let detail_again = MediaKey::with_media_id("SNES", "/p", 7).with_max_size(768);
        assert_eq!(detail, detail_again);
        assert_eq!(map.get(&detail_again), Some(&"detail"));
    }

    #[test]
    fn unsized_key_encodes_without_v4_prefix() {
        // max_size == 0 must keep the legacy v3 encoding so existing keys and
        // their URLs are byte-identical to before this field existed.
        let plain = MediaKey::with_media_id("SNES", "/p", 7);
        let sized = MediaKey::with_media_id("SNES", "/p", 7).with_max_size(512);
        assert_ne!(plain.encode(), sized.encode());
        // The unsized encoding still decodes back to an unsized key.
        let back = MediaKey::decode(&plain.encode()).expect("round-trip");
        assert_eq!(back.max_size, 0);
        assert_eq!(back, plain);
    }

    #[test]
    fn stable_media_image_miss_with_id_hint_is_not_retried() {
        let key = MediaKey::with_media_id("Arcade", "/media/fat/_Arcade/Computer Space.mra", 14912);

        let outcome = classify_single_media_image_error(
            &key,
            "no image found for media: system=\"Arcade\" path=\"/media/fat/_Arcade/Computer Space.mra\"",
            true,
        );

        assert!(matches!(outcome, FetchOutcome::NoImage));
    }

    #[test]
    fn non_stable_media_id_error_still_retries_without_hint() {
        let key = MediaKey::with_media_id("SNES", "/p", 42);

        let outcome = classify_single_media_image_error(&key, "stale media id", true);

        assert!(matches!(outcome, FetchOutcome::Transient));
    }

    #[test]
    fn connection_down_error_does_not_retry_immediately() {
        let key = MediaKey::with_media_id("SNES", "/p", 42);

        let outcome = classify_single_media_image_error(&key, "connection reset by peer", true);

        assert!(matches!(outcome, FetchOutcome::ConnectionDown));
    }

    #[test]
    fn unsupported_delivery_errors_are_detected_narrowly() {
        assert!(is_unsupported_local_path_error(
            "invalid params: json: unknown field delivery"
        ));
        assert!(is_unsupported_local_path_error(
            "media.image: unsupported delivery localPath"
        ));
        assert!(!is_unsupported_local_path_error("connection reset by peer"));
        assert!(!is_unsupported_local_path_error("stale media id"));
    }

    #[test]
    fn legacy_inline_fallback_omits_rejected_delivery_field() {
        let params = inline_fallback_params(MediaImageParams {
            delivery: Some(MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string()),
            ..MediaImageParams::default()
        });
        assert!(params.delivery.is_none());
    }

    #[test]
    fn local_path_bytes_use_existing_image_validation() {
        let key = MediaKey::new("SNES", "/p");
        let image = MediaImageResult {
            delivery: MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string(),
            content_type: "image/webp".to_string(),
            extension: Some("webp".to_string()),
            type_tag: "property:image-boxart".to_string(),
            ..MediaImageResult::default()
        };
        let outcome = classify_media_image_bytes(&key, &image, vec![1, 2, 3]);
        assert!(matches!(
            outcome,
            FetchOutcome::Success {
                ext: "webp",
                type_tag,
                ..
            } if type_tag == "boxart"
        ));
    }

    #[test]
    fn local_path_read_accepts_bounded_regular_files() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(&[1, 2, 3]).expect("write temp image");
        let existing = file.path().to_string_lossy().into_owned();
        let missing = file
            .path()
            .with_extension("missing")
            .to_string_lossy()
            .into_owned();
        let runtime = Builder::new_current_thread().build().expect("runtime");

        assert_eq!(
            runtime.block_on(read_local_image(existing)).expect("read"),
            vec![1, 2, 3]
        );
        assert!(runtime.block_on(read_local_image(missing)).is_err());
    }

    #[test]
    fn local_path_read_rejects_oversized_and_non_regular_paths() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(&[1, 2, 3]).expect("write temp image");
        let path = file.path().to_string_lossy();
        assert!(read_local_image_file(&path, 2)
            .expect_err("oversized file must fail")
            .contains("exceeds"));

        let directory = tempfile::tempdir().expect("temp directory");
        let directory_path = directory.path().to_string_lossy();
        assert_eq!(
            read_local_image_file(&directory_path, 16).expect_err("directory must fail"),
            "thumbnail path is not a regular file"
        );
    }

    fn key(s: &str, p: &str) -> MediaKey {
        MediaKey::new(s, p)
    }

    #[test]
    fn finish_fetch_success_records_and_clears_pending() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/p");
        {
            let mut guard = state.write().unwrap();
            guard.pending.insert(k.clone());
            guard.soft_no_image.insert(k.clone());
        }
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::Success {
                bytes: vec![1, 2, 3],
                ext: "png",
                type_tag: String::new(),
            },
            NoImagePolicy::Memoize,
        )
        .expect("Success returns Some(update)");
        assert_eq!(update.key, k);
        assert_eq!(update.ext, Some("png"));
        let guard = state.read().unwrap();
        assert!(guard.map.contains_key(&k));
        assert_eq!(guard.total_bytes, 3);
        assert!(!guard.pending.contains(&k));
        assert!(!guard.negative.contains(&k));
        assert!(!guard.soft_no_image.contains(&k));
    }

    #[test]
    fn no_image_outcome_records_negative_without_map_entry() {
        // The decode path short-circuits to `FetchOutcome::NoImage`
        // when base64 decoding yields zero bytes — a defensive guard
        // against any future Core regression that lets empty payloads
        // through. We can't drive the decoder directly without a live Store, so exercise the
        // downstream contract: `NoImage` → negative memo, no map
        // entry, pending cleared. This locks in the behaviour that
        // empty payloads do not pollute the cache and the
        // `(system_id, path)` is suppressed from refetch.
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/empty");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        )
        .expect("NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(
            !guard.map.contains_key(&k),
            "empty bytes must not enter the cache map"
        );
        assert_eq!(guard.total_bytes, 0);
        assert!(!guard.pending.contains(&k));
        assert!(
            guard.negative.contains(&k),
            "empty bytes must be absorbed by the negative memo so we do not refetch"
        );
    }

    #[test]
    fn finish_fetch_no_image_records_negative() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/p");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        )
        .expect("NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(!guard.map.contains_key(&k));
        assert!(!guard.pending.contains(&k));
        assert!(guard.negative.contains(&k));
    }

    #[test]
    fn memoized_no_image_removes_cached_bytes_and_media_id_hint() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/stale");
        ok_png(&state, usize::MAX, &k, 4);
        {
            let mut guard = state.write().unwrap();
            guard.media_ids.insert(k.clone(), 42);
            guard.pending.insert(k.clone());
        }
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        )
        .expect("NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(!guard.map.contains_key(&k));
        assert_eq!(guard.total_bytes, 0);
        assert!(!guard.media_ids.contains_key(&k));
        assert!(!guard.pending.contains(&k));
        assert!(guard.negative.contains(&k));
        assert!(!guard.soft_no_image.contains(&k));
    }

    #[test]
    fn protected_memoized_no_image_records_soft_memo_without_global_negative() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/favorite-strict-miss");
        {
            let mut guard = state.write().unwrap();
            guard.pending.insert(k.clone());
            guard.search_seen.insert(k.clone());
        }
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        )
        .expect("protected strict NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(!guard.map.contains_key(&k));
        assert!(!guard.pending.contains(&k));
        assert!(!guard.negative.contains(&k));
        assert!(guard.soft_no_image.contains(&k));
        assert!(guard.search_seen.contains(&k));
    }

    #[test]
    fn protected_memoized_no_image_preserves_cached_bytes() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/favorite-cached");
        ok_png(&state, usize::MAX, &k, 4);
        {
            let mut guard = state.write().unwrap();
            guard.pending.insert(k.clone());
            guard.search_seen.insert(k.clone());
        }
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        )
        .expect("protected strict NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(guard.map.contains_key(&k));
        assert_eq!(guard.total_bytes, 4);
        assert!(!guard.pending.contains(&k));
        assert!(!guard.negative.contains(&k));
        assert!(!guard.soft_no_image.contains(&k));
    }

    #[test]
    fn soft_no_image_records_soft_memo_without_global_negative() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/search-miss");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::SoftMiss,
        )
        .expect("soft NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(!guard.map.contains_key(&k));
        assert!(!guard.pending.contains(&k));
        assert!(!guard.negative.contains(&k));
        assert!(guard.soft_no_image.contains(&k));
    }

    #[test]
    fn soft_no_image_preserves_cached_bytes_and_skips_negative_memo() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/search-row");
        ok_png(&state, usize::MAX, &k, 4);
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::NoImage,
            NoImagePolicy::SoftMiss,
        )
        .expect("soft NoImage returns Some(update)");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none());
        let guard = state.read().unwrap();
        assert!(guard.map.contains_key(&k));
        assert_eq!(guard.total_bytes, 4);
        assert!(!guard.pending.contains(&k));
        assert!(!guard.negative.contains(&k));
        assert!(!guard.soft_no_image.contains(&k));
    }

    #[test]
    fn connection_down_outcome_only_clears_pending() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/p");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::ConnectionDown,
            NoImagePolicy::Memoize,
        );
        assert!(update.is_none());
        let guard = state.read().unwrap();
        assert!(!guard.pending.contains(&k));
        assert!(!guard.map.contains_key(&k));
        assert!(!guard.negative.contains(&k));
        assert!(!guard.attempts.contains_key(&k));
    }

    #[test]
    fn connection_down_classifier_matches_client_disconnect_messages() {
        assert!(is_connection_down_error("disconnected"));
        assert!(is_connection_down_error("not connected"));
        assert!(is_connection_down_error(
            "IO error: Connection reset by peer"
        ));
        assert!(is_connection_down_error("IO error: Connection refused"));
        assert!(!is_connection_down_error("base64 decode failed"));
    }

    fn ok_png(state: &Arc<RwLock<CacheState>>, cap: usize, k: &MediaKey, n: usize) {
        let _ = finish_fetch(
            state,
            cap,
            k,
            FetchOutcome::Success {
                bytes: vec![0; n],
                ext: "png",
                type_tag: String::new(),
            },
            NoImagePolicy::Memoize,
        );
    }

    // Re-fetching an already-cached, already-*read* key (a real path —
    // e.g. a cover preference change re-requests a key the fetch driver
    // already resolved once) must retire its OLD index entry before
    // installing the new one. A gap here leaves a ghost key in
    // `read_lru`/`unread_lru` pointing at a `last_used` no longer in
    // `map` — `evict_until_fits` would then pick that ghost as its
    // victim forever (`remove_entry` returns `None` for a key that isn't
    // in `map`, so neither `total_bytes` shrinks nor the ghost index
    // entry ever gets removed), hanging on the cache's write lock
    // permanently. This asserts the invariant directly rather than
    // relying on eviction happening to still terminate.
    #[test]
    fn replacing_a_read_entry_retires_its_old_index_slot() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let cap = 10_000;
        let a = key("SNES", "/a");
        ok_png(&state, cap, &a, 100);
        {
            let mut g = state.write().unwrap();
            let next = g.next_clock();
            g.touch_entry(&a, next).expect("a present");
        }
        {
            let g = state.read().unwrap();
            assert_eq!(g.read_lru.len(), 1, "a should be in read_lru after touch");
            assert_eq!(g.unread_lru.len(), 0);
        }

        // Re-fetch the same key — the success path replaces the map
        // entry with a fresh, unread one.
        ok_png(&state, cap, &a, 150);

        let g = state.read().unwrap();
        assert_eq!(g.map.len(), 1);
        assert_eq!(
            g.read_lru.len() + g.unread_lru.len(),
            g.map.len(),
            "index must track map exactly, no stale/ghost slots"
        );
        assert_eq!(g.read_lru.len(), 0, "replacement resets read to false");
        assert_eq!(g.unread_lru.len(), 1);
        assert_eq!(
            g.total_bytes, 150,
            "old byte count fully replaced, not summed"
        );
    }

    #[test]
    fn eviction_drops_oldest_when_over_cap() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        // Cap fits exactly two 100-byte entries; a third must evict
        // the oldest.
        let cap = 200;
        let a = key("SNES", "/a");
        let b = key("SNES", "/b");
        let c = key("SNES", "/c");
        ok_png(&state, cap, &a, 100);
        ok_png(&state, cap, &b, 100);
        // Both fit so far.
        {
            let g = state.read().unwrap();
            assert_eq!(g.map.len(), 2);
            assert_eq!(g.total_bytes, 200);
        }
        ok_png(&state, cap, &c, 100);
        let g = state.read().unwrap();
        assert_eq!(g.map.len(), 2);
        assert_eq!(g.total_bytes, 200);
        // `a` was the oldest insert and has not been touched, so it
        // gets evicted ahead of `b` and `c`.
        assert!(!g.map.contains_key(&a), "a should be evicted");
        assert!(g.map.contains_key(&b));
        assert!(g.map.contains_key(&c));
    }

    #[test]
    fn eviction_respects_recent_get_bumps() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let cap = 200;
        let a = key("SNES", "/a");
        let b = key("SNES", "/b");
        let c = key("SNES", "/c");
        ok_png(&state, cap, &a, 100);
        ok_png(&state, cap, &b, 100);
        // Touch `a`'s `last_used` only, deliberately not `read` -- this
        // isolates within-population LRU ordering (does recency alone
        // pick the right victim among still-unread entries) from the
        // read-vs-unread priority `eviction_prefers_read_entries_over_unread`
        // below already covers; `get_bytes`/`touch_entry` always bump both
        // together, so this reaches into the index directly (same private
        // access the rest of this test module already relies on for
        // `map`/`negative`/etc.) rather than through the public API.
        {
            let mut g = state.write().unwrap();
            let next = g.next_clock();
            let entry = g.map.get_mut(&a).expect("a present");
            let old_last_used = entry.last_used;
            entry.last_used = next;
            g.index_remove(old_last_used, false);
            g.index_insert(next, false, a.clone());
        }
        ok_png(&state, cap, &c, 100);
        let g = state.read().unwrap();
        assert!(g.map.contains_key(&a), "a was touched, should survive");
        assert!(!g.map.contains_key(&b), "b was LRU, should be evicted");
        assert!(g.map.contains_key(&c));
    }

    #[test]
    fn negative_memo_caps_at_4096_with_fifo() {
        let mut memo = NegativeMemo::default();
        // Insert N+5 entries; the first 5 must be dropped FIFO.
        for i in 0..(NEGATIVE_MEMO_CAP + 5) {
            memo.insert(key("SNES", &format!("/p/{i}")));
        }
        assert_eq!(memo.set.len(), NEGATIVE_MEMO_CAP);
        assert_eq!(memo.order.len(), NEGATIVE_MEMO_CAP);
        // First 5 should have been popped.
        for i in 0..5 {
            assert!(
                !memo.contains(&key("SNES", &format!("/p/{i}"))),
                "entry {i} should have been evicted"
            );
        }
        // Last entry is still present.
        assert!(memo.contains(&key("SNES", &format!("/p/{}", NEGATIVE_MEMO_CAP + 4))));
    }

    #[test]
    fn negative_memo_dedupes_duplicate_inserts() {
        let mut memo = NegativeMemo::default();
        let k = key("SNES", "/p");
        memo.insert(k.clone());
        memo.insert(k.clone());
        memo.insert(k.clone());
        assert_eq!(memo.set.len(), 1);
        assert_eq!(memo.order.len(), 1);
    }

    #[test]
    fn eviction_prefers_read_entries_over_unread() {
        // Three unread entries fill the cap exactly. Mark the oldest
        // (`a`) as read; inserting `d` must evict `a` even though it is
        // the only entry QtQuick has consumed, because the unread
        // entries `b` and `c` are still waiting on their first paint
        // pass — dropping them would surface as
        // "Failed to get image from provider".
        let state = Arc::new(RwLock::new(CacheState::new()));
        let cap = 300;
        let a = key("SNES", "/a");
        let b = key("SNES", "/b");
        let c = key("SNES", "/c");
        let d = key("SNES", "/d");
        for k in [&a, &b, &c] {
            ok_png(&state, cap, k, 100);
        }
        // Mark `a` as read via the same `touch_entry` path `get_bytes`
        // itself calls, so this exercises the real production code, not
        // a hand-rolled mirror of it.
        {
            let mut state_w = state.write().unwrap();
            let next = state_w.next_clock();
            state_w.touch_entry(&a, next).expect("a present");
        }
        ok_png(&state, cap, &d, 100);
        let state_r = state.read().unwrap();
        assert!(
            !state_r.map.contains_key(&a),
            "read entry a should be evicted"
        );
        assert!(state_r.map.contains_key(&b), "unread b should be pinned");
        assert!(state_r.map.contains_key(&c), "unread c should be pinned");
        assert!(state_r.map.contains_key(&d));
        assert_eq!(state_r.total_bytes, 300);
    }

    #[test]
    fn eviction_falls_back_to_unread_when_no_reads() {
        // No `get_bytes` calls means every entry stays unread. The
        // two-pass eviction must still make progress via the
        // unread-fallback path; otherwise total_bytes climbs unbounded.
        let state = Arc::new(RwLock::new(CacheState::new()));
        let cap = 200;
        let a = key("SNES", "/a");
        let b = key("SNES", "/b");
        let c = key("SNES", "/c");
        for k in [&a, &b, &c] {
            ok_png(&state, cap, k, 100);
        }
        let g = state.read().unwrap();
        assert_eq!(g.map.len(), 2, "fallback path must evict to fit cap");
        assert_eq!(g.total_bytes, 200);
        // `a` was inserted first and never read, so the fallback
        // (LRU by insert clock) drops it.
        assert!(!g.map.contains_key(&a));
        assert!(g.map.contains_key(&b));
        assert!(g.map.contains_key(&c));
    }

    #[test]
    fn oversize_payload_routes_to_negative_memo() {
        // Payloads larger than the entire cap can never fit; let
        // `finish_fetch` divert them to the negative memo instead of
        // inserting and then thrashing `evict_until_fits` trying to
        // make room. The tile renders fallback text and the
        // `(system_id, path)` is suppressed from refetch this session.
        let state = Arc::new(RwLock::new(CacheState::new()));
        let cap = 100;
        let k = key("SNES", "/huge");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            cap,
            &k,
            FetchOutcome::Success {
                bytes: vec![0; cap + 1],
                ext: "png",
                type_tag: String::new(),
            },
            NoImagePolicy::Memoize,
        )
        .expect("oversize Success returns Some(update) with ext=None");
        assert_eq!(update.key, k);
        assert!(update.ext.is_none(), "oversize must report as no image");
        let g = state.read().unwrap();
        assert!(
            !g.map.contains_key(&k),
            "oversize must not enter the cache map"
        );
        assert_eq!(g.total_bytes, 0);
        assert!(!g.pending.contains(&k), "pending must be cleared");
        assert!(
            g.negative.contains(&k),
            "oversize must be absorbed by the negative memo"
        );
    }

    #[test]
    fn finish_fetch_transient_returns_none_and_clears_pending() {
        // Transient is the "may not repeat" outcome (socket flap, RPC
        // error, base64 corruption). The unit-level contract:
        // `finish_fetch` clears `pending`, does NOT memo, does NOT
        // insert, and returns None — the driver-level retry loop
        // re-inserts `pending` and re-enqueues from there.
        let state = Arc::new(RwLock::new(CacheState::new()));
        let k = key("SNES", "/p");
        state.write().unwrap().pending.insert(k.clone());
        let update = finish_fetch(
            &state,
            usize::MAX,
            &k,
            FetchOutcome::Transient,
            NoImagePolicy::Memoize,
        );
        assert!(update.is_none(), "Transient must not broadcast");
        let g = state.read().unwrap();
        assert!(!g.map.contains_key(&k), "no insert on Transient");
        assert!(!g.negative.contains(&k), "no negative memo on Transient");
        assert!(!g.pending.contains(&k), "pending must be cleared");
        assert_eq!(g.total_bytes, 0);
    }

    #[test]
    fn transient_retry_queues_behind_ordered_window() {
        let state = Arc::new(RwLock::new(CacheState::new()));
        let queue: Arc<Mutex<VecDeque<QueueEntry>>> = Arc::new(Mutex::new(VecDeque::new()));
        let queue_notify = Arc::new(Notify::new());
        let (updates_tx, _) = broadcast::channel::<MediaImageUpdate>(64);
        let first = key("NES", "/first");
        let second = key("NES", "/second");
        let retry = QueueEntry {
            key: key("NES", "/retry"),
            page_size: 15,
            no_image_policy: NoImagePolicy::Memoize,
            enqueued_at: Instant::now(),
        };
        {
            let mut q = queue.lock().unwrap();
            q.push_back(QueueEntry {
                key: second.clone(),
                page_size: 15,
                no_image_policy: NoImagePolicy::Memoize,
                enqueued_at: Instant::now(),
            });
            q.push_back(QueueEntry {
                key: first.clone(),
                page_size: 15,
                no_image_policy: NoImagePolicy::Memoize,
                enqueued_at: Instant::now(),
            });
        }
        state.write().unwrap().pending.insert(retry.key.clone());

        process_batch_outcomes(
            &state,
            usize::MAX,
            &updates_tx,
            &queue,
            &queue_notify,
            vec![(retry.clone(), FetchOutcome::Transient)],
        );

        assert_eq!(pop_one(&queue).expect("first").key, first);
        assert_eq!(pop_one(&queue).expect("second").key, second);
        assert_eq!(pop_one(&queue).expect("retry").key, retry.key);
    }

    #[test]
    fn lifo_drains_newest_first() {
        // The cache's queue is a `VecDeque`; the driver drains via
        // `pop_back`. Exercise that ordering directly: pushing A, B,
        // C must drain as C, B, A so the page the user just landed on
        // is serviced ahead of older enqueues.
        use std::collections::VecDeque;
        let mut q: VecDeque<QueueEntry> = VecDeque::new();
        let a = key("SNES", "/a");
        let b = key("SNES", "/b");
        let c = key("SNES", "/c");
        q.push_back(QueueEntry {
            key: a.clone(),
            page_size: 15,
            no_image_policy: NoImagePolicy::Memoize,
            enqueued_at: Instant::now(),
        });
        q.push_back(QueueEntry {
            key: b.clone(),
            page_size: 15,
            no_image_policy: NoImagePolicy::Memoize,
            enqueued_at: Instant::now(),
        });
        q.push_back(QueueEntry {
            key: c.clone(),
            page_size: 15,
            no_image_policy: NoImagePolicy::Memoize,
            enqueued_at: Instant::now(),
        });
        assert_eq!(q.pop_back().map(|e| e.key), Some(c));
        assert_eq!(q.pop_back().map(|e| e.key), Some(b));
        assert_eq!(q.pop_back().map(|e| e.key), Some(a));
        assert!(q.pop_back().is_none());
    }

    #[test]
    fn enqueue_drops_oldest_when_queue_full() {
        // Enqueueing more than MAX_QUEUE_LEN distinct keys must spill
        // the oldest entries off the front of the queue and release
        // them from `pending`, so they can be re-enqueued later when
        // the user navigates back to a page whose enqueues we
        // truncated. Locks in the queue-bound contract end-to-end:
        // queue length capped, pending matches the queue, dropped
        // keys are re-enqueueable.
        let cache = cache_for_test();
        // Push MAX_QUEUE_LEN + 5 distinct keys; the first 5 must be
        // the ones that get dropped.
        for i in 0..(MAX_QUEUE_LEN + 5) {
            cache.enqueue_with_media_id(key("SNES", &format!("/p/{i}")), None, 15);
        }
        let queue_len = cache.queue.lock().unwrap().len();
        assert_eq!(
            queue_len, MAX_QUEUE_LEN,
            "queue must truncate to MAX_QUEUE_LEN"
        );
        let guard = cache.state.read().unwrap();
        assert_eq!(
            guard.pending.len(),
            MAX_QUEUE_LEN,
            "pending must mirror the queue (truncated keys released)"
        );
        for i in 0..5 {
            let stale = key("SNES", &format!("/p/{i}"));
            assert!(
                !guard.pending.contains(&stale),
                "oldest key /p/{i} should have been released from pending"
            );
        }
        for i in 5..(MAX_QUEUE_LEN + 5) {
            let live = key("SNES", &format!("/p/{i}"));
            assert!(
                guard.pending.contains(&live),
                "key /p/{i} should still be pending"
            );
        }
        drop(guard);
        // Re-enqueueing a previously-dropped key must succeed (it's
        // no longer in pending). Verify by checking the queue grows.
        let revived = key("SNES", "/p/0");
        cache.enqueue_with_media_id(revived.clone(), None, 15);
        let queue_len = cache.queue.lock().unwrap().len();
        // Pushing one extra back into a full queue truncates one
        // *other* old entry off the front, so length stays at cap.
        assert_eq!(queue_len, MAX_QUEUE_LEN);
        let guard = cache.state.read().unwrap();
        assert!(
            guard.pending.contains(&revived),
            "re-enqueued key must be pending again"
        );
    }

    #[test]
    fn search_cover_enqueue_uses_soft_no_image_policy() {
        let cache = cache_for_test();
        let k = key("SNES", "/favorite");
        let id_key = MediaKey::with_media_id("SNES", "/favorite", 7);
        cache.enqueue_search_cover_with_media_id(k, Some(7), 15);
        let entry = cache
            .queue
            .lock()
            .unwrap()
            .pop_back()
            .expect("search cover enqueue adds queue entry");
        assert_eq!(entry.key, id_key);
        assert_eq!(entry.no_image_policy, NoImagePolicy::SoftMiss);
        let guard = cache.state.read().unwrap();
        assert_eq!(guard.media_ids.get(&id_key), Some(&7));
        assert!(guard.search_seen.contains(&id_key));
    }

    #[test]
    fn search_cover_enqueue_skips_soft_no_image_keys() {
        let cache = cache_for_test();
        let k = key("SNES", "/soft-missed");
        let id_key = MediaKey::with_media_id("SNES", "/soft-missed", 7);
        cache
            .state
            .write()
            .unwrap()
            .soft_no_image
            .insert(id_key.clone());
        cache.enqueue_search_cover_with_media_id(k, Some(7), 15);
        assert!(cache.queue.lock().unwrap().is_empty());
        assert!(cache.state.read().unwrap().pending.is_empty());
    }

    #[test]
    fn default_enqueue_allows_unprotected_soft_no_image_keys() {
        let cache = cache_for_test();
        let k = key("SNES", "/soft-missed");
        cache.state.write().unwrap().soft_no_image.insert(k.clone());
        cache.enqueue_with_media_id(k.clone(), Some(7), 15);
        let entry = cache
            .queue
            .lock()
            .unwrap()
            .pop_back()
            .expect("default enqueue is not blocked by unprotected soft miss");
        let id_key = MediaKey::with_media_id("SNES", "/soft-missed", 7);
        assert_eq!(entry.key, id_key);
        assert_eq!(entry.no_image_policy, NoImagePolicy::Memoize);
        assert!(cache.state.read().unwrap().pending.contains(&id_key));
    }

    #[test]
    fn default_enqueue_allows_protected_soft_no_image_keys() {
        let cache = cache_for_test();
        let k = key("SNES", "/protected-soft-missed");
        {
            let mut guard = cache.state.write().unwrap();
            guard.soft_no_image.insert(k.clone());
            guard.search_seen.insert(k.clone());
        }
        cache.enqueue_with_media_id(k.clone(), Some(7), 15);
        let entry = cache
            .queue
            .lock()
            .unwrap()
            .pop_back()
            .expect("default enqueue is not blocked by protected soft miss");
        let id_key = MediaKey::with_media_id("SNES", "/protected-soft-missed", 7);
        assert_eq!(entry.key, id_key);
        assert_eq!(entry.no_image_policy, NoImagePolicy::Memoize);
        assert!(cache.state.read().unwrap().pending.contains(&id_key));
    }

    #[test]
    fn pop_one_drains_lifo_one_entry_at_a_time() {
        let cache = cache_for_test();
        cache.enqueue_with_media_id(key("NES", "/first"), None, 15);
        cache.enqueue_with_media_id(key("NES", "/second"), None, 15);
        cache.enqueue_with_media_id(key("NES", "/third"), None, 25);

        assert_eq!(
            pop_one(&cache.queue).expect("third").key,
            key("NES", "/third")
        );
        assert_eq!(
            pop_one(&cache.queue).expect("second").key,
            key("NES", "/second")
        );
        assert_eq!(
            pop_one(&cache.queue).expect("first").key,
            key("NES", "/first")
        );
        assert!(pop_one(&cache.queue).is_none());
    }

    #[test]
    fn is_cached_does_not_bump_lru() {
        // Contract: `is_cached` is a side-effect-free existence
        // query. It must NOT touch `last_used` or `read`, so that
        // role-data lookups (which call it on every QML rebind) do
        // not contaminate the LRU clock. Only `get_bytes` bumps the
        // clock, because only `get_bytes` corresponds to an actual
        // paint pass.
        let cache = cache_for_test();
        let k = key("SNES", "/p");
        ok_png(&cache.state, usize::MAX, &k, 100);
        let last_used_before = cache.state.read().unwrap().map[&k].last_used;
        for _ in 0..10 {
            assert!(cache.is_cached(&k));
        }
        let last_used_after = cache.state.read().unwrap().map[&k].last_used;
        assert_eq!(
            last_used_before, last_used_after,
            "is_cached must not bump last_used"
        );
        assert!(
            !cache.state.read().unwrap().map[&k].read,
            "is_cached must not flip read"
        );
        // get_bytes is the paint signal — it MUST bump.
        let _ = cache.get_bytes(&k);
        let last_used_after_get = cache.state.read().unwrap().map[&k].last_used;
        assert!(
            last_used_after_get > last_used_after,
            "get_bytes must bump last_used"
        );
        assert!(
            cache.state.read().unwrap().map[&k].read,
            "get_bytes must flip read"
        );
    }

    #[test]
    fn is_negative_reports_memo_membership() {
        // Locks in the contract that `is_negative` reflects the
        // negative memo without false positives — the miss-driven
        // re-enqueue path uses this to skip refetching keys Core
        // has definitively said have nothing to fetch.
        let cache = cache_for_test();
        let absent = key("SNES", "/never-fetched");
        let memoised = key("SNES", "/no-image");
        cache
            .state
            .write()
            .unwrap()
            .pending
            .insert(memoised.clone());
        let _ = finish_fetch(
            &cache.state,
            usize::MAX,
            &memoised,
            FetchOutcome::NoImage,
            NoImagePolicy::Memoize,
        );
        assert!(
            cache.is_negative(&memoised),
            "NoImage outcome must populate the negative memo"
        );
        assert!(
            !cache.is_negative(&absent),
            "unrelated keys must not appear negative"
        );
    }
}
