// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Same-screen folder navigation: retain outgoing delegates until the new grid lands.
use crate::router::{lock, Ctx};
use crate::{App, GamesView};
use slint::{ComponentHandle, ModelRc};
use std::time::Duration;

pub fn clear(app: &App) {
    let view = app.global::<GamesView>();
    view.set_slide_anim(false);
    view.set_page_slide(0.0);
    view.set_folder_slide(false);
    view.set_cached_transition(false);
    view.set_folder_from_cells(ModelRc::default());
}

pub fn capture(app: &App, direction: i32) {
    let view = app.global::<GamesView>();
    let cells = view.get_cells();
    let index = view.get_selected_local();
    clear(app);
    if direction != 0 {
        view.set_folder_from_cells(cells);
        view.set_folder_from_index(index);
    }
    view.set_slide_anim(true);
}

pub fn start(ctx: &Ctx, app: &App) {
    let view = app.global::<GamesView>();
    let (direction, ticket, enabled) = {
        let mut shared = lock(&ctx.shared);
        let enabled = !shared.persist.settings.reduce_motion
            && app.global::<crate::Motion>().get_enabled()
            && !app.global::<crate::Shell>().get_browse_list_layout();
        let model = &mut shared.games;
        let direction = std::mem::take(&mut model.folder_direction);
        if direction != 0 && enabled {
            model.sliding = true;
            model.folder_sliding = true;
        }
        (direction, model.ticket, enabled)
    };
    if direction == 0 || !enabled {
        view.set_folder_from_cells(ModelRc::default());
        return;
    }
    view.set_next_cells(view.get_cells());
    view.set_transition_target_index(view.get_selected_local());
    view.set_cells(view.get_folder_from_cells());
    view.set_selected_local(-1);
    view.set_folder_slide(true);
    view.set_slide_dir(direction);
    view.set_slide_anim(true);
    view.set_page_slide(direction as f32);
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(Duration::from_millis(crate::games::SWOOP_MS), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        {
            let mut shared = lock(&ctx.shared);
            if shared.games.ticket != ticket || !shared.games.folder_sliding {
                return;
            }
            shared.games.folder_sliding = false;
            shared.games.sliding = false;
        }
        clear(&app);
        crate::games::render(&ctx, &app);
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(crate::games::REARM_MS), move || {
            if lock(&ctx.shared).games.ticket != ticket {
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.global::<GamesView>().set_slide_anim(true);
            }
        });
    });
}
