// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The "Preferred artwork" picker on a system or folder: which image type
// its games' covers lead with. The rules live in
// `zaparoo_app::artwork_prefs`; the choices are kept in `frontend.toml`'s
// `[artwork]` table.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::artwork_prefs::{self as rules, ArtworkPrefs};

use crate::router::{lock, Ctx, ListContext};
use crate::App;

/// The row that clears a choice, so the system or folder inherits again.
const DEFAULT_ID: &str = "default";

/// Open the picker for `system`, or for its folder at `relative` (Core's
/// launcher-relative path) when that is not empty.
pub fn open_picker(ctx: &Ctx, app: &App, system: &str, relative: &str) {
    if system.is_empty() {
        return;
    }
    let prefs = ctx.media.artwork_prefs();
    let inherited = rules::effective(
        prefs.inherited(system, relative),
        &ctx.media.preferred_image_type(),
    )
    .to_string();
    let chosen = prefs.chosen(system, relative);
    let mut entries = vec![crate::router::menu_row_keyed(
        DEFAULT_ID,
        "artwork:default",
        &inherited,
    )];
    entries.extend(
        rules::choices().map(|kind| crate::router::menu_row_keyed(kind, "artwork:type", kind)),
    );
    let index = chosen
        .and_then(|token| rules::choices().position(|kind| kind == token))
        .map_or(0, |at| at + 1);
    let entries = crate::router::mark_chosen(entries, index);
    lock(&ctx.shared).list_context = ListContext::Artwork {
        system: system.to_string(),
        relative: relative.to_string(),
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_list_setting_id(SharedString::default());
    overlays.set_list_title(SharedString::from("title:artwork"));
    overlays.set_list_form(false);
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(i32::try_from(index).unwrap_or(0));
    overlays.set_list_open(true);
}

/// A picker row was accepted: save the choice, then show every cover it
/// changes under its new type.
pub fn picked(ctx: &Ctx, app: &App, system: &str, relative: &str, id: &str) {
    let choice = (id != DEFAULT_ID).then_some(id);
    if choice.is_some_and(|token| !rules::is_choice(token)) {
        return;
    }
    let mut prefs = ctx.media.artwork_prefs();
    if !apply(&mut prefs, system, relative, choice) {
        return;
    }
    if !crate::router::config_writable(ctx) {
        return;
    }
    let (key, value) = prefs.entry(system, relative);
    if let Err(e) =
        zaparoo_core::config::save_artwork_choice(&ctx.config_path, &key, value.as_deref())
    {
        tracing::warn!("could not save preferred artwork for {key}: {e}");
        crate::router::report_action_error(ctx, app, "setting", "");
        return;
    }
    ctx.media.set_artwork_prefs(prefs);
    // The choice is part of every cover key it reaches, so only those
    // covers are asked for again; the rest stay cached.
    crate::hub_covers::refresh_for_image_type(ctx);
    crate::router::reproject_hub(ctx, app);
    crate::games::reproject(ctx, app);
}

/// Make or clear the choice; false when it was already so.
fn apply(prefs: &mut ArtworkPrefs, system: &str, relative: &str, choice: Option<&str>) -> bool {
    !system.is_empty() && prefs.set(system, relative, choice)
}
