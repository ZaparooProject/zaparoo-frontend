// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Small, renderer-independent compositor for cached page transitions.
//!
//! Slint renders each endpoint once. During motion this module moves pixels
//! from those endpoint frames without traversing the component tree again.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub rect: Rect,
    pub gap: usize,
    pub direction: Direction,
    pub total_frames: u32,
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

    pub fn compose_next(
        &mut self,
        destination: &[T],
        output: &mut [T],
    ) -> Result<Step, TransitionError> {
        let Some(frame_len) = self.frame_width.checked_mul(self.frame_height) else {
            return Err(TransitionError::Frame);
        };
        if destination.len() < frame_len || output.len() < frame_len {
            return Err(TransitionError::Frame);
        }

        self.frame_index = self.frame_index.saturating_add(1);
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
        match self.spec.direction {
            Direction::Up | Direction::Down => {
                let travel = self.spec.rect.height.saturating_add(self.spec.gap);
                let offset = smoothstep_offset(self.frame_index, self.spec.total_frames, travel);
                let (source_top, destination_top) = match self.spec.direction {
                    Direction::Up => (-(offset as isize), travel as isize - offset as isize),
                    Direction::Down => (offset as isize, offset as isize - travel as isize),
                    Direction::Left | Direction::Right => unreachable!(),
                };
                self.blit_source_vertical(output, source_top);
                self.blit_destination_vertical(destination, output, destination_top);
            }
            Direction::Left | Direction::Right => {
                let travel = self.spec.rect.width.saturating_add(self.spec.gap);
                let offset = smoothstep_offset(self.frame_index, self.spec.total_frames, travel);
                let (source_left, destination_left) = match self.spec.direction {
                    Direction::Left => (-(offset as isize), travel as isize - offset as isize),
                    Direction::Right => (offset as isize, offset as isize - travel as isize),
                    Direction::Up | Direction::Down => unreachable!(),
                };
                self.blit_source_horizontal(output, source_left);
                self.blit_destination_horizontal(destination, output, destination_left);
            }
        }

        Ok(Step {
            damage: self.spec.rect,
            finished: false,
        })
    }

    fn fill_region(&self, output: &mut [T]) {
        for row in 0..self.spec.rect.height {
            let start = (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x;
            output[start..start + self.spec.rect.width].fill(self.background);
        }
    }

    fn visible_rows(&self, top: isize) -> Option<(usize, usize, usize)> {
        let region_height = self.spec.rect.height as isize;
        let destination_start = top.max(0);
        let destination_end = (top + region_height).min(region_height);
        if destination_end <= destination_start {
            return None;
        }
        Some((
            (destination_start - top) as usize,
            destination_start as usize,
            (destination_end - destination_start) as usize,
        ))
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

    fn visible_columns(&self, left: isize) -> Option<(usize, usize, usize)> {
        let region_width = self.spec.rect.width as isize;
        let destination_start = left.max(0);
        let destination_end = (left + region_width).min(region_width);
        if destination_end <= destination_start {
            return None;
        }
        Some((
            (destination_start - left) as usize,
            destination_start as usize,
            (destination_end - destination_start) as usize,
        ))
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
            let source_start =
                (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x + source_x;
            let destination_start =
                (self.spec.rect.y + row) * self.frame_width + self.spec.rect.x + destination_x;
            output[destination_start..destination_start + columns]
                .copy_from_slice(&destination[source_start..source_start + columns]);
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
            },
            0xff,
        )
        .ok()
    }

    fn region_row(frame: &[u8], y: usize) -> &[u8] {
        let start = y * FRAME_WIDTH + RECT.x;
        &frame[start..start + RECT.width]
    }

    fn horizontal_active(direction: Direction) -> Option<Active<u8>> {
        let source = frame(0);
        Active::new(
            &source,
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Spec {
                rect: Rect {
                    x: 0,
                    y: 1,
                    width: 4,
                    height: 2,
                },
                gap: 0,
                direction,
                total_frames: 2,
            },
            0xff,
        )
        .ok()
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
    fn left_step_pushes_incoming_page_from_right() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let transition = horizontal_active(Direction::Left);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };

        assert!(transition.compose_next(&destination, &mut output).is_ok());
        assert_eq!(&output[4..8], &[6, 7, 104, 105]);
        assert_eq!(&output[8..12], &[10, 11, 108, 109]);
        assert_eq!(&output[0..4], &source[0..4]);
    }

    #[test]
    fn right_step_pushes_incoming_page_from_left() {
        let source = frame(0);
        let destination = frame(100);
        let mut output = source.clone();
        let transition = horizontal_active(Direction::Right);
        assert!(transition.is_some(), "valid transition rejected");
        let Some(mut transition) = transition else {
            return;
        };

        assert!(transition.compose_next(&destination, &mut output).is_ok());
        assert_eq!(&output[4..8], &[106, 107, 4, 5]);
        assert_eq!(&output[8..12], &[110, 111, 8, 9]);
        assert_eq!(&output[0..4], &source[0..4]);
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
            },
            0xff,
        );
        assert!(matches!(result, Err(TransitionError::Rect)));
    }
}
