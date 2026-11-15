//! Transient NFC writes: cancellation retires callbacks, failure owns its retry payload.
//!
//! One write puts one script on a token (`begin`, or `begin_on_reader` for
//! the reader a token was just read on). A batch writes a deck's games, one
//! token per game, each a page of the same panel (`begin_batch`, rules in
//! `zaparoo_app::token_batch`). Every way out of a pending write tells Core
//! to stop waiting for a token (`readers.write.cancel`).

use crate::media_cache::MediaKey;
use crate::router::{lock, Ctx};
use crate::{App, CardWriteButton, Overlays};
use slint::{ComponentHandle, Model as _};
use zaparoo_app::token_batch::{self, Batch, Button, Outcome};
use zaparoo_core::input_actions::actions;

#[derive(Debug, Default)]
pub struct Model {
    ticket: u64,
    pending: bool,
    /// The reader the pending write was sent to; `None` lets Core pick.
    reader: Option<String>,
    batch: Option<Batch>,
    /// The batch item's cover the panel shows, once asked for.
    art: Option<MediaKey>,
}

/// A write that was still waiting for a token when it was called off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abandoned {
    pub reader: Option<String>,
}

impl Model {
    pub fn begin(&mut self, reader: Option<String>) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.pending = true;
        self.reader = reader;
        self.ticket
    }

    /// Retire the current write and any batch; a write still waiting for a
    /// token comes back so Core can be told to stop.
    pub fn cancel(&mut self) -> Option<Abandoned> {
        let abandoned = self.stop();
        self.batch = None;
        self.art = None;
        abandoned
    }

    /// Retire the current write only, keeping the batch.
    fn stop(&mut self) -> Option<Abandoned> {
        self.ticket = self.ticket.wrapping_add(1);
        let pending = std::mem::take(&mut self.pending);
        pending.then(|| Abandoned {
            reader: self.reader.clone(),
        })
    }

    fn finish(&mut self, ticket: u64) -> bool {
        if !self.pending || self.ticket != ticket {
            return false;
        }
        self.pending = false;
        true
    }
}

/// Tell Core to stop waiting for a token on the write that was called off.
pub fn send_cancel(ctx: &Ctx, abandoned: Option<Abandoned>) {
    let Some(Abandoned { reader }) = abandoned else {
        return;
    };
    let client = ctx.store.client();
    ctx.handle.spawn(async move {
        if let Err(error) = client.readers_write_cancel(reader.as_deref()).await {
            tracing::warn!("token write cancel failed: {}", error.message);
        }
    });
}

/// Call off any write, wherever it was going, without touching the panel.
pub fn abandon(ctx: &Ctx) {
    let abandoned = lock(&ctx.shared).card_write.cancel();
    send_cancel(ctx, abandoned);
}

pub fn cancel(ctx: &Ctx, app: &App) {
    abandon(ctx);
    crate::press_feedback::cancel(app);
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_open(false);
    overlays.set_card_write_batch(false);
}

/// Write `text` to whichever reader Core picks.
pub fn begin(ctx: &Ctx, app: &App, text: String) {
    begin_on_reader(ctx, app, text, None);
}

/// Write `text` to the token on `reader_id` (`None`: Core picks).
pub fn begin_on_reader(ctx: &Ctx, app: &App, text: String, reader_id: Option<String>) {
    {
        let mut shared = lock(&ctx.shared);
        shared.card_write.batch = None;
        shared.card_write.art = None;
    }
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_batch(false);
    publish_buttons(ctx, app);
    write(ctx, app, text, reader_id);
}

/// Write each writable item to a token of its own, one page per item.
/// `false` when no item can be written.
pub fn begin_batch(
    ctx: &Ctx,
    app: &App,
    items: Vec<token_batch::Item>,
    reader_id: Option<String>,
) -> bool {
    let Some(batch) = Batch::new(items) else {
        return false;
    };
    let text = batch
        .current()
        .map(|item| item.zapscript.clone())
        .unwrap_or_default();
    {
        let mut shared = lock(&ctx.shared);
        shared.card_write.batch = Some(batch);
    }
    app.global::<Overlays>().set_card_write_batch(true);
    publish_batch(ctx, app);
    write(ctx, app, text, reader_id);
    true
}

fn write(ctx: &Ctx, app: &App, text: String, reader_id: Option<String>) {
    use zaparoo_core::endpoints::readers_write::ReadersWriteMutation;
    use zaparoo_core::media_types::ReadersWriteParams;

    crate::press_feedback::cancel(app);
    let ticket = lock(&ctx.shared).card_write.begin(reader_id.clone());
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_key(ticket.to_string().into());
    overlays.set_card_write_open(true);
    if text.is_empty() {
        finish(ctx, app, ticket, &text, false);
        return;
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = ctx
            .store
            .run_mutation::<ReadersWriteMutation>(ReadersWriteParams {
                text: text.clone(),
                reader_id,
            })
            .await;
        if let Err(error) = &result {
            tracing::warn!("token write failed: {}", error.message);
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            finish(&ctx, &app, ticket, &text, result.is_ok());
        });
    });
}

fn finish(ctx: &Ctx, app: &App, ticket: u64, text: &str, succeeded: bool) {
    let (current, in_batch) = {
        let mut shared = lock(&ctx.shared);
        let current = shared.card_write.finish(ticket);
        (current, shared.card_write.batch.is_some())
    };
    if !current {
        return;
    }
    if in_batch {
        batch_step(
            ctx,
            app,
            if succeeded {
                Outcome::Written
            } else {
                Outcome::Failed
            },
        );
        return;
    }
    // A write that ran ends on the screen.
    crate::router::forget_menu_return(ctx);
    crate::press_feedback::cancel(app);
    app.global::<Overlays>().set_card_write_open(false);
    if succeeded {
        // The modal just closes; say that the write happened.
        crate::cue::flash(
            ctx,
            app,
            crate::AppCue::TokenWritten,
            zaparoo_app::wait_cue::CONFIRM_MS,
        );
    } else {
        // The queued alert owns the exact command, not a mutable row index.
        crate::router::report_action_error(ctx, app, "card_write", text);
    }
}

/// The batch item's turn ended: the next item's page, the same page with
/// its failure, or the end of the batch.
fn batch_step(ctx: &Ctx, app: &App, outcome: Outcome) {
    let (more, failed, written, next) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.card_write;
        let Some(batch) = model.batch.as_mut() else {
            return;
        };
        let more = batch.record(outcome);
        let next = batch.current().map(|item| item.zapscript.clone());
        (more, batch.failed(), batch.written(), next)
    };
    if !more {
        let abandoned = lock(&ctx.shared).card_write.cancel();
        send_cancel(ctx, abandoned);
        crate::router::forget_menu_return(ctx);
        crate::press_feedback::cancel(app);
        let overlays = app.global::<Overlays>();
        overlays.set_card_write_open(false);
        overlays.set_card_write_batch(false);
        if written > 0 {
            crate::cue::flash(
                ctx,
                app,
                crate::AppCue::TokenWritten,
                zaparoo_app::wait_cue::CONFIRM_MS,
            );
        }
        return;
    }
    publish_batch(ctx, app);
    // A failed item waits for Try again; any other step writes the next.
    if !failed {
        let reader = lock(&ctx.shared).card_write.reader.clone();
        write(ctx, app, next.unwrap_or_default(), reader);
    }
}

/// The batch page: the item's name, art and place in the batch.
fn publish_batch(ctx: &Ctx, app: &App) {
    let (item, step, count, failed) = {
        let shared = lock(&ctx.shared);
        let Some(batch) = shared.card_write.batch.as_ref() else {
            return;
        };
        (
            batch.current().cloned().unwrap_or_default(),
            batch.step(),
            batch.count(),
            batch.failed(),
        )
    };
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_name(item.name.as_str().into());
    overlays.set_card_write_step(i32::try_from(step).unwrap_or(i32::MAX));
    overlays.set_card_write_count(i32::try_from(count).unwrap_or(i32::MAX));
    overlays.set_card_write_failed(failed);
    publish_buttons(ctx, app);
    show_art(ctx, app, &item);
}

fn publish_buttons(ctx: &Ctx, app: &App) {
    let buttons: Vec<CardWriteButton> = {
        let shared = lock(&ctx.shared);
        token_batch::buttons(shared.card_write.batch.as_ref())
            .into_iter()
            .map(CardWriteButton::from)
            .collect()
    };
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_buttons(slint::ModelRc::new(slint::VecModel::from(buttons)));
    overlays.set_card_write_focus(0);
}

/// The batch page's cover box: a third of the scene's height, square.
pub fn art_side(app: &App) -> i32 {
    let scene = crate::router::output_scene(app);
    let height = if scene.swap_axes {
        scene.width
    } else {
        scene.height
    };
    // Whole pixels of a small box: the cast cannot truncate meaningfully.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "whole pixels of a small box"
    )]
    let side = (height * 0.3).round() as i32;
    side
}

fn show_art(ctx: &Ctx, app: &App, item: &token_batch::Item) {
    let overlays = app.global::<Overlays>();
    overlays.set_card_write_has_art(false);
    overlays.set_card_write_art(slint::Image::default());
    let key = (!item.system.is_empty() && !item.path.is_empty()).then(|| {
        let side = art_side(app);
        MediaKey {
            media_id: None,
            system: item.system.clone(),
            path: item.path.clone(),
            folder: false,
            max_size: crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
            fit: zaparoo_app::covers::Fit::new(side, side),
            image_type: None,
            prefer: ctx.media.artwork_for(&item.system, ""),
        }
    });
    lock(&ctx.shared).card_write.art.clone_from(&key);
    let Some(key) = key else { return };
    if let Some(decoded) = ctx.media.get(&key) {
        overlays.set_card_write_art(slint::Image::from_rgba8(decoded.buffer.clone()));
        overlays.set_card_write_has_art(true);
    } else if !ctx.media.is_negative(&key) {
        ctx.media.enqueue(key);
    }
}

/// Covers settled: the batch page's, if it is one of them.
pub fn covers_landed(ctx: &Ctx, app: &App, keys: &[MediaKey]) {
    let wanted = lock(&ctx.shared).card_write.art.clone();
    let Some(wanted) = wanted.filter(|wanted| keys.contains(wanted)) else {
        return;
    };
    if let Some(decoded) = ctx.media.get(&wanted) {
        let overlays = app.global::<Overlays>();
        overlays.set_card_write_art(slint::Image::from_rgba8(decoded.buffer.clone()));
        overlays.set_card_write_has_art(true);
    }
}

/// Focus the panel's button `row` (a pointer over it). `false` when there
/// is no such button.
pub fn focus(app: &App, row: usize) -> bool {
    let overlays = app.global::<Overlays>();
    if row >= overlays.get_card_write_buttons().row_count() {
        return false;
    }
    overlays.set_card_write_focus(i32::try_from(row).unwrap_or(0));
    true
}

/// Input while the panel is open: Left and Right move between its
/// buttons, Accept presses the focused one, Back cancels.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let overlays = app.global::<Overlays>();
    let buttons = overlays.get_card_write_buttons();
    let count = i32::try_from(buttons.row_count()).unwrap_or(1);
    let focus = overlays.get_card_write_focus().clamp(0, (count - 1).max(0));
    let pressed = match action {
        a if a == actions::LEFT => {
            overlays.set_card_write_focus((focus - 1).max(0));
            return;
        }
        a if a == actions::RIGHT => {
            overlays.set_card_write_focus((focus + 1).min(count - 1));
            return;
        }
        a if a == actions::CANCEL => CardWriteButton::Cancel,
        a if a == actions::ACCEPT => buttons
            .row_data(usize::try_from(focus).unwrap_or(0))
            .unwrap_or(CardWriteButton::Cancel),
        _ => return,
    };
    match pressed {
        CardWriteButton::Cancel => {
            cancel(ctx, app);
            crate::router::return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::TokenWrite);
        }
        CardWriteButton::Skip => {
            let abandoned = lock(&ctx.shared).card_write.stop();
            send_cancel(ctx, abandoned);
            batch_step(ctx, app, Outcome::Skipped);
        }
        CardWriteButton::Retry => {
            let (text, reader) = {
                let shared = lock(&ctx.shared);
                let model = &shared.card_write;
                (
                    model
                        .batch
                        .as_ref()
                        .and_then(Batch::current)
                        .map(|item| item.zapscript.clone())
                        .unwrap_or_default(),
                    model.reader.clone(),
                )
            };
            if let Some(batch) = lock(&ctx.shared).card_write.batch.as_mut() {
                batch.clear_failure();
            }
            publish_batch(ctx, app);
            write(ctx, app, text, reader);
        }
    }
}

impl From<Button> for CardWriteButton {
    fn from(button: Button) -> Self {
        match button {
            Button::Retry => Self::Retry,
            Button::Skip => Self::Skip,
            Button::Cancel => Self::Cancel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Abandoned, Model};

    #[test]
    fn cancellation_and_reopen_reject_old_completions() {
        let mut model = Model::default();
        let old = model.begin(None);
        assert_eq!(model.cancel(), Some(Abandoned { reader: None }));
        assert!(!model.finish(old));
        let next = model.begin(None);
        assert!(!model.finish(old));
        assert!(model.finish(next));
        assert!(!model.finish(next));
    }

    #[test]
    fn retry_and_empty_attempts_get_new_tickets() {
        let mut model = Model::default();
        let old = model.begin(None);
        let empty = model.begin(None);
        assert!(!model.finish(old));
        assert!(model.finish(empty));
        let retry = model.begin(None);
        assert!(!model.finish(empty));
        assert!(model.finish(retry));
    }

    #[test]
    fn only_a_waiting_write_is_called_off_on_its_reader() {
        let mut model = Model::default();
        assert_eq!(model.cancel(), None);
        let ticket = model.begin(Some("r1".into()));
        assert_eq!(
            model.cancel(),
            Some(Abandoned {
                reader: Some("r1".into())
            })
        );
        assert_eq!(model.cancel(), None);
        assert!(!model.finish(ticket));
        let done = model.begin(None);
        assert!(model.finish(done));
        assert_eq!(model.cancel(), None);
    }
}
