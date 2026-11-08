// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A list's rows, keyed by their position in the whole list.
//!
//! Most lists are dense: one run of rows from position 0, grown at its end.
//! A large folder is positional: Core reports the list's size and serves a
//! page from any position, so the store knows the total and holds only the
//! runs (blocks) of rows that have been visited, with the rest unloaded and
//! the furthest rows dropped once the store outgrows its budget.

use std::ops::{Index, IndexMut, Range};

/// The next page a positional list should ask for. `cursor`, when set, is
/// the tail cursor of the block that ends at `start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagePlan {
    pub start: usize,
    pub limit: usize,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Block<T> {
    start: usize,
    rows: Vec<T>,
    /// Continues from this block's last row.
    tail: Option<String>,
}

impl<T> Block<T> {
    fn end(&self) -> usize {
        self.start + self.rows.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowStore<T> {
    /// Sorted by position, never touching. A dense store has exactly one,
    /// at position 0.
    blocks: Vec<Block<T>>,
    /// The whole list's size, for a positional store.
    total: Option<usize>,
    /// Leading rows that only the first page carries and no later request
    /// can bring back.
    pinned: usize,
}

impl<T> Default for RowStore<T> {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}

impl<T> FromIterator<T> for RowStore<T> {
    fn from_iter<I: IntoIterator<Item = T>>(rows: I) -> Self {
        rows.into_iter().collect::<Vec<T>>().into()
    }
}

impl<T> From<Vec<T>> for RowStore<T> {
    fn from(rows: Vec<T>) -> Self {
        Self {
            blocks: vec![Block {
                start: 0,
                rows,
                tail: None,
            }],
            total: None,
            pinned: 0,
        }
    }
}

impl<T> RowStore<T> {
    /// A positional store seeded with the list's first page.
    pub fn positional(first: Vec<T>, tail: Option<String>, total: usize, pinned: usize) -> Self {
        Self {
            total: Some(total.max(first.len())),
            pinned: pinned.min(first.len()),
            blocks: vec![Block {
                start: 0,
                rows: first,
                tail,
            }],
        }
    }

    pub fn is_positional(&self) -> bool {
        self.total.is_some()
    }

    /// Leading rows a positional list keeps from its first page.
    pub fn pinned(&self) -> usize {
        self.pinned
    }

    /// The list's length: the whole list for a positional store, loaded or
    /// not, and the rows held for a dense one.
    pub fn len(&self) -> usize {
        self.total
            .unwrap_or_else(|| self.blocks.iter().map(|block| block.rows.len()).sum())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Correct a positional list's size after a page showed it has changed.
    pub fn set_total(&mut self, total: usize) {
        if self.total.is_none() {
            return;
        }
        self.remove_range(total..usize::MAX);
        self.total = Some(total);
    }

    /// Rows a positional store holds; for a dense one, the same as `len`.
    pub fn loaded_len(&self) -> usize {
        self.blocks.iter().map(|block| block.rows.len()).sum()
    }

    fn block_index(&self, position: usize) -> Option<usize> {
        let after = self.blocks.partition_point(|block| block.start <= position);
        let index = after.checked_sub(1)?;
        (position < self.blocks[index].end()).then_some(index)
    }

    pub fn get(&self, position: usize) -> Option<&T> {
        let block = &self.blocks[self.block_index(position)?];
        block.rows.get(position - block.start)
    }

    pub fn get_mut(&mut self, position: usize) -> Option<&mut T> {
        let index = self.block_index(position)?;
        let block = &mut self.blocks[index];
        block.rows.get_mut(position - block.start)
    }

    /// Every row held, in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.blocks.iter().flat_map(|block| block.rows.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.blocks
            .iter_mut()
            .flat_map(|block| block.rows.iter_mut())
    }

    /// The unbroken rows from `position` on; nothing when it is not loaded.
    pub fn iter_from(&self, position: usize) -> impl Iterator<Item = &T> {
        self.block_at(position)
            .map(|(base, rows)| &rows[position - base..])
            .unwrap_or_default()
            .iter()
    }

    /// The block holding `position`: its first position and its rows.
    pub fn block_at(&self, position: usize) -> Option<(usize, &[T])> {
        let block = &self.blocks[self.block_index(position)?];
        Some((block.start, block.rows.as_slice()))
    }

    /// The same, to edit.
    pub fn block_at_mut(&mut self, position: usize) -> Option<(usize, &mut [T])> {
        let index = self.block_index(position)?;
        let block = &mut self.blocks[index];
        Some((block.start, block.rows.as_mut_slice()))
    }

    /// Where each block starts.
    pub fn block_starts(&self) -> Vec<usize> {
        self.blocks.iter().map(|block| block.start).collect()
    }

    /// A dense store's rows. A positional store answers with its first block.
    pub fn dense(&self) -> &[T] {
        self.blocks
            .first()
            .map_or(&[], |block| block.rows.as_slice())
    }

    /// A dense store's rows, for the lists that edit them in place.
    pub fn dense_mut(&mut self) -> &mut Vec<T> {
        if self.blocks.is_empty() {
            self.blocks.push(Block {
                start: 0,
                rows: Vec::new(),
                tail: None,
            });
        }
        &mut self.blocks[0].rows
    }

    /// Add rows to a dense store's end.
    pub fn extend(&mut self, rows: impl IntoIterator<Item = T>) {
        self.dense_mut().extend(rows);
    }

    /// Drop every row of a dense store.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The first unloaded run inside `wanted`, cut to the list's length.
    pub fn first_gap(&self, wanted: Range<usize>) -> Option<Range<usize>> {
        let end = wanted.end.min(self.len());
        let mut cursor = wanted.start;
        for block in &self.blocks {
            if cursor >= end {
                return None;
            }
            if block.end() <= cursor {
                continue;
            }
            if block.start > cursor {
                return Some(cursor..block.start.min(end));
            }
            cursor = block.end();
        }
        (cursor < end).then_some(cursor..end)
    }

    pub fn is_loaded(&self, wanted: Range<usize>) -> bool {
        self.first_gap(wanted).is_none()
    }

    /// The page that fills the first gap in `wanted`, starting on a multiple
    /// of `align` where it can so a turned page arrives whole. A gap directly
    /// after a block continues from that block's cursor, the cheapest request
    /// Core has.
    pub fn plan_fetch(&self, wanted: Range<usize>, chunk: usize, align: usize) -> Option<PagePlan> {
        let gap = self.first_gap(wanted)?;
        let chunk = chunk.max(1);
        let align = align.max(1);
        let after = self
            .blocks
            .partition_point(|block| block.start <= gap.start);
        let previous = after.checked_sub(1).map(|index| &self.blocks[index]);
        let floor = previous
            .map_or(0, Block::end)
            .max(self.pinned)
            .min(gap.start);
        let ceiling = self
            .blocks
            .get(after)
            .map_or_else(|| self.len(), |block| block.start);
        if let Some(cursor) = previous
            .filter(|block| block.end() == gap.start)
            .and_then(|block| block.tail.clone())
        {
            return Some(PagePlan {
                start: gap.start,
                limit: chunk.min(ceiling - gap.start),
                cursor: Some(cursor),
            });
        }
        // Against a block above, read back from it: that is the way the user
        // is heading. Anywhere else, read on from the gap.
        let from = if gap.end == ceiling && ceiling < self.len() && gap.len() < chunk {
            ceiling.saturating_sub(chunk)
        } else {
            gap.start
        };
        let start = (from.max(floor) / align * align).max(floor).min(gap.start);
        let end = (start + chunk.max(gap.end - start)).min(ceiling);
        Some(PagePlan {
            start,
            limit: end - start,
            cursor: None,
        })
    }

    /// Store a page at `start`, joining any block it overlaps or touches. The
    /// new rows win over the old, and the merged block continues from whichever
    /// part of it ends last.
    pub fn insert(&mut self, start: usize, mut rows: Vec<T>, tail: Option<String>) {
        if rows.is_empty() {
            return;
        }
        let end = start + rows.len();
        let first = self.blocks.partition_point(|block| block.end() < start);
        let last = self.blocks.partition_point(|block| block.start <= end);
        let mut merged_start = start;
        let mut merged = Vec::new();
        let mut above = Vec::new();
        let mut merged_tail = tail;
        for mut block in self.blocks.drain(first..last) {
            let block_end = block.end();
            if block_end > end {
                above = block.rows.split_off(end - block.start);
                merged_tail = block.tail.take();
            } else if block_end == end && merged_tail.is_none() {
                merged_tail = block.tail.take();
            }
            if block.start < start {
                merged_start = block.start;
                block.rows.truncate(start - block.start);
                merged = block.rows;
            }
        }
        merged.append(&mut rows);
        merged.append(&mut above);
        self.blocks.insert(
            first,
            Block {
                start: merged_start,
                rows: merged,
                tail: merged_tail,
            },
        );
        if let Some(total) = self.total {
            self.total = Some(total.max(self.blocks.last().map_or(0, Block::end)));
        }
    }

    /// Drop rows from the far ends, `step` at a time, until what is held
    /// weighs no more than `budget`. Pinned rows and `keep` always stay.
    pub fn evict(
        &mut self,
        keep: Range<usize>,
        budget: usize,
        step: usize,
        weight: impl Fn(&T) -> usize,
    ) {
        let step = step.max(1);
        let mut held: usize = self.iter().map(&weight).sum();
        while held > budget {
            let Some(range) = self.furthest_range(&keep, step) else {
                return;
            };
            let dropped: usize = range
                .clone()
                .filter_map(|position| self.get(position))
                .map(&weight)
                .sum();
            self.remove_range(range);
            held = held.saturating_sub(dropped);
        }
    }

    /// Up to `step` held rows at whichever end lies furthest from `keep`.
    fn furthest_range(&self, keep: &Range<usize>, step: usize) -> Option<Range<usize>> {
        let low = self
            .blocks
            .iter()
            .find_map(|block| {
                let start = block.start.max(self.pinned);
                (start < block.end() && start < keep.start).then_some((start, block.end()))
            })
            .map(|(start, end)| start..(start + step).min(end).min(keep.start));
        let high = self
            .blocks
            .last()
            .filter(|block| block.end() > keep.end.max(self.pinned))
            .map(|block| {
                let floor = block.start.max(keep.end).max(self.pinned);
                block.end().saturating_sub(step).max(floor)..block.end()
            });
        match (low, high) {
            (Some(low), Some(high)) => {
                let below = keep.start.saturating_sub(low.start);
                let above = high.end.saturating_sub(keep.end);
                Some(if below > above { low } else { high })
            }
            (low, high) => low.or(high),
        }
    }

    /// Forget a range of positions. The upper part of a split block keeps the
    /// cursor; the lower part has lost the row it continued from.
    fn remove_range(&mut self, range: Range<usize>) {
        let mut kept = Vec::with_capacity(self.blocks.len() + 1);
        for mut block in std::mem::take(&mut self.blocks) {
            let end = block.end();
            if end <= range.start || block.start >= range.end {
                kept.push(block);
                continue;
            }
            let upper = (end > range.end).then(|| Block {
                start: range.end,
                rows: block.rows.split_off(range.end - block.start),
                tail: block.tail.take(),
            });
            block.rows.truncate(range.start.saturating_sub(block.start));
            if !block.rows.is_empty() {
                block.tail = None;
                kept.push(block);
            }
            kept.extend(upper);
        }
        self.blocks = kept;
    }
}

impl<T> IndexMut<usize> for RowStore<T> {
    #[allow(
        clippy::expect_used,
        reason = "the same contract as a slice's index: callers check first"
    )]
    fn index_mut(&mut self, position: usize) -> &mut T {
        self.get_mut(position).expect("row is loaded")
    }
}

impl<T> Index<usize> for RowStore<T> {
    type Output = T;

    #[allow(
        clippy::expect_used,
        reason = "the same contract as a slice's index: callers check first"
    )]
    fn index(&self, position: usize) -> &T {
        self.get(position).expect("row is loaded")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(range: Range<usize>) -> Vec<usize> {
        range.collect()
    }

    fn store() -> RowStore<usize> {
        RowStore::positional(run(0..50), Some("c50".into()), 1000, 0)
    }

    fn held(store: &RowStore<usize>) -> Vec<usize> {
        store.iter().copied().collect()
    }

    #[test]
    fn dense_behaves_like_the_vec_it_replaces() {
        let mut rows = RowStore::from(vec![1, 2, 3]);
        assert!(!rows.is_positional());
        assert_eq!(rows.len(), 3);
        rows.extend([4]);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.get(3), Some(&4));
        assert_eq!(rows.iter_from(2).copied().collect::<Vec<_>>(), [3, 4]);
        assert_eq!(rows.iter_from(9).count(), 0);
        rows.dense_mut().remove(0);
        assert_eq!(rows[0], 2);
        rows.clear();
        assert!(rows.is_empty());
        assert_eq!(rows.first_gap(0..4), None);
    }

    #[test]
    fn positional_length_is_the_whole_list() {
        let rows = store();
        assert_eq!(rows.len(), 1000);
        assert_eq!(rows.loaded_len(), 50);
        assert_eq!(rows.get(49), Some(&49));
        assert_eq!(rows.get(50), None);
        assert!(rows.is_loaded(0..50));
        assert_eq!(rows.first_gap(40..60), Some(50..60));
        assert_eq!(rows.first_gap(990..2000), Some(990..1000));
    }

    #[test]
    fn iter_from_stops_at_the_end_of_its_block() {
        let mut rows = store();
        rows.insert(100, run(100..110), None);
        assert_eq!(rows.iter_from(48).count(), 2);
        assert_eq!(rows.iter_from(50).count(), 0);
        assert_eq!(rows.iter_from(105).count(), 5);
        assert_eq!(rows.first_gap(45..105), Some(50..100));
    }

    #[test]
    fn a_page_touching_a_block_merges_and_continues_from_the_last_rows() {
        let mut rows = store();
        rows.insert(50, run(50..100), Some("c100".into()));
        assert_eq!(rows.blocks.len(), 1);
        assert_eq!(rows.blocks[0].tail.as_deref(), Some("c100"));

        rows.insert(200, run(200..250), Some("c250".into()));
        assert_eq!(rows.blocks.len(), 2);
        rows.insert(100, run(100..200), Some("c200".into()));
        assert_eq!(rows.blocks.len(), 1);
        assert_eq!(rows.blocks[0].tail.as_deref(), Some("c250"));
        assert_eq!(held(&rows), run(0..250));
    }

    #[test]
    fn an_overlapping_page_replaces_the_rows_it_covers() {
        let mut rows = store();
        rows.insert(40, vec![1040; 5], None);
        assert_eq!(rows.get(39), Some(&39));
        assert_eq!(rows.get(40), Some(&1040));
        assert_eq!(rows.get(45), Some(&45));
        assert_eq!(rows.loaded_len(), 50);
        assert_eq!(rows.blocks[0].tail.as_deref(), Some("c50"));
    }

    #[test]
    fn a_page_inserted_before_a_block_keeps_that_blocks_cursor() {
        let mut rows = store();
        rows.insert(100, run(100..150), Some("c150".into()));
        rows.insert(90, run(90..100), None);
        assert_eq!(rows.blocks.len(), 2);
        assert_eq!(rows.blocks[1].start, 90);
        assert_eq!(rows.blocks[1].tail.as_deref(), Some("c150"));
    }

    #[test]
    fn a_page_past_the_stated_total_grows_it() {
        let mut rows = RowStore::positional(run(0..10), None, 20, 0);
        rows.insert(15, run(15..30), None);
        assert_eq!(rows.len(), 30);
    }

    #[test]
    fn a_smaller_total_forgets_the_rows_past_it() {
        let mut rows = store();
        rows.set_total(30);
        assert_eq!(rows.len(), 30);
        assert_eq!(rows.loaded_len(), 30);
        assert_eq!(rows.blocks[0].tail, None);
    }

    #[test]
    fn a_gap_after_a_block_continues_from_its_cursor() {
        let plan = store().plan_fetch(50..62, 48, 12);
        assert_eq!(
            plan,
            Some(PagePlan {
                start: 50,
                limit: 48,
                cursor: Some("c50".into()),
            })
        );
    }

    #[test]
    fn a_distant_gap_starts_on_a_page_boundary() {
        let plan = store().plan_fetch(610..622, 48, 12);
        assert_eq!(
            plan,
            Some(PagePlan {
                start: 600,
                limit: 48,
                cursor: None,
            })
        );
    }

    #[test]
    fn a_gap_below_a_block_reads_back_from_it() {
        let mut rows = store();
        rows.insert(600, run(600..648), None);
        let plan = rows.plan_fetch(588..600, 48, 12);
        assert_eq!(
            plan,
            Some(PagePlan {
                start: 552,
                limit: 48,
                cursor: None,
            })
        );
    }

    #[test]
    fn a_page_never_reaches_into_the_next_block_or_past_the_end() {
        let mut rows = store();
        rows.insert(70, run(70..100), None);
        let forward = rows.plan_fetch(50..70, 48, 12);
        assert_eq!(forward.map(|plan| plan.limit), Some(20));
        let tail = rows.plan_fetch(990..1000, 48, 12);
        assert_eq!(
            tail,
            Some(PagePlan {
                start: 984,
                limit: 16,
                cursor: None,
            })
        );
    }

    #[test]
    fn a_gap_wider_than_a_chunk_is_covered_from_its_start() {
        let mut rows = store();
        rows.insert(600, run(600..648), None);
        let plan = rows.plan_fetch(300..600, 48, 12);
        assert_eq!(plan.as_ref().map(|plan| plan.start), Some(300));
        assert_eq!(plan.map(|plan| plan.limit), Some(300));
    }

    #[test]
    fn plans_never_ask_for_the_rows_only_the_first_page_carries() {
        let mut rows = RowStore::positional(run(0..10), None, 100, 3);
        rows.evict(50..60, 3, 5, |_| 1);
        assert_eq!(rows.get(2), Some(&2));
        assert_eq!(rows.get(3), None);
        let plan = rows.plan_fetch(3..10, 48, 12);
        assert_eq!(plan.map(|plan| plan.start), Some(3));
    }

    #[test]
    fn eviction_drops_the_furthest_rows_first_and_keeps_the_view() {
        let mut rows = store();
        rows.insert(50, run(50..150), Some("c150".into()));
        rows.evict(100..112, 100, 25, |_| 1);
        assert_eq!(rows.loaded_len(), 100);
        assert_eq!(rows.get(0), None);
        assert_eq!(rows.get(49), None);
        assert_eq!(rows.get(50), Some(&50));
        assert_eq!(rows.blocks[0].tail.as_deref(), Some("c150"));

        rows.evict(50..62, 50, 25, |_| 1);
        assert!(rows.is_loaded(50..62));
        assert_eq!(rows.get(100), None);
        assert_eq!(rows.blocks.last().and_then(|b| b.tail.clone()), None);
    }

    #[test]
    fn eviction_stops_when_only_the_kept_rows_remain() {
        let mut rows = store();
        rows.evict(10..20, 0, 7, |_| 1);
        assert_eq!(rows.loaded_len(), 10);
        assert!(rows.is_loaded(10..20));
    }

    #[test]
    fn eviction_reaches_between_blocks() {
        let mut rows = store();
        rows.insert(600, run(600..650), None);
        rows.insert(300, run(300..350), None);
        rows.evict(300..350, 100, 10, |_| 1);
        assert_eq!(rows.loaded_len(), 100);
        assert!(rows.is_loaded(300..350));
    }
}
