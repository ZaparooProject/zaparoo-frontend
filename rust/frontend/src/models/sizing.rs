// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.SizingRules` — the cxx-qt face of `zaparoo_app::sizing`. Marshal, call,
// marshal: every rule lives in the toolkit-free crate, and nothing here does
// arithmetic of its own.
//
// The eight writable properties are exactly the inputs `Sizing.qml` already
// owned; the QML singleton keeps them and pushes them here on change, so
// `Main.qml` and `MainLayout.qml` still write to `Sizing` and nothing outside
// the theme module moved.
//
// Everything derived is a READ + NOTIFY property rather than an invokable, and
// that distinction is load-bearing: a QML binding captures its dependencies by
// watching property reads, so a value fetched through an invokable never
// re-evaluates its binding when the scene resizes. `models/hub_layout.rs`
// carries the same warning and the `revision` property that works around it.
// Invokables here are limited to helpers whose arguments already carry a
// notifying dependency (a viewport size, a painted width).
//
// One recompute per input change, then a changed signal only for the values
// that actually moved. Resizes and rotations are rare; bindings that did not
// need to wake up do not.

use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::QString;
use std::pin::Pin;
use zaparoo_app::sizing::{
    self, declared_grid_shape, derive, detail_cover_source_size, games_grid_cover_box,
    games_grid_cover_source_size, games_grid_shape, max_expressible_cover_tier, snap_cover_tier,
    snap_logo_width, systems_grid_shape, GridKind, Inputs, InterfaceProfile,
};

#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool backs a Q_PROPERTY and has to be its own field for the bridge macro; the two guard flags are private state"
)]
pub struct SizingRulesRust {
    /// Guards `recompute` against re-entry. Emitting a changed signal runs
    /// dependent QML bindings inline, and one of those can write another
    /// input back through a setter. Without the guard, the nested call
    /// computes the right values and the outer call then overwrites them with
    /// the snapshot it took before emitting.
    recomputing: bool,
    recompute_pending: bool,
    screen_width: f64,
    screen_height: f64,
    crt_native_path: bool,
    bitmap_type: bool,
    swap_percentage_axes: bool,
    interface_profile: QString,

    effective_height: i32,
    resolution_height: i32,
    tier: QString,
    handheld_profile: bool,
    radius_md: i32,
    radius_sm: i32,
    corner_antialiasing: bool,
    font_hero: i32,
    font_title: i32,
    font_section: i32,
    font_body: i32,
    font_caption: i32,
    font_small: i32,
    card_border_width: i32,
    focus_border_width: i32,
    focus_ring_width: i32,
    press_edge_height: i32,
    help_bar_height: i32,
    help_bar_clearance: i32,
    visible_covers: i32,
    hub_grid_columns: i32,
    hub_grid_rows: i32,
    hub_active_label_height: i32,
    hub_grid_top_margin: i32,
    hub_grid_height_budget: i32,
    hub_grid_side_inset: i32,
    hub_grid_column_gap: i32,
    hub_grid_top_inset: i32,
    hub_grid_bottom_inset: i32,
    hub_grid_row_gap: i32,
    hub_grid_width_fit: i32,
    hub_grid_height_fit: i32,
    hub_tile_size: i32,
    hub_tile_width: i32,
    hub_tile_height: i32,
    systems_grid_columns: i32,
    systems_grid_rows: i32,
    games_grid_columns: i32,
    games_grid_rows: i32,
    header_row_height: i32,
    header_stack_gap: i32,
    header_top_margin: i32,
    header_side_margin: i32,
    header_height: i32,
    header_bottom: i32,
}

impl Default for SizingRulesRust {
    /// Seeded from `sizing::Inputs::default()`, which mirrors `Sizing.qml`'s
    /// own property defaults, so the very first binding pass agrees with Rust
    /// before the scene has pushed real dimensions.
    fn default() -> Self {
        let inputs = Inputs::default();
        let mut model = Self {
            recomputing: false,
            recompute_pending: false,
            screen_width: inputs.screen_width,
            screen_height: inputs.screen_height,
            crt_native_path: false,
            bitmap_type: false,
            swap_percentage_axes: false,
            interface_profile: QString::from(inputs.interface_profile.as_str()),
            effective_height: 0,
            resolution_height: 0,
            tier: QString::default(),
            handheld_profile: false,
            radius_md: 0,
            radius_sm: 0,
            corner_antialiasing: false,
            font_hero: 0,
            font_title: 0,
            font_section: 0,
            font_body: 0,
            font_caption: 0,
            font_small: 0,
            card_border_width: 0,
            focus_border_width: 0,
            focus_ring_width: 0,
            press_edge_height: 0,
            help_bar_height: 0,
            help_bar_clearance: 0,
            visible_covers: 0,
            hub_grid_columns: 0,
            hub_grid_rows: 0,
            hub_active_label_height: 0,
            hub_grid_top_margin: 0,
            hub_grid_height_budget: 0,
            hub_grid_side_inset: 0,
            hub_grid_column_gap: 0,
            hub_grid_top_inset: 0,
            hub_grid_bottom_inset: 0,
            hub_grid_row_gap: 0,
            hub_grid_width_fit: 0,
            hub_grid_height_fit: 0,
            hub_tile_size: 0,
            hub_tile_width: 0,
            hub_tile_height: 0,
            systems_grid_columns: 0,
            systems_grid_rows: 0,
            games_grid_columns: 0,
            games_grid_rows: 0,
            header_row_height: 0,
            header_stack_gap: 0,
            header_top_margin: 0,
            header_side_margin: 0,
            header_height: 0,
            header_bottom: 0,
        };
        model.apply(&derive(&inputs));
        model
    }
}

impl SizingRulesRust {
    fn inputs(&self) -> Inputs {
        Inputs {
            screen_width: self.screen_width,
            screen_height: self.screen_height,
            crt_native_path: self.crt_native_path,
            bitmap_type: self.bitmap_type,
            swap_percentage_axes: self.swap_percentage_axes,
            interface_profile: InterfaceProfile::from_name(&self.interface_profile.to_string()),
        }
    }

    /// Overwrite every derived field. Used by `Default` before any signal
    /// machinery exists; the live path goes through `recompute` so it can
    /// report which fields moved.
    fn apply(&mut self, d: &sizing::Derived) {
        self.effective_height = d.effective_height;
        self.resolution_height = d.resolution_height;
        self.tier = QString::from(d.tier.as_str());
        self.handheld_profile = d.handheld_profile;
        self.radius_md = d.radius_md;
        self.radius_sm = d.radius_sm;
        self.corner_antialiasing = d.corner_antialiasing;
        self.font_hero = d.font_hero;
        self.font_title = d.font_title;
        self.font_section = d.font_section;
        self.font_body = d.font_body;
        self.font_caption = d.font_caption;
        self.font_small = d.font_small;
        self.card_border_width = d.card_border_width;
        self.focus_border_width = d.focus_border_width;
        self.focus_ring_width = d.focus_ring_width;
        self.press_edge_height = d.press_edge_height;
        self.help_bar_height = d.help_bar_height;
        self.help_bar_clearance = d.help_bar_clearance;
        self.visible_covers = d.visible_covers;
        self.hub_grid_columns = d.hub_grid_columns;
        self.hub_grid_rows = d.hub_grid_rows;
        self.hub_active_label_height = d.hub_active_label_height;
        self.hub_grid_top_margin = d.hub_grid_top_margin;
        self.hub_grid_height_budget = d.hub_grid_height_budget;
        self.hub_grid_side_inset = d.hub_grid_side_inset;
        self.hub_grid_column_gap = d.hub_grid_column_gap;
        self.hub_grid_top_inset = d.hub_grid_top_inset;
        self.hub_grid_bottom_inset = d.hub_grid_bottom_inset;
        self.hub_grid_row_gap = d.hub_grid_row_gap;
        self.hub_grid_width_fit = d.hub_grid_width_fit;
        self.hub_grid_height_fit = d.hub_grid_height_fit;
        self.hub_tile_size = d.hub_tile_size;
        self.hub_tile_width = d.hub_tile_width;
        self.hub_tile_height = d.hub_tile_height;
        self.systems_grid_columns = d.systems_grid_columns;
        self.systems_grid_rows = d.systems_grid_rows;
        self.games_grid_columns = d.games_grid_columns;
        self.games_grid_rows = d.games_grid_rows;
        self.header_row_height = d.header_row_height;
        self.header_stack_gap = d.header_stack_gap;
        self.header_top_margin = d.header_top_margin;
        self.header_side_margin = d.header_side_margin;
        self.header_height = d.header_height;
        self.header_bottom = d.header_bottom;
    }
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(f64, screen_width, READ, WRITE = set_screen_width, NOTIFY)]
        #[qproperty(f64, screen_height, READ, WRITE = set_screen_height, NOTIFY)]
        #[qproperty(bool, crt_native_path, READ, WRITE = set_crt_native_path, NOTIFY)]
        #[qproperty(bool, bitmap_type, READ, WRITE = set_bitmap_type, NOTIFY)]
        #[qproperty(bool, swap_percentage_axes, READ, WRITE = set_swap_percentage_axes, NOTIFY)]
        #[qproperty(QString, interface_profile, READ, WRITE = set_interface_profile, NOTIFY)]
        #[qproperty(i32, effective_height, READ, NOTIFY)]
        #[qproperty(i32, resolution_height, READ, NOTIFY)]
        #[qproperty(QString, tier, READ, NOTIFY)]
        #[qproperty(bool, handheld_profile, READ, NOTIFY)]
        #[qproperty(i32, radius_md, READ, NOTIFY)]
        #[qproperty(i32, radius_sm, READ, NOTIFY)]
        #[qproperty(bool, corner_antialiasing, READ, NOTIFY)]
        #[qproperty(i32, font_hero, READ, NOTIFY)]
        #[qproperty(i32, font_title, READ, NOTIFY)]
        #[qproperty(i32, font_section, READ, NOTIFY)]
        #[qproperty(i32, font_body, READ, NOTIFY)]
        #[qproperty(i32, font_caption, READ, NOTIFY)]
        #[qproperty(i32, font_small, READ, NOTIFY)]
        #[qproperty(i32, card_border_width, READ, NOTIFY)]
        #[qproperty(i32, focus_border_width, READ, NOTIFY)]
        #[qproperty(i32, focus_ring_width, READ, NOTIFY)]
        #[qproperty(i32, press_edge_height, READ, NOTIFY)]
        #[qproperty(i32, help_bar_height, READ, NOTIFY)]
        #[qproperty(i32, help_bar_clearance, READ, NOTIFY)]
        #[qproperty(i32, visible_covers, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_columns, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_rows, READ, NOTIFY)]
        #[qproperty(i32, hub_active_label_height, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_top_margin, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_height_budget, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_side_inset, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_column_gap, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_top_inset, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_bottom_inset, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_row_gap, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_width_fit, READ, NOTIFY)]
        #[qproperty(i32, hub_grid_height_fit, READ, NOTIFY)]
        #[qproperty(i32, hub_tile_size, READ, NOTIFY)]
        #[qproperty(i32, hub_tile_width, READ, NOTIFY)]
        #[qproperty(i32, hub_tile_height, READ, NOTIFY)]
        #[qproperty(i32, systems_grid_columns, READ, NOTIFY)]
        #[qproperty(i32, systems_grid_rows, READ, NOTIFY)]
        #[qproperty(i32, games_grid_columns, READ, NOTIFY)]
        #[qproperty(i32, games_grid_rows, READ, NOTIFY)]
        #[qproperty(i32, header_row_height, READ, NOTIFY)]
        #[qproperty(i32, header_stack_gap, READ, NOTIFY)]
        #[qproperty(i32, header_top_margin, READ, NOTIFY)]
        #[qproperty(i32, header_side_margin, READ, NOTIFY)]
        #[qproperty(i32, header_height, READ, NOTIFY)]
        #[qproperty(i32, header_bottom, READ, NOTIFY)]
        type SizingRules = super::SizingRulesRust;

        #[qinvokable]
        fn set_screen_width(self: Pin<&mut SizingRules>, value: f64);

        #[qinvokable]
        fn set_screen_height(self: Pin<&mut SizingRules>, value: f64);

        #[qinvokable]
        fn set_crt_native_path(self: Pin<&mut SizingRules>, value: bool);

        #[qinvokable]
        fn set_bitmap_type(self: Pin<&mut SizingRules>, value: bool);

        #[qinvokable]
        fn set_swap_percentage_axes(self: Pin<&mut SizingRules>, value: bool);

        #[qinvokable]
        fn set_interface_profile(self: Pin<&mut SizingRules>, value: QString);

        /// Snap a painted cover size up to the smallest of Core's resize
        /// tiers that covers it, so the decoded texture matches the WebP
        /// Core delivers and small resolution wobble cannot move the tier.
        /// Pure in its argument, so a binding that computes the argument
        /// already carries the dependency.
        #[qinvokable]
        fn snap_cover_tier(self: &SizingRules, px: f64) -> i32;

        /// Snap a painted logo width up to the smallest pre-sized rung in
        /// `resources/images/logo/`. Pure in its argument.
        #[qinvokable]
        fn snap_logo_width(self: &SizingRules, px: f64) -> i32;

        /// Largest cover tier not wider than the viewport itself. Pure in its
        /// argument.
        #[qinvokable]
        fn max_expressible_cover_tier(self: &SizingRules, viewport_width: f64) -> i32;

        // The rest take the scene configuration explicitly rather than
        // reading the stored properties, and that is deliberate. A QML
        // binding registers a dependency by watching property reads while it
        // evaluates; a cxx-qt invokable reports nothing. If these read the
        // stored inputs, a binding calling one with constant viewport
        // arguments would never re-evaluate on a resize, a rotation or a CRT
        // switch. Passing the configuration through means the facade in
        // `Sizing.qml` has to read those properties to make the call, which is
        // exactly the dependency the binding needs. Reading them and
        // discarding the result does not work: the QML compiler elides it.

        #[qinvokable]
        fn games_grid_columns_for(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn games_grid_rows_for(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn systems_grid_columns_for(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn systems_grid_rows_for(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        /// Hand-declared page geometry for CRT and the common framebuffer
        /// sizes. Returns -1 when the scene has none and the adaptive scorer
        /// should run instead.
        #[qinvokable]
        fn declared_grid_columns(
            self: &SizingRules,
            kind: &QString,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn declared_grid_rows(
            self: &SizingRules,
            kind: &QString,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn games_grid_cover_box(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn games_grid_cover_source_size(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;

        #[qinvokable]
        fn detail_cover_source_size(
            self: &SizingRules,
            viewport_width: f64,
            viewport_height: f64,
            screen_width: f64,
            screen_height: f64,
            crt_native_path: bool,
            swap_percentage_axes: bool,
        ) -> i32;
    }

    impl cxx_qt::Initialize for SizingRules {}
}

impl Initialize for ffi::SizingRules {
    fn initialize(self: Pin<&mut Self>) {
        // `SizingRulesRust::default()` already resolved every derived value from
        // `Inputs::default()`, so there is nothing to seed here. The impl is
        // required because the bridge declares it.
    }
}

impl ffi::SizingRules {
    #[allow(
        clippy::float_cmp,
        reason = "an exact match means the property was rewritten with the value it already holds, which is precisely the recompute we want to skip"
    )]
    fn set_screen_width(mut self: Pin<&mut Self>, value: f64) {
        if self.screen_width == value {
            return;
        }
        self.as_mut().rust_mut().screen_width = value;
        self.as_mut().screen_width_changed();
        self.recompute();
    }

    #[allow(
        clippy::float_cmp,
        reason = "an exact match means the property was rewritten with the value it already holds, which is precisely the recompute we want to skip"
    )]
    fn set_screen_height(mut self: Pin<&mut Self>, value: f64) {
        if self.screen_height == value {
            return;
        }
        self.as_mut().rust_mut().screen_height = value;
        self.as_mut().screen_height_changed();
        self.recompute();
    }

    fn set_crt_native_path(mut self: Pin<&mut Self>, value: bool) {
        if self.crt_native_path == value {
            return;
        }
        self.as_mut().rust_mut().crt_native_path = value;
        self.as_mut().crt_native_path_changed();
        self.recompute();
    }

    fn set_bitmap_type(mut self: Pin<&mut Self>, value: bool) {
        if self.bitmap_type == value {
            return;
        }
        self.as_mut().rust_mut().bitmap_type = value;
        self.as_mut().bitmap_type_changed();
        self.recompute();
    }

    fn set_swap_percentage_axes(mut self: Pin<&mut Self>, value: bool) {
        if self.swap_percentage_axes == value {
            return;
        }
        self.as_mut().rust_mut().swap_percentage_axes = value;
        self.as_mut().swap_percentage_axes_changed();
        self.recompute();
    }

    fn set_interface_profile(mut self: Pin<&mut Self>, value: QString) {
        if self.interface_profile == value {
            return;
        }
        self.as_mut().rust_mut().interface_profile = value;
        self.as_mut().interface_profile_changed();
        self.recompute();
    }

    /// Re-derive everything and emit a changed signal only for the values
    /// that actually moved.
    fn recompute(mut self: Pin<&mut Self>) {
        if self.recomputing {
            // Re-entered from a binding woken by one of our own signals. Let
            // the outer pass run again rather than interleaving two snapshots.
            self.as_mut().rust_mut().recompute_pending = true;
            return;
        }
        self.as_mut().rust_mut().recomputing = true;
        loop {
            self.as_mut().rust_mut().recompute_pending = false;
            self.as_mut().recompute_once();
            if !self.recompute_pending {
                break;
            }
        }
        self.as_mut().rust_mut().recomputing = false;
    }

    fn recompute_once(mut self: Pin<&mut Self>) {
        let next = derive(&self.inputs());
        self.as_mut().emit_type_and_shape_changes(&next);
        self.as_mut().emit_hub_changes(&next);
        self.as_mut().emit_grid_and_header_changes(&next);
    }

    /// Resolution tier, corner radii, the type ladder, stroke widths and the
    /// help-bar band.
    fn emit_type_and_shape_changes(mut self: Pin<&mut Self>, next: &sizing::Derived) {
        if self.effective_height != next.effective_height {
            self.as_mut().rust_mut().effective_height = next.effective_height;
            self.as_mut().effective_height_changed();
        }
        if self.resolution_height != next.resolution_height {
            self.as_mut().rust_mut().resolution_height = next.resolution_height;
            self.as_mut().resolution_height_changed();
        }
        let tier = QString::from(next.tier.as_str());
        if self.tier != tier {
            self.as_mut().rust_mut().tier = tier;
            self.as_mut().tier_changed();
        }
        if self.handheld_profile != next.handheld_profile {
            self.as_mut().rust_mut().handheld_profile = next.handheld_profile;
            self.as_mut().handheld_profile_changed();
        }
        if self.radius_md != next.radius_md {
            self.as_mut().rust_mut().radius_md = next.radius_md;
            self.as_mut().radius_md_changed();
        }
        if self.radius_sm != next.radius_sm {
            self.as_mut().rust_mut().radius_sm = next.radius_sm;
            self.as_mut().radius_sm_changed();
        }
        if self.corner_antialiasing != next.corner_antialiasing {
            self.as_mut().rust_mut().corner_antialiasing = next.corner_antialiasing;
            self.as_mut().corner_antialiasing_changed();
        }
        if self.font_hero != next.font_hero {
            self.as_mut().rust_mut().font_hero = next.font_hero;
            self.as_mut().font_hero_changed();
        }
        if self.font_title != next.font_title {
            self.as_mut().rust_mut().font_title = next.font_title;
            self.as_mut().font_title_changed();
        }
        if self.font_section != next.font_section {
            self.as_mut().rust_mut().font_section = next.font_section;
            self.as_mut().font_section_changed();
        }
        if self.font_body != next.font_body {
            self.as_mut().rust_mut().font_body = next.font_body;
            self.as_mut().font_body_changed();
        }
        if self.font_caption != next.font_caption {
            self.as_mut().rust_mut().font_caption = next.font_caption;
            self.as_mut().font_caption_changed();
        }
        if self.font_small != next.font_small {
            self.as_mut().rust_mut().font_small = next.font_small;
            self.as_mut().font_small_changed();
        }
        if self.card_border_width != next.card_border_width {
            self.as_mut().rust_mut().card_border_width = next.card_border_width;
            self.as_mut().card_border_width_changed();
        }
        if self.focus_border_width != next.focus_border_width {
            self.as_mut().rust_mut().focus_border_width = next.focus_border_width;
            self.as_mut().focus_border_width_changed();
        }
        if self.focus_ring_width != next.focus_ring_width {
            self.as_mut().rust_mut().focus_ring_width = next.focus_ring_width;
            self.as_mut().focus_ring_width_changed();
        }
        if self.press_edge_height != next.press_edge_height {
            self.as_mut().rust_mut().press_edge_height = next.press_edge_height;
            self.as_mut().press_edge_height_changed();
        }
        if self.help_bar_height != next.help_bar_height {
            self.as_mut().rust_mut().help_bar_height = next.help_bar_height;
            self.as_mut().help_bar_height_changed();
        }
        if self.help_bar_clearance != next.help_bar_clearance {
            self.as_mut().rust_mut().help_bar_clearance = next.help_bar_clearance;
            self.as_mut().help_bar_clearance_changed();
        }
        if self.visible_covers != next.visible_covers {
            self.as_mut().rust_mut().visible_covers = next.visible_covers;
            self.as_mut().visible_covers_changed();
        }
    }

    /// The Hub's fixed page shape and every inset, gap and fit derived from it.
    fn emit_hub_changes(mut self: Pin<&mut Self>, next: &sizing::Derived) {
        if self.hub_grid_columns != next.hub_grid_columns {
            self.as_mut().rust_mut().hub_grid_columns = next.hub_grid_columns;
            self.as_mut().hub_grid_columns_changed();
        }
        if self.hub_grid_rows != next.hub_grid_rows {
            self.as_mut().rust_mut().hub_grid_rows = next.hub_grid_rows;
            self.as_mut().hub_grid_rows_changed();
        }
        if self.hub_active_label_height != next.hub_active_label_height {
            self.as_mut().rust_mut().hub_active_label_height = next.hub_active_label_height;
            self.as_mut().hub_active_label_height_changed();
        }
        if self.hub_grid_top_margin != next.hub_grid_top_margin {
            self.as_mut().rust_mut().hub_grid_top_margin = next.hub_grid_top_margin;
            self.as_mut().hub_grid_top_margin_changed();
        }
        if self.hub_grid_height_budget != next.hub_grid_height_budget {
            self.as_mut().rust_mut().hub_grid_height_budget = next.hub_grid_height_budget;
            self.as_mut().hub_grid_height_budget_changed();
        }
        if self.hub_grid_side_inset != next.hub_grid_side_inset {
            self.as_mut().rust_mut().hub_grid_side_inset = next.hub_grid_side_inset;
            self.as_mut().hub_grid_side_inset_changed();
        }
        if self.hub_grid_column_gap != next.hub_grid_column_gap {
            self.as_mut().rust_mut().hub_grid_column_gap = next.hub_grid_column_gap;
            self.as_mut().hub_grid_column_gap_changed();
        }
        if self.hub_grid_top_inset != next.hub_grid_top_inset {
            self.as_mut().rust_mut().hub_grid_top_inset = next.hub_grid_top_inset;
            self.as_mut().hub_grid_top_inset_changed();
        }
        if self.hub_grid_bottom_inset != next.hub_grid_bottom_inset {
            self.as_mut().rust_mut().hub_grid_bottom_inset = next.hub_grid_bottom_inset;
            self.as_mut().hub_grid_bottom_inset_changed();
        }
        if self.hub_grid_row_gap != next.hub_grid_row_gap {
            self.as_mut().rust_mut().hub_grid_row_gap = next.hub_grid_row_gap;
            self.as_mut().hub_grid_row_gap_changed();
        }
        if self.hub_grid_width_fit != next.hub_grid_width_fit {
            self.as_mut().rust_mut().hub_grid_width_fit = next.hub_grid_width_fit;
            self.as_mut().hub_grid_width_fit_changed();
        }
        if self.hub_grid_height_fit != next.hub_grid_height_fit {
            self.as_mut().rust_mut().hub_grid_height_fit = next.hub_grid_height_fit;
            self.as_mut().hub_grid_height_fit_changed();
        }
        if self.hub_tile_size != next.hub_tile_size {
            self.as_mut().rust_mut().hub_tile_size = next.hub_tile_size;
            self.as_mut().hub_tile_size_changed();
        }
        if self.hub_tile_width != next.hub_tile_width {
            self.as_mut().rust_mut().hub_tile_width = next.hub_tile_width;
            self.as_mut().hub_tile_width_changed();
        }
        if self.hub_tile_height != next.hub_tile_height {
            self.as_mut().rust_mut().hub_tile_height = next.hub_tile_height;
            self.as_mut().hub_tile_height_changed();
        }
    }

    /// Browse page shapes for the full scene, and the header band every screen
    /// clears.
    fn emit_grid_and_header_changes(mut self: Pin<&mut Self>, next: &sizing::Derived) {
        if self.systems_grid_columns != next.systems_grid_columns {
            self.as_mut().rust_mut().systems_grid_columns = next.systems_grid_columns;
            self.as_mut().systems_grid_columns_changed();
        }
        if self.systems_grid_rows != next.systems_grid_rows {
            self.as_mut().rust_mut().systems_grid_rows = next.systems_grid_rows;
            self.as_mut().systems_grid_rows_changed();
        }
        if self.games_grid_columns != next.games_grid_columns {
            self.as_mut().rust_mut().games_grid_columns = next.games_grid_columns;
            self.as_mut().games_grid_columns_changed();
        }
        if self.games_grid_rows != next.games_grid_rows {
            self.as_mut().rust_mut().games_grid_rows = next.games_grid_rows;
            self.as_mut().games_grid_rows_changed();
        }
        if self.header_row_height != next.header_row_height {
            self.as_mut().rust_mut().header_row_height = next.header_row_height;
            self.as_mut().header_row_height_changed();
        }
        if self.header_stack_gap != next.header_stack_gap {
            self.as_mut().rust_mut().header_stack_gap = next.header_stack_gap;
            self.as_mut().header_stack_gap_changed();
        }
        if self.header_top_margin != next.header_top_margin {
            self.as_mut().rust_mut().header_top_margin = next.header_top_margin;
            self.as_mut().header_top_margin_changed();
        }
        if self.header_side_margin != next.header_side_margin {
            self.as_mut().rust_mut().header_side_margin = next.header_side_margin;
            self.as_mut().header_side_margin_changed();
        }
        if self.header_height != next.header_height {
            self.as_mut().rust_mut().header_height = next.header_height;
            self.as_mut().header_height_changed();
        }
        if self.header_bottom != next.header_bottom {
            self.as_mut().rust_mut().header_bottom = next.header_bottom;
            self.as_mut().header_bottom_changed();
        }
    }

    fn snap_cover_tier(&self, px: f64) -> i32 {
        snap_cover_tier(px)
    }

    fn snap_logo_width(&self, px: f64) -> i32 {
        snap_logo_width(px)
    }

    fn max_expressible_cover_tier(&self, viewport_width: f64) -> i32 {
        max_expressible_cover_tier(viewport_width)
    }

    fn games_grid_columns_for(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        games_grid_shape(&inputs, viewport_width, viewport_height).columns
    }

    fn games_grid_rows_for(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        games_grid_shape(&inputs, viewport_width, viewport_height).rows
    }

    fn systems_grid_columns_for(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        systems_grid_shape(&inputs, viewport_width, viewport_height).columns
    }

    fn systems_grid_rows_for(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        systems_grid_shape(&inputs, viewport_width, viewport_height).rows
    }

    fn declared_grid_columns(
        &self,
        kind: &QString,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        declared_shape(
            kind,
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        )
        .map_or(-1, |shape| shape.columns)
    }

    fn declared_grid_rows(
        &self,
        kind: &QString,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        declared_shape(
            kind,
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        )
        .map_or(-1, |shape| shape.rows)
    }

    fn games_grid_cover_box(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        games_grid_cover_box(&inputs, viewport_width, viewport_height)
    }

    fn games_grid_cover_source_size(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        games_grid_cover_source_size(&inputs, viewport_width, viewport_height)
    }

    fn detail_cover_source_size(
        &self,
        viewport_width: f64,
        viewport_height: f64,
        screen_width: f64,
        screen_height: f64,
        crt_native_path: bool,
        swap_percentage_axes: bool,
    ) -> i32 {
        let inputs = scene_inputs(
            screen_width,
            screen_height,
            crt_native_path,
            swap_percentage_axes,
        );
        detail_cover_source_size(&inputs, viewport_width, viewport_height)
    }
}

/// The subset of `Inputs` the argument-taking helpers need. `bitmap_type` and
/// `interface_profile` only reach font sizing and the Hub shape, neither of
/// which is resolved through these, so the facade does not have to thread them
/// through.
fn scene_inputs(
    screen_width: f64,
    screen_height: f64,
    crt_native_path: bool,
    swap_percentage_axes: bool,
) -> Inputs {
    Inputs {
        screen_width,
        screen_height,
        crt_native_path,
        swap_percentage_axes,
        ..Inputs::default()
    }
}

fn declared_shape(
    kind: &QString,
    screen_width: f64,
    screen_height: f64,
    crt_native_path: bool,
    swap_percentage_axes: bool,
) -> Option<sizing::GridShape> {
    let kind = GridKind::from_name(&kind.to_string())?;
    let inputs = scene_inputs(
        screen_width,
        screen_height,
        crt_native_path,
        swap_percentage_axes,
    );
    declared_grid_shape(&inputs, kind)
}
