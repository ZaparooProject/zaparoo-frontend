// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Port of `src/ui/components/PagedGrid.qml`'s navigation and geometry
// rules. Items flow row-major within a page; pages stack vertically, so
// Down at the bottom row swaps in the next page (same column, top row) and
// Up at the top row swaps in the previous page. Left and Right wrap within
// the current row and never change pages. Selection is a flat index over
// the source model; page, row and column derive from it.
//
// A paginated model (the games browse) loads a slice at a time: a move
// onto a page that is not loaded yet is stashed as a pending target, the
// host is asked to fetch more, and the move commits once the rows land.
// `skip_empty_cells` (the Hub outside a Move session) treats blank cells
// as unreachable.
//
// Pinned by the tests below, transcribed from `tests/ui/tst_paged_grid.qml`.

/// The reserved chrome around the cell area, in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
    pub column_gap: i32,
    pub row_gap: i32,
}

/// Resolved cell geometry for one grid size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fit {
    pub available_width: i32,
    pub available_height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub content_width: i32,
    pub content_height: i32,
    /// The cell block centers against the full inset-to-inset area.
    pub block_offset_x: i32,
    pub block_offset_y: i32,
}

/// One cell's rectangle relative to the grid's own origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn div_floor(a: i32, b: i32) -> i32 {
    (f64::from(a) / f64::from(b)).floor() as i32
}

/// Fit `columns` x `rows` cells into `width` x `height`. `height_budget`
/// replaces `height` for the height fit when a caller derives its own
/// height from the fitted cell (the Hub); `square_cells` clamps both axes
/// to the tighter fit.
pub fn fit(
    columns: i32,
    rows: i32,
    width: i32,
    height: i32,
    height_budget: Option<i32>,
    square_cells: bool,
    insets: &Insets,
) -> Fit {
    let columns = columns.max(1);
    let rows = rows.max(1);
    let available_width = (width - insets.left - insets.right).max(0);
    let available_height = (height - insets.top - insets.bottom).max(0);
    let height_fit_available =
        (height_budget.unwrap_or(height) - insets.top - insets.bottom).max(0);
    let width_fit = div_floor(available_width - (columns - 1) * insets.column_gap, columns).max(0);
    let height_fit = div_floor(height_fit_available - (rows - 1) * insets.row_gap, rows).max(0);
    let (cell_width, cell_height) = if square_cells {
        let side = width_fit.min(height_fit);
        (side, side)
    } else {
        (width_fit, height_fit)
    };
    let content_width = columns * cell_width + (columns - 1) * insets.column_gap;
    let content_height = rows * cell_height + (rows - 1) * insets.row_gap;
    Fit {
        available_width,
        available_height,
        cell_width,
        cell_height,
        content_width,
        content_height,
        block_offset_x: div_floor(available_width - content_width, 2).max(0),
        block_offset_y: div_floor(available_height - content_height, 2).max(0),
    }
}

/// The rectangle of the page-local cell at (`row`, `column`).
pub fn cell_rect(fit: &Fit, insets: &Insets, row: i32, column: i32) -> CellRect {
    CellRect {
        x: insets.left + fit.block_offset_x + column * (fit.cell_width + insets.column_gap),
        y: insets.top + fit.block_offset_y + row * (fit.cell_height + insets.row_gap),
        width: fit.cell_width,
        height: fit.cell_height,
    }
}

/// A fetch the grid wants the host to run; `urgent` means the user is
/// waiting on it (a wrap or jump), not a background prefetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadMore {
    pub urgent: bool,
}

/// Selection and paging state over a flat model.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per PagedGrid.qml property the host binds"
)]
pub struct Grid {
    columns: usize,
    rows: usize,
    current_index: usize,
    item_count: usize,
    /// Blank cells (the Hub's padding and spacer tiles), one flag per item.
    empty: Vec<bool>,
    /// Caller-supplied dataset total for paginated models; `None` falls
    /// back to the loaded count.
    pub total_items_override: Option<usize>,
    /// False for cursor chains whose final count is unknown: navigation
    /// requests another page instead of wrapping at the loaded edge.
    pub pagination_total_known: bool,
    /// The model says more rows are coming.
    pub has_more_pages: bool,
    /// A fetch is in flight; pending targets wait for it before asking
    /// for another page.
    pub loading_more: bool,
    /// Pages of buffer kept ahead before a prefetch fires.
    pub load_ahead_pages: usize,
    /// Blank cells are unreachable (see the module comment).
    pub skip_empty_cells: bool,
    pending_page: Option<(usize, usize, usize)>,
    pending_index: Option<usize>,
    previous_item_count: usize,
    requests: Vec<LoadMore>,
}

impl Grid {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            columns: columns.max(1),
            rows: rows.max(1),
            current_index: 0,
            item_count: 0,
            empty: Vec::new(),
            total_items_override: None,
            pagination_total_known: true,
            has_more_pages: false,
            loading_more: false,
            load_ahead_pages: 2,
            skip_empty_cells: false,
            pending_page: None,
            pending_index: None,
            previous_item_count: 0,
            requests: Vec::new(),
        }
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Change the shape; the index is clamped into the loaded range.
    pub fn set_shape(&mut self, columns: usize, rows: usize) {
        self.columns = columns.max(1);
        self.rows = rows.max(1);
        if self.item_count > 0 && self.current_index >= self.item_count {
            self.current_index = 0;
        }
    }

    pub fn page_size(&self) -> usize {
        self.columns * self.rows
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }

    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Loaded pages. Snaps to the last full page while more chunks are
    /// on the way so a half-full trailing page never shows mid-fetch.
    pub fn page_count(&self) -> usize {
        if self.item_count == 0 {
            return 1;
        }
        if self.has_more_pages {
            (self.item_count / self.page_size()).max(1)
        } else {
            self.item_count.div_ceil(self.page_size())
        }
    }

    pub fn current_page(&self) -> usize {
        self.current_index / self.page_size()
    }

    pub fn current_column(&self) -> usize {
        (self.current_index % self.page_size()) % self.columns
    }

    pub fn current_row(&self) -> usize {
        (self.current_index % self.page_size()) / self.columns
    }

    pub fn has_pages_above(&self) -> bool {
        self.current_page() > 0
    }

    /// A model reporting more rows always counts as a page below, known
    /// total or not.
    pub fn has_pages_below(&self) -> bool {
        self.current_page() + 1 < self.page_count() || self.has_more_pages
    }

    pub fn total_items(&self) -> usize {
        match self.total_items_override {
            Some(total) if self.pagination_total_known => total,
            _ => self.item_count,
        }
    }

    pub fn total_page_count(&self) -> usize {
        self.total_items().div_ceil(self.page_size()).max(1)
    }

    pub fn has_pending_target(&self) -> bool {
        self.pending_page.is_some() || self.pending_index.is_some()
    }

    pub fn has_pending_jump(&self) -> bool {
        self.pending_index.is_some()
    }

    pub fn pending_jump_index(&self) -> Option<usize> {
        self.pending_index
    }

    #[cfg(test)]
    fn pending_page(&self) -> Option<(usize, usize, usize)> {
        self.pending_page
    }

    /// Fetch requests raised since the last drain, oldest first.
    pub fn take_load_requests(&mut self) -> Vec<LoadMore> {
        std::mem::take(&mut self.requests)
    }

    fn request_load(&mut self, urgent: bool) {
        self.requests.push(LoadMore { urgent });
    }

    fn clear_pending_target(&mut self) {
        self.pending_page = None;
        self.pending_index = None;
    }

    pub fn set_current_index_immediate(&mut self, index: usize) {
        self.current_index = index;
    }

    /// Disarm model-relative navigation before a scope replacement can
    /// shrink the row count.
    pub fn prepare_for_model_replacement(&mut self) {
        self.clear_pending_target();
        self.current_index = 0;
    }

    pub fn is_empty_at(&self, index: usize) -> bool {
        index < self.item_count && self.empty.get(index).copied().unwrap_or(false)
    }

    /// Replace the blank-cell flags (missing entries read as real cells).
    pub fn set_empty_flags(&mut self, flags: Vec<bool>) {
        self.empty = flags;
    }

    /// The model's row count changed (`onItemCountChanged`): a shrink
    /// drops any pending target, growth tries to commit one, and the
    /// index is always kept inside the loaded rows.
    pub fn set_item_count(&mut self, count: usize) {
        self.item_count = count;
        self.empty.resize(count, false);
        if count < self.previous_item_count {
            self.clear_pending_target();
        } else if count > self.previous_item_count {
            self.commit_pending_target();
        }
        if count > 0 && self.current_index >= count {
            self.current_index = 0;
        }
        self.previous_item_count = count;
    }

    pub fn set_has_more_pages(&mut self, more: bool) {
        if self.has_more_pages == more {
            return;
        }
        self.has_more_pages = more;
        if self.has_pending_target() {
            self.commit_pending_target();
        }
    }

    pub fn set_loading_more(&mut self, loading: bool) {
        if self.loading_more == loading {
            return;
        }
        self.loading_more = loading;
        if !loading && self.has_pending_target() {
            self.commit_pending_target();
        }
    }

    fn near_loaded_edge(&self) -> bool {
        self.current_page() + self.load_ahead_pages + 1 >= self.page_count()
    }

    /// First non-empty cell on `page`, scanning row-major from the
    /// page-local `from` and wrapping within the page's filled span.
    fn first_non_empty_on_page(&self, page: usize, from: usize) -> Option<usize> {
        let page_start = page * self.page_size();
        let items_on_page = self
            .page_size()
            .min(self.item_count.saturating_sub(page_start));
        if items_on_page == 0 {
            return None;
        }
        (0..items_on_page)
            .map(|step| page_start + (from + step) % items_on_page)
            .find(|&idx| !self.is_empty_at(idx))
    }

    /// Turn the page by `delta`, skipping blank landing slots and blank
    /// pages when `skip_empty_cells` is set. Returns whether the index
    /// changed now; a stashed fetch or nowhere to go returns false.
    pub fn page_by(&mut self, delta: i32) -> bool {
        if !self.skip_empty_cells {
            return self.page_by_step(delta);
        }
        let start = self.current_index;
        for _ in 0..self.page_count() {
            if !self.page_by_step(delta) {
                break;
            }
            let local = self.current_index - self.current_page() * self.page_size();
            match self.first_non_empty_on_page(self.current_page(), local) {
                None => {}
                Some(found) => {
                    self.current_index = found;
                    return true;
                }
            }
        }
        self.current_index = start;
        false
    }

    fn page_by_step(&mut self, delta: i32) -> bool {
        if self.item_count == 0 || delta == 0 {
            return false;
        }
        let current_page = self.current_page() as i64;
        let target_page = if !self.pagination_total_known && self.has_more_pages {
            let target = current_page + i64::from(delta);
            if target < 0 {
                return false;
            }
            target as usize
        } else {
            let total = self.total_page_count() as i64;
            if total <= 1 {
                return false;
            }
            (((current_page + i64::from(delta)) % total + total) % total) as usize
        };
        if target_page == self.current_page() {
            return false;
        }
        if target_page > self.page_count() - 1 {
            self.pending_index = None;
            self.pending_page = Some((target_page, self.current_row(), self.current_column()));
            self.request_load(true);
            return false;
        }
        self.clear_pending_target();
        let target_slot = target_page * self.page_size()
            + self.current_row() * self.columns
            + self.current_column();
        let last_on_page = ((target_page + 1) * self.page_size()).min(self.item_count);
        if last_on_page == 0 {
            return false;
        }
        let new_index = target_slot.min(last_on_page - 1);
        if new_index == self.current_index {
            return false;
        }
        self.current_index = new_index;
        if self.near_loaded_edge() {
            self.request_load(false);
        }
        true
    }

    /// Jump to an exact absolute index over the full dataset, loading the
    /// intervening pages if needed.
    pub fn jump_to_index(&mut self, target: usize) -> bool {
        if self.item_count == 0 || self.total_items() == 0 {
            return false;
        }
        let target = target.min(self.total_items() - 1);
        if target < self.item_count {
            self.clear_pending_target();
            self.current_index = target;
            if self.near_loaded_edge() {
                self.request_load(false);
            }
            return true;
        }
        self.pending_page = None;
        self.pending_index = Some(target);
        self.request_load(true);
        false
    }

    /// Commit a pending move once its destination is loaded, or settle on
    /// what is loaded when the model says nothing more is coming.
    pub fn commit_pending_target(&mut self) {
        if let Some(want) = self.pending_index {
            if want < self.item_count {
                self.clear_pending_target();
                self.current_index = want;
                return;
            }
            if self.has_more_pages {
                if !self.loading_more {
                    self.request_load(true);
                }
                return;
            }
            self.clear_pending_target();
            if self.item_count > 0 {
                self.current_index = want.min(self.item_count - 1);
            }
            return;
        }
        let Some((page, row, col)) = self.pending_page else {
            return;
        };
        let total_last = self.total_page_count() - 1;
        let target_page = if self.pagination_total_known || !self.has_more_pages {
            page.min(total_last)
        } else {
            page
        };
        let target_idx = target_page * self.page_size() + row * self.columns + col;
        if target_idx >= self.item_count
            || (self.has_more_pages && target_page >= self.page_count())
        {
            if self.has_more_pages {
                if !self.loading_more {
                    self.request_load(true);
                }
                return;
            }
            self.pending_page = None;
            let page_start = target_page * self.page_size();
            let last_loaded_on_page = ((target_page + 1) * self.page_size()).min(self.item_count);
            if last_loaded_on_page > page_start {
                self.current_index = last_loaded_on_page - 1;
                return;
            }
            if self.item_count > 0 {
                self.current_index = self.item_count - 1;
            }
            return;
        }
        self.pending_page = None;
        self.current_index = target_idx;
    }

    /// Step the selection by (`d_col`, `d_row`). Cardinal moves only.
    pub fn move_selection(&mut self, d_col: i32, d_row: i32) -> bool {
        if !self.skip_empty_cells {
            return self.move_selection_step(d_col, d_row);
        }
        if d_row != 0 {
            let Some(candidate) = self.nearest_vertical_candidate(d_row, self.current_index) else {
                return false;
            };
            self.clear_pending_target();
            self.current_index = candidate;
            return true;
        }
        let start = self.current_index;
        for _ in 0..self.item_count {
            if !self.move_selection_step(d_col, d_row) {
                break;
            }
            if self.current_index != start && !self.is_empty_at(self.current_index) {
                return true;
            }
        }
        self.current_index = start;
        false
    }

    /// The best real cell in the vertical direction, scored the way
    /// Android's `FocusFinder` scores rects: 13 x major distance squared
    /// plus minor distance squared, over the whole board, wrapping past
    /// the far edge when nothing lies in the pressed direction.
    fn nearest_vertical_candidate(&self, d_row: i32, from: usize) -> Option<usize> {
        let page_size = self.page_size();
        let total_rows = (self.total_page_count() * self.rows) as i64;
        let src_local = from % page_size;
        let src_col = (src_local % self.columns) as i64;
        let src_vrow = ((from / page_size) * self.rows + src_local / self.columns) as i64;
        let position = |idx: usize| {
            let local = idx % page_size;
            let vrow = ((idx / page_size) * self.rows + local / self.columns) as i64;
            let col = (local % self.columns) as i64;
            (vrow, col)
        };
        let score = |major: i64, minor: i64| 13 * major * major + minor * minor;
        let real_cells =
            || (0..self.item_count).filter(|&idx| idx != from && !self.is_empty_at(idx));

        // Pass 1: only cells strictly in the pressed direction.
        let direct = real_cells()
            .filter_map(|idx| {
                let (vrow, col) = position(idx);
                let ahead = if d_row > 0 {
                    vrow > src_vrow
                } else {
                    vrow < src_vrow
                };
                ahead.then(|| (score((vrow - src_vrow).abs(), (col - src_col).abs()), idx))
            })
            .min_by_key(|(s, _)| *s);
        if let Some((_, idx)) = direct {
            return Some(idx);
        }
        // Pass 2: nothing there, wrap past the far edge.
        real_cells()
            .map(|idx| {
                let (vrow, col) = position(idx);
                let major = if d_row > 0 {
                    vrow + (total_rows - src_vrow)
                } else {
                    src_vrow + (total_rows - vrow)
                };
                (score(major, (col - src_col).abs()), idx)
            })
            .min_by_key(|(s, _)| *s)
            .map(|(_, idx)| idx)
    }

    fn move_selection_step(&mut self, d_col: i32, d_row: i32) -> bool {
        if self.item_count == 0 {
            return false;
        }
        let page_size = self.page_size();
        let mut new_page = self.current_page();
        let mut new_row = self.current_row();
        let mut new_col = self.current_column();

        if d_col != 0 {
            self.clear_pending_target();
            let row_first = self.current_page() * page_size + self.current_row() * self.columns;
            let row_last = (self.item_count - 1).min(row_first + self.columns - 1);
            let max_col_on_row = row_last - row_first;
            let candidate = self.current_column() as i64 + i64::from(d_col);
            new_col = if candidate < 0 {
                max_col_on_row
            } else if candidate as usize > max_col_on_row {
                0
            } else {
                candidate as usize
            };
        }

        if d_row != 0 {
            let row_candidate = self.current_row() as i64 + i64::from(d_row);
            let items_on_page = page_size.min(self.item_count - self.current_page() * page_size);
            let last_filled_row = (items_on_page.saturating_sub(1)) / self.columns;
            if row_candidate < 0 {
                if !self.pagination_total_known && self.has_more_pages && self.current_page() == 0 {
                    return false;
                }
                let target_page = if self.current_page() == 0 {
                    self.total_page_count() - 1
                } else {
                    self.current_page() - 1
                };
                if target_page > self.page_count() - 1 {
                    self.pending_index = None;
                    self.pending_page = Some((target_page, self.rows - 1, self.current_column()));
                    self.request_load(true);
                    return false;
                }
                new_page = target_page;
                new_row = self.rows - 1;
            } else if row_candidate as usize >= self.rows
                || row_candidate as usize > last_filled_row
            {
                let last_page = self.total_page_count() - 1;
                let target_page = if !self.pagination_total_known
                    && self.has_more_pages
                    && self.current_page() + 1 >= self.page_count()
                {
                    self.current_page() + 1
                } else if self.current_page() == last_page {
                    0
                } else {
                    self.current_page() + 1
                };
                if target_page > self.page_count() - 1 {
                    self.pending_index = None;
                    self.pending_page = Some((target_page, 0, self.current_column()));
                    self.request_load(true);
                    return false;
                }
                new_page = target_page;
                new_row = 0;
            } else {
                new_row = row_candidate as usize;
            }
        }

        let mut new_index = new_page * page_size + new_row * self.columns + new_col;
        if new_index >= self.item_count {
            let last_on_page = ((new_page + 1) * page_size).min(self.item_count);
            if last_on_page == 0 {
                return false;
            }
            new_index = last_on_page - 1;
        }
        if new_index == self.current_index {
            if self.near_loaded_edge() {
                self.request_load(false);
            }
            return false;
        }
        self.clear_pending_target();
        self.current_index = new_index;
        if self.near_loaded_edge() {
            self.request_load(false);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The QML harness: a 4x3 grid (page size 12).
    fn grid(count: usize) -> Grid {
        let mut g = Grid::new(4, 3);
        g.set_item_count(count);
        g
    }

    fn append(g: &mut Grid, upto: usize) {
        g.set_item_count(upto);
    }

    fn requests(g: &mut Grid) -> usize {
        g.take_load_requests().len()
    }

    fn partial(loaded: usize, total: usize) -> Grid {
        let mut g = grid(loaded);
        g.total_items_override = Some(total);
        g.has_more_pages = true;
        g.set_current_index_immediate(0);
        assert_eq!(g.page_count(), loaded.div_ceil(12));
        assert_eq!(g.total_page_count(), total.div_ceil(12));
        g.take_load_requests();
        g
    }

    // -- geometry --

    #[test]
    fn integer_cell_remainders_are_centered() {
        let insets = Insets {
            left: 4,
            right: 0,
            top: 3,
            bottom: 4,
            column_gap: 5,
            row_gap: 7,
        };
        let f = fit(3, 2, 307, 293, None, false, &insets);
        let right_remainder = f.available_width - f.block_offset_x - f.content_width;
        let bottom_remainder = f.available_height - f.block_offset_y - f.content_height;
        assert!((f.block_offset_x - right_remainder).abs() <= 1);
        assert!((f.block_offset_y - bottom_remainder).abs() <= 1);
        let rect = cell_rect(&f, &insets, 0, 0);
        assert_eq!(rect.x, insets.left + f.block_offset_x);
        assert_eq!(rect.y, insets.top + f.block_offset_y);
        assert_eq!((rect.width, rect.height), (f.cell_width, f.cell_height));
    }

    #[test]
    fn square_cells_clamp_to_the_tighter_axis() {
        let insets = Insets::default();
        let f = fit(5, 1, 500, 9999, Some(300), true, &insets);
        assert_eq!((f.cell_width, f.cell_height), (100, 100));
        let f = fit(5, 1, 500, 9999, Some(60), true, &insets);
        assert_eq!((f.cell_width, f.cell_height), (60, 60));
    }

    #[test]
    fn square_cells_and_height_budget_defaults_are_unchanged() {
        let insets = Insets::default();
        let f = fit(5, 1, 500, 9999, None, false, &insets);
        assert_eq!((f.cell_width, f.cell_height), (100, 9999));
    }

    #[test]
    fn height_budget_default_uses_own_height() {
        let insets = Insets::default();
        let f = fit(5, 1, 500, 9999, None, true, &insets);
        assert_eq!((f.cell_width, f.cell_height), (100, 100));
    }

    #[test]
    fn cell_block_centers_against_full_inset_to_inset_width() {
        let insets = Insets {
            left: 10,
            right: 10,
            ..Insets::default()
        };
        let f = fit(3, 1, 220, 100, None, false, &insets);
        assert_eq!(f.block_offset_x, 1);
        let midpoint = insets.left + f.block_offset_x + f.content_width / 2;
        assert_eq!(midpoint, 110);
    }

    #[test]
    fn cell_geometry_is_identical_regardless_of_page_count() {
        // No gutter is reserved, so pagination cannot shift the block.
        let insets = Insets {
            left: 10,
            right: 10,
            ..Insets::default()
        };
        let single = fit(3, 1, 220, 100, None, false, &insets);
        let mut g = Grid::new(3, 1);
        g.set_item_count(2);
        assert_eq!(g.total_page_count(), 1);
        g.total_items_override = Some(999);
        assert!(g.total_page_count() > 1);
        assert_eq!(fit(3, 1, 220, 100, None, false, &insets), single);
    }

    // -- navigation --

    #[test]
    fn geometry_matches_pinned_resolution() {
        let g = grid(0);
        assert_eq!((g.columns(), g.rows(), g.page_size()), (4, 3, 12));
    }

    #[test]
    fn empty_model_refuses_movement() {
        let mut g = grid(0);
        assert!(!g.move_selection(1, 0));
        assert!(!g.move_selection(0, 1));
        assert_eq!(g.current_index(), 0);
    }

    #[test]
    fn prepare_for_model_replacement_clears_pending_target() {
        let mut g = grid(20);
        g.total_items_override = Some(100);
        g.has_more_pages = true;
        g.set_current_index_immediate(13);
        assert!(!g.jump_to_index(50));
        assert!(g.has_pending_target());
        g.prepare_for_model_replacement();
        assert!(!g.has_pending_target());
        assert_eq!(g.current_index(), 0);
    }

    #[test]
    fn within_page_steps() {
        let mut g = grid(20);
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 1);
        let mut g = grid(20);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 4);
    }

    #[test]
    fn vertical_paging_crosses_page_boundaries() {
        let mut g = grid(24);
        g.set_current_index_immediate(8);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 12);

        g.set_current_index_immediate(12);
        assert!(g.move_selection(0, -1));
        assert_eq!(g.current_index(), 8);

        g.set_current_index_immediate(20);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 0);

        g.set_current_index_immediate(0);
        assert!(g.move_selection(0, -1));
        assert_eq!(g.current_index(), 20);
    }

    #[test]
    fn up_at_page_zero_wraps_to_partial_last_page_clamped() {
        let mut g = grid(20);
        assert!(g.move_selection(0, -1));
        assert_eq!(g.current_index(), 19);
    }

    #[test]
    fn down_overshoot_to_partial_page_clamps_to_last_existing() {
        let mut g = grid(13);
        g.set_current_index_immediate(11);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 12);
    }

    #[test]
    fn down_below_last_filled_row_on_partial_page_wraps_to_page_zero() {
        let mut g = grid(14);
        g.set_current_index_immediate(13);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 1);
    }

    #[test]
    fn up_from_partial_page_retreats_to_previous_page() {
        let mut g = grid(14);
        g.set_current_index_immediate(13);
        assert!(g.move_selection(0, -1));
        assert_eq!(g.current_index(), 9);
    }

    #[test]
    fn single_page_up_and_down_wrap_within_the_page() {
        let mut g = grid(12);
        assert_eq!(g.page_count(), 1);
        assert!(g.move_selection(0, -1));
        assert_eq!(g.current_index(), 8);

        let mut g = grid(6);
        g.set_current_index_immediate(5);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 1);
    }

    #[test]
    fn horizontal_wraps_within_the_row() {
        let mut g = grid(24);
        g.set_current_index_immediate(3);
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 0);

        g.set_current_index_immediate(0);
        assert!(g.move_selection(-1, 0));
        assert_eq!(g.current_index(), 3);

        let mut g = grid(14);
        g.set_current_index_immediate(13);
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 12);
        assert!(g.move_selection(-1, 0));
        assert_eq!(g.current_index(), 13);

        let mut g = grid(6);
        assert!(g.move_selection(-1, 0));
        assert_eq!(g.current_index(), 3);

        let mut g = grid(2);
        assert!(g.move_selection(-1, 0));
        assert_eq!(g.current_index(), 1);

        let mut g = grid(6);
        g.set_current_index_immediate(5);
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 4);
    }

    #[test]
    fn no_movement_returns_false() {
        let mut g = grid(20);
        assert!(!g.move_selection(0, 0));
        assert_eq!(g.current_index(), 0);
    }

    #[test]
    fn item_count_reset_falls_back_to_first_item() {
        let mut g = grid(20);
        g.set_current_index_immediate(19);
        g.set_item_count(10);
        assert_eq!(g.current_index(), 0);
    }

    #[test]
    fn page_by_advances_wraps_and_clamps() {
        let mut g = grid(24);
        g.set_current_index_immediate(2);
        assert!(g.page_by(1));
        assert_eq!((g.current_page(), g.current_index()), (1, 14));

        let mut g = grid(24);
        assert!(g.page_by(-1));
        assert_eq!(g.current_page(), 1);

        let mut g = grid(6);
        assert!(!g.page_by(1));
        assert!(!g.page_by(-1));

        let mut g = grid(14);
        g.set_current_index_immediate(5);
        assert!(g.page_by(1));
        assert_eq!(g.current_index(), 13);
    }

    #[test]
    fn has_pages_flags_track_current_page() {
        let mut g = grid(36);
        assert_eq!((g.has_pages_above(), g.has_pages_below()), (false, true));
        g.set_current_index_immediate(12);
        assert_eq!((g.has_pages_above(), g.has_pages_below()), (true, true));
        g.set_current_index_immediate(24);
        assert_eq!((g.has_pages_above(), g.has_pages_below()), (true, false));

        let g = grid(6);
        assert_eq!((g.has_pages_above(), g.has_pages_below()), (false, false));
    }

    #[test]
    fn has_pages_below_true_for_known_total_on_first_full_page() {
        let mut g = grid(12);
        g.has_more_pages = true;
        g.pagination_total_known = true;
        assert_eq!(g.page_count(), 1);
        assert!(g.has_pages_below());
    }

    #[test]
    fn unbounded_pages_keep_down_arrow_at_loaded_edge() {
        let mut g = grid(24);
        g.pagination_total_known = false;
        g.has_more_pages = true;
        g.set_current_index_immediate(12);
        assert_eq!(g.current_page(), g.page_count() - 1);
        assert!(g.has_pages_above());
        assert!(g.has_pages_below());
    }

    #[test]
    fn unbounded_page_next_fetches_instead_of_wrapping() {
        let mut g = grid(24);
        g.pagination_total_known = false;
        g.has_more_pages = true;
        g.set_current_index_immediate(12);
        g.take_load_requests();
        assert!(!g.page_by(1));
        assert_eq!(g.current_index(), 12);
        assert_eq!(g.pending_page(), Some((2, 0, 0)));
        assert!(requests(&mut g) >= 1);
    }

    #[test]
    fn unbounded_page_zero_does_not_wrap_backward() {
        let mut g = grid(24);
        g.pagination_total_known = false;
        g.has_more_pages = true;
        assert!(!g.page_by(-1));
        assert!(!g.move_selection(0, -1));
        assert_eq!(g.current_index(), 0);
        assert!(!g.has_pending_target());
    }

    #[test]
    fn unbounded_list_ignores_stale_total_hint() {
        let mut g = grid(24);
        g.total_items_override = Some(5);
        g.pagination_total_known = false;
        assert_eq!(g.total_items(), 24);
        assert_eq!(g.total_page_count(), 2);
    }

    #[test]
    fn unbounded_pending_page_commits_after_append() {
        let mut g = grid(24);
        g.total_items_override = Some(5);
        g.pagination_total_known = false;
        g.has_more_pages = true;
        g.set_current_index_immediate(12);
        assert!(!g.page_by(1));
        assert_eq!(g.pending_page(), Some((2, 0, 0)));
        append(&mut g, 36);
        assert_eq!(g.current_index(), 24);
        assert_eq!(g.pending_page(), None);
    }

    #[test]
    fn failed_unbounded_page_does_not_auto_retry() {
        let mut g = grid(24);
        g.pagination_total_known = false;
        g.has_more_pages = true;
        g.loading_more = true;
        g.set_current_index_immediate(12);
        assert!(!g.page_by(1));
        assert_eq!(g.pending_page(), Some((2, 0, 0)));
        g.set_has_more_pages(false);
        assert_eq!(g.pending_page(), None);
        g.take_load_requests();
        g.set_loading_more(false);
        assert_eq!(requests(&mut g), 0);
    }

    #[test]
    fn total_page_count_uses_override() {
        let mut g = grid(24);
        assert_eq!(g.page_count(), 2);
        g.total_items_override = Some(60);
        assert_eq!(g.total_page_count(), 5);
        g.total_items_override = None;
        assert_eq!(g.total_page_count(), 2);
    }

    #[test]
    fn up_at_page_zero_unloaded_target_stashes_pending() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        assert_eq!(g.current_index(), 0);
        assert_eq!(g.pending_page(), Some((4, 2, 0)));
        assert!(requests(&mut g) >= 1);
    }

    #[test]
    fn pending_target_waits_for_active_append_before_next_fetch() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        assert_eq!(g.pending_page(), Some((4, 2, 0)));
        g.loading_more = true;
        g.take_load_requests();
        append(&mut g, 36);
        assert_eq!(requests(&mut g), 0);
        g.set_loading_more(false);
        assert_eq!(requests(&mut g), 1);
        assert_eq!(g.pending_page(), Some((4, 2, 0)));
    }

    #[test]
    fn pending_target_commits_when_pages_load() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        append(&mut g, 60);
        assert_eq!(g.current_index(), 56);
        assert_eq!(g.pending_page(), None);
    }

    #[test]
    fn pending_target_clamps_when_target_partial_page() {
        let mut g = grid(24);
        g.total_items_override = Some(50);
        g.has_more_pages = true;
        assert_eq!(g.total_page_count(), 5);
        assert!(!g.move_selection(0, -1));
        assert_eq!(g.pending_page(), Some((4, 2, 0)));
        append(&mut g, 50);
        assert_eq!(g.current_index(), 0);
        g.set_has_more_pages(false);
        assert_eq!(g.current_index(), 49);
    }

    #[test]
    fn pending_target_chains_fetch_when_still_short() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        g.take_load_requests();
        append(&mut g, 36);
        assert_eq!(g.current_index(), 0);
        assert_eq!(g.pending_page(), Some((4, 2, 0)));
        assert!(requests(&mut g) >= 1);
    }

    #[test]
    fn pending_target_settles_when_has_more_pages_clears() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        g.set_has_more_pages(false);
        assert_eq!(g.pending_page(), None);
        assert_eq!(g.current_index(), 23);
    }

    #[test]
    fn pending_target_cancels_on_horizontal_move() {
        let mut g = partial(24, 60);
        g.set_current_index_immediate(1);
        assert!(!g.move_selection(0, -1));
        assert!(g.move_selection(1, 0));
        assert_eq!(g.pending_page(), None);
    }

    #[test]
    fn pending_target_clears_on_model_shrink() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        g.set_item_count(0);
        assert_eq!(g.pending_page(), None);
    }

    #[test]
    fn down_past_last_loaded_stashes_pending() {
        let mut g = partial(24, 60);
        g.set_current_index_immediate(20);
        assert!(!g.move_selection(0, 1));
        assert_eq!(g.current_index(), 20);
        assert_eq!(g.pending_page(), Some((2, 0, 0)));
        assert!(requests(&mut g) >= 1);
    }

    #[test]
    fn page_by_past_loaded_stashes_pending() {
        let mut g = partial(24, 60);
        g.set_current_index_immediate(2);
        assert!(!g.page_by(2));
        assert_eq!(g.current_index(), 2);
        assert_eq!(g.pending_page(), Some((2, 0, 2)));
    }

    #[test]
    fn load_ahead_pages_fires_at_page_count_minus_three() {
        let mut g = grid(60);
        assert_eq!(g.load_ahead_pages, 2);
        g.has_more_pages = true;
        g.set_current_index_immediate(20);
        g.take_load_requests();
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_page(), 2);
        assert!(requests(&mut g) >= 1);

        let mut g = grid(60);
        g.has_more_pages = true;
        g.set_current_index_immediate(8);
        g.take_load_requests();
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_page(), 1);
        assert_eq!(requests(&mut g), 0);
    }

    #[test]
    fn has_pending_target_tracks_pending_state() {
        let mut g = partial(24, 60);
        assert!(!g.has_pending_target());
        assert!(!g.move_selection(0, -1));
        assert!(g.has_pending_target());
        append(&mut g, 60);
        assert!(!g.has_pending_target());
    }

    #[test]
    fn jump_to_index_lands_or_stashes() {
        let mut g = grid(60);
        assert!(g.jump_to_index(37));
        assert_eq!(g.current_index(), 37);

        let mut g = grid(20);
        assert!(g.jump_to_index(999));
        assert_eq!(g.current_index(), 19);

        let mut g = partial(24, 60);
        assert!(!g.jump_to_index(50));
        assert_eq!(g.current_index(), 0);
        assert_eq!(g.pending_jump_index(), Some(50));
        assert_eq!(g.pending_page(), None);
        assert!(g.has_pending_jump());
        assert!(requests(&mut g) >= 1);
    }

    #[test]
    fn jump_to_index_commits_exact_index_when_loaded() {
        let mut g = partial(24, 60);
        assert!(!g.jump_to_index(50));
        append(&mut g, 60);
        assert_eq!(g.current_index(), 50);
        assert!(!g.has_pending_jump());
    }

    #[test]
    fn jump_to_index_commits_on_first_crossing_not_a_page_early() {
        let mut g = partial(24, 60);
        assert!(!g.jump_to_index(50));
        append(&mut g, 50);
        assert_eq!(g.current_index(), 0);
        assert_eq!(g.pending_jump_index(), Some(50));
        append(&mut g, 51);
        assert_eq!(g.current_index(), 50);
        assert_eq!(g.pending_jump_index(), None);
    }

    #[test]
    fn jump_to_index_truncated_dataset_lands_on_nearest_loaded() {
        let mut g = partial(24, 60);
        assert!(!g.jump_to_index(50));
        append(&mut g, 45);
        assert_eq!(g.current_index(), 0);
        g.set_has_more_pages(false);
        assert_eq!(g.current_index(), 44);
        assert_eq!(g.pending_jump_index(), None);
    }

    #[test]
    fn jump_to_index_pending_cleared_by_directional_move() {
        let mut g = grid(60);
        g.has_more_pages = true;
        g.total_items_override = Some(120);
        assert!(!g.jump_to_index(100));
        assert!(g.has_pending_jump());
        assert!(g.move_selection(1, 0));
        assert!(!g.has_pending_jump());
    }

    #[test]
    fn has_pending_jump_false_for_page_wrap_target() {
        let mut g = partial(24, 60);
        assert!(!g.move_selection(0, -1));
        assert!(g.has_pending_target());
        assert!(!g.has_pending_jump());
    }

    // -- blank cells --

    #[test]
    fn is_empty_row_remains_reachable_by_navigation() {
        let mut g = Grid::new(3, 1);
        g.set_item_count(3);
        g.set_empty_flags(vec![false, false, true]);
        g.set_current_index_immediate(1);
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 2);
    }

    /// The QML `skipEmptyModel` board: 3 x 2, two pages.
    ///   page 0            page 1
    ///   real-a . real-b   .   real-c .
    ///     .    .   .      real-d  .  .
    fn skip_board() -> Grid {
        let mut g = Grid::new(3, 2);
        g.set_item_count(12);
        let mut flags = vec![true; 12];
        for real in [0, 2, 7, 9] {
            flags[real] = false;
        }
        g.set_empty_flags(flags);
        g.skip_empty_cells = true;
        g
    }

    #[test]
    fn skip_empty_cells_skips_within_a_row() {
        let mut g = skip_board();
        assert!(g.move_selection(1, 0));
        assert_eq!(g.current_index(), 2);
    }

    #[test]
    fn skip_empty_cells_prefers_the_nearer_off_column_tile() {
        let mut g = skip_board();
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 7);
    }

    #[test]
    fn skip_empty_cells_page_by_lands_on_first_real_cell() {
        let mut g = skip_board();
        assert!(g.page_by(1));
        assert_eq!(g.current_index(), 7);
    }

    #[test]
    fn skip_empty_cells_wraps_to_the_nearest_candidate_at_the_far_edge() {
        let mut g = skip_board();
        g.set_current_index_immediate(9);
        assert!(g.move_selection(0, 1));
        assert_eq!(g.current_index(), 0);
    }

    #[test]
    fn skip_empty_cells_with_no_reachable_cell_does_not_move() {
        let mut g = Grid::new(2, 2);
        g.set_item_count(4);
        g.set_empty_flags(vec![false, true, true, true]);
        g.skip_empty_cells = true;
        assert!(!g.move_selection(0, 1));
        assert_eq!(g.current_index(), 0);
        assert!(!g.move_selection(0, -1));
        assert_eq!(g.current_index(), 0);
    }
}
