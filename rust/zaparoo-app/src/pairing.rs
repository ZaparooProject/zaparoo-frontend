// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Pairing a device with Core: which phase the panel is in, which late
//! answer still belongs to the run on screen, and when leaving still owes
//! Core a `clients.pair.cancel`.
//!
//! The panel makes one hard promise: a pairing is never left open behind
//! the user. That promise is a rule, not a view detail, so the cancel
//! decision lives here where it can be held to it without a UI. `cancels`
//! counts the calls the driver owes Core, and every run that ever put a
//! PIN on screen ends with exactly one unless a device actually used it.

/// What the pairing panel is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// The panel is not up.
    #[default]
    Closed,
    /// `clients.pair.start` is in flight.
    Starting,
    /// A PIN is on screen and Core is waiting for a device to use it.
    Showing,
    /// A device finished pairing; the panel names it.
    Paired,
    /// The deadline passed with nobody having used the PIN.
    Expired,
}

/// What to do with the PIN `clients.pair.start` answered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// It belongs to the run on screen: show it.
    Shown,
    /// The user already left, so it belongs to nobody: call it off.
    Abandoned,
}

/// What a second passing did to the run on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// Still live, with this many seconds left to show.
    Counting(i64),
    /// The PIN ran out. Core is owed the cancel this already counted.
    Expired,
    /// The tick belongs to a run that is over; it changes nothing.
    Stale,
}

/// The shortest and longest pairing window the panel will count down.
///
/// Core's deadline is absolute, so the frontend only has to work out how
/// long is left on it. A frontend whose clock disagrees with Core's (a
/// `MiSTer` with no RTC, say) would otherwise read a fresh PIN as long dead
/// or as good for years, and neither is a countdown anybody can act on.
/// Clamping keeps the panel to a window a person could plausibly still be
/// standing in front of; the PIN's real authority is always Core's.
const MIN_WINDOW_SECS: i64 = 30;
const MAX_WINDOW_SECS: i64 = 15 * 60;

/// Whole seconds to count down for a deadline of `expires_at` read at
/// `now`, both in seconds since the Unix epoch.
pub fn window(expires_at: i64, now: i64) -> i64 {
    expires_at
        .saturating_sub(now)
        .clamp(MIN_WINDOW_SECS, MAX_WINDOW_SECS)
}

/// One run of the flow, from the row being accepted to the panel closing.
#[derive(Debug, Default)]
pub struct Session {
    pub phase: Phase,
    /// The PIN Core minted, while one is worth reading out.
    pub pin: String,
    /// Seconds left on it. Counted down locally from the window Core's
    /// deadline implied, rather than re-read from a wall clock that can
    /// jump under a running countdown.
    pub expires_in: i64,
    /// The name the device reported when it finished pairing.
    pub client_name: String,
    /// Bumped whenever a run starts or ends, so a late reply and an armed
    /// countdown from an earlier run both land on nothing.
    ticket: u64,
    /// How many `clients.pair.cancel` calls this session has owed Core.
    /// The driver sends exactly one per increment.
    pub cancels: u64,
}

impl Session {
    /// Start a run. Answers the ticket every later answer must carry.
    pub fn begin(&mut self) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.phase = Phase::Starting;
        self.pin.clear();
        self.client_name.clear();
        self.expires_in = 0;
        self.ticket
    }

    /// Core answered with a PIN, good for `seconds` (see `window`). A
    /// reply for a run the user already left hands back a PIN nobody will
    /// ever read, so it has to be called off rather than dropped: the
    /// start may well have reached Core after the user walked away.
    pub fn started(&mut self, ticket: u64, pin: &str, seconds: i64) -> Started {
        if ticket != self.ticket || self.phase != Phase::Starting {
            self.cancels = self.cancels.wrapping_add(1);
            return Started::Abandoned;
        }
        self.phase = Phase::Showing;
        self.pin = pin.to_string();
        self.expires_in = seconds.max(0);
        Started::Shown
    }

    /// `clients.pair.start` failed. Answers whether the run on screen is
    /// the one that failed, so a late failure closes nothing.
    pub fn failed(&mut self, ticket: u64) -> bool {
        if ticket != self.ticket || self.phase != Phase::Starting {
            return false;
        }
        self.close();
        true
    }

    /// A `clients.paired` notification arrived. Only a run with a PIN on
    /// screen can be the one that was used; anything else is a device
    /// pairing through some other path, which this panel says nothing
    /// about.
    pub fn paired(&mut self, name: &str) -> bool {
        if self.phase != Phase::Showing {
            return false;
        }
        self.phase = Phase::Paired;
        self.client_name = name.to_string();
        self.pin.clear();
        self.expires_in = 0;
        true
    }

    /// A second passed on the PIN.
    pub fn tick(&mut self, ticket: u64) -> Tick {
        if ticket != self.ticket || self.phase != Phase::Showing {
            return Tick::Stale;
        }
        self.expires_in = self.expires_in.saturating_sub(1).max(0);
        if self.expires_in > 0 {
            return Tick::Counting(self.expires_in);
        }
        self.phase = Phase::Expired;
        self.pin.clear();
        self.cancels = self.cancels.wrapping_add(1);
        Tick::Expired
    }

    /// The user left the panel. Answers whether Core still holds a pairing
    /// this has to call off. A run still waiting on its own start reply
    /// does not: that reply is abandoned when it lands, which is the only
    /// ordering that works when the start may not have reached Core yet.
    pub fn leave(&mut self) -> bool {
        let owed = self.phase == Phase::Showing;
        if owed {
            self.cancels = self.cancels.wrapping_add(1);
        }
        self.close();
        owed
    }

    /// Retire the run and its ticket without deciding anything about Core.
    fn close(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.phase = Phase::Closed;
        self.pin.clear();
        self.client_name.clear();
        self.expires_in = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{window, Phase, Session, Started, Tick, MAX_WINDOW_SECS, MIN_WINDOW_SECS};

    fn showing(seconds: i64) -> (Session, u64) {
        let mut session = Session::default();
        let ticket = session.begin();
        assert_eq!(session.started(ticket, "482913", seconds), Started::Shown);
        assert_eq!(session.phase, Phase::Showing);
        (session, ticket)
    }

    #[test]
    fn walking_away_from_a_shown_pin_calls_it_off() {
        let (mut session, _) = showing(300);
        assert!(session.leave());
        assert_eq!(session.cancels, 1);
        assert_eq!(session.phase, Phase::Closed);
        assert!(session.pin.is_empty());
        // Leaving a panel that is already closed owes nothing more.
        assert!(!session.leave());
        assert_eq!(session.cancels, 1);
    }

    #[test]
    fn a_reply_for_a_run_the_user_left_is_called_off_exactly_once() {
        let mut session = Session::default();
        let ticket = session.begin();
        // Nothing is owed while the start is still in flight: Core may not
        // have minted a PIN yet.
        assert!(!session.leave());
        assert_eq!(session.cancels, 0);
        assert_eq!(session.started(ticket, "482913", 300), Started::Abandoned);
        assert_eq!(session.cancels, 1);
        assert_eq!(session.phase, Phase::Closed);
        assert!(session.pin.is_empty());
    }

    #[test]
    fn a_device_that_pairs_ends_the_run_with_nothing_owed() {
        let (mut session, _) = showing(300);
        assert!(session.paired("Wizzo's phone"));
        assert_eq!(session.phase, Phase::Paired);
        assert_eq!(session.client_name, "Wizzo's phone");
        assert!(session.pin.is_empty(), "a used PIN is not left on screen");
        // Closing a finished pairing has nothing to call off.
        assert!(!session.leave());
        assert_eq!(session.cancels, 0);
    }

    #[test]
    fn only_a_run_showing_a_pin_answers_a_paired_notification() {
        let mut session = Session::default();
        assert!(!session.paired("Someone else's phone"));
        let ticket = session.begin();
        assert!(!session.paired("Someone else's phone"));
        assert_eq!(session.started(ticket, "482913", 300), Started::Shown);
        assert!(session.paired("Wizzo's phone"));
        // The panel names one device, not whichever paired last.
        assert!(!session.paired("A second phone"));
        assert_eq!(session.client_name, "Wizzo's phone");
    }

    #[test]
    fn the_countdown_retires_the_pin_and_calls_it_off_once() {
        let (mut session, ticket) = showing(3);
        assert_eq!(session.tick(ticket), Tick::Counting(2));
        assert_eq!(session.tick(ticket), Tick::Counting(1));
        assert_eq!(session.tick(ticket), Tick::Expired);
        assert_eq!(session.phase, Phase::Expired);
        assert!(
            session.pin.is_empty(),
            "an expired PIN is not left on screen"
        );
        assert_eq!(session.cancels, 1);
        // The countdown cannot expire twice, and leaving after it owes
        // nothing more.
        assert_eq!(session.tick(ticket), Tick::Stale);
        assert!(!session.leave());
        assert_eq!(session.cancels, 1);
    }

    #[test]
    fn a_countdown_armed_by_an_earlier_run_changes_nothing() {
        let (mut session, stale) = showing(1);
        assert!(session.leave());
        let ticket = session.begin();
        assert_eq!(session.started(ticket, "551104", 300), Started::Shown);
        assert_eq!(session.tick(stale), Tick::Stale);
        assert_eq!(session.phase, Phase::Showing);
        assert_eq!(session.pin, "551104");
        assert_eq!(session.expires_in, 300);
        assert_eq!(session.cancels, 1);
    }

    #[test]
    fn a_failed_start_closes_only_the_run_that_failed() {
        let mut session = Session::default();
        let ticket = session.begin();
        assert!(session.failed(ticket));
        assert_eq!(session.phase, Phase::Closed);
        assert_eq!(session.cancels, 0, "a start that failed left nothing open");
        let next = session.begin();
        assert!(!session.failed(ticket));
        assert_eq!(session.phase, Phase::Starting);
        assert!(session.failed(next));
    }

    #[test]
    fn the_countdown_window_survives_a_disagreeing_clock() {
        assert_eq!(window(1_700_000_300, 1_700_000_000), 300);
        // Core's deadline read as already gone, or as years away.
        assert_eq!(window(1_700_000_000, 1_700_000_900), MIN_WINDOW_SECS);
        assert_eq!(window(i64::MAX, 0), MAX_WINDOW_SECS);
        assert_eq!(window(i64::MIN, i64::MAX), MIN_WINDOW_SECS);
    }
}
