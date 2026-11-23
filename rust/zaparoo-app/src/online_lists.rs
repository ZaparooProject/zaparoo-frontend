// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The lists behind the Online page: cloud backup snapshots, local
//! backups and one local backup's details, and the remote-control
//! activity log: what each row says. The
//! wording of a row mirrors Core's own TUI (`pkg/ui/tui/remote_activity.go`):
//! identifiers Core sends (an operation type, an origin, an outcome) are
//! shown as Core named them, with anything that could break the layout
//! replaced first.

/// What a list row is. An entry (a snapshot, a local backup, an activity
/// entry) carries its own text; every other kind is named by the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowKind {
    #[default]
    Entry,
    /// "Back up now", leading the cloud list.
    Run,
    /// A local backup's details: when it was made.
    Created,
    /// A local backup's details: its size.
    Size,
    /// A local backup's details: files it skipped (`count`, possibly 0
    /// when Core named none).
    Skipped,
    /// A local backup's details: Core's error summary in `detail`.
    Error,
    /// A local backup's details: one category, `count` files of `detail`
    /// size.
    Category(BackupCategory),
    /// Delete this local backup.
    Delete,
}

/// The categories a backup holds, in the order Core's TUI lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupCategory {
    Zaparoo,
    Settings,
    Inputs,
    Saves,
    Savestates,
}

impl BackupCategory {
    pub const ALL: [Self; 5] = [
        Self::Zaparoo,
        Self::Settings,
        Self::Inputs,
        Self::Saves,
        Self::Savestates,
    ];

    /// Core's token for the category.
    pub fn token(self) -> &'static str {
        match self {
            Self::Zaparoo => "zaparoo",
            Self::Settings => "settings",
            Self::Inputs => "inputs",
            Self::Saves => "saves",
            Self::Savestates => "savestates",
        }
    }
}

/// One row a list modal shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListRow {
    pub kind: RowKind,
    /// What Accept answers with: a snapshot id or a local backup's name.
    pub id: String,
    pub label: String,
    pub detail: String,
    /// A number the view words itself (files in a category, skipped files).
    pub count: i64,
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
        kind: RowKind::Entry,
        id: String::new(),
        label: format!(
            "{}  {}",
            short_time(entry.created_at),
            clean(entry.operation_type)
        ),
        detail,
        count: 0,
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
        kind: RowKind::Entry,
        id: snapshot.id.to_string(),
        label: short_time(snapshot.created_at),
        detail,
        count: 0,
        enabled: !snapshot.incompatible,
        this_device: snapshot.current_device,
    }
}

/// A local backup ZIP as `settings.backup.list` names it.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalBackup<'a> {
    pub name: &'a str,
    pub created_at: &'a str,
    pub size: i64,
}

/// The rows for the local backup list, newest first: when on the first
/// line, its size on the second. Accept answers with the file name.
pub fn local_backup_rows(backups: &[LocalBackup<'_>]) -> Vec<ListRow> {
    let mut sorted: Vec<_> = backups.iter().collect();
    sorted.sort_by(|a, b| b.created_at.cmp(a.created_at));
    sorted
        .into_iter()
        .map(|backup| ListRow {
            kind: RowKind::Entry,
            id: backup.name.to_string(),
            label: if backup.created_at.is_empty() {
                clean(backup.name)
            } else {
                short_time(backup.created_at)
            },
            detail: bytes(backup.size),
            count: 0,
            enabled: true,
            this_device: false,
        })
        .collect()
}

/// One category of an inspected local backup.
#[derive(Debug, Clone, Copy, Default)]
pub struct Category {
    pub files: i64,
    pub bytes: i64,
    pub enabled: bool,
}

/// An inspected local backup, as `settings.backup.inspect` describes it.
#[derive(Debug, Clone, Default)]
pub struct LocalDetails<'a> {
    pub name: &'a str,
    pub created_at: &'a str,
    pub size: i64,
    /// "success" or "partial".
    pub status: &'a str,
    pub warnings: usize,
    pub error: Option<&'a str>,
    /// Each category Core reported, by its token.
    pub categories: &'a [(&'a str, Category)],
}

/// The rows of one local backup's details page: when, how big, what was
/// skipped or went wrong, each category it holds, then Delete. Only Delete
/// can be accepted.
pub fn local_detail_rows(details: &LocalDetails<'_>) -> Vec<ListRow> {
    let info = |kind: RowKind, detail: String, count: i64| ListRow {
        kind,
        id: details.name.to_string(),
        label: String::new(),
        detail,
        count,
        enabled: true,
        this_device: false,
    };
    let mut rows = vec![
        info(RowKind::Created, short_time(details.created_at), 0),
        info(RowKind::Size, bytes(details.size), 0),
    ];
    if details.status == "partial" || details.warnings > 0 {
        rows.push(info(
            RowKind::Skipped,
            String::new(),
            i64::try_from(details.warnings).unwrap_or(i64::MAX),
        ));
    }
    if let Some(error) = details.error.filter(|e| !e.is_empty()) {
        rows.push(info(RowKind::Error, clean(error), 0));
    }
    for category in BackupCategory::ALL {
        let entry = details
            .categories
            .iter()
            .find(|(token, _)| *token == category.token())
            .map(|(_, entry)| *entry)
            .filter(|entry| entry.enabled);
        if let Some(entry) = entry {
            rows.push(info(
                RowKind::Category(category),
                bytes(entry.bytes),
                entry.files.max(0),
            ));
        }
    }
    rows.push(info(RowKind::Delete, String::new(), 0));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_backups_list_newest_first_by_their_time_and_size() {
        let rows = local_backup_rows(&[
            LocalBackup {
                name: "old.zip",
                created_at: "2026-10-01T09:00:00Z",
                size: 2048,
            },
            LocalBackup {
                name: "new.zip",
                created_at: "2026-10-02T09:45:11Z",
                size: 100,
            },
            LocalBackup {
                name: "odd\nname.zip",
                created_at: "",
                size: 0,
            },
        ]);
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["new.zip", "old.zip", "odd\nname.zip"]);
        assert_eq!(rows[0].label, "2 Oct 09:45");
        assert_eq!(rows[0].detail, "100 B");
        assert_eq!(rows[1].detail, "2.0 KB");
        assert_eq!(
            rows[2].label, "odd\u{FFFD}name.zip",
            "no time: the file name"
        );
        assert!(rows.iter().all(|r| r.kind == RowKind::Entry && r.enabled));
    }

    #[test]
    fn local_details_list_what_the_backup_holds_then_delete() {
        let categories = [
            (
                "saves",
                Category {
                    files: 12,
                    bytes: 4096,
                    enabled: true,
                },
            ),
            (
                "settings",
                Category {
                    files: 3,
                    bytes: 10,
                    enabled: true,
                },
            ),
            (
                "inputs",
                Category {
                    files: 9,
                    bytes: 9,
                    enabled: false,
                },
            ),
            (
                "unknown",
                Category {
                    files: 1,
                    bytes: 1,
                    enabled: true,
                },
            ),
        ];
        let rows = local_detail_rows(&LocalDetails {
            name: "b.zip",
            created_at: "2026-10-02T09:45:11Z",
            size: 4106,
            status: "partial",
            warnings: 2,
            error: Some("disk\nfull"),
            categories: &categories,
        });
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Created,
                RowKind::Size,
                RowKind::Skipped,
                RowKind::Error,
                RowKind::Category(BackupCategory::Settings),
                RowKind::Category(BackupCategory::Saves),
                RowKind::Delete,
            ],
            "a category left out of the backup, or one this build does not know, is not listed"
        );
        assert_eq!(rows[2].count, 2);
        assert_eq!(rows[3].detail, "disk\u{FFFD}full");
        assert_eq!(rows[5].count, 12);
        assert_eq!(rows[5].detail, "4.0 KB");
        assert!(rows.iter().all(|r| r.id == "b.zip"));
    }

    #[test]
    fn a_complete_local_backup_has_no_skipped_or_error_row() {
        let rows = local_detail_rows(&LocalDetails {
            name: "b.zip",
            created_at: "2026-10-02T09:45:11Z",
            size: 1,
            status: "success",
            ..LocalDetails::default()
        });
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [RowKind::Created, RowKind::Size, RowKind::Delete]);
    }

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
