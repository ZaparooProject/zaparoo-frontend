//! One retained source for asynchronous navigation. Rows are moved, not copied;
//! no screenshots or second screen tree are retained. Persistence stays on the
//! coherent source until the destination is ready.

use crate::router::{lock, Ctx};
use crate::{App, Shell};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use zaparoo_core::persist::PersistedState;

struct Source {
    persist: PersistedState,
    games: crate::games::GamesModel,
    systems: crate::systems::SystemsModel,
}

thread_local! {
    static SOURCE: RefCell<Option<Source>> = const { RefCell::new(None) };
    static SEQUENCE: Cell<u64> = const { Cell::new(0) };
}

pub fn stage(ctx: &Ctx, app: &App) {
    if SOURCE.with(|source| source.borrow().is_some()) || app.global::<Shell>().get_boot_curtain() {
        return;
    }
    let mut shared = lock(&ctx.shared);
    let mut games = std::mem::take(&mut shared.games);
    // Copy navigation metadata without duplicating an arbitrarily long list.
    let rows = std::mem::take(&mut games.rows);
    shared.games = games.clone();
    games.rows = rows;
    shared.games.ticket = games.ticket.wrapping_add(1);
    shared.games.detail_seq = games.detail_seq.wrapping_add(1);
    shared.games.persist_seq = games.persist_seq.wrapping_add(1);
    let source = Source {
        persist: shared.persist.clone(),
        games,
        systems: shared.systems_model.clone(),
    };
    SOURCE.with(|slot| *slot.borrow_mut() = Some(source));
    drop(shared);
    let ticket = SEQUENCE.with(|seq| {
        seq.set(seq.get().wrapping_add(1));
        seq.get()
    });
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(15), move || {
        if SEQUENCE.with(Cell::get) == ticket {
            if let Some(app) = weak.upgrade() {
                fail(&ctx, &app, "navigation timed out");
            }
        }
    });
}

pub fn active() -> bool {
    SOURCE.with(|source| source.borrow().is_some())
}

pub fn retaining(app: &App, screens: &[crate::Screen]) -> bool {
    screens.contains(&app.global::<Shell>().get_active_screen()) && active()
}

pub fn source_persist() -> Option<PersistedState> {
    SOURCE.with(|source| {
        source
            .borrow()
            .as_ref()
            .map(|source| source.persist.clone())
    })
}

pub fn finish(app: &App) {
    SEQUENCE.with(|seq| seq.set(seq.get().wrapping_add(1)));
    SOURCE.with(|source| *source.borrow_mut() = None);
    crate::router::clear_pending(app);
    let motion = app.global::<crate::Motion>();
    motion.set_epoch(motion.get_epoch().wrapping_add(1));
}

pub fn cancel(ctx: &Ctx, app: &App) -> bool {
    let source = SOURCE.with(|source| source.borrow_mut().take());
    let Some(mut source) = source else {
        crate::router::clear_pending(app);
        return false;
    };
    {
        let mut shared = lock(&ctx.shared);
        source.games.ticket = shared.games.ticket.wrapping_add(1);
        source.games.detail_seq = shared.games.detail_seq.wrapping_add(1);
        source.games.persist_seq = shared.games.persist_seq.wrapping_add(1);
        source.games.press_seq = shared.games.press_seq.wrapping_add(1);
        source.games.loading_more = false;
        source.games.covers_paused = false;
        source.games.grid.set_loading_more(false);
        source.games.release_pulse += 1;
        source.systems.transition_seq = shared.systems_model.transition_seq.wrapping_add(1);
        source.systems.release_pulse += 1;
        shared.games = source.games;
        shared.systems_model = source.systems;
        shared.persist = source.persist;
    }
    finish(app);
    crate::router::save_persist(&ctx.shared);
    crate::router::refresh_layout(app);
    match app.global::<Shell>().get_active_screen() {
        crate::Screen::Games | crate::Screen::Favorites | crate::Screen::Recents => {
            crate::games::render(ctx, app);
            crate::games::resume_after_cancel(ctx, app);
        }
        crate::Screen::Systems | crate::Screen::FavoriteSystems => crate::systems::render(ctx, app),
        crate::Screen::Hub => {
            lock(&ctx.shared).hub.release_pulse += 1;
            crate::hub::render(ctx, app);
        }
        _ => {}
    }
    true
}

pub fn fail(ctx: &Ctx, app: &App, message: &str) -> bool {
    if !cancel(ctx, app) {
        return false;
    }
    tracing::warn!("navigation failed: {message}");
    crate::router::report_action_error(ctx, app, "browse", "");
    true
}
