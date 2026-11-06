// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The music player's geometry for a scene. Every page keeps the Settings
// pages' card margins (`zaparoo_app::settings::page_geometry`), so the
// screen opened from a Settings tile sits where that page's card does: the
// player's card hugs its content and centers in the band, and the playlist
// and library pages fill the band with one browse list. Shared by the
// driver, the CRT head's mirror and the snapshot tool, so all three place
// the same pixels.

use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::media_list::{self as list_rules, ListGeometry};
use zaparoo_app::music as rules;

/// The card the player sits on, the player inside it, and the list the
/// other pages show.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Layout {
    pub card_x: i32,
    pub card_y: i32,
    pub card_width: i32,
    pub card_height: i32,
    pub player: rules::Geometry,
    pub list: ListGeometry,
}

/// The browse list in the card: the games list's row metrics and insets,
/// with as many rows as the card holds.
fn list_in_card(
    inputs: &zaparoo_app::sizing::Inputs,
    rotated: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> ListGeometry {
    let view = if rotated {
        View::GamesListTate
    } else {
        View::GamesList
    };
    let profile = layouts::profile(ThemeId::current(inputs), view, inputs);
    let Body::List { list, .. } = profile.body else {
        return ListGeometry::default();
    };
    let content = (height - list.card_padding_top - list.card_padding_bottom).max(0);
    let row_height = if list.row_height > 0 {
        list.row_height
    } else {
        inputs.pct_h(6.0).max(1)
    };
    let visible_rows = list_rules::list_visible_count(content, row_height, list.row_spacing, 0);
    // The card ends on the last whole row, so no row shows half cut off.
    let rows = i32::try_from(visible_rows).unwrap_or(1);
    let fit = list.card_padding_top
        + list.card_padding_bottom
        + rows * row_height
        + (rows - 1).max(0) * list.row_spacing;
    let height = fit.min(height).max(row_height);
    ListGeometry {
        card_x: x,
        card_y: y,
        card_width: width,
        card_height: height,
        list_x: 0,
        list_y: 0,
        list_width: width,
        list_height: height,
        row_height,
        visible_rows,
        ..ListGeometry::default()
    }
}

/// The layout for a scene. `chips` is whether the page on show has a Menu
/// chip row (the player and the library do); its band comes out of the
/// content's budget where the scene draws the row, and only there.
pub(crate) fn layout_for(
    inputs: &zaparoo_app::sizing::Inputs,
    rotated: bool,
    chips: bool,
) -> Layout {
    let derived = zaparoo_app::sizing::derive(inputs);
    let row_block = if chips && derived.menu_row {
        derived.menu_row_block
    } else {
        0
    };
    // Every page keeps the Settings pages' margins, in the band their card
    // would fill below the row: the player hugs its content and centers in
    // it, and the lists fill it.
    let page = zaparoo_app::settings::page_geometry(inputs);
    let band_y = page.card_y + row_block;
    let band_height = (page.card_height - row_block).max(0);
    let player = rules::geometry(&derived, page.card_width, band_height, page.pad);
    let list = list_in_card(
        inputs,
        rotated,
        page.card_x,
        band_y,
        page.card_width,
        band_height,
    );
    Layout {
        card_x: page.card_x,
        card_y: band_y + (band_height - player.height) / 2,
        card_width: page.card_width,
        card_height: player.height,
        player,
        list,
    }
}

/// Push a layout onto a view: the HDMI head, the CRT head mirroring it,
/// or a snapshot.
pub(crate) fn push_layout(view: &crate::MusicView<'_>, layout: &Layout) {
    let g = &layout.player;
    let px = |v: i32| v as f32;
    view.set_card_x(px(layout.card_x));
    view.set_card_y(px(layout.card_y));
    view.set_card_width(px(layout.card_width));
    view.set_card_height(px(layout.card_height));
    view.set_padding(px(g.padding));
    view.set_art_x(px(g.art_x));
    view.set_art_y(px(g.art_y));
    view.set_art_size(px(g.art_size));
    view.set_text_x(px(g.text_x));
    view.set_text_y(px(g.text_y));
    view.set_text_width(px(g.text_width));
    view.set_title_height(px(g.title_height));
    view.set_line_height(px(g.line_height));
    view.set_album_line(g.album_line);
    view.set_progress_y(px(g.progress_y));
    view.set_progress_height(px(g.progress_height));
    view.set_time_width(px(g.time_width));
    view.set_bar_x(px(g.bar_x));
    view.set_bar_width(px(g.bar_width));
    view.set_controls_x(px(g.controls_x));
    view.set_controls_y(px(g.controls_y));
    view.set_button_size(px(g.button_size));
    view.set_button_gap(px(g.button_gap));
    view.set_label_y(px(g.label_y));
    view.set_label_height(px(g.label_height));
    view.set_list_x(px(layout.list.card_x));
    view.set_list_y(px(layout.list.card_y));
    view.set_list_width(px(layout.list.card_width));
    view.set_list_height(px(layout.list.card_height));
    view.set_list_row_height(px(layout.list.row_height));
}
