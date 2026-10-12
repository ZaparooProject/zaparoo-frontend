// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The contract between the frontend and the `zaparoo-update` module.
//!
//! `zaparoo-update` is either the public stub in this workspace or the
//! private implementation that official builds substitute for it. Both
//! speak these types, so neither side can drift from the other: the module
//! owns the update run and the screen's state machine and publishes a
//! [`ViewState`]; the frontend forwards [`Input`] and renders the state
//! through the `UpdateView` global in `ui/update_view.slint`.
//!
//! Everything user-visible travels as an enum or an identifier. The
//! `.slint` side composes the sentences, so translators see whole
//! sentences with `{}` placeholders and Rust never formats prose.

use std::sync::Arc;

/// Which page of the Update screen is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The start page. Nothing runs until the person chooses Start.
    #[default]
    Intro,
    /// The updater is running.
    Running,
    /// Cancel was requested; the updater is being stopped.
    Stopping,
    /// The run ended: summary and actions.
    Finished,
    /// The list of installed, updated, removed and failed files.
    Details,
    /// A Linux system image is being written; leaving is locked.
    Linux,
    /// The reboot countdown after a Linux update.
    Rebooting,
}

/// A button on the intro and finished pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Start,
    Back,
    Details,
    Errors,
    Ok,
    Reboot,
}

/// What the finished page's title and body say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    #[default]
    None,
    /// The updater tool is not installed on this device.
    Unavailable,
    UpToDate,
    Complete,
    /// Complete, and a Zaparoo file needs a reboot to take effect.
    CompleteRebootNeeded,
    CompleteWithErrors,
    Failed,
    LinuxComplete,
    LinuxFailed,
}

/// The status line under the progress track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    #[default]
    None,
    /// Waiting for the first file.
    Starting,
    /// No structured output for a while; the update is still working.
    Slow,
    /// `arg` is the file being processed.
    File,
    /// `arg` is a raw line from the updater tool (not translatable).
    Tool,
    /// `arg` is a transition id, or empty.
    Transition,
    Stopping,
    /// Stopping has taken a while.
    StoppingSlow,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub kind: StatusKind,
    pub arg: String,
}

/// A failure the updater reported, as a closed set. `arg` on [`ViewState`]
/// carries the one runtime value the sentence needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ErrorCode {
    #[default]
    None,
    NotResponding,
    Unavailable,
    ToolTooOld,
    SecureConnection,
    Critical,
    StateSave,
    StateLoad,
    Network,
    SomeFiles,
    SomeDatabases,
    Config,
    NoCerts,
    FullPartition,
    /// `arg` is the code the updater sent.
    Unexpected,
    Generic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub installed: i32,
    pub updated: i32,
    pub removed: i32,
    pub failed: i32,
    pub duplicated: i32,
    pub not_overwritten: i32,
    pub database_errors: i32,
    pub zip_errors: i32,
    pub folder_errors: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinuxPhase {
    #[default]
    Preparing,
    FetchImage,
    FetchTool,
    Extract,
    UserFiles,
    Flash,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinuxView {
    pub phase: LinuxPhase,
    pub current_version: String,
    pub new_version: String,
    pub failed: bool,
}

/// One "from your memberships" message the updater sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Membership {
    pub topic: String,
    pub message: String,
    pub info: String,
}

/// The pill filter on the details page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Installed,
    Updated,
    Removed,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowKind {
    #[default]
    File,
    Folder,
    Database,
    LinuxUpdate,
    LinuxUpdateDetail,
    DuplicateCategory,
    DuplicateFile,
    NotOverwrittenCategory,
    NotOverwrittenFile,
    DatabaseError,
    DatabaseErrorReason,
    ZipError,
    ZipErrorReason,
    FolderError,
    FolderErrorReason,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowStatus {
    #[default]
    None,
    Installed,
    Updated,
    Removed,
    Failed,
}

/// Which well-known folder a folder row names, so the label can be worded
/// in the person's language instead of shown as a path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FolderKind {
    #[default]
    Plain,
    Arcade,
    ArcadeCore,
    ArcadeAlternative,
    Console,
    Computer,
    Downloader,
    UpdateAll,
}

/// One flattened row of the details list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub status: RowStatus,
    pub folder: FolderKind,
    /// Nesting depth, for indentation.
    pub depth: i32,
    pub collapsed: bool,
    pub database: String,
    /// The file or folder name, or a Linux version. Never translated.
    pub label: String,
    /// The full path or the updater's reason, when there is one.
    pub reason: String,
    pub file_count: i32,
    pub counts: Counts,
}

/// The panel that explains one row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowInfo {
    pub row: Row,
    /// Other databases involved (duplicates), first the kept one.
    pub databases: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub filter: Filter,
    /// True when the filter row shows (more than two options have rows).
    pub filter_visible: bool,
    /// True while focus is on the filter row, false on the list.
    pub filter_focused: bool,
    pub filter_focus: Filter,
    pub focused_row: i32,
    pub row_count: i32,
    pub database_count: i32,
    pub counts: Counts,
    pub info: Option<RowInfo>,
}

/// A hint for the help bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Help {
    pub button: HelpButton,
    pub label: HelpLabel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpButton {
    Dpad,
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpLabel {
    Move,
    Select,
    Start,
    Back,
    Cancel,
    Close,
    Toggle,
    Info,
}

/// Everything the screen renders, published whole on every change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub page: Page,
    /// The updater's "major.minor" version, empty when unknown.
    pub version: String,
    pub available: bool,
    pub status: Status,
    /// Progress in basis points, 0 to 10000.
    pub progress_bp: i32,
    /// How many decimals the percentage shows, from the size of the run.
    pub progress_decimals: u8,
    /// False until the first file starts; the track is indeterminate.
    pub progress_known: bool,
    pub outcome: Outcome,
    pub error: ErrorCode,
    pub error_arg: String,
    pub counts: Counts,
    pub linux: LinuxView,
    pub membership: Vec<Membership>,
    pub membership_index: i32,
    /// A transition id from Update All, empty when none.
    pub transition: String,
    pub buttons: Vec<Button>,
    pub button_focus: i32,
    pub countdown_secs: i32,
    pub details: Details,
    pub help: Vec<Help>,
    pub allows_screensaver: bool,
}

/// One change to the details list, applied in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowsDelta {
    Reset(Vec<Row>),
    Insert { at: usize, rows: Vec<Row> },
    Remove { at: usize, count: usize },
    Replace { at: usize, row: Row },
}

/// A normalized key or pointer press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// The screen was opened. Resets a finished screen to the intro page.
    Enter,
    Up,
    Down,
    Left,
    Right,
    PagePrev,
    PageNext,
    Accept,
    Cancel,
    /// The "Stop update?" dialog was answered.
    StopConfirmed,
    StopDeclined,
    /// `reboot now` failed to start or returned an error.
    RebootFailed,
    TapButton(usize),
    TapRow(usize),
    TapFilter(Filter),
}

/// Something the module needs the frontend to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Return to the Hub.
    LeaveToHub,
    /// Ask "Stop update?".
    ConfirmStop,
    /// Close that question: the run ended or leaving got locked.
    CloseStopConfirm,
    /// Run `reboot now`; answer failure with [`Input::RebootFailed`].
    Reboot,
}

/// What the module publishes. Delivered in order on an arbitrary thread;
/// a sink must hand off to its own thread and never call back into the
/// session from inside the call.
#[derive(Clone, Debug)]
pub enum Event {
    State(Arc<ViewState>),
    Rows(RowsDelta),
    Effect(Effect),
}

/// Where a session sends its [`Event`]s.
pub type Sink = Arc<dyn Fn(Event) + Send + Sync + 'static>;
