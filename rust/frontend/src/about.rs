//! About provenance and durable, output-independent scroll position.
use crate::router::{lock, Ctx};
use crate::{AboutView, App};
use slint::ComponentHandle;
use std::sync::Arc;

pub fn bind(ctx: &Arc<Ctx>, app: &App) {
    let view = app.global::<AboutView>();
    view.set_version(env!("CARGO_PKG_VERSION").into());
    view.set_commit(zaparoo_build_info::COMMIT.into());
    view.set_channel(zaparoo_build_info::CHANNEL.into());
    view.set_build_date(zaparoo_build_info::BUILD_DATE.into());
    // Seed before the first frame, including direct cold restoration to About.
    view.set_scroll_milli(
        i32::try_from(lock(&ctx.shared).persist.about_scroll_milli.min(1000)).unwrap_or(0),
    );
    let ctx = ctx.clone();
    let weak = app.as_weak();
    view.on_scroll_requested(move |position| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let position = position.clamp(0, 1000);
        let changed = {
            let mut shared = lock(&ctx.shared);
            let previous = shared.persist.about_scroll_milli;
            shared.persist.about_scroll_milli = position as u32;
            previous != shared.persist.about_scroll_milli
        };
        app.global::<AboutView>().set_scroll_milli(position);
        if changed {
            crate::router::save_persist(&ctx.shared);
        }
    });
}
