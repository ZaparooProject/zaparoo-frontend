// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Paddle clock: a paddle game that plays itself and keeps time. The left
//! score is the hour and the right score the minute; when the minute turns
//! the left paddle lets the ball by, and when the hour turns the right one
//! does, so the score always reads as the time. The court shifts a few
//! pixels with every point so nothing sits still on the screen for long.

use super::{Look, Rng, Sprite};

/// The court's inset from the scene's edges, in height units.
pub const MARGIN: f32 = 0.06;
pub const PADDLE_W: f32 = 0.022;
pub const PADDLE_H: f32 = 0.17;
pub const BALL: f32 = 0.055;
/// Seconds the ball takes to cross the court.
pub const CROSS_SECONDS: f32 = 1.7;
/// Paddle speed, in height units per second.
pub const PADDLE_SPEED: f32 = 1.6;
/// How far the court can drift from center, in height units.
pub const SHIFT: f32 = 0.012;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct Paddle {
    aspect: f32,
    ball_x: f32,
    ball_y: f32,
    vx: f32,
    vy: f32,
    left_y: f32,
    right_y: f32,
    /// The time the score shows: (hour, minute).
    shown: (u8, u8),
    /// The side that will miss next, while the score is behind the clock.
    loser: Option<Side>,
    /// Where the court sits, offset from center.
    shift: (f32, f32),
    rng: Rng,
}

impl Paddle {
    pub fn new(aspect: f32, hour: u8, minute: u8, seed: u64) -> Self {
        let mut p = Self {
            aspect,
            ball_x: 0.0,
            ball_y: 0.0,
            vx: 0.0,
            vy: 0.0,
            left_y: (1.0 - PADDLE_H) / 2.0,
            right_y: (1.0 - PADDLE_H) / 2.0,
            shown: (hour, minute),
            loser: None,
            shift: (0.0, 0.0),
            rng: Rng::new(seed),
        };
        let first = if p.rng.coin() {
            Side::Left
        } else {
            Side::Right
        };
        p.serve(first);
        p
    }

    fn court_left(&self) -> f32 {
        MARGIN + self.shift.0
    }

    fn court_right(&self) -> f32 {
        self.aspect - MARGIN + self.shift.0
    }

    fn court_top(&self) -> f32 {
        MARGIN + self.shift.1
    }

    fn court_bottom(&self) -> f32 {
        1.0 - MARGIN + self.shift.1
    }

    /// Put the ball in the middle, heading for `toward`.
    fn serve(&mut self, toward: Side) {
        self.ball_x = (self.court_left() + self.court_right() - BALL) / 2.0;
        self.ball_y = self
            .rng
            .range(self.court_top() + 0.1, self.court_bottom() - 0.1 - BALL);
        let speed = (self.court_right() - self.court_left()) / CROSS_SECONDS;
        self.vx = if toward == Side::Left { -speed } else { speed };
        self.vy = self.rng.range(0.25, 0.55) * if self.rng.coin() { 1.0 } else { -1.0 };
    }

    /// Advance by `dt` seconds with the wall clock at `hour:minute`.
    /// True when a point was scored (the score changed).
    pub fn step(&mut self, dt: f32, hour: u8, minute: u8) -> bool {
        if self.loser.is_none() && (hour, minute) != self.shown {
            // The hour turning is the hour side's point.
            self.loser = Some(if hour == self.shown.0 {
                Side::Left
            } else {
                Side::Right
            });
        }
        self.ball_x += self.vx * dt;
        self.ball_y += self.vy * dt;
        if self.ball_y <= self.court_top() {
            self.ball_y = self.court_top();
            self.vy = self.vy.abs();
        } else if self.ball_y + BALL >= self.court_bottom() {
            self.ball_y = self.court_bottom() - BALL;
            self.vy = -self.vy.abs();
        }
        self.move_paddles(dt);

        let left_face = self.court_left() + PADDLE_W;
        let right_face = self.court_right() - PADDLE_W;
        if self.vx < 0.0 && self.ball_x <= left_face {
            if self.loser == Some(Side::Left) {
                if self.ball_x + BALL < self.court_left() - MARGIN {
                    return self.point(hour, minute, Side::Left);
                }
            } else {
                self.ball_x = left_face;
                self.left_y = self.cover(self.left_y);
                self.vx = -self.vx;
                self.vy = self.english(self.left_y);
            }
        } else if self.vx > 0.0 && self.ball_x + BALL >= right_face {
            if self.loser == Some(Side::Right) {
                if self.ball_x > self.court_right() + MARGIN {
                    return self.point(hour, minute, Side::Right);
                }
            } else {
                self.ball_x = right_face - BALL;
                self.right_y = self.cover(self.right_y);
                self.vx = -self.vx;
                self.vy = self.english(self.right_y);
            }
        }
        false
    }

    fn point(&mut self, hour: u8, minute: u8, missed: Side) -> bool {
        self.shown = (hour, minute);
        self.loser = None;
        self.shift = (self.rng.range(-SHIFT, SHIFT), self.rng.range(-SHIFT, SHIFT));
        self.serve(missed);
        true
    }

    /// A paddle at `y` moved just enough to meet the ball.
    fn cover(&self, y: f32) -> f32 {
        let lo = self.ball_y + BALL - PADDLE_H + 0.01;
        let hi = self.ball_y - 0.01;
        y.clamp(lo.min(hi), hi.max(lo))
    }

    /// Where the ball goes after meeting a paddle at `y`: off the edge of
    /// the paddle it leaves at a steeper angle.
    fn english(&self, paddle_y: f32) -> f32 {
        let offset = (self.ball_y + BALL / 2.0 - (paddle_y + PADDLE_H / 2.0)) / (PADDLE_H / 2.0);
        offset.clamp(-1.0, 1.0) * 0.7 + self.vy.signum() * 0.1
    }

    fn move_paddles(&mut self, dt: f32) {
        let ball_mid = self.ball_y + BALL / 2.0;
        let center = (self.court_top() + self.court_bottom() - PADDLE_H) / 2.0;
        for side in [Side::Left, Side::Right] {
            let incoming = match side {
                Side::Left => self.vx < 0.0,
                Side::Right => self.vx > 0.0,
            };
            let target = if self.loser == Some(side) && incoming {
                // Step aside, the way a player who misjudged it would.
                if ball_mid > self.court_top().midpoint(self.court_bottom()) {
                    ball_mid - PADDLE_H - 0.2
                } else {
                    ball_mid + 0.2
                }
            } else if incoming {
                ball_mid - PADDLE_H / 2.0
            } else {
                center
            };
            let target = target.clamp(self.court_top(), self.court_bottom() - PADDLE_H);
            let y = match side {
                Side::Left => &mut self.left_y,
                Side::Right => &mut self.right_y,
            };
            let max = PADDLE_SPEED * dt;
            *y += (target - *y).clamp(-max, max);
        }
    }

    /// The time the score shows.
    pub fn score(&self) -> (u8, u8) {
        self.shown
    }

    /// The left paddle, the right paddle, and the ball (the bolt mark).
    pub fn sprites(&self) -> [Sprite; 3] {
        [
            Sprite::new(
                self.court_left(),
                self.left_y,
                PADDLE_W,
                PADDLE_H,
                Look::Block(0),
            ),
            Sprite::new(
                self.court_right() - PADDLE_W,
                self.right_y,
                PADDLE_W,
                PADDLE_H,
                Look::Block(0),
            ),
            Sprite::new(self.ball_x, self.ball_y, BALL, BALL, Look::Mark),
        ]
    }

    /// The court's offset from center, for the score and the net.
    pub fn shift(&self) -> (f32, f32) {
        self.shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When, and the shown time and score after the change.
    type Change = (f32, (u8, u8), (u8, u8));

    /// Run for `seconds` from 10:58:30 and return every (time, score)
    /// change.
    fn run(seconds: f32) -> Vec<Change> {
        let mut p = Paddle::new(16.0 / 9.0, 10, 58, 1);
        let dt = 1.0 / 30.0;
        let mut t = 0.0;
        let mut changes = Vec::new();
        let start = 10.0 * 3600.0 + 58.0 * 60.0 + 30.0;
        while t < seconds {
            let now = start + t;
            let hour = ((now / 3600.0) as u32 % 24) as u8;
            let minute = ((now / 60.0) as u32 % 60) as u8;
            if p.step(dt, hour, minute) {
                changes.push((t, (hour, minute), p.score()));
            }
            t += dt;
        }
        changes
    }

    #[test]
    fn the_score_follows_the_clock_within_a_few_seconds() {
        let changes = run(200.0);
        // 10:59 at t=30, 11:00 at t=90, 11:01 at t=150.
        assert_eq!(changes.len(), 3, "{changes:?}");
        for ((t, now, score), due) in changes.iter().zip([30.0, 90.0, 150.0]) {
            assert_eq!(now, score);
            assert!(*t >= due && *t < due + 5.0, "scored at {t}, due {due}");
        }
        assert_eq!(changes[1].2, (11, 0));
    }

    #[test]
    fn nobody_misses_while_the_score_is_right() {
        let mut p = Paddle::new(4.0 / 3.0, 9, 15, 4);
        for _ in 0..(30 * 600) {
            assert!(!p.step(1.0 / 30.0, 9, 15));
        }
    }

    #[test]
    fn everything_stays_on_screen_while_the_score_is_right() {
        let mut p = Paddle::new(16.0 / 9.0, 1, 2, 5);
        for _ in 0..(30 * 120) {
            p.step(1.0 / 30.0, 1, 2);
            for s in p.sprites() {
                assert!(s.x >= 0.0 && s.x + s.w <= 16.0 / 9.0);
                assert!(s.y >= 0.0 && s.y + s.h <= 1.0);
            }
        }
    }
}
