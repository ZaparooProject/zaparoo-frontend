// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Playtime as the person playing sees it: the time left in the header,
// the notices for a limit reached or extra time granted, and the extra
// time an admin grants from Settings > Profiles > Playtime limits.

use std::time::{Duration, Instant};

use crate::settings::duration_seconds;
use crate::top_played::PlayTime;

/// The time left that the header shows: the smaller of the session's and
/// the day's, whichever Core reports. `None` while limits are off or Core
/// reports neither.
pub fn header_remaining(
    limits_enabled: bool,
    session_remaining: Option<&str>,
    daily_remaining: Option<&str>,
) -> Option<Duration> {
    if !limits_enabled {
        return None;
    }
    [session_remaining, daily_remaining]
        .into_iter()
        .flatten()
        .filter_map(duration_seconds)
        .map(Duration::from_secs_f64)
        .min()
}

/// The time left `elapsed` after Core reported `remaining`. It only runs
/// down while a game is being played.
pub fn left(remaining: Duration, elapsed: Duration, session_active: bool) -> Duration {
    if session_active {
        remaining.saturating_sub(elapsed)
    } else {
        remaining
    }
}

/// The header's reading of `left`, in whole minutes rounded down.
pub fn reading(left: Duration) -> PlayTime {
    PlayTime::from_seconds(left.as_secs())
}

/// How long until the reading of `left` changes, or `None` once nothing
/// is left to count down.
pub fn next_tick(left: Duration) -> Option<Duration> {
    if left.is_zero() {
        return None;
    }
    let into_minute = left.as_millis() % 60_000;
    Some(Duration::from_millis(
        u64::try_from(into_minute).unwrap_or(0) + 1,
    ))
}

/// The row that grants extra time.
pub const EXTRA_TIME_ROW: &str = "playtimeExtraTime";

/// The extra time choices: minutes, or the rest of today.
pub const EXTRA_TIME: [&str; 4] = ["15", "30", "60", "today"];

/// The `playtime.extend` mode and duration for an extra time choice.
pub fn extend_request(choice: &str) -> Option<(&'static str, Option<String>)> {
    match choice {
        "today" => Some(("today", None)),
        minutes if EXTRA_TIME.contains(&minutes) => Some(("duration", Some(format!("{minutes}m")))),
        _ => None,
    }
}

/// Whether there is a session to grant extra time to: limits on, and a
/// session that has not reset.
pub fn can_extend(limits_enabled: bool, state: &str) -> bool {
    limits_enabled && state != "reset"
}

/// The extra time row's value: "today" while the session limit is waived
/// for the day, else the minutes granted so far, else "none".
pub fn extra_time_value(session_extension: Option<&str>, extended_until: Option<&str>) -> String {
    if extended_until.is_some_and(|until| !until.is_empty()) {
        return "today".to_string();
    }
    session_extension
        .and_then(crate::settings::duration_minutes)
        .filter(|minutes| *minutes > 0)
        .map_or_else(|| "none".to_string(), |minutes| minutes.to_string())
}

/// Which limit Core stopped the game for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitReason {
    Session,
    Daily,
}

impl LimitReason {
    /// Core's `reason` token; anything else is not a reason to show.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "session" => Some(Self::Session),
            "daily" => Some(Self::Daily),
            _ => None,
        }
    }
}

/// What a `playtime.extended` granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// Extra time on the session.
    Time(PlayTime),
    /// No session limit for the rest of the day.
    RestOfToday,
}

impl Grant {
    /// From Core's `mode` and `duration`; an unknown mode or a duration
    /// that does not read is not a grant to show.
    pub fn from_params(mode: &str, duration: Option<&str>) -> Option<Self> {
        match mode {
            "today" => Some(Self::RestOfToday),
            "duration" => duration
                .and_then(duration_seconds)
                .map(|seconds| Self::Time(PlayTime::from_seconds(seconds as u64))),
            _ => None,
        }
    }
}

/// A playtime notice for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    Reached(LimitReason),
    Extended(Grant),
}

/// Decides which playtime notices reach the status line, and when. A
/// notice that arrives while the frontend is dormant (a game in front of
/// it) is held for the resume, the latest one winning. A limit reached
/// with no game running is Core refusing a launch, which the launch's own
/// error already says. Extra time this frontend granted itself is not
/// announced back to the person who just chose it.
#[derive(Debug, Clone, Default)]
pub struct NoticeGate {
    held: Option<Notice>,
    own_grant: OwnGrant,
}

/// Our own `playtime.extend`, as far as its notification goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum OwnGrant {
    #[default]
    None,
    /// Sent, not answered yet.
    InFlight,
    /// Answered; Core's notification may still arrive until the deadline.
    Answered(Instant),
}

impl NoticeGate {
    /// How long after our own grant's reply its notification may still
    /// arrive. Core sends both from the same request, the notification
    /// over its own queue.
    pub const OWN_GRANT_ECHO: Duration = Duration::from_secs(3);

    pub fn new() -> Self {
        Self::default()
    }

    /// `playtime.limit.reached` arrived. `game_running` is whether a game
    /// was running when it did.
    pub fn reached(
        &mut self,
        reason: LimitReason,
        game_running: bool,
        dormant: bool,
    ) -> Option<Notice> {
        if !game_running {
            return None;
        }
        self.show(Notice::Reached(reason), dormant)
    }

    /// `playtime.extended` arrived.
    pub fn extended(&mut self, grant: Grant, dormant: bool, now: Instant) -> Option<Notice> {
        match std::mem::take(&mut self.own_grant) {
            OwnGrant::InFlight => return None,
            OwnGrant::Answered(deadline) if now < deadline => return None,
            OwnGrant::Answered(_) | OwnGrant::None => {}
        }
        self.show(Notice::Extended(grant), dormant)
    }

    /// Our own `playtime.extend` was sent.
    pub fn begin_own_grant(&mut self) {
        self.own_grant = OwnGrant::InFlight;
    }

    /// Our own `playtime.extend` was answered. A refused grant sends no
    /// notification to wait for.
    pub fn end_own_grant(&mut self, granted: bool, now: Instant) {
        if self.own_grant == OwnGrant::InFlight {
            self.own_grant = if granted {
                OwnGrant::Answered(now + Self::OWN_GRANT_ECHO)
            } else {
                OwnGrant::None
            };
        }
    }

    /// The frontend came back from a game: the held notice, if any.
    pub fn resume(&mut self) -> Option<Notice> {
        self.held.take()
    }

    fn show(&mut self, notice: Notice, dormant: bool) -> Option<Notice> {
        if dormant {
            self.held = Some(notice);
            None
        } else {
            Some(notice)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shows_the_smaller_of_session_and_day() {
        assert_eq!(
            header_remaining(true, Some("14m30s"), Some("1h2m0s")),
            Some(Duration::from_secs(870))
        );
        assert_eq!(
            header_remaining(true, None, Some("1h2m0s")),
            Some(Duration::from_mins(62))
        );
        assert_eq!(header_remaining(true, None, None), None);
        assert_eq!(header_remaining(false, Some("14m"), Some("1h")), None);
        assert_eq!(header_remaining(true, Some("soon"), None), None);
    }

    #[test]
    fn reading_rounds_down_to_whole_minutes() {
        assert_eq!(reading(Duration::from_secs(3899)), PlayTime::Hours(1, 4));
        assert_eq!(
            reading(Duration::from_secs(45 * 60 + 59)),
            PlayTime::Minutes(45)
        );
        assert_eq!(reading(Duration::from_secs(59)), PlayTime::Under);
    }

    #[test]
    fn time_runs_down_only_while_playing() {
        let remaining = Duration::from_mins(10);
        let elapsed = Duration::from_secs(90);
        assert_eq!(left(remaining, elapsed, true), Duration::from_secs(510));
        assert_eq!(left(remaining, elapsed, false), remaining);
        assert_eq!(
            left(remaining, Duration::from_mins(15), true),
            Duration::ZERO
        );
    }

    #[test]
    fn countdown_ticks_when_the_minute_changes() {
        assert_eq!(
            next_tick(Duration::from_secs(125)),
            Some(Duration::from_millis(5_001))
        );
        assert_eq!(
            next_tick(Duration::from_mins(2)),
            Some(Duration::from_millis(1))
        );
        assert_eq!(next_tick(Duration::ZERO), None);
    }

    #[test]
    fn extra_time_choices_map_to_extend_requests() {
        assert_eq!(extend_request("15"), Some(("duration", Some("15m".into()))));
        assert_eq!(extend_request("60"), Some(("duration", Some("60m".into()))));
        assert_eq!(extend_request("today"), Some(("today", None)));
        assert_eq!(extend_request("45"), None);
        for choice in EXTRA_TIME {
            assert!(extend_request(choice).is_some());
        }
    }

    #[test]
    fn extra_time_needs_a_session() {
        assert!(can_extend(true, "active"));
        assert!(can_extend(true, "cooldown"));
        assert!(!can_extend(true, "reset"));
        assert!(!can_extend(false, "active"));
    }

    #[test]
    fn extra_time_value_names_the_grant() {
        assert_eq!(extra_time_value(None, None), "none");
        assert_eq!(extra_time_value(Some("0s"), None), "none");
        assert_eq!(extra_time_value(Some("30m0s"), None), "30");
        assert_eq!(
            extra_time_value(Some("30m0s"), Some("2026-11-25T00:00:00Z")),
            "today"
        );
    }

    #[test]
    fn grants_read_from_core_params() {
        assert_eq!(
            Grant::from_params("duration", Some("15m0s")),
            Some(Grant::Time(PlayTime::Minutes(15)))
        );
        assert_eq!(Grant::from_params("today", None), Some(Grant::RestOfToday));
        assert_eq!(Grant::from_params("duration", None), None);
        assert_eq!(Grant::from_params("forever", None), None);
        assert_eq!(LimitReason::from_token("daily"), Some(LimitReason::Daily));
        assert_eq!(LimitReason::from_token("other"), None);
    }

    #[test]
    fn reached_shows_only_when_a_game_was_stopped() {
        let mut gate = NoticeGate::new();
        assert_eq!(gate.reached(LimitReason::Session, false, false), None);
        assert_eq!(
            gate.reached(LimitReason::Session, true, false),
            Some(Notice::Reached(LimitReason::Session))
        );
    }

    #[test]
    fn dormant_notices_wait_for_resume_and_the_latest_wins() {
        let mut gate = NoticeGate::new();
        let now = Instant::now();
        assert_eq!(gate.reached(LimitReason::Session, true, true), None);
        assert_eq!(gate.extended(Grant::RestOfToday, true, now), None);
        assert_eq!(gate.reached(LimitReason::Daily, true, true), None);
        assert_eq!(gate.resume(), Some(Notice::Reached(LimitReason::Daily)));
        assert_eq!(gate.resume(), None);
    }

    #[test]
    fn our_own_grant_is_not_announced() {
        let mut gate = NoticeGate::new();
        let now = Instant::now();
        let grant = Grant::Time(PlayTime::Minutes(15));

        gate.begin_own_grant();
        assert_eq!(gate.extended(grant, false, now), None);
        gate.end_own_grant(true, now);
        assert_eq!(
            gate.extended(grant, false, now),
            Some(Notice::Extended(grant))
        );

        // The notification may land after the reply.
        gate.begin_own_grant();
        gate.end_own_grant(true, now);
        assert_eq!(
            gate.extended(grant, false, now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            gate.extended(grant, false, now + Duration::from_secs(2)),
            Some(Notice::Extended(grant))
        );

        // Past the echo window, a grant is someone else's.
        gate.begin_own_grant();
        gate.end_own_grant(true, now);
        assert_eq!(
            gate.extended(grant, false, now + NoticeGate::OWN_GRANT_ECHO),
            Some(Notice::Extended(grant))
        );

        // A refused grant expects nothing.
        gate.begin_own_grant();
        gate.end_own_grant(false, now);
        assert_eq!(
            gate.extended(grant, false, now),
            Some(Notice::Extended(grant))
        );
    }
}
