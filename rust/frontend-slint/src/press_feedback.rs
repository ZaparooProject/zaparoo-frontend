//! Keep the accepting control visible through its physical push, then dispatch.

use crate::{App, Overlays, PressFeedback, PressOwner, Screen, Shell};
use slint::{ComponentHandle, Model};
use std::cell::Cell;
use std::time::Duration;

// 34 ms downstroke plus a short fully depressed hold. This is local button
// feedback, not a route animation; reduced motion dispatches without waiting.
pub(crate) const PUSH_MS: u64 = 90;

thread_local! {
    static TICKET: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub owner: PressOwner,
    pub index: i32,
    key: String,
}

fn target(owner: PressOwner, index: i32, key: impl Into<String>) -> Target {
    Target {
        owner,
        index,
        key: key.into(),
    }
}

fn field_target(
    owner: PressOwner,
    index: i32,
    scope: &str,
    rows: &slint::ModelRc<crate::SettingsRow>,
) -> Option<Target> {
    let row = rows.row_data(usize::try_from(index).ok()?)?;
    if row.control == crate::ControlKind::Toggle
        || !row.enabled
        || row.kind != crate::RowKind::Field
    {
        return None;
    }
    Some(target(owner, index, format!("{scope}:{}", row.id)))
}

/// Follow the router's ownership order; never animate a covered control.
pub fn current(app: &App) -> Option<Target> {
    current_with(app, || {})
}

pub fn prepare(ctx: &crate::router::Ctx, app: &App) -> Option<Target> {
    current_with(app, || {
        // A moving page already owns its incoming selection. Seat it before
        // resolving the button, since the outgoing strip can hide its focus.
        match app.global::<Shell>().get_active_screen() {
            Screen::Systems | Screen::FavoriteSystems => crate::systems::interrupt_page(ctx, app),
            Screen::Games | Screen::Favorites | Screen::Recents => {
                crate::games::interrupt_page(ctx, app);
            }
            _ => {}
        }
    })
}

fn dialog_target(ov: &Overlays<'_>) -> Option<Target> {
    let index = ov.get_dialog_focus();
    let button = ov
        .get_dialog_buttons()
        .row_data(usize::try_from(index).ok()?)?;
    Some(target(
        PressOwner::Dialog,
        index,
        format!(
            "{:?}:{:?}:{:?}:{}:{}:{button:?}",
            ov.get_dialog_kind(),
            ov.get_dialog_error(),
            ov.get_first_run_phase(),
            ov.get_dialog_detail(),
            ov.get_dialog_arg()
        ),
    ))
}

fn current_with(app: &App, prepare_grid: impl FnOnce()) -> Option<Target> {
    let shell = app.global::<Shell>();
    let ov = app.global::<Overlays>();
    if shell.get_saver_armed() || shell.get_dormant() || ov.get_crt_calibration_open() {
        return None;
    }
    if ov.get_dialog_open() {
        return dialog_target(&ov);
    }
    if shell.get_boot_curtain() {
        return None;
    }
    let log = app.global::<crate::LogUploadView>();
    if log.get_open() {
        return (log.get_phase() != crate::LogPhase::Uploading).then(|| Target {
            owner: PressOwner::Log,
            index: 0,
            key: format!("{:?}", log.get_phase()),
        });
    }
    let setup = app.global::<crate::SetupModalView>();
    if setup.get_open() {
        if setup.get_picker_page() {
            return None;
        }
        return field_target(
            PressOwner::Setup,
            setup.get_index(),
            &format!("{:?}", setup.get_kind()),
            &setup.get_rows(),
        );
    }
    if ov.get_qr_open() {
        return None;
    }
    if ov.get_card_write_open() {
        return Some(target(
            PressOwner::CardWrite,
            0,
            ov.get_card_write_key().to_string(),
        ));
    }
    if ov.get_letter_open() {
        let index = ov.get_letter_index();
        let row = ov
            .get_letter_buckets()
            .row_data(usize::try_from(index).ok()?)?;
        return Some(target(PressOwner::Letter, index, format!("{row:?}")));
    }
    if ov.get_list_open() {
        if ov.get_launcher_saving() {
            return None;
        }
        let index = ov.get_list_index();
        let row = ov
            .get_list_entries()
            .row_data(usize::try_from(index).ok()?)?;
        return Some(target(
            PressOwner::List,
            index,
            format!(
                "{}:{}:{}",
                ov.get_list_title(),
                ov.get_list_setting_id(),
                row.id
            ),
        ));
    }
    if ov.get_context_open() {
        let index = ov.get_context_index();
        let row = ov
            .get_context_entries()
            .row_data(usize::try_from(index).ok()?)?;
        return Some(target(PressOwner::Context, index, row.id.to_string()));
    }
    if app.global::<crate::GameInfoView>().get_modal_open() || shell.get_transitioning() {
        return None;
    }
    if shell.get_active_screen() == Screen::Settings {
        let view = app.global::<crate::SettingsView>();
        return field_target(
            PressOwner::Settings,
            view.get_index(),
            view.get_page().token(),
            &view.get_rows(),
        );
    }
    prepare_grid();
    grid_target(app)
}

fn grid_target(app: &App) -> Option<Target> {
    let shell = app.global::<Shell>();
    let (owner, index, scope, rows) = match shell.get_active_screen() {
        Screen::Hub => {
            let view = app.global::<crate::HubView>();
            if view.get_move_armed() {
                return None;
            }
            (
                PressOwner::Hub,
                view.get_selected_local(),
                view.get_page().to_string(),
                view.get_cells(),
            )
        }
        Screen::Systems | Screen::FavoriteSystems => {
            let view = app.global::<crate::SystemsView>();
            if shell.get_systems_list_layout() {
                return None;
            }
            (
                PressOwner::Systems,
                view.get_selected_local(),
                format!(
                    "{:?}:{}:{}",
                    view.get_mode(),
                    view.get_category(),
                    view.get_page()
                ),
                view.get_cells(),
            )
        }
        Screen::Games | Screen::Favorites | Screen::Recents => {
            let view = app.global::<crate::GamesView>();
            if shell.get_browse_list_layout() {
                return None;
            }
            (
                PressOwner::Games,
                view.get_selected_local(),
                format!(
                    "{:?}:{}:{}",
                    view.get_mode(),
                    view.get_title(),
                    view.get_page()
                ),
                view.get_cells(),
            )
        }
        _ => return None,
    };
    let cell = rows.row_data(usize::try_from(index).ok()?)?;
    if cell.is_empty || cell.disabled {
        return None;
    }
    Some(target(
        owner,
        index,
        format!(
            "{scope}:{}:{}:{}",
            cell.name, cell.label_key, cell.glyph_key
        ),
    ))
}

pub fn pending(app: &App) -> bool {
    app.global::<PressFeedback>().get_owner() != PressOwner::None
}

pub fn cancel(app: &App) {
    TICKET.with(|ticket| ticket.set(ticket.get().wrapping_add(1)));
    let feedback = app.global::<PressFeedback>();
    if feedback.get_owner() == PressOwner::Settings {
        let view = app.global::<crate::SettingsView>();
        view.set_release_pulse(view.get_release_pulse().wrapping_add(1));
    }
    feedback.set_owner(PressOwner::None);
}

/// Retain the source control for the downstroke and depressed hold. A new
/// action cancels this ticket; a changed owner can never receive an old Accept.
pub fn dispatch(app: &App, target: &Target, commit: impl FnOnce(&App) + 'static) {
    cancel(app);
    if !app.global::<crate::Motion>().get_enabled() {
        commit(app);
        return;
    }
    let ticket = TICKET.with(Cell::get);
    let feedback = app.global::<PressFeedback>();
    feedback.set_index(target.index);
    feedback.set_owner(target.owner);
    match target.owner {
        PressOwner::Settings => {
            let view = app.global::<crate::SettingsView>();
            view.set_activate_pulse(view.get_activate_pulse().wrapping_add(1));
        }
        PressOwner::Setup => {
            let view = app.global::<crate::SetupModalView>();
            view.set_activate_pulse(view.get_activate_pulse().wrapping_add(1));
        }
        _ => {}
    }
    let weak = app.as_weak();
    let target = target.clone();
    slint::Timer::single_shot(Duration::from_millis(PUSH_MS), move || {
        if TICKET.with(Cell::get) != ticket {
            return;
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        let still_owned = current(&app).as_ref() == Some(&target);
        cancel(&app);
        if still_owned {
            commit(&app);
        }
    });
    app.window().request_redraw();
}
