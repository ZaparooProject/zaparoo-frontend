// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Small, renderer-independent compositor for cached page transitions.
//!
//! Slint renders each endpoint once. During motion this module moves pixels
//! from those endpoint frames without traversing the component tree again.

use std::time::{Duration, Instant};

const FRAME_PERIOD: Duration = Duration::from_micros(16_667);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    fn right(self) -> usize {
        self.x + self.width
    }

    fn bottom(self) -> usize {
        self.y + self.height
    }

    fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    fn fits(self, frame_width: usize, frame_height: usize) -> bool {
        self.width > 0
            && self.height > 0
            && self
                .x
                .checked_add(self.width)
                .is_some_and(|right| right <= frame_width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|bottom| bottom <= frame_height)
    }
}

/// Which way the pages travel, in the coordinates of the frame they are
/// composed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// How the software renderer turns the scene into the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rotation {
    None,
    /// A quarter turn to the right.
    Cw,
    /// A quarter turn to the left.
    Ccw,
}

/// Maps a region and the direction its pages travel from the scene's own
/// coordinates into the frame the renderer rotates that scene into.
/// `scene` is the scene's width and height before rotation. None when the
/// region does not fit the scene.
pub(crate) fn to_frame(
    rect: Rect,
    direction: Direction,
    rotation: Rotation,
    scene: (usize, usize),
) -> Option<(Rect, Direction)> {
    let rect = rect_to_frame(rect, rotation, scene)?;
    Some((
        rect,
        match rotation {
            Rotation::None => direction,
            // The scene's top edge is the frame's right edge.
            Rotation::Cw => match direction {
                Direction::Up => Direction::Right,
                Direction::Down => Direction::Left,
                Direction::Left => Direction::Up,
                Direction::Right => Direction::Down,
            },
            // The scene's top edge is the frame's left edge.
            Rotation::Ccw => match direction {
                Direction::Up => Direction::Left,
                Direction::Down => Direction::Right,
                Direction::Left => Direction::Down,
                Direction::Right => Direction::Up,
            },
        },
    ))
}

/// Maps a region from the scene's own coordinates into the frame the
/// renderer rotates that scene into. None when it does not fit the scene.
pub(crate) fn rect_to_frame(rect: Rect, rotation: Rotation, scene: (usize, usize)) -> Option<Rect> {
    if !rect.fits(scene.0, scene.1) {
        return None;
    }
    let turned = |x, y| Rect {
        x,
        y,
        width: rect.height,
        height: rect.width,
    };
    Some(match rotation {
        Rotation::None => rect,
        Rotation::Cw => turned(scene.1 - rect.y - rect.height, rect.x),
        Rotation::Ccw => turned(rect.y, scene.0 - rect.x - rect.width),
    })
}

/// The clock a transition's steps are counted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Started {
    /// Wall time. A late presentation jumps directly to the current step
    /// instead of replaying obsolete cached frames.
    Wall(Instant),
    /// The frame loop's stepped timeline, as its value at the start. It
    /// moves one refresh per presented frame, so a late frame delays the
    /// slide by that frame and every step is shown.
    Timeline(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub rect: Rect,
    pub gap: usize,
    pub direction: Direction,
    pub total_frames: u32,
    pub started: Started,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub damage: Rect,
    pub finished: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransitionError {
    Frame,
    Rect,
    Duration,
}

#[derive(Debug)]
pub(crate) struct Active<T> {
    spec: Spec,
    source_region: Vec<T>,
    background: T,
    /// A still background in frame layout, and each page's per-row content
    /// spans `(start, len)`: with one, only the content moves and the
    /// background shows through everywhere else, gap included.
    bg: Option<std::sync::Arc<Vec<T>>>,
    source_spans: Vec<Vec<(usize, usize)>>,
    destination_spans: Option<Vec<Vec<(usize, usize)>>>,
    frame_index: u32,
    frame_width: usize,
    frame_height: usize,
}

impl<T: Copy> Active<T> {
    pub fn new(
        source: &[T],
        frame_width: usize,
        frame_height: usize,
        spec: Spec,
        background: T,
    ) -> Result<Self, TransitionError> {
        if spec.total_frames == 0 {
            return Err(TransitionError::Duration);
        }
        if !spec.rect.fits(frame_width, frame_height) {
            return Err(TransitionError::Rect);
        }
        let Some(frame_len) = frame_width.checked_mul(frame_height) else {
            return Err(TransitionError::Frame);
        };
        if source.len() < frame_len {
            return Err(TransitionError::Frame);
        }

        let mut source_region = Vec::with_capacity(spec.rect.width * spec.rect.height);
        for row in 0..spec.rect.height {
            let start = (spec.rect.y + row) * frame_width + spec.rect.x;
            source_region.extend_from_slice(&source[start..start + spec.rect.width]);
        }

        Ok(Self {
            spec,
            source_region,
            background,
            bg: None,
            source_spans: Vec::new(),
            destination_spans: None,
            frame_index: 0,
            frame_width,
            frame_height,
        })
    }

    /// Composes the step the transition's own clock has reached.
    /// `timeline` is the stepped timeline's current value; a transition
    /// started on wall time ignores it.
    pub fn compose_now(
        &mut self,
        destination: &[T],
        output: &mut [T],
        timeline: Duration,
    ) -> Result<Step, TransitionError> {
        let elapsed = match self.spec.started {
            Started::Wall(started) => started.elapsed(),
            Started::Timeline(started) => timeline.saturating_sub(started),
        };
        self.compose_at(destination, output, elapsed)
    }

    fn compose_at(
        &mut self,
        destination: &[T],
        output: &mut [T],
        elapsed: Duration,
    ) -> Result<Step, TransitionError> {
        // The first frame is step one, and the step follows the time that
        // has passed on the transition's clock.
        let step = elapsed.as_micros() / FRAME_PERIOD.as_micros() + 1;
        self.compose_frame(destination, output, u32::try_from(step).unwrap_or(u32::MAX))
    }

    /// How far the pages have travelled at `step`, in frame pixels.
    #[cfg(test)]
    pub(crate) fn offset_at(&self, step: u32) -> usize {
        eased_offset(step, self.spec.total_frames, self.travel())
    }

    #[cfg(test)]
    pub(crate) fn compose_step(
        &mut self,
        destination: &[T],
        output: &mut [T],
        step: u32,
    ) -> Result<Step, TransitionError> {
        self.compose_frame(destination, output, step)
    }

    #[cfg(test)]
    fn compose_next(
        &mut self,
        destination: &[T],
        output: &mut [T],
    ) -> Result<Step, TransitionError> {
        self.compose_frame(destination, output, self.frame_index.saturating_add(1))
    }

    fn compose_frame(
        &mut self,
        destination: &[T],
        output: &mut [T],
        step: u32,
    ) -> Result<Step, TransitionError> {
        let Some(frame_len) = self.frame_width.checked_mul(self.frame_height) else {
            return Err(TransitionError::Frame);
        };
        if destination.len() < frame_len || output.len() < frame_len {
            return Err(TransitionError::Frame);
        }

        self.frame_index = self.frame_index.max(step);
        if self.frame_index >= self.spec.total_frames {
            output[..frame_len].copy_from_slice(&destination[..frame_len]);
            return Ok(Step {
                damage: Rect {
                    x: 0,
                    y: 0,
                    width: self.frame_width,
                    height: self.frame_height,
                },
                finished: true,
            });
        }

        self.fill_region(output);
        let travel = self.travel();
        let offset = eased_offset(self.frame_index, self.spec.total_frames, travel);
        // Where each page's leading edge sits along the axis of travel.
        let (source_start, destination_start) = match self.spec.direction {
            Direction::Up | Direction::Left => {
                (-(offset as isize), travel as isize - offset as isize)
            }
            Direction::Down | Direction::Right => {
                (offset as isize, offset as isize - travel as isize)
            }
        };
        match self.spec.direction {
            Direction::Up | Direction::Down => {
                self.blit_source_vertical(output, source_start);
                self.blit_destination_vertical(destination, output, destination_start);
            }
            Direction::Left | Direction::Right => {
                self.blit_source_horizontal(output, source_start);
                self.blit_destination_horizontal(destination, output, destination_start);
            }
        }

        Ok(Step {
            damage: self.spec.rect,
            finished: false,
        })
    }

    /// One page period along the axis of travel.
    fn travel(&self) -> usize {
        match self.spec.direction {
            Direction::Up | Direction::Down => self.spec.rect.height,
            Direction::Left | Direction::Right => self.spec.rect.width,
        }
        .saturating_add(self.spec.gap)
    }

    fn fill_region(&self, output: &mut [T]) {
        for row in 0..self.spec.rect.height {
            let start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
            if let Some(bg) = &self.bg {
                output[start..start + self.spec.rect.width]
                    .copy_from_slice(&bg[start..start + self.spec.rect.width]);
            } else {
                output[start..start + self.spec.rect.width].fill(self.background);
            }
        }
    }

    /// Copy a page row's content spans that fall inside the page
    /// columns `[from, from + columns)` to output row `out_row`, shifted so
    /// page column `from` lands at band column `to`.
    #[allow(
        clippy::too_many_arguments,
        reason = "one call shape for the four blits; the row geometry is plain numbers"
    )]
    fn copy_spans(
        &self,
        spans: &[(usize, usize)],
        page: &[T],
        page_row_start: usize,
        output: &mut [T],
        out_row: usize,
        from: usize,
        to: usize,
        columns: usize,
    ) {
        let out_start = (self.spec.rect.y + out_row) * self.frame_width + self.spec.rect.x;
        for &(start, len) in spans {
            let a = start.max(from);
            let b = (start + len).min(from + columns);
            if a >= b {
                continue;
            }
            let dst = out_start + to + (a - from);
            output[dst..dst + (b - a)]
                .copy_from_slice(&page[page_row_start + a..page_row_start + b]);
        }
    }

    /// The part of a page still inside the region once its leading edge
    /// sits at `start` along an axis `extent` long: where it begins in the
    /// page, where that lands in the region, and how much of it shows.
    fn visible_span(start: isize, extent: usize) -> Option<(usize, usize, usize)> {
        let extent = extent as isize;
        let destination_start = start.max(0);
        let destination_end = (start + extent).min(extent);
        if destination_end <= destination_start {
            return None;
        }
        Some((
            (destination_start - start) as usize,
            destination_start as usize,
            (destination_end - destination_start) as usize,
        ))
    }

    fn visible_rows(&self, top: isize) -> Option<(usize, usize, usize)> {
        Self::visible_span(top, self.spec.rect.height)
    }

    fn visible_columns(&self, left: isize) -> Option<(usize, usize, usize)> {
        Self::visible_span(left, self.spec.rect.width)
    }

    fn blit_source_horizontal(&self, output: &mut [T], left: isize) {
        let Some((source_x, destination_x, columns)) = self.visible_columns(left) else {
            return;
        };
        if self.bg.is_some() {
            for row in 0..self.spec.rect.height {
                self.copy_spans(
                    &self.source_spans[row],
                    &self.source_region,
                    row * self.spec.rect.width,
                    output,
                    row,
                    source_x,
                    destination_x,
                    columns,
                );
            }
            return;
        }
        for row in 0..self.spec.rect.height {
            let source_start = row * self.spec.rect.width + source_x;
            let destination_start =
                (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x + destination_x;
            output[destination_start..destination_start + columns]
                .copy_from_slice(&self.source_region[source_start..source_start + columns]);
        }
    }

    fn blit_destination_horizontal(&self, destination: &[T], output: &mut [T], left: isize) {
        let Some((source_x, destination_x, columns)) = self.visible_columns(left) else {
            return;
        };
        if let (Some(_), Some(spans)) = (&self.bg, &self.destination_spans) {
            for (row, row_spans) in spans.iter().enumerate() {
                let row_start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
                self.copy_spans(
                    row_spans,
                    destination,
                    row_start,
                    output,
                    row,
                    source_x,
                    destination_x,
                    columns,
                );
            }
            return;
        }
        for row in 0..self.spec.rect.height {
            let row_start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
            output[row_start + destination_x..row_start + destination_x + columns].copy_from_slice(
                &destination[row_start + source_x..row_start + source_x + columns],
            );
        }
    }

    fn blit_source_vertical(&self, output: &mut [T], top: isize) {
        let Some((source_y, destination_y, rows)) = self.visible_rows(top) else {
            return;
        };
        if self.bg.is_some() {
            for row in 0..rows {
                self.copy_spans(
                    &self.source_spans[source_y + row],
                    &self.source_region,
                    (source_y + row) * self.spec.rect.width,
                    output,
                    destination_y + row,
                    0,
                    0,
                    self.spec.rect.width,
                );
            }
            return;
        }
        for row in 0..rows {
            let source_start = (source_y + row) * self.spec.rect.width;
            let destination_start =
                (self.spec.rect.y + destination_y + row) * self.frame_width + self.spec.rect.x;
            output[destination_start..destination_start + self.spec.rect.width].copy_from_slice(
                &self.source_region[source_start..source_start + self.spec.rect.width],
            );
        }
    }

    fn blit_destination_vertical(&self, destination: &[T], output: &mut [T], top: isize) {
        let Some((source_y, destination_y, rows)) = self.visible_rows(top) else {
            return;
        };
        if let (Some(_), Some(spans)) = (&self.bg, &self.destination_spans) {
            for row in 0..rows {
                self.copy_spans(
                    &spans[source_y + row],
                    destination,
                    (self.spec.rect.y + source_y + row) * self.frame_width + self.spec.rect.x,
                    output,
                    destination_y + row,
                    0,
                    0,
                    self.spec.rect.width,
                );
            }
            return;
        }
        for row in 0..rows {
            let source_start =
                (self.spec.rect.y + source_y + row) * self.frame_width + self.spec.rect.x;
            let destination_start =
                (self.spec.rect.y + destination_y + row) * self.frame_width + self.spec.rect.x;
            output[destination_start..destination_start + self.spec.rect.width]
                .copy_from_slice(&destination[source_start..source_start + self.spec.rect.width]);
        }
    }
}

impl<T: Blend> Active<T> {
    /// Slide only content over a still background. A pixel that equals the
    /// background's is background; everything else is content, including
    /// the partly transparent edges of tiles, which carry a sliver of the
    /// background they were drawn over until the last step, when the frame
    /// is Slint's own again. Returns the share of the outgoing band that
    /// matched the background exactly.
    pub fn set_background(&mut self, bg: std::sync::Arc<Vec<T>>) -> f64 {
        let (w, h) = (self.spec.rect.width, self.spec.rect.height);
        let mut matched = 0usize;
        let mut spans = Vec::with_capacity(h);
        for row in 0..h {
            let bg_start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
            let page = &self.source_region[row * w..(row + 1) * w];
            let (row_spans, row_matched) = content_spans(page, &bg[bg_start..bg_start + w]);
            matched += row_matched;
            spans.push(row_spans);
        }
        self.source_spans = spans;
        self.bg = Some(bg);
        matched as f64 / (w * h).max(1) as f64
    }

    /// The incoming page's spans, taken once Slint has rendered it.
    pub fn ensure_destination_spans(&mut self, destination: &[T]) -> Option<f64> {
        let bg = self.bg.as_ref()?;
        if self.destination_spans.is_some() {
            return None;
        }
        let (w, h) = (self.spec.rect.width, self.spec.rect.height);
        let mut matched = 0usize;
        let mut spans = Vec::with_capacity(h);
        for row in 0..h {
            let start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
            let (row_spans, row_matched) =
                content_spans(&destination[start..start + w], &bg[start..start + w]);
            matched += row_matched;
            spans.push(row_spans);
        }
        self.destination_spans = Some(spans);
        Some(matched as f64 / (w * h).max(1) as f64)
    }
}

/// Runs of `page` that differ from `bg`, and how many pixels matched.
fn content_spans<T: Blend>(page: &[T], bg: &[T]) -> (Vec<(usize, usize)>, usize) {
    let mut spans = Vec::new();
    let mut matched = 0;
    let mut start = None;
    for (x, (p, b)) in page.iter().zip(bg).enumerate() {
        if p.same(*b) {
            matched += 1;
            if let Some(s) = start.take() {
                spans.push((s, x - s));
            }
        } else if start.is_none() {
            start = Some(x);
        }
    }
    if let Some(s) = start {
        spans.push((s, page.len() - s));
    }
    (spans, matched)
}

/// A presenter pixel the cover fade can mix.
pub(crate) trait Blend: Copy {
    fn same(self, other: Self) -> bool;
    /// Whether two runs of pixels match. Pixel types that can be compared
    /// as plain memory override this.
    fn same_run(left: &[Self], right: &[Self]) -> bool {
        left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.same(*b))
    }
    /// `old` taken `weight` parts in `BLEND_ONE` of the way to `new`.
    fn mix(old: Self, new: Self, weight: u32) -> Self;
}

pub(crate) const BLEND_ONE: u32 = 32;

/// Refreshes a cover takes to fade in: `Motion.cover-reveal-ms`, 180 ms,
/// at 60 Hz.
const COVER_FADE_FRAMES: u32 = 11;

struct Fade<T> {
    rect: Rect,
    /// What was under the cover before it landed.
    old: Vec<T>,
    /// What Slint painted when it landed. Empty until that render.
    new: Vec<T>,
    started: Started,
    weight: u32,
}

/// Covers fading in on grid tiles, composed on the way to the display.
/// Slint paints a landed cover once, at full strength, and never repaints
/// it for the fade: each output row that crosses a fading cover mixes the
/// pixels that were there before into the canonical frame.
pub(crate) struct CoverFades<T> {
    fades: Vec<Fade<T>>,
    frame_width: usize,
    frame_height: usize,
    /// Fades that ended early, to republish from the canonical frame.
    ended: Vec<Rect>,
    damage: Vec<Rect>,
    row: Vec<T>,
}

impl<T: Blend> CoverFades<T> {
    pub fn new() -> Self {
        Self {
            fades: Vec::new(),
            frame_width: 0,
            frame_height: 0,
            ended: Vec::new(),
            damage: Vec::new(),
            row: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        !self.fades.is_empty()
    }

    /// Drops every fade without republishing anything. For callers that
    /// are about to republish the region the fades sat in themselves.
    pub fn clear(&mut self) {
        self.fades.clear();
        self.ended.clear();
    }

    /// Keeps what is under each region before the render that lands their
    /// covers. A fade already running where a new one starts ends there.
    pub fn begin(
        &mut self,
        canonical: &[T],
        width: usize,
        height: usize,
        rects: &[Rect],
        started: Started,
    ) {
        if (width, height) != (self.frame_width, self.frame_height) {
            self.clear();
            self.frame_width = width;
            self.frame_height = height;
        }
        if canonical.len() < width * height {
            return;
        }
        for &rect in rects {
            if !rect.fits(width, height) {
                continue;
            }
            self.end_where(|fade| fade.rect.intersects(rect));
            self.fades.push(Fade {
                rect,
                old: copy_region(canonical, width, rect),
                new: Vec::new(),
                started,
                weight: 0,
            });
        }
    }

    /// After Slint renders `damage` into `canonical`: a fade that just
    /// landed narrows to the pixels its cover changed, and one whose pixels
    /// Slint has since painted over (the focus ring arriving, a page
    /// change, a modal) ends there, showing what Slint painted.
    pub fn rendered(&mut self, canonical: &[T], damage: &[Rect]) {
        let width = self.frame_width;
        if canonical.len() < width * self.frame_height {
            self.clear();
            return;
        }
        let mut index = 0;
        while index < self.fades.len() {
            let fade = &mut self.fades[index];
            if fade.new.is_empty() {
                let Some(bounds) = changed_bounds(&fade.old, canonical, width, fade.rect) else {
                    // Nothing under it changed, so there is nothing to fade.
                    self.fades.swap_remove(index);
                    continue;
                };
                fade.old = crop(&fade.old, fade.rect, bounds);
                fade.new = copy_region(canonical, width, bounds);
                fade.rect = bounds;
            } else if damage.iter().any(|&rect| {
                rect.intersects(fade.rect)
                    && !region_matches(canonical, width, fade.rect, &fade.new, rect)
            }) {
                self.ended.push(fade.rect);
                self.fades.swap_remove(index);
                continue;
            }
            index += 1;
        }
    }

    /// Takes every fade to the step its clock has reached and returns the
    /// regions that changed on the display: each running fade, each one
    /// that finished or ended, which now shows the canonical frame.
    /// `timeline` is the stepped timeline's current value.
    pub fn step(&mut self, timeline: Duration) -> &[Rect] {
        self.damage.clear();
        self.damage.append(&mut self.ended);
        let damage = &mut self.damage;
        self.fades.retain_mut(|fade| {
            if fade.new.is_empty() {
                return true;
            }
            let elapsed = match fade.started {
                Started::Wall(started) => started.elapsed(),
                Started::Timeline(started) => timeline.saturating_sub(started),
            };
            let step = elapsed.as_micros() / FRAME_PERIOD.as_micros() + 1;
            damage.push(fade.rect);
            if step >= u128::from(COVER_FADE_FRAMES) {
                return false;
            }
            fade.weight = fade_weight(step as u32);
            true
        });
        &self.damage
    }

    /// The span of output row `y` from `x0` that `canonical` holds, with
    /// every running fade across it mixed in.
    pub fn row<'a>(&'a mut self, y: usize, x0: usize, canonical: &'a [T]) -> &'a [T] {
        let x1 = x0 + canonical.len();
        let mut mixed = false;
        for fade in &self.fades {
            let rect = fade.rect;
            if fade.new.is_empty()
                || y < rect.y
                || y >= rect.bottom()
                || rect.x >= x1
                || rect.right() <= x0
            {
                continue;
            }
            if !mixed {
                self.row.clear();
                self.row.extend_from_slice(canonical);
                mixed = true;
            }
            let from = rect.x.max(x0);
            let to = rect.right().min(x1);
            let start = (y - rect.y) * rect.width + from - rect.x;
            let old = &fade.old[start..start + to - from];
            for (out, &before) in self.row[from - x0..to - x0].iter_mut().zip(old) {
                *out = T::mix(before, *out, fade.weight);
            }
        }
        if mixed {
            &self.row
        } else {
            canonical
        }
    }

    /// The stretches of output row `y` between `x0` and `x1` that running
    /// fades cover, mixed, each handed to `out` with the column it starts
    /// at. `row` is the whole canonical row. For a presenter that copies
    /// the row as it is and then overwrites only these.
    pub fn mixed_segments(
        &mut self,
        y: usize,
        x0: usize,
        x1: usize,
        row: &[T],
        mut out: impl FnMut(usize, &[T]),
    ) {
        for fade in &self.fades {
            let rect = fade.rect;
            if fade.new.is_empty() || y < rect.y || y >= rect.bottom() {
                continue;
            }
            let from = rect.x.max(x0);
            let to = rect.right().min(x1);
            if to <= from {
                continue;
            }
            let start = (y - rect.y) * rect.width + from - rect.x;
            self.row.clear();
            self.row.extend(
                fade.old[start..start + to - from]
                    .iter()
                    .zip(&row[from..to])
                    .map(|(&before, &now)| T::mix(before, now, fade.weight)),
            );
            out(from, &self.row);
        }
    }

    fn end_where(&mut self, ends: impl Fn(&Fade<T>) -> bool) {
        let ended = &mut self.ended;
        self.fades.retain(|fade| {
            let end = ends(fade);
            if end && !fade.new.is_empty() {
                ended.push(fade.rect);
            }
            !end
        });
    }
}

/// How far toward the landed cover a fade is at `step`, eased out like
/// the opacity animation it stands in for.
fn fade_weight(step: u32) -> u32 {
    let t = f64::from(step) / f64::from(COVER_FADE_FRAMES);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    ((eased * f64::from(BLEND_ONE)).round() as u32).min(BLEND_ONE)
}

fn copy_region<T: Copy>(frame: &[T], width: usize, rect: Rect) -> Vec<T> {
    let mut region = Vec::with_capacity(rect.width * rect.height);
    for row in rect.y..rect.bottom() {
        let start = row * width + rect.x;
        region.extend_from_slice(&frame[start..start + rect.width]);
    }
    region
}

/// Whether `frame` still holds `region`, copied for `rect`, where `rect`
/// meets `damage`: nothing outside what was repainted can have changed.
fn region_matches<T: Blend>(
    frame: &[T],
    width: usize,
    rect: Rect,
    region: &[T],
    damage: Rect,
) -> bool {
    let x0 = rect.x.max(damage.x);
    let x1 = rect.right().min(damage.right());
    let y0 = rect.y.max(damage.y);
    let y1 = rect.bottom().min(damage.bottom());
    if x1 <= x0 || y1 <= y0 {
        return true;
    }
    (y0..y1).all(|y| {
        let start = (y - rect.y) * rect.width + x0 - rect.x;
        T::same_run(
            &frame[y * width + x0..y * width + x1],
            &region[start..start + x1 - x0],
        )
    })
}

/// The smallest region inside `rect` where `frame` no longer holds `old`.
fn changed_bounds<T: Blend>(old: &[T], frame: &[T], width: usize, rect: Rect) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for row in 0..rect.height {
        let start = (rect.y + row) * width + rect.x;
        let now = &frame[start..start + rect.width];
        let then = &old[row * rect.width..(row + 1) * rect.width];
        if T::same_run(now, then) {
            continue;
        }
        let Some(first) = now.iter().zip(then).position(|(a, b)| !a.same(*b)) else {
            continue;
        };
        let last = now
            .iter()
            .zip(then)
            .rposition(|(a, b)| !a.same(*b))
            .unwrap_or(first);
        x0 = x0.min(first);
        x1 = x1.max(last + 1);
        y0 = y0.min(row);
        y1 = row + 1;
    }
    (x1 > x0).then(|| Rect {
        x: rect.x + x0,
        y: rect.y + y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// The part of a region copied for `rect` that `bounds`, inside it, covers.
fn crop<T: Copy>(region: &[T], rect: Rect, bounds: Rect) -> Vec<T> {
    let mut cropped = Vec::with_capacity(bounds.width * bounds.height);
    for row in bounds.y - rect.y..bounds.bottom() - rect.y {
        let start = row * rect.width + bounds.x - rect.x;
        cropped.extend_from_slice(&region[start..start + bounds.width]);
    }
    cropped
}

/// Eased out, like the slide Slint draws where it draws one: the page
/// leaves at speed and settles into place.
fn eased_offset(frame: u32, total_frames: u32, travel: usize) -> usize {
    let t = (f64::from(frame) / f64::from(total_frames)).min(1.0);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    (eased * travel as f64).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_WIDTH: usize = 4;
    const FRAME_HEIGHT: usize = 6;
    const RECT: Rect = Rect {
        x: 1,
        y: 1,
        width: 2,
        height: 4,
    };

    fn frame(base: u8) -> Vec<u8> {
        (0..FRAME_WIDTH * FRAME_HEIGHT)
            .map(|index| base.saturating_add(index as u8))
            .collect()
    }

    fn active(direction: Direction) -> Option<Active<u8>> {
        active_over(direction, 2)
    }

    fn active_over(direction: Direction, total_frames: u32) -> Option<Active<u8>> {
        let source = frame(0);
        Active::new(
            &source,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Spec {
                rect: RECT,
                gap: 1,
                direction,
                total_frames,
                started: Started::Wall(Instant::now()),
            },
            0xff,
        )
        .ok()
    }

    #[test]
    fn overdue_cached_motion_lands_without_replaying_frames() {
        let transition = active(Direction::Up);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };
        let started = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        transition.spec.started = Started::Wall(started);
        let destination = frame(100);
        let mut output = frame(0);
        let result = transition.compose_now(&destination, &mut output, Duration::ZERO);
        assert!(matches!(result, Ok(Step { finished: true, .. })));
        assert_eq!(output, destination);
    }

    #[test]
    fn timeline_motion_takes_one_step_per_period_and_ignores_wall_time() {
        let source = frame(0);
        let destination = frame(100);
        let start = Duration::from_secs(7);
        let spec = Spec {
            rect: RECT,
            gap: 1,
            direction: Direction::Up,
            total_frames: 4,
            started: Started::Timeline(start),
        };
        let transition = Active::new(&source, FRAME_WIDTH, FRAME_HEIGHT, spec, 0xff);
        assert!(transition.is_ok(), "valid transition rejected");
        let Ok(mut transition) = transition else {
            return;
        };
        let mut output = source.clone();
        // However long the frames really took, each period is one step.
        for (periods, step) in [(0_u32, 1_u32), (1, 2), (1, 2), (2, 3)] {
            std::thread::sleep(Duration::from_millis(3));
            let result =
                transition.compose_now(&destination, &mut output, start + FRAME_PERIOD * periods);
            assert!(matches!(
                result,
                Ok(Step {
                    finished: false,
                    ..
                })
            ));
            assert_eq!(transition.frame_index, step);
        }
        // A timeline that reads earlier than the start is still step one.
        let result = transition.compose_now(&destination, &mut output, Duration::ZERO);
        assert!(result.is_ok());
        assert_eq!(transition.frame_index, 3);
        let result = transition.compose_now(&destination, &mut output, start + FRAME_PERIOD * 3);
        assert!(matches!(result, Ok(Step { finished: true, .. })));
        assert_eq!(output, destination);
    }

    #[test]
    fn a_pal_timeline_keeps_the_slide_the_same_length_in_time() {
        let source = frame(0);
        let destination = frame(100);
        let spec = Spec {
            rect: RECT,
            gap: 0,
            direction: Direction::Down,
            total_frames: 15,
            started: Started::Timeline(Duration::ZERO),
        };
        let transition = Active::new(&source, FRAME_WIDTH, FRAME_HEIGHT, spec, 0xff);
        assert!(transition.is_ok(), "valid transition rejected");
        let Ok(mut transition) = transition else {
            return;
        };
        let mut output = source.clone();
        let pal = Duration::from_millis(20);
        let mut frames = 0_u32;
        loop {
            let result = transition.compose_now(&destination, &mut output, pal * frames);
            frames += 1;
            if matches!(result, Ok(Step { finished: true, .. })) {
                break;
            }
            assert!(result.is_ok() && frames < 15);
        }
        // Fifteen 60 Hz steps are a quarter second: thirteen 50 Hz frames.
        assert_eq!(frames, 13);
        assert_eq!(output, destination);
    }

    #[test]
    fn cached_motion_does_not_advance_without_elapsed_time() {
        let transition = active(Direction::Down);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };
        let destination = frame(100);
        let mut output = frame(0);
        assert!(transition
            .compose_at(&destination, &mut output, Duration::ZERO)
            .is_ok());
        let first = output.clone();
        assert!(matches!(
            transition.compose_at(&destination, &mut output, Duration::ZERO),
            Ok(Step {
                finished: false,
                ..
            })
        ));
        assert_eq!(output, first);
    }

    fn region_row(frame: &[u8], y: usize) -> &[u8] {
        let start = y * FRAME_WIDTH + RECT.x;
        &frame[start..start + RECT.width]
    }

    #[test]
    fn upward_step_places_source_above_incoming_page() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let transition = active_over(Direction::Up, 3);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };

        let result = transition.compose_next(&destination, &mut output);
        assert!(matches!(
            result,
            Ok(Step {
                finished: false,
                ..
            })
        ));

        // A third of the way through a five-row journey, eased out to three
        // rows: final source row, one-row background gap, then first two
        // destination rows.
        assert_eq!(region_row(&output, 1), &[17, 18]);
        assert_eq!(region_row(&output, 2), &[0xff, 0xff]);
        assert_eq!(region_row(&output, 3), &[105, 106]);
        assert_eq!(region_row(&output, 4), &[109, 110]);
        assert_eq!(output[0], source[0]);
    }

    #[test]
    fn downward_step_places_incoming_page_above_source() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let transition = active_over(Direction::Down, 3);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };

        let result = transition.compose_next(&destination, &mut output);
        assert!(matches!(
            result,
            Ok(Step {
                finished: false,
                ..
            })
        ));

        assert_eq!(region_row(&output, 1), &[113, 114]);
        assert_eq!(region_row(&output, 2), &[117, 118]);
        assert_eq!(region_row(&output, 3), &[0xff, 0xff]);
        assert_eq!(region_row(&output, 4), &[5, 6]);
    }

    #[test]
    fn leftward_step_places_source_left_of_incoming_page() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let spec = Spec {
            rect: Rect {
                x: 0,
                y: 1,
                width: 4,
                height: 2,
            },
            gap: 1,
            direction: Direction::Left,
            total_frames: 5,
            started: Started::Wall(Instant::now()),
        };
        let transition = Active::new(&source, FRAME_WIDTH, FRAME_HEIGHT, spec, 0xff);
        assert!(transition.is_ok(), "valid transition rejected");
        let Ok(mut transition) = transition else {
            return;
        };
        // Step one of five on a five-column journey, eased out, is two
        // columns.
        assert_eq!(transition.offset_at(1), 2);
        assert!(transition
            .compose_step(&destination, &mut output, 1)
            .is_ok());
        // Last two source columns, the one-column gap, first destination
        // column.
        assert_eq!(&output[4..8], &[6, 7, 0xff, 104]);
        assert_eq!(&output[8..12], &[10, 11, 0xff, 108]);
        assert_eq!(&output[..4], &source[..4]);
        assert_eq!(&output[12..], &source[12..]);
    }

    #[test]
    fn rightward_step_places_incoming_page_left_of_source() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let spec = Spec {
            rect: Rect {
                x: 0,
                y: 1,
                width: 4,
                height: 2,
            },
            gap: 1,
            direction: Direction::Right,
            total_frames: 5,
            started: Started::Wall(Instant::now()),
        };
        let transition = Active::new(&source, FRAME_WIDTH, FRAME_HEIGHT, spec, 0xff);
        assert!(transition.is_ok(), "valid transition rejected");
        let Ok(mut transition) = transition else {
            return;
        };
        assert!(transition
            .compose_step(&destination, &mut output, 1)
            .is_ok());
        // Last destination column, the gap, first two source columns.
        assert_eq!(&output[4..8], &[107, 0xff, 4, 5]);
        assert_eq!(&output[8..12], &[111, 0xff, 8, 9]);
    }

    #[test]
    fn scene_regions_map_into_the_rotated_frame() {
        // A 6x4 scene; the band is its rows 1..3, full width.
        let band = Rect {
            x: 0,
            y: 1,
            width: 6,
            height: 2,
        };
        assert_eq!(
            to_frame(band, Direction::Up, Rotation::None, (6, 4)),
            Some((band, Direction::Up))
        );
        // Turned right the frame is 4x6 and the scene's top is its right
        // edge: rows 1..3 become columns 1..3 counted from the right.
        assert_eq!(
            to_frame(band, Direction::Up, Rotation::Cw, (6, 4)),
            Some((
                Rect {
                    x: 1,
                    y: 0,
                    width: 2,
                    height: 6
                },
                Direction::Right
            ))
        );
        // Turned left the scene's top is the frame's left edge.
        assert_eq!(
            to_frame(band, Direction::Down, Rotation::Ccw, (6, 4)),
            Some((
                Rect {
                    x: 1,
                    y: 0,
                    width: 2,
                    height: 6
                },
                Direction::Right
            ))
        );
        // An off-center region tells the two turns apart.
        let corner = Rect {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
        };
        assert_eq!(
            to_frame(corner, Direction::Left, Rotation::Cw, (6, 4)).map(|m| m.0),
            Some(Rect {
                x: 3,
                y: 1,
                width: 1,
                height: 2
            })
        );
        assert_eq!(
            to_frame(corner, Direction::Left, Rotation::Ccw, (6, 4)).map(|m| m.0),
            Some(Rect {
                x: 0,
                y: 3,
                width: 1,
                height: 2
            })
        );
        for rotation in [Rotation::None, Rotation::Cw, Rotation::Ccw] {
            let tall = Rect { height: 4, ..band };
            assert!(to_frame(tall, Direction::Up, rotation, (6, 4)).is_none());
        }
    }

    #[test]
    fn every_direction_maps_to_a_distinct_frame_direction_and_back() {
        use Direction::{Down, Left, Right, Up};
        let unit = Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        for direction in [Up, Down, Left, Right] {
            let cw = to_frame(unit, direction, Rotation::Cw, (1, 1)).map(|m| m.1);
            let back = cw.and_then(|d| to_frame(unit, d, Rotation::Ccw, (1, 1)).map(|m| m.1));
            assert_eq!(back, Some(direction));
            assert_ne!(cw, Some(direction));
        }
    }

    #[test]
    fn final_step_restores_complete_destination() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source;
        let transition = active(Direction::Up);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };
        assert!(transition.compose_next(&destination, &mut output).is_ok());

        let result = transition.compose_next(&destination, &mut output);
        assert!(matches!(result, Ok(Step { finished: true, .. })));
        assert_eq!(output, destination);
    }

    #[test]
    fn rejects_out_of_bounds_regions() {
        let source = frame(0);
        let result = Active::new(
            &source,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Spec {
                rect: Rect {
                    x: 3,
                    y: 1,
                    width: 2,
                    height: 2,
                },
                gap: 1,
                direction: Direction::Up,
                total_frames: 15,
                started: Started::Wall(Instant::now()),
            },
            0xff,
        );
        assert!(matches!(result, Err(TransitionError::Rect)));
    }

    impl Blend for u8 {
        fn same(self, other: Self) -> bool {
            self == other
        }

        fn mix(old: Self, new: Self, weight: u32) -> Self {
            ((u32::from(old) * (BLEND_ONE - weight) + u32::from(new) * weight) / BLEND_ONE) as u8
        }
    }

    const TILE: Rect = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };

    /// A 4x6 frame whose rows 1..3, columns 1..3 hold `cover`.
    fn landed(cover: u8) -> Vec<u8> {
        let mut frame = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        for y in 1..3 {
            frame[y * FRAME_WIDTH + 1..y * FRAME_WIDTH + 3].fill(cover);
        }
        frame
    }

    fn output_row(fades: &mut CoverFades<u8>, frame: &[u8], y: usize) -> Vec<u8> {
        fades
            .row(y, 0, &frame[y * FRAME_WIDTH..(y + 1) * FRAME_WIDTH])
            .to_vec()
    }

    #[test]
    fn a_landed_cover_mixes_in_over_its_changed_pixels_and_ends_on_the_frame() {
        let before = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        let after = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &before,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&after, &[TILE]);
        let changed = Rect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        assert_eq!(fades.step(Duration::ZERO), &[changed]);
        let first = output_row(&mut fades, &after, 1);
        assert_eq!(first[0], 10, "outside the changed pixels is the frame");
        assert!(first[1] > 10 && first[1] < 250, "partway: {}", first[1]);
        assert_eq!(output_row(&mut fades, &after, 3), &after[12..16]);

        let mut last = first[1];
        for frame in 1..COVER_FADE_FRAMES - 1 {
            assert_eq!(fades.step(FRAME_PERIOD * frame), &[changed]);
            let now = output_row(&mut fades, &after, 2)[2];
            assert!(now > last, "step {frame} moves toward the cover");
            last = now;
        }
        assert_eq!(fades.step(FRAME_PERIOD * COVER_FADE_FRAMES), &[changed]);
        assert!(!fades.is_active());
        assert_eq!(output_row(&mut fades, &after, 1), &after[4..8]);
    }

    #[test]
    fn mixed_segments_cover_only_the_fading_stretch_of_a_row() {
        let before = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        let after = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &before,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&after, &[TILE]);
        let _ = fades.step(Duration::ZERO);
        let expected = output_row(&mut fades, &after, 1);
        let mut segments = Vec::new();
        fades.mixed_segments(1, 0, FRAME_WIDTH, &after[4..8], |x, mixed| {
            segments.push((x, mixed.to_vec()));
        });
        assert_eq!(segments, [(1, expected[1..3].to_vec())]);
        let mut none = 0;
        fades.mixed_segments(4, 0, FRAME_WIDTH, &after[16..20], |_, _| none += 1);
        assert_eq!(none, 0, "a row the fade does not cross has nothing to mix");
    }

    #[test]
    fn a_tile_slint_did_not_change_has_nothing_to_fade() {
        let frame = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &frame,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&frame, &[TILE]);
        assert!(!fades.is_active());
        assert!(fades.step(Duration::ZERO).is_empty());
    }

    #[test]
    fn a_repaint_over_a_fade_ends_it_only_when_it_changes_its_pixels() {
        let before = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        let after = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &before,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&after, &[TILE]);
        let _ = fades.step(Duration::ZERO);

        // Slint repainted the tile with the same pixels: the fade goes on.
        fades.rendered(&after, &[TILE]);
        assert!(fades.is_active());

        // The focus ring arrives over the cover: the fade ends on it.
        let mut ringed = after.clone();
        ringed[FRAME_WIDTH + 1] = 99;
        fades.rendered(&ringed, &[TILE]);
        assert!(!fades.is_active());
        assert_eq!(
            fades.step(FRAME_PERIOD),
            &[Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 2
            }]
        );
        assert_eq!(output_row(&mut fades, &ringed, 1), &ringed[4..8]);
    }

    #[test]
    fn a_repaint_beside_a_fade_leaves_it_running() {
        let before = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        let after = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &before,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&after, &[TILE]);
        let mut header = after.clone();
        header[5 * FRAME_WIDTH] = 0;
        fades.rendered(
            &header,
            &[Rect {
                x: 0,
                y: 5,
                width: 4,
                height: 1,
            }],
        );
        assert!(fades.is_active());
    }

    #[test]
    fn a_new_frame_size_drops_fades_composed_for_the_old_one() {
        let before = vec![10; FRAME_WIDTH * FRAME_HEIGHT];
        let after = landed(250);
        let mut fades = CoverFades::new();
        fades.begin(
            &before,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            &[TILE],
            Started::Timeline(Duration::ZERO),
        );
        fades.rendered(&after, &[TILE]);
        fades.begin(
            &after,
            FRAME_HEIGHT,
            FRAME_WIDTH,
            &[],
            Started::Timeline(Duration::ZERO),
        );
        assert!(!fades.is_active());
    }

    #[test]
    fn fade_weights_ease_out_toward_the_cover() {
        let weights: Vec<u32> = (1..COVER_FADE_FRAMES).map(fade_weight).collect();
        assert!(weights.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(weights[0] > BLEND_ONE / COVER_FADE_FRAMES);
        assert!(weights.iter().all(|&weight| weight <= BLEND_ONE));
    }

    /// A 6x4 frame whose band is rows 1..3. The background is 50, the
    /// outgoing page has content 1 at column 1, the incoming page content 2
    /// at column 4.
    fn background_slide(direction: Direction) -> (Active<u8>, Vec<u8>, std::sync::Arc<Vec<u8>>) {
        let bg: Vec<u8> = (0..24).map(|i| 50 + (i % 6) as u8).collect();
        let mut source = bg.clone();
        let mut destination = bg.clone();
        for row in 1..3 {
            source[row * 6 + 1] = 1;
            destination[row * 6 + 4] = 2;
        }
        let spec = Spec {
            rect: Rect {
                x: 0,
                y: 1,
                width: 6,
                height: 2,
            },
            gap: 0,
            direction,
            total_frames: 4,
            started: Started::Wall(Instant::now()),
        };
        let active = Active::new(&source, 6, 4, spec, 0);
        assert!(active.is_ok(), "valid transition rejected");
        let Ok(mut active) = active else {
            unreachable!();
        };
        let bg = std::sync::Arc::new(bg);
        let matched = active.set_background(bg.clone());
        assert!((matched - 10.0 / 12.0).abs() < 1e-9);
        active.ensure_destination_spans(&destination);
        (active, destination, bg)
    }

    #[test]
    fn over_a_background_only_content_moves() {
        let (mut active, destination, bg) = background_slide(Direction::Left);
        let mut output = vec![0u8; 24];
        assert!(active.compose_step(&destination, &mut output, 2).is_ok());
        for row in 1..3 {
            let line = &output[row * 6..row * 6 + 6];
            // Every pixel is background or content, never the gap color
            // or a background pixel carried from another column.
            for (x, &px) in line.iter().enumerate() {
                assert!(
                    px == 1 || px == 2 || px == bg[row * 6 + x],
                    "{row} {x} {px}"
                );
            }
            assert!(line.contains(&1) || line.contains(&2));
        }
        // Rows outside the band are untouched.
        assert!(output[..6].iter().all(|&px| px == 0));
    }

    #[test]
    fn over_a_background_the_last_step_is_the_canonical_frame() {
        for direction in [
            Direction::Up,
            Direction::Left,
            Direction::Down,
            Direction::Right,
        ] {
            let (mut active, destination, _) = background_slide(direction);
            let mut output = vec![0u8; 24];
            let step = active.compose_step(&destination, &mut output, 4);
            assert!(matches!(step, Ok(Step { finished: true, .. })));
            assert_eq!(output, destination);
        }
    }

    #[test]
    fn content_spans_are_the_runs_that_differ() {
        let bg = [5u8, 5, 5, 5, 5, 5];
        let page = [5u8, 1, 1, 5, 2, 5];
        assert_eq!(content_spans(&page, &bg), (vec![(1, 2), (4, 1)], 3));
        assert_eq!(content_spans(&[1u8, 1], &[5u8, 5]), (vec![(0, 2)], 0));
    }
}
