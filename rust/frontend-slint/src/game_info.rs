//! Game Info lifecycle and image selection. Core metadata stays transient.

use crate::games::GameRow;
use crate::media_cache::{DecodedImage, MediaKey};
use crate::router::{lock, Ctx};
use crate::{App, GameInfoView};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_core::input_actions::actions;

#[derive(Debug, Default)]
pub struct GameInfoModel {
    ticket: u64,
    images: Vec<MediaKey>,
    selected: usize,
}

impl GameInfoModel {
    fn reset(&mut self) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.images.clear();
        self.selected = 0;
        self.ticket
    }

    fn cycle(&mut self, delta: i32) -> bool {
        let next = (self.selected as i64 + i64::from(delta))
            .clamp(0, self.images.len().saturating_sub(1) as i64) as usize;
        let changed = self.selected != next;
        self.selected = next;
        changed
    }
}

pub fn open(ctx: &Ctx, app: &App, entry: &GameRow) {
    let (ticket, system) = {
        let mut shared = lock(&ctx.shared);
        let system = if entry.system_id.is_empty() {
            shared.games.system_id.clone()
        } else {
            entry.system_id.clone()
        };
        (shared.game_info.reset(), system)
    };
    let view = app.global::<GameInfoView>();
    view.set_modal_name(entry.name.trim().into());
    view.set_modal_system(system.clone().into());
    view.set_modal_path(entry.path.clone().into());
    view.set_modal_description(SharedString::default());
    view.set_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
    view.set_media_missing(false);
    view.set_failed(false);
    view.set_scroll_position(0.0);
    view.set_image_count(0);
    view.set_image_index(0);
    view.set_modal_has_cover(false);
    view.set_modal_cover(slint::Image::default());
    view.set_modal_open(true);
    let valid = !system.trim().is_empty() && !entry.path.trim().is_empty();
    view.set_loading(valid);
    if !valid {
        return;
    }

    let params =
        zaparoo_core::media_types::MediaMetaParams::for_media(system.clone(), entry.path.clone());
    let key = MediaKey {
        media_id: entry.media_id,
        system,
        path: entry.path.clone(),
        max_size: crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
        image_type: None,
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = ctx.store.client().media_meta(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if lock(&ctx.shared).game_info.ticket != ticket {
                return;
            }
            let view = app.global::<GameInfoView>();
            view.set_loading(false);
            match result {
                Ok(result) => {
                    let meta = result.media;
                    if !meta.title.name.trim().is_empty() {
                        view.set_modal_name(meta.title.name.trim().into());
                    }
                    view.set_modal_description(crate::game_info_data::description(&meta).into());
                    view.set_media_missing(meta.is_missing);
                    let rows = crate::game_info_data::rows(&meta, &key.path)
                        .into_iter()
                        .map(|(key, value)| crate::DetailRow {
                            key: key.into(),
                            value: value.into(),
                        })
                        .collect::<Vec<_>>();
                    view.set_rows(ModelRc::new(VecModel::from(rows)));
                    lock(&ctx.shared).game_info.images = crate::game_info_data::image_types(&meta)
                        .into_iter()
                        .map(|kind| MediaKey {
                            image_type: Some(kind),
                            ..key.clone()
                        })
                        .collect();
                    show_image(&ctx, &app);
                }
                Err(error) => {
                    tracing::warn!(path = %key.path, "game info fetch failed: {}", error.message);
                    view.set_failed(true);
                }
            }
        });
    });
}

pub fn close(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).game_info.reset();
    let view = app.global::<GameInfoView>();
    view.set_modal_open(false);
    view.set_loading(false);
    view.set_modal_cover(slint::Image::default());
    view.set_modal_has_cover(false);
    view.set_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
    view.set_modal_description(SharedString::default());
}

fn show_image(ctx: &Ctx, app: &App) {
    let (index, count, key) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (
            model.selected,
            model.images.len(),
            model.images.get(model.selected).cloned(),
        )
    };
    let view = app.global::<GameInfoView>();
    view.set_image_count(i32::try_from(count).unwrap_or(i32::MAX));
    view.set_image_index(i32::try_from(index).unwrap_or(0));
    view.set_modal_has_cover(false);
    view.set_modal_cover(slint::Image::default());
    if let Some(key) = key {
        if let Some(decoded) = ctx.media.get(&key) {
            cover_landed(ctx, app, &key, &decoded);
        } else {
            ctx.media.enqueue(key);
        }
    }
}

pub fn cover_landed(ctx: &Ctx, app: &App, key: &MediaKey, decoded: &DecodedImage) {
    let matches = {
        let shared = lock(&ctx.shared);
        shared.game_info.images.get(shared.game_info.selected) == Some(key)
    };
    let view = app.global::<GameInfoView>();
    if view.get_modal_open() && matches {
        view.set_modal_cover(slint::Image::from_rgba8(decoded.buffer.clone()));
        view.set_modal_has_cover(true);
    }
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::ACCEPT | actions::CANCEL => close(ctx, app),
        actions::LEFT | actions::RIGHT => {
            let delta = if action == actions::LEFT { -1 } else { 1 };
            let changed = lock(&ctx.shared).game_info.cycle(delta);
            if changed {
                show_image(ctx, app);
            }
        }
        actions::UP | actions::DOWN | actions::PAGE_PREV | actions::PAGE_NEXT => {
            app.invoke_game_info_scroll(action.into());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_and_reopen_retire_metadata_and_carousel_is_clamped() {
        let mut model = GameInfoModel::default();
        let first = model.reset();
        model.images = ["boxart", "screenshot"]
            .into_iter()
            .map(|kind| MediaKey {
                media_id: None,
                system: "SNES".into(),
                path: "same.sfc".into(),
                max_size: 256,
                image_type: Some(kind.into()),
            })
            .collect();
        assert!(!model.cycle(-1));
        assert!(model.cycle(1));
        assert!(!model.cycle(1));
        assert!(model.cycle(-1));
        model.reset();
        let reopened = model.reset();
        assert_ne!(first, reopened);
        assert!(model.images.is_empty());
        assert_eq!(model.selected, 0);
    }
}
