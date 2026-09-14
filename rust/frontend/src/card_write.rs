//! Transient NFC writes: cancellation retires callbacks, failure owns its retry payload.

use crate::router::{lock, Ctx};
use crate::{App, Overlays};
use slint::ComponentHandle;

#[derive(Debug, Default)]
pub struct Model {
    ticket: u64,
    pending: bool,
}

impl Model {
    pub fn begin(&mut self) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.pending = true;
        self.ticket
    }

    pub fn cancel(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.pending = false;
    }

    fn finish(&mut self, ticket: u64) -> bool {
        if !self.pending || self.ticket != ticket {
            return false;
        }
        self.pending = false;
        true
    }
}

pub fn cancel(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).card_write.cancel();
    crate::press_feedback::cancel(app);
    app.global::<Overlays>().set_card_write_open(false);
}

pub fn begin(ctx: &Ctx, app: &App, text: String) {
    use zaparoo_core::endpoints::readers_write::ReadersWriteMutation;
    use zaparoo_core::media_types::ReadersWriteParams;

    crate::press_feedback::cancel(app);
    let ticket = lock(&ctx.shared).card_write.begin();
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
            .run_mutation::<ReadersWriteMutation>(ReadersWriteParams { text: text.clone() })
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
    let current = lock(&ctx.shared).card_write.finish(ticket);
    if !current {
        return;
    }
    crate::press_feedback::cancel(app);
    app.global::<Overlays>().set_card_write_open(false);
    if !succeeded {
        // The queued alert owns the exact command, not a mutable row index.
        crate::router::report_action_error(ctx, app, "card_write", text);
    }
}

#[cfg(test)]
mod tests {
    use super::Model;

    #[test]
    fn cancellation_and_reopen_reject_old_completions() {
        let mut model = Model::default();
        let old = model.begin();
        model.cancel();
        assert!(!model.finish(old));
        let next = model.begin();
        assert!(!model.finish(old));
        assert!(model.finish(next));
        assert!(!model.finish(next));
    }

    #[test]
    fn retry_and_empty_attempts_get_new_tickets() {
        let mut model = Model::default();
        let old = model.begin();
        let empty = model.begin();
        assert!(!model.finish(old));
        assert!(model.finish(empty));
        let retry = model.begin();
        assert!(!model.finish(empty));
        assert!(model.finish(retry));
    }
}
