// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The two lists behind the Online page: cloud backup snapshots, and the
//! remote-control activity log: what each row says. The
//! wording of a row mirrors Core's own TUI (`pkg/ui/tui/remote_activity.go`):
//! identifiers Core sends (an operation type, an origin, an outcome) are
//! shown as Core named them, with anything that could break the layout
//! replaced first.

/// One row a list modal shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListRow {
    /// What Accept answers with: a snapshot id, or a fixed action key.
    pub id: String,
    pub label: String,
    pub detail: String,
    /// A row the user cannot act on (an incompatible snapshot).
    pub enabled: bool,
    /// A snapshot this device made: the view says so in its own words.
    pub this_device: bool,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Why Core refused a cloud backup request, as far as its message says.
/// Core sends these as plain messages with no error category, so this
/// reads the same phrases Core's own TUI does (`backupActionErrorText` in
/// `pkg/ui/tui/settings.go`); anything it does not recognize is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Failure {
    /// The device has no linked account (or its link was dropped).
    Unlinked,
    /// The account has no active Warp subscription.
    NeedsWarp,
    /// A backup was uploaded too recently.
    RateLimited,
    /// The account's cloud storage is full.
    QuotaExceeded,
    /// Another backup, restore or request holds Core.
    Busy,
    /// A game is starting or running; a restore will not run over it.
    MediaActive,
    /// A previous restore is waiting on a restart.
    RestartNeeded,
    /// The snapshot was made by a newer Core.
    NeedsNewer,
    /// This device has no room to stage the backup.
    DiskSpace,
    /// Unreachable, or a reason this build does not name.
    #[default]
    Other,
}

impl Failure {
    /// The stable key the error dialog words this cause by.
    pub fn token(self) -> &'static str {
        match self {
            Self::Unlinked => "unlinked",
            Self::NeedsWarp => "needs_warp",
            Self::RateLimited => "rate_limited",
            Self::QuotaExceeded => "quota_exceeded",
            Self::Busy => "busy",
            Self::MediaActive => "media_active",
            Self::RestartNeeded => "restart_needed",
            Self::NeedsNewer => "needs_newer",
            Self::DiskSpace => "disk_space",
            Self::Other => "",
        }
    }
}

pub fn failure(message: &str) -> Failure {
    let message = message.to_lowercase();
    let has = |phrase: &str| message.contains(phrase);
    if has("remote backup is unlinked") || has("device not linked") {
        Failure::Unlinked
    } else if has("remote backup is not available for this account") {
        Failure::NeedsWarp
    } else if has("remote backup rate limited") {
        Failure::RateLimited
    } else if has("remote backup quota exceeded") {
        Failure::QuotaExceeded
    } else if (has("backup operation ") && has(" has been running since "))
        || has("zaparoo is busy with another request or a media launch")
    {
        Failure::Busy
    } else if has("cannot restore backup while media is active")
        || has("cannot restore backup while media is launching")
        || has("media launch is in progress")
    {
        Failure::MediaActive
    } else if has("until zaparoo restarts to finish the previous restore")
        || has("backup restore rollback requires recovery")
        || has("pending backup restore transaction exists")
        || has("backup restore restart is pending")
    {
        Failure::RestartNeeded
    } else if has("backup requires a newer core version") {
        Failure::NeedsNewer
    } else if has("insufficient disk space") {
        Failure::DiskSpace
    } else {
        Failure::Other
    }
}

/// Replace anything that is not safe to draw as one line of text.
pub fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

/// An RFC 3339 timestamp as "2 Jan 15:04" (UTC, as Core stores it), or the
/// cleaned raw value when it does not read as one.
pub fn short_time(raw: &str) -> String {
    let parts = (|| {
        let (date, rest) = raw.split_once('T')?;
        let mut date = date.split('-');
        let _year = date.next()?;
        let month: usize = date.next()?.parse().ok()?;
        let day: u32 = date.next()?.parse().ok()?;
        let clock = rest.get(..5)?;
        let (hour, minute) = clock.split_once(':')?;
        hour.parse::<u32>().ok()?;
        minute.parse::<u32>().ok()?;
        Some((MONTHS.get(month.checked_sub(1)?)?, day, clock.to_string()))
    })();
    match parts {
        Some((month, day, clock)) => format!("{day} {month} {clock}"),
        None => clean(raw),
    }
}

/// A byte count the way a settings list reads it: "812 B", "4.2 MB".
pub fn bytes(count: i64) -> String {
    let count = count.max(0);
    if count < 1024 {
        return format!("{count} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = count as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}

/// A remote operation Core ran for the account.
#[derive(Debug, Clone, Copy, Default)]
pub struct Activity<'a> {
    pub created_at: &'a str,
    pub operation_type: &'a str,
    pub origin_kind: &'a str,
    pub origin_key_name: &'a str,
    pub state: &'a str,
    pub status: &'a str,
    pub error_code: &'a str,
}

/// "account" or "api-key (name)": who issued the operation.
fn origin(entry: &Activity<'_>) -> String {
    let kind = clean(entry.origin_kind);
    if entry.origin_key_name.is_empty() {
        kind
    } else {
        format!("{kind} ({})", clean(entry.origin_key_name))
    }
}

/// The row for one activity entry: when and what on the first line; who
/// asked and how it went on the second.
pub fn activity_row(entry: &Activity<'_>) -> ListRow {
    let outcome = if entry.status.is_empty() {
        entry.state
    } else {
        entry.status
    };
    let mut detail = format!("{}, {}", origin(entry), clean(outcome));
    if !entry.error_code.is_empty() {
        detail.push_str(": ");
        detail.push_str(&clean(entry.error_code));
    }
    ListRow {
        id: String::new(),
        label: format!(
            "{}  {}",
            short_time(entry.created_at),
            clean(entry.operation_type)
        ),
        detail,
        enabled: true,
        this_device: false,
    }
}

/// A cloud snapshot in the account's catalog.
#[derive(Debug, Clone, Copy, Default)]
pub struct Snapshot<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub size_bytes: i64,
    /// The device that made it, when Core names one.
    pub device_name: Option<&'a str>,
    /// This device made it.
    pub current_device: bool,
    /// Made by a newer Core than this one: listed, never restorable.
    pub incompatible: bool,
}

/// The row for one snapshot. The size and the device that made it share the
/// second line (the view names this device itself); an incompatible one
/// cannot be accepted.
pub fn snapshot_row(snapshot: &Snapshot<'_>) -> ListRow {
    let mut detail = bytes(snapshot.size_bytes);
    if !snapshot.current_device {
        if let Some(name) = snapshot.device_name.filter(|name| !name.is_empty()) {
            detail.push_str(", ");
            detail.push_str(&clean(name));
        }
    }
    ListRow {
        id: snapshot.id.to_string(),
        label: short_time(snapshot.created_at),
        detail,
        enabled: !snapshot.incompatible,
        this_device: snapshot.current_device,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_time_reads_core_timestamps_and_keeps_anything_else() {
        assert_eq!(short_time("2026-10-02T09:45:11Z"), "2 Oct 09:45");
        assert_eq!(short_time("2026-01-15T23:05:00.123+10:00"), "15 Jan 23:05");
        assert_eq!(short_time("yesterday"), "yesterday");
        assert_eq!(short_time("2026-13-02T09:45:11Z"), "2026-13-02T09:45:11Z");
        assert_eq!(short_time("a\nb"), "a\u{FFFD}b");
    }

    #[test]
    fn a_refused_backup_request_is_worded_by_its_cause() {
        assert_eq!(
            failure("failed to list remote backups: remote backup is unlinked"),
            Failure::Unlinked
        );
        assert_eq!(failure("device not linked"), Failure::Unlinked);
        assert_eq!(
            failure("remote backup is not available for this account"),
            Failure::NeedsWarp
        );
        assert_eq!(
            failure("failed to run remote backup: remote backup rate limited"),
            Failure::RateLimited
        );
        assert_eq!(
            failure("Remote backup quota exceeded"),
            Failure::QuotaExceeded,
            "matching ignores case, as Core's TUI does"
        );
        assert_eq!(
            failure("backup operation upload has been running since 10:02"),
            Failure::Busy
        );
        assert_eq!(
            failure("cannot restore backup while media is active"),
            Failure::MediaActive
        );
        assert_eq!(
            failure("backup restore restart is pending"),
            Failure::RestartNeeded
        );
        assert_eq!(
            failure("backup requires a newer Core version"),
            Failure::NeedsNewer
        );
        assert_eq!(failure("insufficient disk space"), Failure::DiskSpace);
        assert_eq!(failure("dial tcp: i/o timeout"), Failure::Other);
        assert_eq!(failure(""), Failure::Other);
        // Every named cause has its own dialog wording; only the unnamed
        // one falls back to the generic body.
        assert_eq!(Failure::Other.token(), "");
        assert_eq!(Failure::RateLimited.token(), "rate_limited");
    }

    #[test]
    fn bytes_scale_to_a_readable_unit() {
        assert_eq!(bytes(-5), "0 B");
        assert_eq!(bytes(812), "812 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(4_404_019), "4.2 MB");
        assert_eq!(bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn an_activity_row_says_when_what_who_and_how_it_went() {
        let row = activity_row(&Activity {
            created_at: "2026-10-02T09:45:11Z",
            operation_type: "launch",
            origin_kind: "account",
            state: "terminal",
            status: "failed",
            error_code: "host_foreground_required",
            ..Activity::default()
        });
        assert_eq!(row.label, "2 Oct 09:45  launch");
        assert_eq!(
            row.detail, "account, failed: host_foreground_required",
            "an outcome with an error code carries it after a colon"
        );
        assert!(row.enabled);
    }

    #[test]
    fn an_activity_without_a_result_shows_its_state_and_a_named_key() {
        let row = activity_row(&Activity {
            created_at: "2026-10-02T09:45:11Z",
            operation_type: "stop",
            origin_kind: "api_key",
            origin_key_name: "my\nkey",
            state: "executing",
            ..Activity::default()
        });
        assert_eq!(row.detail, "api_key (my\u{FFFD}key), executing");
    }

    #[test]
    fn a_snapshot_row_names_its_size_and_where_it_came_from() {
        let mine = snapshot_row(&Snapshot {
            id: "s1",
            created_at: "2026-10-02T09:45:11Z",
            size_bytes: 2048,
            device_name: Some("Handheld"),
            current_device: true,
            incompatible: false,
        });
        assert_eq!(mine.id, "s1");
        assert_eq!(mine.label, "2 Oct 09:45");
        assert_eq!(mine.detail, "2.0 KB");
        assert!(mine.this_device, "the view words this, not the row text");
        assert!(mine.enabled);

        let other = snapshot_row(&Snapshot {
            id: "s2",
            created_at: "2026-10-01T01:00:00Z",
            size_bytes: 10,
            device_name: Some("MiSTer"),
            ..Snapshot::default()
        });
        assert_eq!(other.detail, "10 B, MiSTer");
        assert!(!other.this_device);

        let unnamed = snapshot_row(&Snapshot {
            id: "s3",
            device_name: Some(""),
            ..Snapshot::default()
        });
        assert_eq!(unnamed.detail, "0 B");
    }

    #[test]
    fn an_incompatible_snapshot_lists_but_cannot_be_accepted() {
        let row = snapshot_row(&Snapshot {
            id: "s9",
            incompatible: true,
            ..Snapshot::default()
        });
        assert!(!row.enabled);
    }
}
