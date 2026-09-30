// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Update screen driver. The `zaparoo-update` module owns the update
//! run and the screen's state machine; this file hosts its session, maps the
//! module's `ViewState` onto the `UpdateView` global, and carries out the
//! effects it asks for. Without the private module the crate is a stub, the
//! updater is never available, and the Hub shows no Update tile.

use crate::router::Ctx;
use crate::{
    App, DialogButton, DialogKind, Screen, UpdateButton, UpdateCounts, UpdateError, UpdateFilter,
    UpdateFolderKind, UpdateHelp, UpdateHelpLabel, UpdateLinuxPhase, UpdateMembership,
    UpdateOutcome, UpdatePage, UpdateRow, UpdateRowKind, UpdateRowStatus, UpdateStatusKind,
    UpdateView,
};
use slint::{ComponentHandle, Model as _, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use zaparoo_core::input_actions::actions;
use zaparoo_update::UpdateSession;
use zaparoo_update_api as api;

static SESSION: OnceLock<UpdateSession> = OnceLock::new();

thread_local! {
    /// The details list model and the last state applied, both owned by
    /// the UI thread.
    static ROWS: RefCell<Option<Rc<VecModel<UpdateRow>>>> = const { RefCell::new(None) };
    static LAST: RefCell<Option<Arc<api::ViewState>>> = const { RefCell::new(None) };
}

/// Whether this build and device can run an update. A hosted build never
/// can: the embedding host owns the device.
pub fn available() -> bool {
    !cfg!(feature = "hosted") && zaparoo_update::is_updater_available()
}

macro_rules! map_enum {
    ($from:ty => $to:ty { $($variant:ident),+ $(,)? }) => {
        impl From<$from> for $to {
            fn from(value: $from) -> Self {
                match value { $(<$from>::$variant => <$to>::$variant),+ }
            }
        }
    };
}

map_enum!(api::Page => UpdatePage { Intro, Running, Stopping, Finished, Details, Linux, Rebooting });
map_enum!(api::Button => UpdateButton { Start, Back, Details, Errors, Ok, Reboot });
map_enum!(api::Outcome => UpdateOutcome {
    None, Unavailable, UpToDate, Complete, CompleteRebootNeeded, CompleteWithErrors, Failed,
    LinuxComplete, LinuxFailed,
});
map_enum!(api::StatusKind => UpdateStatusKind {
    None, Starting, Slow, File, Tool, Transition, Stopping, StoppingSlow,
});
map_enum!(api::ErrorCode => UpdateError {
    None, NotResponding, Unavailable, ToolTooOld, SecureConnection, Critical, StateSave,
    StateLoad, Network, SomeFiles, SomeDatabases, Config, NoCerts, FullPartition, Unexpected,
    Generic,
});
map_enum!(api::LinuxPhase => UpdateLinuxPhase {
    Preparing, FetchImage, FetchTool, Extract, UserFiles, Flash,
});
map_enum!(api::Filter => UpdateFilter { All, Installed, Updated, Removed, Failed });
map_enum!(api::RowKind => UpdateRowKind {
    File, Folder, Database, LinuxUpdate, LinuxUpdateDetail, DuplicateCategory, DuplicateFile,
    NotOverwrittenCategory, NotOverwrittenFile, DatabaseError, DatabaseErrorReason, ZipError,
    ZipErrorReason, FolderError, FolderErrorReason,
});
map_enum!(api::RowStatus => UpdateRowStatus { None, Installed, Updated, Removed, Failed });
map_enum!(api::FolderKind => UpdateFolderKind {
    Plain, Arcade, ArcadeCore, ArcadeAlternative, Console, Computer, Downloader, UpdateAll,
});
map_enum!(api::HelpLabel => UpdateHelpLabel {
    Move, Select, Start, Back, Cancel, Close, Toggle, Info,
});
map_enum!(UpdateFilter => api::Filter { All, Installed, Updated, Removed, Failed });

fn counts(counts: &api::Counts) -> UpdateCounts {
    UpdateCounts {
        installed: counts.installed,
        updated: counts.updated,
        removed: counts.removed,
        failed: counts.failed,
        duplicated: counts.duplicated,
        not_overwritten: counts.not_overwritten,
        database_errors: counts.database_errors,
        zip_errors: counts.zip_errors,
        folder_errors: counts.folder_errors,
    }
}

fn row(row: &api::Row) -> UpdateRow {
    UpdateRow {
        kind: row.kind.into(),
        status: row.status.into(),
        folder: row.folder.into(),
        depth: row.depth,
        collapsed: row.collapsed,
        database: SharedString::from(row.database.as_str()),
        label: SharedString::from(row.label.as_str()),
        reason: SharedString::from(row.reason.as_str()),
        file_count: row.file_count,
        counts: counts(&row.counts),
    }
}

fn help_glyph(button: api::HelpButton) -> &'static str {
    match button {
        api::HelpButton::Dpad => "Dpad",
        api::HelpButton::A => "ButtonA",
        api::HelpButton::B => "ButtonB",
    }
}

/// Create the session and connect the screen's taps. Runs once at startup;
/// a device without an updater gets neither.
pub fn bind(ctx: &Arc<Ctx>, app: &App) {
    let view = app.global::<UpdateView>();
    view.set_available(available());
    if !available() {
        return;
    }
    let rows = Rc::new(VecModel::<UpdateRow>::default());
    view.set_rows(ModelRc::from(rows.clone()));
    ROWS.with(|slot| *slot.borrow_mut() = Some(rows));

    let weak = app.as_weak();
    let sink_ctx = ctx.clone();
    let sink: api::Sink = Arc::new(move |event| {
        let ctx = sink_ctx.clone();
        let _ = weak.upgrade_in_event_loop(move |app| apply(&ctx, &app, event));
    });
    let _ = SESSION.set(UpdateSession::new(ctx.handle.clone(), sink));

    let weak = app.as_weak();
    view.on_tap_button(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let view = app.global::<UpdateView>();
        if crate::press_feedback::pending(&app)
            || crate::press_feedback::current(&app)
                .is_none_or(|target| target.owner != crate::PressOwner::Update)
            || usize::try_from(index)
                .ok()
                .is_none_or(|i| i >= view.get_buttons().row_count())
        {
            return;
        }
        view.set_button_focus(index);
        let Some(target) = crate::press_feedback::current(&app) else {
            return;
        };
        crate::press_feedback::dispatch(&app, &target, move |_| {
            if let (Some(session), Ok(index)) = (SESSION.get(), usize::try_from(index)) {
                session.input(api::Input::TapButton(index));
            }
        });
    });
    view.on_tap_row(|index| {
        if let (Some(session), Ok(index)) = (SESSION.get(), usize::try_from(index)) {
            session.input(api::Input::TapRow(index));
        }
    });
    view.on_tap_filter(|filter| {
        if let Some(session) = SESSION.get() {
            session.input(api::Input::TapFilter(filter.into()));
        }
    });
}

/// Open the screen. The module resets a finished screen to its intro page
/// first, and the route commits behind that state in the same event-loop
/// turn, so no frame shows the previous visit.
pub fn enter(app: &App) {
    let Some(session) = SESSION.get() else {
        return;
    };
    session.input(api::Input::Enter);
    let _ = app.as_weak().upgrade_in_event_loop(|app| {
        crate::router::transition_to_screen(&app, Screen::Update, 1);
    });
}

/// One key press while the Update screen is the active surface.
pub fn handle_action(app: &App, action: &str) {
    let input = match action {
        actions::UP => api::Input::Up,
        actions::DOWN => api::Input::Down,
        actions::LEFT => api::Input::Left,
        actions::RIGHT => api::Input::Right,
        actions::ACCEPT => api::Input::Accept,
        actions::CANCEL => api::Input::Cancel,
        actions::PAGE_PREV => api::Input::PagePrev,
        actions::PAGE_NEXT => api::Input::PageNext,
        _ => return,
    };
    match SESSION.get() {
        Some(session) => session.input(input),
        // No module behind the screen: the only sensible key is Back.
        None if input == api::Input::Cancel => {
            crate::router::transition_to_screen(app, Screen::Hub, -1);
        }
        None => {}
    }
}

/// The "Stop update?" dialog was answered.
pub fn stop_answered(confirmed: bool) {
    if let Some(session) = SESSION.get() {
        session.input(if confirmed {
            api::Input::StopConfirmed
        } else {
            api::Input::StopDeclined
        });
    }
}

fn apply(ctx: &Arc<Ctx>, app: &App, event: api::Event) {
    match event {
        api::Event::State(state) => {
            // A run that just ended frees the screensaver, but no key press
            // follows a finished update to restart its idle clock.
            if apply_state(app, &state) {
                crate::router::reset_idle(ctx, app);
            }
        }
        api::Event::Rows(delta) => apply_rows(&delta),
        api::Event::Effect(effect) => run_effect(ctx, app, effect),
    }
}

fn apply_rows(delta: &api::RowsDelta) {
    ROWS.with(|slot| {
        let slot = slot.borrow();
        let Some(rows) = slot.as_ref() else {
            return;
        };
        match delta {
            api::RowsDelta::Reset(all) => rows.set_vec(all.iter().map(row).collect::<Vec<_>>()),
            api::RowsDelta::Insert { at, rows: new } => {
                for (offset, item) in new.iter().enumerate() {
                    rows.insert((at + offset).min(rows.row_count()), row(item));
                }
            }
            api::RowsDelta::Remove { at, count } => {
                for _ in 0..*count {
                    if *at < rows.row_count() {
                        rows.remove(*at);
                    }
                }
            }
            api::RowsDelta::Replace { at, row: new } => {
                if *at < rows.row_count() {
                    rows.set_row_data(*at, row(new));
                }
            }
        }
    });
}

#[allow(
    clippy::too_many_lines,
    reason = "one explicit inventory of the view state makes a missed field reviewable"
)]
/// Publish `state` to the global. True when it just freed the screensaver.
fn apply_state(app: &App, state: &Arc<api::ViewState>) -> bool {
    let previous = LAST.with(|last| last.borrow_mut().replace(state.clone()));
    let prev = previous.as_deref();
    let view = app.global::<UpdateView>();

    view.set_page(state.page.into());
    view.set_version(SharedString::from(state.version.as_str()));
    view.set_available(state.available);
    view.set_status_kind(state.status.kind.into());
    view.set_status_arg(SharedString::from(state.status.arg.as_str()));
    view.set_progress_bp(state.progress_bp);
    view.set_progress_decimals(i32::from(state.progress_decimals));
    view.set_progress_known(state.progress_known);
    view.set_outcome(state.outcome.into());
    view.set_error(state.error.into());
    view.set_error_arg(SharedString::from(state.error_arg.as_str()));
    view.set_counts(counts(&state.counts));
    view.set_linux_phase(state.linux.phase.into());
    view.set_linux_current_version(SharedString::from(state.linux.current_version.as_str()));
    view.set_linux_new_version(SharedString::from(state.linux.new_version.as_str()));
    view.set_linux_failed(state.linux.failed);
    view.set_membership_index(state.membership_index);
    view.set_transition(SharedString::from(state.transition.as_str()));
    view.set_button_focus(state.button_focus);
    view.set_countdown_secs(state.countdown_secs);
    view.set_filter(state.details.filter.into());
    view.set_filter_focus(state.details.filter_focus.into());
    view.set_filter_visible(state.details.filter_visible);
    view.set_filter_focused(state.details.filter_focused);
    view.set_focused_row(state.details.focused_row);
    view.set_row_count(state.details.row_count);
    view.set_database_count(state.details.database_count);
    view.set_details_counts(counts(&state.details.counts));
    view.set_allows_screensaver(state.allows_screensaver);

    // The models below only rebuild when their content changed: a fresh
    // model is a change even when it holds the same rows.
    if prev.is_none_or(|p| p.membership != state.membership) {
        let items = state
            .membership
            .iter()
            .map(|m| UpdateMembership {
                topic: SharedString::from(m.topic.as_str()),
                message: SharedString::from(m.message.as_str()),
                info: SharedString::from(m.info.as_str()),
            })
            .collect::<Vec<_>>();
        view.set_membership(ModelRc::new(VecModel::from(items)));
    }
    if prev.is_none_or(|p| p.buttons != state.buttons) {
        let items = state
            .buttons
            .iter()
            .map(|b| UpdateButton::from(*b))
            .collect::<Vec<_>>();
        view.set_buttons(ModelRc::new(VecModel::from(items)));
    }
    if prev.is_none_or(|p| p.help != state.help) {
        let items = state
            .help
            .iter()
            .map(|h| UpdateHelp {
                button: SharedString::from(help_glyph(h.button)),
                label: h.label.into(),
            })
            .collect::<Vec<_>>();
        view.set_help(ModelRc::new(VecModel::from(items)));
    }
    if prev.is_none_or(|p| p.details.info != state.details.info) {
        match &state.details.info {
            Some(info) => {
                view.set_info_row(row(&info.row));
                let dbs = info
                    .databases
                    .iter()
                    .map(|db| SharedString::from(db.as_str()))
                    .collect::<Vec<_>>();
                view.set_info_databases(ModelRc::new(VecModel::from(dbs)));
                view.set_info_open(true);
            }
            None => view.set_info_open(false),
        }
    }

    state.allows_screensaver && prev.is_some_and(|p| !p.allows_screensaver)
}

fn run_effect(ctx: &Arc<Ctx>, app: &App, effect: api::Effect) {
    match effect {
        api::Effect::LeaveToHub => {
            crate::router::transition_to_screen(app, Screen::Hub, -1);
        }
        api::Effect::ConfirmStop => crate::router::open_dialog(
            app,
            DialogKind::UpdateStop,
            "",
            "",
            &[DialogButton::No, DialogButton::Yes],
            0,
        ),
        api::Effect::CloseStopConfirm => {
            if app.global::<crate::Overlays>().get_dialog_kind() == DialogKind::UpdateStop {
                crate::router::close_dialog(app);
            }
        }
        api::Effect::Reboot => {
            // A debounced selection write may still be pending.
            crate::router::save_persist(&ctx.shared);
            std::thread::spawn(|| {
                let ok = std::process::Command::new("reboot")
                    .arg("now")
                    .status()
                    .is_ok_and(|status| status.success());
                if !ok {
                    tracing::warn!("reboot now failed");
                    if let Some(session) = SESSION.get() {
                        session.input(api::Input::RebootFailed);
                    }
                }
            });
        }
    }
}

// The software renderer, which a headless App needs, only the MiSTer feature
// set links.
#[cfg(all(test, feature = "mister"))]
mod tests {
    use super::*;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Platform, WindowAdapter};

    struct TestPlatform;

    impl Platform for TestPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
        }

        fn duration_since_start(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
    }

    fn file_row(label: &str, status: api::RowStatus) -> api::Row {
        api::Row {
            kind: api::RowKind::File,
            status,
            label: label.into(),
            depth: 2,
            ..api::Row::default()
        }
    }

    fn rows_model(app: &App) -> Vec<String> {
        let rows = app.global::<UpdateView>().get_rows();
        (0..rows.row_count())
            .filter_map(|i| rows.row_data(i))
            .map(|row| row.label.to_string())
            .collect()
    }

    #[test]
    fn state_rows_and_screensaver_release_reach_the_global() -> Result<(), slint::PlatformError> {
        assert!(slint::platform::set_platform(Box::new(TestPlatform)).is_ok());
        let app = App::new()?;
        let view = app.global::<UpdateView>();
        assert_eq!(view.get_page(), UpdatePage::Intro);
        assert!(view.get_allows_screensaver());

        let rows = Rc::new(VecModel::<UpdateRow>::default());
        view.set_rows(ModelRc::from(rows.clone()));
        ROWS.with(|slot| *slot.borrow_mut() = Some(rows));
        LAST.with(|last| *last.borrow_mut() = None);

        let running = Arc::new(api::ViewState {
            page: api::Page::Running,
            progress_bp: 4250,
            progress_decimals: 1,
            progress_known: true,
            status: api::Status {
                kind: api::StatusKind::File,
                arg: "games/a.rom".into(),
            },
            buttons: vec![api::Button::Back],
            help: vec![api::Help {
                button: api::HelpButton::B,
                label: api::HelpLabel::Cancel,
            }],
            allows_screensaver: false,
            ..api::ViewState::default()
        });
        assert!(!apply_state(&app, &running));
        assert_eq!(view.get_page(), UpdatePage::Running);
        assert_eq!(view.get_progress_bp(), 4250);
        assert_eq!(view.get_status_kind(), UpdateStatusKind::File);
        assert_eq!(view.get_status_arg(), "games/a.rom");
        assert!(!view.get_allows_screensaver());
        assert_eq!(view.get_buttons().row_count(), 1);
        assert_eq!(
            view.get_help().row_data(0).map(|h| h.button),
            Some("ButtonB".into())
        );

        let finished = Arc::new(api::ViewState {
            page: api::Page::Finished,
            outcome: api::Outcome::CompleteWithErrors,
            error: api::ErrorCode::SomeFiles,
            allows_screensaver: true,
            details: api::Details {
                info: Some(api::RowInfo {
                    row: file_row("info.rom", api::RowStatus::Failed),
                    databases: vec!["db_a".into(), "db_b".into()],
                }),
                ..api::Details::default()
            },
            ..api::ViewState::default()
        });
        assert!(
            apply_state(&app, &finished),
            "the run ended: idle clock restarts"
        );
        assert_eq!(view.get_outcome(), UpdateOutcome::CompleteWithErrors);
        assert_eq!(view.get_error(), UpdateError::SomeFiles);
        assert!(view.get_info_open());
        assert_eq!(view.get_info_row().status, UpdateRowStatus::Failed);
        assert_eq!(view.get_info_databases().row_count(), 2);
        assert!(
            !apply_state(&app, &finished),
            "an identical state changes nothing"
        );

        apply_rows(&api::RowsDelta::Reset(vec![
            file_row("a", api::RowStatus::Installed),
            file_row("d", api::RowStatus::Updated),
        ]));
        apply_rows(&api::RowsDelta::Insert {
            at: 1,
            rows: vec![
                file_row("b", api::RowStatus::None),
                file_row("c", api::RowStatus::None),
            ],
        });
        assert_eq!(rows_model(&app), ["a", "b", "c", "d"]);
        apply_rows(&api::RowsDelta::Remove { at: 1, count: 2 });
        apply_rows(&api::RowsDelta::Replace {
            at: 1,
            row: file_row("z", api::RowStatus::Removed),
        });
        assert_eq!(rows_model(&app), ["a", "z"]);
        assert_eq!(view.get_rows().row_data(1).map(|r| r.depth), Some(2));
        // Out-of-range deltas are ignored rather than panicking.
        apply_rows(&api::RowsDelta::Remove { at: 9, count: 3 });
        apply_rows(&api::RowsDelta::Replace {
            at: 9,
            row: file_row("x", api::RowStatus::None),
        });
        assert_eq!(rows_model(&app), ["a", "z"]);
        Ok(())
    }

    #[test]
    fn filters_and_actions_map_to_inputs() {
        assert_eq!(api::Filter::from(UpdateFilter::Failed), api::Filter::Failed);
        assert_eq!(
            UpdateFilter::from(api::Filter::Removed),
            UpdateFilter::Removed
        );
        assert_eq!(help_glyph(api::HelpButton::Dpad), "Dpad");
    }
}
