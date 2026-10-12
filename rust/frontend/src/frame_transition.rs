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
        Rotation::None => (rect, direction),
        // The scene's top edge is the frame's right edge.
        Rotation::Cw => (
            turned(scene.1 - rect.y - rect.height, rect.x),
            match direction {
                Direction::Up => Direction::Right,
                Direction::Down => Direction::Left,
                Direction::Left => Direction::Up,
                Direction::Right => Direction::Down,
            },
        ),
        // The scene's top edge is the frame's left edge.
        Rotation::Ccw => (
            turned(rect.y, scene.0 - rect.x - rect.width),
            match direction {
                Direction::Up => Direction::Left,
                Direction::Down => Direction::Right,
                Direction::Left => Direction::Down,
                Direction::Right => Direction::Up,
            },
        ),
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
        smoothstep_offset(step, self.spec.total_frames, self.travel())
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
        let offset = smoothstep_offset(self.frame_index, self.spec.total_frames, travel);
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
            output[start..start + self.spec.rect.width].fill(self.background);
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

fn smoothstep_offset(frame: u32, total_frames: u32, travel: usize) -> usize {
    let t = f64::from(frame) / f64::from(total_frames);
    let eased = t * t * (3.0 - 2.0 * t);
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
        let source = frame(0);
        Active::new(
            &source,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Spec {
                rect: RECT,
                gap: 1,
                direction,
                total_frames: 2,
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
        let transition = active(Direction::Up);
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

        // Halfway through a five-row journey: final source row, one-row
        // background gap, then first two destination rows.
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
        let transition = active(Direction::Down);
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
        // Step two of five on a five-column journey is two columns.
        assert_eq!(transition.offset_at(2), 2);
        assert!(transition
            .compose_step(&destination, &mut output, 2)
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
            .compose_step(&destination, &mut output, 2)
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
}
