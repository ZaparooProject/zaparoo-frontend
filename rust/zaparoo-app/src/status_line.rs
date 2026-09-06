// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Port of `src/ui/components/StatusLine.qml`'s message ladder: the header
// shows, in priority order, only one of the Core link state, an active
// background task, a terminal message held briefly after a task ends, or a
// transient event. The ladder is toolkit-free and clock-agnostic: callers
// feed it the current link and task state plus `Instant`s, and it answers
// with a stable message kind and its arguments. The view composes the
// sentence, so every user-visible string stays in the `.slint` catalogs.

use std::time::{Duration, Instant};

/// Core link state, mirroring `zaparoo_core::client::ConnectionState`
/// without the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Link {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Unreachable,
}

/// Tier 1 input: the link plus the catalog endpoint's own error state
/// (`AppStatus.connection_state == ERROR` in the Qt build).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinkInput {
    pub link: Link,
    pub catalog_error: bool,
    pub last_error: String,
}

/// Tier 2 input: the `MediaStatus` fields the ladder reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags, one per Core status field"
)]
pub struct TaskInput {
    pub indexing: bool,
    pub optimizing: bool,
    pub scraping: bool,
    pub paused: bool,
    pub scrape_paused: bool,
    pub current_step: i32,
    pub total_steps: i32,
    pub current_step_display: String,
    pub scrape_current_step: i32,
    pub scrape_total_steps: i32,
    pub scrape_current_step_display: String,
    pub total_files: i32,
    pub scrape_state: String,
    pub scrape_error: String,
    pub scrape_matched: i32,
    pub scrape_total: i32,
}

impl TaskInput {
    fn index_busy(&self) -> bool {
        self.indexing || self.optimizing
    }
}

/// Tier 4 input: a classified Core notification. `TokenScanned` is
/// deliberately never shown (the Qt line ignores it too) but is kept so the
/// classifier stays complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    TokenScanned(String),
    PlaytimeWarning(String),
    InboxMessage(String),
}

/// What the line says, as a stable kind the view maps to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    None,
    Disconnected,
    Reconnecting,
    Connecting,
    /// `arg` carries the last error, possibly empty.
    CoreError,
    IndexingPaused,
    /// `arg` carries the current step display, possibly empty.
    Indexing,
    /// `arg` carries the current step display, possibly empty.
    Optimizing,
    ImportingPaused,
    /// `arg` carries the current step display, possibly empty.
    Importing,
    /// `arg` carries the formatted file count.
    IndexedFiles,
    IndexingComplete,
    /// `arg` carries the scrape error.
    ImportFailed,
    /// `arg` and `arg2` carry the formatted matched and total counts.
    Imported,
    /// `arg` carries the remaining time text.
    PlaytimeWarning,
    /// `arg` carries the inbox title.
    Inbox,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Disconnected => "disconnected",
            Self::Reconnecting => "reconnecting",
            Self::Connecting => "connecting",
            Self::CoreError => "core-error",
            Self::IndexingPaused => "indexing-paused",
            Self::Indexing => "indexing",
            Self::Optimizing => "optimizing",
            Self::ImportingPaused => "importing-paused",
            Self::Importing => "importing",
            Self::IndexedFiles => "indexed-files",
            Self::IndexingComplete => "indexing-complete",
            Self::ImportFailed => "import-failed",
            Self::Imported => "imported",
            Self::PlaytimeWarning => "playtime-warning",
            Self::Inbox => "inbox",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Message {
    pub kind: Kind,
    pub arg: String,
    pub arg2: String,
}

impl Message {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    fn with_arg(kind: Kind, arg: impl Into<String>) -> Self {
        Self {
            kind,
            arg: arg.into(),
            arg2: String::new(),
        }
    }
}

/// The resolved line plus the track state next to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per StatusLine.qml binding the view reads"
)]
pub struct Output {
    pub message: Message,
    pub is_error: bool,
    /// The segmented track paints only while a task is active.
    pub show_track: bool,
    pub paused: bool,
    /// False while Core reports no step count (optimize/vacuum): one cell
    /// marches instead of a fraction filling.
    pub total_known: bool,
    pub current_step: i32,
    pub total_steps: i32,
    /// Whole percent, present only when the active task has a known total.
    pub percent: Option<i32>,
}

/// Formats a count for the terminal messages; the ladder does not know
/// the language, so the driver supplies it.
pub type CountFormatter<'a> = &'a dyn Fn(i32) -> String;

/// The one piece of real state in the line: the terminal and event
/// messages that must outlive the moment their condition went false.
#[derive(Debug, Clone)]
pub struct Ladder {
    media_activity_enabled: bool,
    index_was_busy: bool,
    scrape_was_busy: bool,
    terminal: Option<(Message, Instant)>,
    event: Option<(Message, Instant)>,
    terminal_dwell: Duration,
    event_dwell: Duration,
}

impl Default for Ladder {
    fn default() -> Self {
        Self::new()
    }
}

impl Ladder {
    pub const TERMINAL_DWELL: Duration = Duration::from_secs(6);
    pub const EVENT_DWELL: Duration = Duration::from_secs(5);

    pub fn new() -> Self {
        Self {
            media_activity_enabled: false,
            index_was_busy: false,
            scrape_was_busy: false,
            terminal: None,
            event: None,
            terminal_dwell: Self::TERMINAL_DWELL,
            event_dwell: Self::EVENT_DWELL,
        }
    }

    /// Shorter dwells for tests.
    #[must_use]
    pub fn with_dwells(mut self, terminal: Duration, event: Duration) -> Self {
        self.terminal_dwell = terminal;
        self.event_dwell = event;
        self
    }

    /// Turns the task tiers on and seeds the busy edges from the current
    /// task state, so the first change afterwards cannot be misread as a
    /// fresh edge (the Qt `onMediaActivityEnabledChanged` seed).
    pub fn enable_media_activity(&mut self, task: &TaskInput) {
        self.media_activity_enabled = true;
        self.index_was_busy = task.index_busy();
        self.scrape_was_busy = task.scraping;
    }

    /// Feed a task state change; latches the terminal message on a
    /// busy-to-idle edge and clears it on an idle-to-busy edge.
    pub fn observe_task(&mut self, task: &TaskInput, now: Instant, count: CountFormatter) {
        let index_busy = task.index_busy();
        if !self.index_was_busy && index_busy {
            self.terminal = None;
        } else if self.index_was_busy && !index_busy {
            let message = if task.total_files > 0 {
                Message::with_arg(Kind::IndexedFiles, count(task.total_files))
            } else {
                Message::new(Kind::IndexingComplete)
            };
            self.terminal = Some((message, now));
        }
        self.index_was_busy = index_busy;

        if !self.scrape_was_busy && task.scraping {
            self.terminal = None;
        } else if self.scrape_was_busy && !task.scraping {
            let message = if task.scrape_state == "failed" {
                Message::with_arg(Kind::ImportFailed, task.scrape_error.clone())
            } else {
                Message {
                    kind: Kind::Imported,
                    arg: count(task.scrape_matched),
                    arg2: count(task.scrape_total),
                }
            };
            self.terminal = Some((message, now));
        }
        self.scrape_was_busy = task.scraping;
    }

    /// Feed a classified notification. Dropped, not queued, while a higher
    /// tier owns the line; Core retains the underlying record.
    pub fn observe_event(
        &mut self,
        link: &LinkInput,
        task: &TaskInput,
        event: &Event,
        now: Instant,
    ) {
        if connection_message(link).is_some()
            || self.task_active(link, task)
            || self.terminal_message(now).is_some()
        {
            return;
        }
        let message = match event {
            Event::TokenScanned(_) => return,
            Event::PlaytimeWarning(remaining) => {
                Message::with_arg(Kind::PlaytimeWarning, remaining.clone())
            }
            Event::InboxMessage(title) => Message::with_arg(Kind::Inbox, title.clone()),
        };
        self.event = Some((message, now));
    }

    fn task_active(&self, link: &LinkInput, task: &TaskInput) -> bool {
        self.media_activity_enabled
            && connection_message(link).is_none()
            && (task.indexing || task.optimizing || task.scraping)
    }

    fn terminal_message(&self, now: Instant) -> Option<&Message> {
        self.terminal
            .as_ref()
            .filter(|(_, since)| now.duration_since(*since) < self.terminal_dwell)
            .map(|(message, _)| message)
    }

    fn event_message(&self, now: Instant) -> Option<&Message> {
        self.event
            .as_ref()
            .filter(|(_, since)| now.duration_since(*since) < self.event_dwell)
            .map(|(message, _)| message)
    }

    /// Resolve the ladder at `now`.
    pub fn render(&self, link: &LinkInput, task: &TaskInput, now: Instant) -> Output {
        let task_active = self.task_active(link, task);
        let message = if let Some(message) = connection_message(link) {
            message
        } else if task_active {
            task_message(task)
        } else if let Some(message) = self.terminal_message(now) {
            message.clone()
        } else if let Some(message) = self.event_message(now) {
            message.clone()
        } else {
            Message::default()
        };

        let paused = if task.indexing {
            task.paused
        } else {
            task.scrape_paused
        };
        let total_known = !task.optimizing;
        let (current_step, total_steps) = if task.indexing {
            (task.current_step, task.total_steps)
        } else {
            (task.scrape_current_step, task.scrape_total_steps)
        };
        let percent = (task_active && total_known && total_steps > 0).then(|| {
            let fraction =
                (f64::from(current_step) / f64::from(total_steps.max(1))).clamp(0.0, 1.0);
            (fraction * 100.0).round() as i32
        });

        Output {
            message,
            is_error: link.link == Link::Unreachable || link.catalog_error,
            show_track: task_active,
            paused,
            total_known,
            current_step,
            total_steps,
            percent,
        }
    }

    /// When the line next changes on its own (a dwell expiring), if ever.
    pub fn next_deadline(&self) -> Option<Instant> {
        let terminal = self
            .terminal
            .as_ref()
            .map(|(_, since)| *since + self.terminal_dwell);
        let event = self
            .event
            .as_ref()
            .map(|(_, since)| *since + self.event_dwell);
        match (terminal, event) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

fn connection_message(link: &LinkInput) -> Option<Message> {
    match link.link {
        Link::Unreachable | Link::Disconnected => Some(Message::new(Kind::Disconnected)),
        Link::Reconnecting => Some(Message::new(Kind::Reconnecting)),
        Link::Connecting => Some(Message::new(Kind::Connecting)),
        Link::Connected if link.catalog_error => {
            Some(Message::with_arg(Kind::CoreError, link.last_error.clone()))
        }
        Link::Connected => None,
    }
}

fn task_message(task: &TaskInput) -> Message {
    if task.indexing {
        if task.paused {
            return Message::new(Kind::IndexingPaused);
        }
        return Message::with_arg(Kind::Indexing, task.current_step_display.clone());
    }
    if task.optimizing {
        return Message::with_arg(Kind::Optimizing, task.current_step_display.clone());
    }
    if task.scrape_paused {
        return Message::new(Kind::ImportingPaused);
    }
    Message::with_arg(Kind::Importing, task.scrape_current_step_display.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(n: i32) -> String {
        n.to_string()
    }

    fn connected() -> LinkInput {
        LinkInput {
            link: Link::Connected,
            ..LinkInput::default()
        }
    }

    #[test]
    fn link_state_wins_over_everything() {
        let mut ladder = Ladder::new();
        let task = TaskInput {
            indexing: true,
            ..TaskInput::default()
        };
        ladder.enable_media_activity(&TaskInput::default());
        let now = Instant::now();
        ladder.observe_task(&task, now, &plain);
        let link = LinkInput {
            link: Link::Reconnecting,
            ..LinkInput::default()
        };
        let out = ladder.render(&link, &task, now);
        assert_eq!(out.message.kind, Kind::Reconnecting);
        assert!(!out.show_track);
        assert!(!out.is_error);

        let link = LinkInput {
            link: Link::Unreachable,
            ..LinkInput::default()
        };
        let out = ladder.render(&link, &task, now);
        assert_eq!(out.message.kind, Kind::Disconnected);
        assert!(out.is_error);
        assert_eq!(out.percent, None);
    }

    #[test]
    fn catalog_error_reports_the_last_error() {
        let ladder = Ladder::new();
        let link = LinkInput {
            link: Link::Connected,
            catalog_error: true,
            last_error: "boom".into(),
        };
        let out = ladder.render(&link, &TaskInput::default(), Instant::now());
        assert_eq!(out.message.kind, Kind::CoreError);
        assert_eq!(out.message.arg, "boom");
        assert!(out.is_error);
    }

    #[test]
    fn active_task_shows_progress_and_percent() {
        let mut ladder = Ladder::new();
        ladder.enable_media_activity(&TaskInput::default());
        let task = TaskInput {
            indexing: true,
            current_step: 3,
            total_steps: 12,
            current_step_display: "SNES".into(),
            ..TaskInput::default()
        };
        let out = ladder.render(&connected(), &task, Instant::now());
        assert_eq!(out.message.kind, Kind::Indexing);
        assert_eq!(out.message.arg, "SNES");
        assert!(out.show_track);
        assert!(out.total_known);
        assert_eq!(out.percent, Some(25));

        let optimizing = TaskInput {
            optimizing: true,
            ..TaskInput::default()
        };
        let out = ladder.render(&connected(), &optimizing, Instant::now());
        assert_eq!(out.message.kind, Kind::Optimizing);
        assert!(!out.total_known);
        assert_eq!(out.percent, None);

        let paused = TaskInput {
            scraping: true,
            scrape_paused: true,
            ..TaskInput::default()
        };
        let out = ladder.render(&connected(), &paused, Instant::now());
        assert_eq!(out.message.kind, Kind::ImportingPaused);
        assert!(out.paused);
    }

    #[test]
    fn tasks_are_hidden_until_media_activity_is_enabled() {
        let ladder = Ladder::new();
        let task = TaskInput {
            indexing: true,
            ..TaskInput::default()
        };
        let out = ladder.render(&connected(), &task, Instant::now());
        assert_eq!(out.message.kind, Kind::None);
        assert!(!out.show_track);
    }

    #[test]
    fn terminal_message_holds_for_the_dwell_then_clears() {
        let mut ladder = Ladder::new().with_dwells(Duration::from_secs(6), Duration::from_secs(5));
        ladder.enable_media_activity(&TaskInput::default());
        let t0 = Instant::now();
        let busy = TaskInput {
            indexing: true,
            total_files: 1234,
            ..TaskInput::default()
        };
        ladder.observe_task(&busy, t0, &plain);
        let idle = TaskInput {
            total_files: 1234,
            ..TaskInput::default()
        };
        ladder.observe_task(&idle, t0, &plain);
        let out = ladder.render(&connected(), &idle, t0);
        assert_eq!(out.message.kind, Kind::IndexedFiles);
        assert_eq!(out.message.arg, "1234");
        assert!(!out.show_track);
        assert_eq!(ladder.next_deadline(), Some(t0 + Duration::from_secs(6)));

        let later = t0 + Duration::from_secs(6);
        let out = ladder.render(&connected(), &idle, later);
        assert_eq!(out.message.kind, Kind::None);
    }

    #[test]
    fn scrape_end_reports_failure_or_totals() {
        let mut ladder = Ladder::new();
        ladder.enable_media_activity(&TaskInput::default());
        let t0 = Instant::now();
        let busy = TaskInput {
            scraping: true,
            ..TaskInput::default()
        };
        ladder.observe_task(&busy, t0, &plain);
        let failed = TaskInput {
            scrape_state: "failed".into(),
            scrape_error: "no network".into(),
            ..TaskInput::default()
        };
        ladder.observe_task(&failed, t0, &plain);
        let out = ladder.render(&connected(), &failed, t0);
        assert_eq!(out.message.kind, Kind::ImportFailed);
        assert_eq!(out.message.arg, "no network");

        ladder.observe_task(&busy, t0, &plain);
        let done = TaskInput {
            scrape_matched: 40,
            scrape_total: 50,
            ..TaskInput::default()
        };
        ladder.observe_task(&done, t0, &plain);
        let out = ladder.render(&connected(), &done, t0);
        assert_eq!(out.message.kind, Kind::Imported);
        assert_eq!(
            (out.message.arg.as_str(), out.message.arg2.as_str()),
            ("40", "50")
        );
    }

    #[test]
    fn a_new_task_clears_the_held_terminal_message() {
        let mut ladder = Ladder::new();
        ladder.enable_media_activity(&TaskInput::default());
        let t0 = Instant::now();
        let busy = TaskInput {
            indexing: true,
            ..TaskInput::default()
        };
        ladder.observe_task(&busy, t0, &plain);
        ladder.observe_task(&TaskInput::default(), t0, &plain);
        assert_eq!(
            ladder
                .render(&connected(), &TaskInput::default(), t0)
                .message
                .kind,
            Kind::IndexingComplete
        );
        ladder.observe_task(&busy, t0, &plain);
        assert_eq!(
            ladder.render(&connected(), &busy, t0).message.kind,
            Kind::Indexing
        );
        assert_eq!(ladder.next_deadline(), None);
    }

    #[test]
    fn events_show_only_when_nothing_higher_owns_the_line() {
        let mut ladder = Ladder::new().with_dwells(Duration::from_secs(6), Duration::from_secs(5));
        ladder.enable_media_activity(&TaskInput::default());
        let t0 = Instant::now();
        let idle = TaskInput::default();

        ladder.observe_event(&connected(), &idle, &Event::TokenScanned("x".into()), t0);
        assert_eq!(
            ladder.render(&connected(), &idle, t0).message.kind,
            Kind::None
        );

        ladder.observe_event(
            &connected(),
            &idle,
            &Event::PlaytimeWarning("5m".into()),
            t0,
        );
        let out = ladder.render(&connected(), &idle, t0);
        assert_eq!(out.message.kind, Kind::PlaytimeWarning);
        assert_eq!(out.message.arg, "5m");
        assert_eq!(
            ladder
                .render(&connected(), &idle, t0 + Duration::from_secs(5))
                .message
                .kind,
            Kind::None
        );

        let busy = TaskInput {
            indexing: true,
            ..TaskInput::default()
        };
        ladder.observe_event(
            &connected(),
            &busy,
            &Event::InboxMessage("Hi".into()),
            t0 + Duration::from_secs(10),
        );
        assert_eq!(
            ladder
                .render(&connected(), &idle, t0 + Duration::from_secs(10))
                .message
                .kind,
            Kind::None
        );

        ladder.observe_event(
            &connected(),
            &idle,
            &Event::InboxMessage("Hi".into()),
            t0 + Duration::from_secs(10),
        );
        assert_eq!(
            ladder
                .render(&connected(), &idle, t0 + Duration::from_secs(10))
                .message
                .kind,
            Kind::Inbox
        );
    }
}
