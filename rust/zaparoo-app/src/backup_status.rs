// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// What Core reports about cloud backup, as closed sets: the protection
// Zaparoo Online records for this device, the last scheduled backup that
// failed or was skipped, and whether a running backup is paused or slowed
// by a game. Core sends these as tokens; anything it adds later reads as
// unknown rather than as one of the known states.

/// How well Zaparoo Online says this device is protected
/// (`protection.state` in `settings.backup.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Protection {
    /// A state this frontend does not know yet.
    #[default]
    Unknown,
    Never,
    Removed,
    Failed,
    Stale,
    Protected,
}

impl Protection {
    pub fn from_token(token: &str) -> Self {
        match token {
            "never" => Self::Never,
            "removed" => Self::Removed,
            "failed" => Self::Failed,
            "stale" => Self::Stale,
            "protected" => Self::Protected,
            _ => Self::Unknown,
        }
    }
}

/// How the last unsuccessful scheduled backup ended
/// (`lastAttempt.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttemptStatus {
    #[default]
    Unknown,
    Failed,
    Skipped,
}

impl AttemptStatus {
    pub fn from_token(token: &str) -> Self {
        match token {
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => Self::Unknown,
        }
    }
}

/// Why the last scheduled backup failed or was skipped
/// (`lastAttempt.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttemptReason {
    /// A code this frontend does not know yet: the row says only that it
    /// failed or was skipped.
    #[default]
    Unknown,
    Busy,
    Offline,
    NotAvailable,
    Interrupted,
    QuotaExceeded,
    RateLimited,
    FilesChanged,
    RequiresNewerCore,
    BackupTooLarge,
    PayloadTooLarge,
    MissingObjects,
    ServerError,
    NetworkError,
    Error,
}

impl AttemptReason {
    pub fn from_token(token: &str) -> Self {
        match token {
            "busy" => Self::Busy,
            "offline" => Self::Offline,
            "not_available" => Self::NotAvailable,
            "interrupted" => Self::Interrupted,
            "quota_exceeded" => Self::QuotaExceeded,
            "rate_limited" => Self::RateLimited,
            "files_changed" => Self::FilesChanged,
            "requires_newer_core" => Self::RequiresNewerCore,
            "backup_too_large" => Self::BackupTooLarge,
            "payload_too_large" => Self::PayloadTooLarge,
            "missing_objects" => Self::MissingObjects,
            "server_error" => Self::ServerError,
            "network_error" => Self::NetworkError,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }
}

/// Whether a backup or restore is running in Core, and at what pace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    /// Running at full speed.
    Running,
    /// Waiting for the running game to stop.
    Paused,
    /// Running slowed to stay out of the running game's way.
    Slowed,
}

impl Activity {
    pub fn is_running(self) -> bool {
        self != Self::Idle
    }

    /// The pace a `backup.state` notification reports. A paused operation
    /// is paused whatever `throttled` says; `finished` ends it.
    pub fn from_event(paused: bool, throttled: bool, finished: bool) -> Self {
        if finished {
            Self::Idle
        } else if paused {
            Self::Paused
        } else if throttled {
            Self::Slowed
        } else {
            Self::Running
        }
    }

    /// The pace after reading `activeOperation` from
    /// `settings.backup.status`. The status only says whether something
    /// runs, not its pace, so a pace an event already reported stands.
    #[must_use]
    pub fn seeded(self, active_operation: &str) -> Self {
        if active_operation.is_empty() {
            Self::Idle
        } else if self == Self::Idle {
            Self::Running
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Activity, AttemptReason, AttemptStatus, Protection};

    #[test]
    fn tokens_map_to_their_states_and_anything_else_is_unknown() {
        assert_eq!(Protection::from_token("protected"), Protection::Protected);
        assert_eq!(Protection::from_token("stale"), Protection::Stale);
        assert_eq!(Protection::from_token("never"), Protection::Never);
        assert_eq!(Protection::from_token("removed"), Protection::Removed);
        assert_eq!(Protection::from_token("failed"), Protection::Failed);
        assert_eq!(Protection::from_token("archived"), Protection::Unknown);
        assert_eq!(Protection::from_token(""), Protection::Unknown);

        assert_eq!(AttemptStatus::from_token("failed"), AttemptStatus::Failed);
        assert_eq!(AttemptStatus::from_token("skipped"), AttemptStatus::Skipped);
        assert_eq!(AttemptStatus::from_token("Failed"), AttemptStatus::Unknown);

        assert_eq!(
            AttemptReason::from_token("quota_exceeded"),
            AttemptReason::QuotaExceeded
        );
        assert_eq!(
            AttemptReason::from_token("requires_newer_core"),
            AttemptReason::RequiresNewerCore
        );
        assert_eq!(AttemptReason::from_token("error"), AttemptReason::Error);
        assert_eq!(
            AttemptReason::from_token("solar_flare"),
            AttemptReason::Unknown
        );
    }

    #[test]
    fn a_pause_outranks_a_slowdown_and_finished_ends_both() {
        assert_eq!(Activity::from_event(true, true, false), Activity::Paused);
        assert_eq!(Activity::from_event(false, true, false), Activity::Slowed);
        assert_eq!(Activity::from_event(false, false, false), Activity::Running);
        assert_eq!(Activity::from_event(true, true, true), Activity::Idle);
    }

    #[test]
    fn the_status_says_whether_something_runs_and_keeps_its_pace() {
        assert_eq!(Activity::Idle.seeded("remote-upload"), Activity::Running);
        assert_eq!(Activity::Paused.seeded("remote-upload"), Activity::Paused);
        assert_eq!(Activity::Slowed.seeded("local-create"), Activity::Slowed);
        assert_eq!(Activity::Paused.seeded(""), Activity::Idle);
        assert!(!Activity::Idle.is_running());
        assert!(Activity::Slowed.is_running());
    }
}
