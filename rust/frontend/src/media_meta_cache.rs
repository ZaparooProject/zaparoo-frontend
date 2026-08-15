// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Process-wide, in-memory cache for `media.meta` results, keyed by the
// canonical `(system_id, path)` pair (`MediaKey`). It exists so the list +
// detail browse view can paint a focused row's metadata synchronously instead
// of blanking and re-fetching on every move:
//
//   * The detail pane reads the cache the moment the focused row changes. A
//     warm neighbor (prefetched while dwelling on the previous row) is an
//     instant hit, so the table never shows the previous row's stale values
//     and never flickers blank-then-populate.
//
//   * A `None` outcome (Core returned an error or nothing usable) is memoized
//     as a definite negative, so an item with no metadata resolves instantly
//     on revisit and never flashes.
//
// In-memory only and strictly bounded by an LRU cap — Core is the canonical
// metadata store and the frontend must not persist scraped metadata, nor grow
// without bound on MiSTer's tight RAM budget (see CLAUDE.md). Metadata is a
// variable-sized strings and collections, so every entry is conservatively
// byte-accounted under a hard 4 MiB cap.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::debug;
use zaparoo_core::media_types::{
    MediaMeta, MediaMetaBatchResult, MediaMetaParams, MediaMetaProperty, TagInfo,
    MEDIA_META_BATCH_MAX_ITEMS,
};

use crate::media_image_cache::MediaKey;
use crate::models::{global_handle, global_store};

const META_CACHE_CAP_BYTES: usize = 4 * 1024 * 1024;
const HASH_ENTRY_OVERHEAD: usize = 16;

/// Outcome of a synchronous cache probe.
pub enum MetaLookup {
    /// Metadata is cached for this key. Boxed because `MediaMeta` is large and
    /// the other variants carry no data — keeps the enum cheap to pass around.
    Hit(Box<MediaMeta>),
    /// Core was asked and returned nothing usable — a memoized negative so
    /// revisits resolve instantly and never re-fetch.
    Negative,
    /// Not in the cache; the caller should fetch.
    Miss,
}

struct Entry {
    /// `Some` = positive hit, `None` = memoized negative.
    meta: Option<MediaMeta>,
    /// Conservative owned-byte weight, including key and hash-entry overhead.
    weight: usize,
    /// LRU recency stamp, bumped on insert and on every read.
    clock: u64,
}

struct State {
    map: HashMap<MediaKey, Entry>,
    /// Keys with a prefetch fetch in flight, so overlapping windows do not
    /// request the same row twice.
    inflight: HashSet<MediaKey>,
    used_bytes: usize,
    cap_bytes: usize,
    clock: u64,
}

pub struct MediaMetaCache {
    state: Mutex<State>,
}

impl MediaMetaCache {
    fn new() -> Self {
        Self::new_with_cap(META_CACHE_CAP_BYTES)
    }

    fn new_with_cap(cap_bytes: usize) -> Self {
        Self {
            state: Mutex::new(State {
                map: HashMap::new(),
                inflight: HashSet::new(),
                used_bytes: 0,
                cap_bytes,
                clock: 0,
            }),
        }
    }

    /// Probe the cache for `key`, bumping its LRU recency on a hit.
    pub fn lookup(&self, key: &MediaKey) -> MetaLookup {
        #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
        let mut guard = self.state.lock().unwrap();
        guard.clock += 1;
        let now = guard.clock;
        match guard.map.get_mut(key) {
            Some(entry) => {
                entry.clock = now;
                match &entry.meta {
                    Some(meta) => MetaLookup::Hit(Box::new(meta.clone())),
                    None => MetaLookup::Negative,
                }
            }
            None => MetaLookup::Miss,
        }
    }

    /// Insert a resolved fetch outcome. `Some` is a positive hit, `None` a
    /// negative memo. Replacements update byte accounting, oversized entries
    /// are rejected, and least-recently-used entries are evicted to the cap.
    pub fn store(&self, key: MediaKey, meta: Option<MediaMeta>) {
        #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
        let mut guard = self.state.lock().unwrap();
        store_locked(&mut guard, key, meta);
    }

    /// Best-effort background warm of uncached neighbors. One ordered batch is
    /// issued after cached/in-flight filtering; results become synchronous hits
    /// for the next focus move.
    pub fn prefetch(&self, requests: Vec<(MediaKey, MediaMetaParams)>) {
        let to_fetch = self.prepare_prefetch(requests);
        if to_fetch.is_empty() {
            return;
        }
        global_handle().spawn(async move {
            let (keys, params): (Vec<_>, Vec<_>) = to_fetch.into_iter().unzip();
            let result = global_store().client().media_meta_batch(params).await;
            let cache = global_media_meta_cache();
            match result {
                Ok(batch) => cache.finish_prefetch(keys, Some(batch)),
                Err(error) => {
                    debug!(error = %error.message, "media_meta_cache: batch prefetch failed");
                    cache.finish_prefetch(keys, None);
                }
            }
        });
    }

    fn prepare_prefetch(
        &self,
        requests: Vec<(MediaKey, MediaMetaParams)>,
    ) -> Vec<(MediaKey, MediaMetaParams)> {
        #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
        let mut guard = self.state.lock().unwrap();
        let mut to_fetch = Vec::new();
        for (key, params) in requests {
            if guard.map.contains_key(&key) || guard.inflight.contains(&key) {
                continue;
            }
            guard.inflight.insert(key.clone());
            to_fetch.push((key, params));
            if to_fetch.len() == MEDIA_META_BATCH_MAX_ITEMS {
                break;
            }
        }
        to_fetch
    }

    /// Apply ordered results. Transport/protocol failures release every key
    /// without a negative memo so later focused fetches can retry.
    fn finish_prefetch(&self, keys: Vec<MediaKey>, result: Option<MediaMetaBatchResult>) {
        #[allow(clippy::unwrap_used, reason = "Mutex poisoning is unrecoverable")]
        let mut guard = self.state.lock().unwrap();
        let Some(batch) = result else {
            clear_inflight_locked(&mut guard, &keys);
            return;
        };
        if batch.items.len() != keys.len() {
            debug!(
                expected = keys.len(),
                actual = batch.items.len(),
                "media_meta_cache: malformed batch response length"
            );
            clear_inflight_locked(&mut guard, &keys);
            return;
        }
        for (key, item) in keys.into_iter().zip(batch.items) {
            match (item.media, item.error) {
                (Some(meta), None) => store_locked(&mut guard, key, Some(meta)),
                (None, Some(_)) => store_locked(&mut guard, key, None),
                _ => {
                    guard.inflight.remove(&key);
                    debug!("media_meta_cache: unmatched batch item");
                }
            }
        }
    }
}

fn clear_inflight_locked(guard: &mut State, keys: &[MediaKey]) {
    for key in keys {
        guard.inflight.remove(key);
    }
}

fn store_locked(guard: &mut State, key: MediaKey, meta: Option<MediaMeta>) {
    guard.inflight.remove(&key);
    if let Some(previous) = guard.map.remove(&key) {
        guard.used_bytes = guard.used_bytes.saturating_sub(previous.weight);
    }

    let weight = entry_weight(&key, meta.as_ref());
    if weight > guard.cap_bytes {
        debug!(
            weight,
            cap = guard.cap_bytes,
            "media_meta_cache: rejected oversized entry"
        );
        return;
    }

    guard.clock += 1;
    let now = guard.clock;
    guard.used_bytes = guard.used_bytes.saturating_add(weight);
    guard.map.insert(
        key,
        Entry {
            meta,
            weight,
            clock: now,
        },
    );
    while guard.used_bytes > guard.cap_bytes {
        let victim = guard
            .map
            .iter()
            .min_by_key(|(_, entry)| entry.clock)
            .map(|(key, _)| key.clone());
        let Some(victim) = victim else {
            break;
        };
        if let Some(entry) = guard.map.remove(&victim) {
            guard.used_bytes = guard.used_bytes.saturating_sub(entry.weight);
            debug!("media_meta_cache: evicted entry");
        }
    }
}

fn entry_weight(key: &MediaKey, meta: Option<&MediaMeta>) -> usize {
    size_of::<MediaKey>()
        .saturating_add(size_of::<Entry>())
        .saturating_add(HASH_ENTRY_OVERHEAD)
        .saturating_add(key.system_id.len())
        .saturating_add(key.path.len())
        .saturating_add(key.image_type.as_deref().map_or(0, str::len))
        .saturating_add(6 * size_of::<usize>())
        .saturating_add(meta.map_or(0, media_meta_heap_weight))
}

fn media_meta_heap_weight(meta: &MediaMeta) -> usize {
    string_heap_weight(&meta.path)
        .saturating_add(string_heap_weight(&meta.parent_dir))
        .saturating_add(tags_heap_weight(&meta.tags, meta.tags.capacity()))
        .saturating_add(properties_heap_weight(&meta.properties))
        .saturating_add(strings_heap_weight(
            &meta.available_image_types,
            meta.available_image_types.capacity(),
        ))
        .saturating_add(string_heap_weight(&meta.title.slug))
        .saturating_add(
            meta.title
                .secondary_slug
                .as_ref()
                .map_or(0, string_heap_weight),
        )
        .saturating_add(string_heap_weight(&meta.title.name))
        .saturating_add(string_heap_weight(&meta.title.system.id))
        .saturating_add(string_heap_weight(&meta.title.system.name))
        .saturating_add(tags_heap_weight(
            &meta.title.tags,
            meta.title.tags.capacity(),
        ))
        .saturating_add(properties_heap_weight(&meta.title.properties))
        .saturating_add(strings_heap_weight(
            &meta.title.available_image_types,
            meta.title.available_image_types.capacity(),
        ))
}

fn string_heap_weight(value: &String) -> usize {
    value.capacity()
}

fn strings_heap_weight(values: &[String], capacity: usize) -> usize {
    capacity
        .saturating_mul(size_of::<String>())
        .saturating_add(values.iter().map(string_heap_weight).sum::<usize>())
}

fn tags_heap_weight(tags: &[TagInfo], capacity: usize) -> usize {
    capacity
        .saturating_mul(size_of::<TagInfo>())
        .saturating_add(
            tags.iter()
                .map(|tag| {
                    string_heap_weight(&tag.tag)
                        .saturating_add(string_heap_weight(&tag.tag_type))
                        .saturating_add(string_heap_weight(&tag.label))
                })
                .sum::<usize>(),
        )
}

fn properties_heap_weight(properties: &HashMap<String, MediaMetaProperty>) -> usize {
    properties
        .capacity()
        .saturating_mul(
            size_of::<String>()
                .saturating_add(size_of::<MediaMetaProperty>())
                .saturating_add(HASH_ENTRY_OVERHEAD),
        )
        .saturating_add(
            properties
                .iter()
                .map(|(key, property)| {
                    string_heap_weight(key)
                        .saturating_add(string_heap_weight(&property.text))
                        .saturating_add(string_heap_weight(&property.content_type))
                        .saturating_add(property.extension.as_ref().map_or(0, string_heap_weight))
                })
                .sum::<usize>(),
        )
}

static GLOBAL_MEDIA_META_CACHE: OnceLock<Arc<MediaMetaCache>> = OnceLock::new();

/// Lazily initialise the process-wide media metadata cache and return a handle.
/// Constructed on first call from any thread; subsequent calls return the same
/// `Arc`.
pub fn global_media_meta_cache() -> Arc<MediaMetaCache> {
    GLOBAL_MEDIA_META_CACHE
        .get_or_init(|| Arc::new(MediaMetaCache::new()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::media_types::MediaMetaBatchItemResult;

    fn key(path: &str) -> MediaKey {
        MediaKey::new("SNES", path)
    }

    fn params(path: &str) -> MediaMetaParams {
        MediaMetaParams::for_media("SNES", path)
    }

    fn meta_with_path(path: &str) -> MediaMeta {
        MediaMeta {
            path: path.to_string(),
            ..MediaMeta::default()
        }
    }

    fn prepared_keys(prepared: &[(MediaKey, MediaMetaParams)]) -> Vec<MediaKey> {
        prepared.iter().map(|(key, _)| key.clone()).collect()
    }

    #[test]
    fn positive_hit_round_trips() {
        let cache = MediaMetaCache::new();
        cache.store(key("a"), Some(meta_with_path("a")));
        assert!(matches!(cache.lookup(&key("a")), MetaLookup::Hit(meta) if meta.path == "a"));
    }

    #[test]
    fn negative_is_memoized() {
        let cache = MediaMetaCache::new();
        cache.store(key("b"), None);
        assert!(matches!(cache.lookup(&key("b")), MetaLookup::Negative));
    }

    #[test]
    fn miss_for_unknown_key() {
        let cache = MediaMetaCache::new();
        assert!(matches!(cache.lookup(&key("missing")), MetaLookup::Miss));
    }

    #[test]
    fn evicts_least_recently_used_to_byte_cap() {
        let a = key("a");
        let b = key("b");
        let c = key("c");
        let meta_a = meta_with_path("a");
        let meta_b = meta_with_path("b");
        let cap = entry_weight(&a, Some(&meta_a)) + entry_weight(&b, Some(&meta_b));
        let cache = MediaMetaCache::new_with_cap(cap);
        cache.store(a.clone(), Some(meta_a));
        cache.store(b.clone(), Some(meta_b));
        assert!(matches!(cache.lookup(&a), MetaLookup::Hit(_)));
        cache.store(c.clone(), Some(meta_with_path("c")));
        assert!(matches!(cache.lookup(&b), MetaLookup::Miss));
        assert!(matches!(cache.lookup(&a), MetaLookup::Hit(_)));
        assert!(matches!(cache.lookup(&c), MetaLookup::Hit(_)));
        #[allow(clippy::unwrap_used, reason = "test mutex must remain healthy")]
        let guard = cache.state.lock().unwrap();
        assert!(guard.used_bytes <= guard.cap_bytes);
    }

    #[test]
    fn replacement_updates_byte_weight() {
        let cache = MediaMetaCache::new_with_cap(16 * 1024);
        let cache_key = key("a");
        cache.store(cache_key.clone(), Some(meta_with_path("a")));
        #[allow(clippy::unwrap_used, reason = "test mutex must remain healthy")]
        let before = cache.state.lock().unwrap().used_bytes;
        let larger = meta_with_path(&"x".repeat(1024));
        let expected = entry_weight(&cache_key, Some(&larger));
        cache.store(cache_key, Some(larger));
        #[allow(clippy::unwrap_used, reason = "test mutex must remain healthy")]
        let after = cache.state.lock().unwrap().used_bytes;
        assert!(after > before);
        assert_eq!(after, expected);
    }

    #[test]
    fn oversized_entry_is_not_cached() {
        let cache = MediaMetaCache::new_with_cap(512);
        let cache_key = key("large");
        cache.store(cache_key.clone(), Some(meta_with_path(&"x".repeat(2048))));
        assert!(matches!(cache.lookup(&cache_key), MetaLookup::Miss));
        #[allow(clippy::unwrap_used, reason = "test mutex must remain healthy")]
        let guard = cache.state.lock().unwrap();
        assert_eq!(guard.used_bytes, 0);
    }

    #[test]
    fn mixed_batch_results_map_to_ordered_keys() {
        let cache = MediaMetaCache::new();
        let prepared =
            cache.prepare_prefetch(vec![(key("a"), params("a")), (key("b"), params("b"))]);
        let keys = prepared_keys(&prepared);
        cache.finish_prefetch(
            keys,
            Some(MediaMetaBatchResult {
                items: vec![
                    MediaMetaBatchItemResult {
                        media: Some(meta_with_path("a")),
                        error: None,
                    },
                    MediaMetaBatchItemResult {
                        media: None,
                        error: Some("not found".into()),
                    },
                ],
            }),
        );
        assert!(matches!(cache.lookup(&key("a")), MetaLookup::Hit(meta) if meta.path == "a"));
        assert!(matches!(cache.lookup(&key("b")), MetaLookup::Negative));
    }

    #[test]
    fn transport_and_short_responses_release_without_poisoning() {
        let cache = MediaMetaCache::new();
        let prepared =
            cache.prepare_prefetch(vec![(key("a"), params("a")), (key("b"), params("b"))]);
        cache.finish_prefetch(prepared_keys(&prepared), None);
        assert!(matches!(cache.lookup(&key("a")), MetaLookup::Miss));
        assert_eq!(
            cache.prepare_prefetch(vec![(key("a"), params("a"))]).len(),
            1
        );

        let prepared = cache.prepare_prefetch(vec![(key("b"), params("b"))]);
        cache.finish_prefetch(
            prepared_keys(&prepared),
            Some(MediaMetaBatchResult { items: Vec::new() }),
        );
        assert!(matches!(cache.lookup(&key("b")), MetaLookup::Miss));
        assert_eq!(
            cache.prepare_prefetch(vec![(key("b"), params("b"))]).len(),
            1
        );
    }

    #[test]
    fn unmatched_batch_item_releases_without_negative_memo() {
        let cache = MediaMetaCache::new();
        let prepared = cache.prepare_prefetch(vec![(key("a"), params("a"))]);
        cache.finish_prefetch(
            prepared_keys(&prepared),
            Some(MediaMetaBatchResult {
                items: vec![MediaMetaBatchItemResult::default()],
            }),
        );
        assert!(matches!(cache.lookup(&key("a")), MetaLookup::Miss));
        assert_eq!(
            cache.prepare_prefetch(vec![(key("a"), params("a"))]).len(),
            1
        );
    }

    #[test]
    fn overlapping_prefetch_windows_do_not_duplicate_requests() {
        let cache = MediaMetaCache::new();
        let first = cache.prepare_prefetch(vec![(key("a"), params("a")), (key("b"), params("b"))]);
        let second = cache.prepare_prefetch(vec![(key("b"), params("b")), (key("c"), params("c"))]);
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].0.path.as_ref(), "c");
    }

    #[test]
    fn prefetch_never_prepares_more_than_core_batch_cap() {
        let cache = MediaMetaCache::new();
        let requests = (0..=MEDIA_META_BATCH_MAX_ITEMS)
            .map(|i| {
                let path = format!("{i}");
                (key(&path), params(&path))
            })
            .collect();
        assert_eq!(
            cache.prepare_prefetch(requests).len(),
            MEDIA_META_BATCH_MAX_ITEMS
        );
    }
}
