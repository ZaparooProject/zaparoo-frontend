// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Bounded embedded-art work: current page, next page, previous page. Replacing
//! that window discards stale queued work; one already-running job may finish.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

pub const CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
/// More than three pages at the densest supported grid, also bounding misses.
const MAX_WANTED: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bounds {
    pub width: u32,
    pub height: u32,
}

impl Bounds {
    /// Stable raster buckets avoid rebuilding pixels for every resize step.
    /// Never upscale source art; these are bounding boxes, not output sizes.
    pub fn new(width: u32, height: u32) -> Self {
        let bucket = |n: u32| n.clamp(32, 512).next_power_of_two();
        Self {
            width: bucket(width),
            height: bucket(height),
        }
    }
}

#[derive(Debug)]
struct Entry<V> {
    value: V,
    bytes: usize,
    used: u64,
}

#[derive(Debug)]
pub struct Cache<K, V> {
    map: HashMap<K, Entry<V>>,
    bytes: usize,
    cap: usize,
    tick: u64,
    wanted: HashSet<K>,
    pending: VecDeque<K>,
    running: Option<K>,
}

impl<K: Clone + Eq + Hash, V> Cache<K, V> {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            bytes: 0,
            cap,
            tick: 0,
            wanted: HashSet::new(),
            pending: VecDeque::new(),
            running: None,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick = self.tick.wrapping_add(1);
        let entry = self.map.get_mut(key)?;
        entry.used = self.tick;
        Some(&entry.value)
    }

    /// Keys are already ordered visible/destination, next, previous. Only
    /// this window can queue work; repeated paints never accumulate jobs.
    pub fn request(&mut self, keys: impl IntoIterator<Item = K>) {
        self.wanted.clear();
        self.pending.clear();
        for key in keys.into_iter().take(MAX_WANTED) {
            if self.wanted.insert(key.clone())
                && !self.map.contains_key(&key)
                && self.running.as_ref() != Some(&key)
            {
                self.pending.push_back(key);
            }
        }
    }

    pub fn next_job(&mut self) -> Option<K> {
        if self.running.is_some() {
            return None;
        }
        let key = self.pending.pop_front()?;
        self.running = Some(key.clone());
        Some(key)
    }

    /// A late result may populate only the current window. Returning false
    /// means no UI notification: it cannot patch a page the user has left.
    pub fn finish(&mut self, key: K, value: V, bytes: usize) -> bool {
        if self.running.as_ref() == Some(&key) {
            self.running = None;
        }
        if !self.wanted.contains(&key) || bytes > self.cap {
            return false;
        }
        self.tick = self.tick.wrapping_add(1);
        if let Some(old) = self.map.insert(
            key,
            Entry {
                value,
                bytes,
                used: self.tick,
            },
        ) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        while self.bytes > self.cap || self.map.len() > MAX_ENTRIES {
            let Some(key) = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some(old) = self.map.remove(&key) {
                self.bytes -= old.bytes;
            }
        }
        true
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
        self.wanted.clear();
        self.pending.clear();
        // Do not admit a second job while the old generation drains.
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_replaces_queue_in_priority_order_and_drops_stale_completion() {
        let mut cache = Cache::new(16);
        cache.request([1, 2, 1, 3]);
        assert_eq!(cache.next_job(), Some(1));
        assert_eq!(cache.next_job(), None, "exactly one job");
        cache.request([4, 5, 6]);
        assert!(!cache.finish(1, (), 4));
        assert_eq!(cache.next_job(), Some(4));
        assert!(cache.finish(4, (), 4));
        assert_eq!(cache.next_job(), Some(5));
        assert!(cache.finish(5, (), 4));
        assert_eq!(cache.next_job(), Some(6));
    }

    #[test]
    fn bytes_and_negative_entries_are_bounded_and_reads_touch_lru() {
        let mut cache = Cache::new(8);
        for key in [1, 2] {
            cache.request([key]);
            assert_eq!(cache.next_job(), Some(key));
            assert!(cache.finish(key, Some(key), 4));
        }
        assert_eq!(cache.get(&1), Some(&Some(1)));
        cache.request([3]);
        assert_eq!(cache.next_job(), Some(3));
        assert!(cache.finish(3, Some(3), 4));
        assert_eq!(cache.bytes(), 8);
        assert_eq!(cache.get(&2), None);
        for key in 4..1000 {
            cache.request([key]);
            assert_eq!(cache.next_job(), Some(key));
            assert!(cache.finish(key, None, 0));
        }
        assert!(cache.map.len() <= MAX_ENTRIES);
        assert!(cache.bytes() <= 8);
    }

    #[test]
    fn clear_rejects_old_generation_without_admitting_parallel_scratch() {
        let mut cache = Cache::new(8);
        cache.request([1]);
        assert_eq!(cache.next_job(), Some(1));
        cache.clear();
        cache.request([2]);
        assert_eq!(cache.next_job(), None);
        assert!(!cache.finish(1, (), 4));
        assert_eq!(cache.next_job(), Some(2));
        assert!(cache.finish(2, (), 4));
        assert_eq!(cache.bytes(), 4);
    }

    #[test]
    fn queue_and_render_buckets_are_bounded() {
        let mut cache = Cache::<_, ()>::new(CACHE_BYTES);
        cache.request(0..1000);
        assert_eq!(cache.pending.len(), MAX_WANTED);
        assert_eq!(
            Bounds::new(129, 70),
            Bounds {
                width: 256,
                height: 128
            }
        );
        assert_eq!(
            Bounds::new(u32::MAX, 0),
            Bounds {
                width: 512,
                height: 32
            }
        );
    }
}
