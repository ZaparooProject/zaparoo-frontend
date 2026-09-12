// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Same-screen folder navigation keeps its source visible until the fill lands.
use crate::router::{lock, Ctx};
use crate::{App, GamesView};
use slint::{ComponentHandle, ModelRc};

pub fn clear(app: &App) {
    let view = app.global::<GamesView>();
    view.set_slide_anim(false);
    view.set_page_slide(0.0);
    view.set_folder_slide(false);
    view.set_cached_transition(false);
    view.set_folder_from_cells(ModelRc::default());
}

pub fn capture(app: &App, direction: i32) {
    clear(app);
    app.global::<GamesView>().set_slide_anim(true);
    if direction != 0 {
        crate::router::begin_pending(app, crate::Screen::Games);
    }
}

pub fn start(ctx: &Ctx, app: &App) {
    let direction = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        model.folder_sliding = false;
        model.sliding = false;
        std::mem::take(&mut model.folder_direction)
    };
    let view = app.global::<GamesView>();
    view.set_folder_slide(false);
    view.set_folder_from_cells(ModelRc::default());
    view.set_next_cells(ModelRc::default());
    if direction != 0 {
        crate::router::clear_pending(app);
        crate::games::render(ctx, app);
    }
}
