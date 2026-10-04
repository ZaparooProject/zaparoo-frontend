// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Linking this device to a Zaparoo Online account: the panel's phases, the
//! stale answers it must ignore, and when it owes Core a cancel. Core holds
//! the account token; the panel only ever sees the code and URL to show.

/// Where the panel is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Closed,
    /// Asked Core to start a link; no code yet.
    Starting,
    /// A code is up and Core is waiting for approval.
    Showing,
    Linked,
    /// Core could not start, or the service refused the link.
    Failed,
    /// The code ran out before anyone approved it.
    Expired,
}

/// What became of a code Core answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    Shown,
    /// Nobody is watching this run any more; Core must be told to drop it.
    Abandoned,
}

/// What one second did to the countdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// Still counting. The flag asks for a status poll, so a missed
    /// notification cannot leave an approved link on screen as pending.
    Counting {
        poll: bool,
    },
    Expired,
    Stale,
}

/// What Accept does on the link panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptAction {
    /// The run is finished (linked, failed, or expired): leave the panel.
    Close,
    /// A code is showing and the host can open it itself: do that instead.
    OpenUrl,
    /// A code is showing but there is nowhere to send it, or nothing is
    /// showing yet (still starting): Accept does nothing.
    Nothing,
}

/// What Accept does right now. `finished` and `showing` describe the same
/// `Session` Accept is pressed against; `can_open_url` is the host's own
/// capability, not part of the session.
pub fn accept_action(finished: bool, showing: bool, can_open_url: bool) -> AcceptAction {
    if finished {
        AcceptAction::Close
    } else if showing && can_open_url {
        AcceptAction::OpenUrl
    } else {
        AcceptAction::Nothing
    }
}

const MIN_WINDOW_SECS: i64 = 30;
const MAX_WINDOW_SECS: i64 = 30 * 60;
/// Seconds between status polls while a code is shown.
pub const POLL_SECS: i64 = 5;

/// Seconds a code stays up: what Core says is left, bounded so a clock
/// disagreement neither flashes it nor keeps it forever.
pub fn window(expires_in: i64) -> i64 {
    expires_in.clamp(MIN_WINDOW_SECS, MAX_WINDOW_SECS)
}

#[derive(Debug, Default)]
pub struct Session {
    pub phase: Phase,
    pub code: String,
    pub url: String,
    /// The URL with the code in it, for the QR code.
    pub url_complete: String,
    pub expires_in: i64,
    ticket: u64,
    shown_for: i64,
}

impl Session {
    /// Open the panel for a new run and answer its ticket.
    pub fn begin(&mut self) -> u64 {
        self.close();
        self.phase = Phase::Starting;
        self.ticket
    }

    /// Core answered a code for run `ticket`.
    pub fn started(
        &mut self,
        ticket: u64,
        code: &str,
        url: &str,
        url_complete: &str,
        seconds: i64,
    ) -> Started {
        if ticket != self.ticket || self.phase != Phase::Starting {
            return Started::Abandoned;
        }
        self.phase = Phase::Showing;
        self.code = code.to_string();
        self.url = url.to_string();
        self.url_complete = if url_complete.is_empty() {
            url.to_string()
        } else {
            url_complete.to_string()
        };
        self.expires_in = seconds.max(0);
        self.shown_for = 0;
        Started::Shown
    }

    /// Core refused to start run `ticket`.
    pub fn failed(&mut self, ticket: u64) -> bool {
        if ticket != self.ticket || self.phase != Phase::Starting {
            return false;
        }
        self.fail();
        true
    }

    /// Core reported the flow's status. Only a shown code can move on;
    /// `cancelled` from elsewhere ends it as a failure the user can read.
    pub fn status(&mut self, status: &str) -> bool {
        if self.phase != Phase::Showing {
            return false;
        }
        match status {
            "approved" => {
                self.phase = Phase::Linked;
                self.clear_code();
            }
            "failed" | "cancelled" => self.fail(),
            _ => return false,
        }
        true
    }

    /// A second passed for run `ticket`.
    pub fn tick(&mut self, ticket: u64) -> Tick {
        if ticket != self.ticket || self.phase != Phase::Showing {
            return Tick::Stale;
        }
        self.expires_in = self.expires_in.saturating_sub(1).max(0);
        self.shown_for += 1;
        if self.expires_in > 0 {
            return Tick::Counting {
                poll: self.shown_for % POLL_SECS == 0,
            };
        }
        self.phase = Phase::Expired;
        self.clear_code();
        Tick::Expired
    }

    /// Leave the panel. Answers whether Core still holds a pending link
    /// that must be cancelled.
    pub fn leave(&mut self) -> bool {
        let owed = matches!(
            self.phase,
            Phase::Starting | Phase::Showing | Phase::Expired
        );
        self.close();
        owed
    }

    /// The panel is finished: Accept closes it.
    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Linked | Phase::Failed | Phase::Expired)
    }

    fn fail(&mut self) {
        self.phase = Phase::Failed;
        self.clear_code();
    }

    fn clear_code(&mut self) {
        self.code.clear();
        self.url.clear();
        self.url_complete.clear();
        self.expires_in = 0;
    }

    fn close(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.phase = Phase::Closed;
        self.clear_code();
        self.shown_for = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accept_action, window, AcceptAction, Phase, Session, Started, Tick, MAX_WINDOW_SECS,
        MIN_WINDOW_SECS, POLL_SECS,
    };

    fn showing(seconds: i64) -> (Session, u64) {
        let mut session = Session::default();
        let ticket = session.begin();
        assert_eq!(
            session.started(
                ticket,
                "ABCD-1234",
                "https://online.example/link",
                "https://online.example/link?code=ABCD1234",
                seconds
            ),
            Started::Shown
        );
        (session, ticket)
    }

    #[test]
    fn approval_links_and_forgets_the_code() {
        let (mut session, _) = showing(600);
        assert!(!session.status("pending"));
        assert!(session.status("approved"));
        assert_eq!(session.phase, Phase::Linked);
        assert!(session.code.is_empty() && session.url_complete.is_empty());
        assert!(session.finished());
        assert!(!session.leave(), "an approved link owes Core nothing");
    }

    #[test]
    fn walking_away_from_a_live_code_cancels_it() {
        let (mut session, _) = showing(600);
        assert!(session.leave());
        assert_eq!(session.phase, Phase::Closed);
        assert!(!session.leave());
    }

    #[test]
    fn a_code_for_a_closed_run_is_abandoned() {
        let mut session = Session::default();
        let ticket = session.begin();
        session.leave();
        assert_eq!(
            session.started(ticket, "ABCD", "u", "", 600),
            Started::Abandoned
        );
        assert_eq!(session.phase, Phase::Closed);
        assert!(!session.failed(ticket));
    }

    #[test]
    fn countdown_polls_and_expires() {
        let (mut session, ticket) = showing(POLL_SECS + 1);
        for second in 1..POLL_SECS {
            assert_eq!(
                session.tick(ticket),
                Tick::Counting { poll: false },
                "{second}"
            );
        }
        assert_eq!(session.tick(ticket), Tick::Counting { poll: true });
        assert_eq!(session.tick(ticket), Tick::Expired);
        assert_eq!(session.phase, Phase::Expired);
        assert_eq!(session.tick(ticket), Tick::Stale);
        assert!(session.leave(), "Core may still hold the expired request");
    }

    #[test]
    fn failures_are_shown_not_dropped() {
        let mut session = Session::default();
        let ticket = session.begin();
        assert!(session.failed(ticket));
        assert_eq!(session.phase, Phase::Failed);
        let (mut shown, _) = showing(600);
        assert!(shown.status("failed"));
        assert_eq!(shown.phase, Phase::Failed);
    }

    #[test]
    fn missing_complete_url_falls_back_to_the_plain_one() {
        let mut session = Session::default();
        let ticket = session.begin();
        session.started(ticket, "C", "https://online.example/link", "", 600);
        assert_eq!(session.url_complete, "https://online.example/link");
    }

    #[test]
    fn window_is_bounded() {
        assert_eq!(window(-5), MIN_WINDOW_SECS);
        assert_eq!(window(600), 600);
        assert_eq!(window(i64::MAX), MAX_WINDOW_SECS);
    }

    #[test]
    fn accept_closes_a_finished_run_regardless_of_the_host() {
        assert_eq!(accept_action(true, false, false), AcceptAction::Close);
        assert_eq!(accept_action(true, true, true), AcceptAction::Close);
    }

    #[test]
    fn accept_opens_the_url_only_while_showing_on_a_capable_host() {
        assert_eq!(accept_action(false, true, true), AcceptAction::OpenUrl);
        assert_eq!(
            accept_action(false, true, false),
            AcceptAction::Nothing,
            "no host handoff: nothing to do but wait"
        );
        assert_eq!(
            accept_action(false, false, true),
            AcceptAction::Nothing,
            "still starting, no code yet: nothing to open"
        );
    }
}
