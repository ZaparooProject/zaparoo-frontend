//! Qt `DeferredAction` timing: paint the commitment cue before changing its owner.

use crate::{App, Overlays, PressFeedback, Shell};
use slint::{ComponentHandle, Model, SharedString};
use std::cell::Cell;
use std::time::Duration;

thread_local! {
    static TICKET: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub owner: &'static str,
    pub index: i32,
    key: String,
}

fn target(owner: &'static str, index: i32, key: impl Into<String>) -> Target {
    Target {
        owner,
        index,
        key: key.into(),
    }
}

fn field_target(
    owner: &'static str,
    index: i32,
    scope: &str,
    rows: &slint::ModelRc<crate::SettingsRow>,
) -> Option<Target> {
    let row = rows.row_data(usize::try_from(index).ok()?)?;
    if row.control == "toggle" || !row.enabled || row.kind != "field" {
        return None;
    }
    Some(target(owner, index, format!("{scope}:{}", row.id)))
}

/// Follow the router's ownership order; never animate a covered control.
pub fn current(app: &App) -> Option<Target> {
    let shell = app.global::<Shell>();
    let ov = app.global::<Overlays>();
    if shell.get_saver_armed() || ov.get_crt_calibration_open() {
        return None;
    }
    if ov.get_dialog_open() {
        let index = ov.get_dialog_focus();
        let button = ov
            .get_dialog_buttons()
            .row_data(usize::try_from(index).ok()?)?;
        return Some(target(
            "dialog",
            index,
            format!(
                "{}:{}:{}:{button}",
                ov.get_dialog_kind(),
                ov.get_dialog_detail(),
                ov.get_dialog_arg()
            ),
        ));
    }
    if shell.get_boot_curtain() {
        return None;
    }
    let log = app.global::<crate::LogUploadView>();
    if log.get_open() {
        return (log.get_phase() != "uploading").then(|| Target {
            owner: "log",
            index: 0,
            key: log.get_phase().to_string(),
        });
    }
    let setup = app.global::<crate::SetupModalView>();
    if setup.get_open() {
        if setup.get_picker_page() {
            return None;
        }
        return field_target(
            "setup",
            setup.get_index(),
            &setup.get_kind(),
            &setup.get_rows(),
        );
    }
    if ov.get_qr_open() {
        return None;
    }
    if ov.get_card_write_open() {
        return Some(target("card-write", 0, ov.get_card_write_key().to_string()));
    }
    if ov.get_letter_open() {
        let index = ov.get_letter_index();
        let row = ov
            .get_letter_buckets()
            .row_data(usize::try_from(index).ok()?)?;
        return Some(target("letter", index, format!("{row:?}")));
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
            "list",
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
        return Some(target("context", index, row.id.to_string()));
    }
    if app.global::<crate::GameInfoView>().get_modal_open()
        || shell.get_transitioning()
        || shell.get_route_transitioning()
    {
        return None;
    }
    if shell.get_active_screen() == "settings" {
        let view = app.global::<crate::SettingsView>();
        return field_target(
            "settings",
            view.get_index(),
            &view.get_page(),
            &view.get_rows(),
        );
    }
    None
}

pub fn pending(app: &App) -> bool {
    !app.global::<PressFeedback>().get_owner().is_empty()
}

pub fn cancel(app: &App) {
    TICKET.with(|ticket| ticket.set(ticket.get().wrapping_add(1)));
    app.global::<PressFeedback>()
        .set_owner(SharedString::default());
}

/// Capture the target, not a later selection. Cancel invalidates the
/// ticket; a changed target never receives the old accept.
pub fn defer(app: &App, target: Target, commit: impl FnOnce(&App) + 'static) {
    cancel(app);
    if !app.global::<crate::Motion>().get_enabled() {
        commit(app);
        return;
    }
    let ticket = TICKET.with(Cell::get);
    let feedback = app.global::<PressFeedback>();
    feedback.set_index(target.index);
    feedback.set_owner(target.owner.into());
    match target.owner {
        "settings" => {
            let view = app.global::<crate::SettingsView>();
            view.set_activate_pulse(view.get_activate_pulse().wrapping_add(1));
        }
        "setup" => {
            let view = app.global::<crate::SetupModalView>();
            view.set_activate_pulse(view.get_activate_pulse().wrapping_add(1));
        }
        _ => {}
    }
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(34), move || {
        if TICKET.with(Cell::get) != ticket {
            return;
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        let valid = current(&app).as_ref() == Some(&target);
        cancel(&app);
        if valid {
            commit(&app);
        }
    });
}
