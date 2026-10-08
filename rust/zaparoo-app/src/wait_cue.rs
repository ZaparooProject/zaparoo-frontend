// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! When a "waiting" cue shows. One rule for every surface that tells the
//! user something is loading, saving or starting: the cue waits
//! `CUE_DELAY_MS` before it appears, so an answer that arrives quickly
//! shows nothing at all, and once it has appeared it stays at least
//! `CUE_HOLD_MS`, so one that arrives just after never flashes a word.
//!
//! The rule is clock-agnostic. The driver starts a timer for each
//! duration and reports back when it runs out; a sequence number retires
//! the timers of a wait that has since been replaced.

/// How long a wait runs before its cue appears.
pub const CUE_DELAY_MS: u64 = 300;
/// How long a cue that has appeared stays, at least.
pub const CUE_HOLD_MS: u64 = 200;

/// How long the header line confirms an action whose result is not on
/// screen to see: a Hub change made from a browse screen, a token written.
/// Not a wait, so it shows at once.
pub const CONFIRM_MS: u64 = 2000;

/// Identifies one wait to the calls that follow its `begin`.
pub type Token = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    /// Begun; the delay has not run out.
    Pending,
    /// On screen. `held` once the minimum hold has run out, `ended` once
    /// the wait itself is over and only the hold keeps the cue up.
    Shown { held: bool, ended: bool },
}

/// One cue slot: the latest wait owns it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaitCue {
    seq: Token,
    phase: Phase,
}

impl WaitCue {
    /// A wait starts and takes the slot from whatever held it. The driver
    /// clears any cue on screen and starts the delay timer for the token.
    pub fn begin(&mut self) -> Token {
        self.seq += 1;
        self.phase = Phase::Pending;
        self.seq
    }

    /// The delay timer for `token` ran out. True when its cue should
    /// appear now; the driver then starts the hold timer.
    pub fn delay_elapsed(&mut self, token: Token) -> bool {
        if token != self.seq || self.phase != Phase::Pending {
            return false;
        }
        self.phase = Phase::Shown {
            held: false,
            ended: false,
        };
        true
    }

    /// The hold timer for `token` ran out. True when the cue should be
    /// cleared now, because its wait ended while the hold kept it up.
    pub fn hold_elapsed(&mut self, token: Token) -> bool {
        if token != self.seq {
            return false;
        }
        match self.phase {
            Phase::Shown { ended: true, .. } => {
                self.phase = Phase::Idle;
                true
            }
            Phase::Shown { ended: false, .. } => {
                self.phase = Phase::Shown {
                    held: true,
                    ended: false,
                };
                false
            }
            Phase::Idle | Phase::Pending => false,
        }
    }

    /// The wait for `token` is over. True when the cue should be cleared
    /// now; false when nothing is showing for it, or the hold still has
    /// to run out first.
    pub fn end(&mut self, token: Token) -> bool {
        if token != self.seq {
            return false;
        }
        match self.phase {
            Phase::Shown { held: true, .. } => {
                self.phase = Phase::Idle;
                true
            }
            Phase::Shown { held: false, .. } => {
                self.phase = Phase::Shown {
                    held: false,
                    ended: true,
                };
                false
            }
            Phase::Pending => {
                self.phase = Phase::Idle;
                false
            }
            Phase::Idle => false,
        }
    }

    /// Whether `token`'s wait is still running, shown or not: its text may
    /// still change.
    pub fn is_waiting(&self, token: Token) -> bool {
        token == self.seq
            && matches!(
                self.phase,
                Phase::Pending | Phase::Shown { ended: false, .. }
            )
    }

    /// Whether `token`'s cue is on screen right now.
    pub fn is_shown(&self, token: Token) -> bool {
        token == self.seq && matches!(self.phase, Phase::Shown { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fast_answer_shows_nothing() {
        let mut cue = WaitCue::default();
        let wait = cue.begin();
        assert!(!cue.end(wait), "nothing was on screen to clear");
        assert!(!cue.delay_elapsed(wait), "its timer finds the wait over");
        assert!(!cue.is_shown(wait));
    }

    #[test]
    fn a_slow_answer_shows_and_clears_when_it_arrives() {
        let mut cue = WaitCue::default();
        let wait = cue.begin();
        assert!(cue.delay_elapsed(wait));
        assert!(cue.is_shown(wait));
        assert!(!cue.hold_elapsed(wait), "the wait is still running");
        assert!(cue.end(wait));
        assert!(!cue.is_shown(wait));
    }

    #[test]
    fn an_answer_just_after_the_cue_appears_does_not_flash_it() {
        let mut cue = WaitCue::default();
        let wait = cue.begin();
        assert!(cue.delay_elapsed(wait));
        assert!(!cue.end(wait), "the minimum hold keeps it up");
        assert!(cue.is_shown(wait));
        assert!(!cue.is_waiting(wait));
        assert!(cue.hold_elapsed(wait), "then the hold clears it");
        assert!(!cue.is_shown(wait));
    }

    #[test]
    fn a_newer_wait_retires_the_older_ones_timers() {
        let mut cue = WaitCue::default();
        let first = cue.begin();
        assert!(cue.delay_elapsed(first));
        let second = cue.begin();
        assert!(!cue.is_shown(first));
        assert!(!cue.hold_elapsed(first));
        assert!(!cue.end(first), "the slot is no longer its to clear");
        assert!(cue.is_waiting(second));
        assert!(cue.delay_elapsed(second));
        assert!(!cue.hold_elapsed(second));
        assert!(cue.end(second));
    }

    #[test]
    fn the_delay_is_shorter_than_a_noticeable_wait_and_the_hold_shorter_still() {
        assert_eq!(CUE_DELAY_MS, 300);
        assert_eq!(CUE_HOLD_MS, 200);
    }
}
