// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Resolution-agnostic sizing: the half of `src/ui/theme/Sizing.qml` that has
// tables or loops in it. The QML singleton stays as the facade and keeps the
// one-line percentage helpers (`pctH`, `pctW`, `px`, `stroke`, `center`,
// `half`, `fontSize`); everything here is what those helpers feed into.
//
// Why the split is where it is: a QML binding captures its dependencies by
// watching property reads during evaluation. `Sizing.pctH(5.5)` re-evaluates
// on resize only because `pctH` is QML JavaScript and the engine sees it read
// `screenHeight`. A cxx-qt *invokable* registers nothing, so moving those
// helpers would freeze ~510 call sites at their startup value. Derived values
// cross as notifying properties instead, which bindings do track. See
// `docs/qt-to-rust-extraction.md`.
//
// Parity with the QML is pinned by `tests/fixtures/sizing_golden.txt`, 192
// cases captured from the QML implementation before any of it moved here.
//
// The rounding helper matters. QML runs JavaScript, and `Math.round` rounds
// half towards +infinity (`Math.round(-2.5) === -2`), while Rust's
// `f64::round` rounds half away from zero (`(-2.5f64).round() == -3.0`). Every
// rounding in this module goes through `js_round` so the two agree on the
// negative halves that `center()`-style arithmetic can produce.

/// JavaScript `Math.round`: half rounds towards positive infinity.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Discrete logical-resolution tier. Shape and type hierarchy key off this
/// rather than off raw pixels, so a 240p CRT and a 240p digital scene get the
/// same silhouette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    T240,
    T480,
    T540,
    T720,
    T1080,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::T240 => "240",
            Self::T480 => "480",
            Self::T540 => "540",
            Self::T720 => "720",
            Self::T1080 => "1080",
        }
    }

    fn from_resolution_height(height: i32) -> Self {
        if height >= 900 {
            Self::T1080
        } else if height >= 660 {
            Self::T720
        } else if height >= 520 {
            Self::T540
        } else if height >= 400 {
            Self::T480
        } else {
            Self::T240
        }
    }

    /// True for every tier except 240. At 240p the radii are a deliberate
    /// one-pixel corner cut, and coverage antialiasing only softens the
    /// neighbouring pixels without adding shape information.
    fn corner_antialiasing(self) -> bool {
        self != Self::T240
    }

    fn radius_md(self) -> i32 {
        match self {
            Self::T1080 => 8,
            Self::T720 => 6,
            Self::T540 => 4,
            Self::T480 => 3,
            Self::T240 => 2,
        }
    }

    /// Six semantic text sizes per tier, largest first, so adjacent roles
    /// never collapse into each other at 240p/480p/540p.
    fn font_ladder(self) -> [i32; 6] {
        match self {
            Self::T1080 => [43, 35, 31, 28, 26, 24],
            Self::T720 => [29, 23, 21, 19, 17, 16],
            Self::T540 => [24, 20, 18, 17, 15, 14],
            Self::T480 => [22, 18, 17, 16, 14, 13],
            Self::T240 => [14, 12, 11, 10, 9, 8],
        }
    }
}

/// Whether the frontend is presenting on a handheld. Changes page density,
/// never persisted Hub order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterfaceProfile {
    #[default]
    Standard,
    Handheld,
}

impl InterfaceProfile {
    /// Mirrors the QML `interfaceProfile === "handheld"` test: anything that
    /// is not exactly `handheld` is the standard profile.
    pub fn from_name(name: &str) -> Self {
        if name == "handheld" {
            Self::Handheld
        } else {
            Self::Standard
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Handheld => "handheld",
        }
    }
}

/// Everything the sizing rules read. These are exactly the writable
/// properties on the QML singleton, in the same units: `screen_width` and
/// `screen_height` are the *scene's* logical dimensions, so they already have
/// the CRT safe-area inset removed and the axes swapped in a rotated layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    pub screen_width: f64,
    pub screen_height: f64,
    /// Rendering path, not a geometry tier: a CRT scene exposes the
    /// action-safe canvas after 5% has been trimmed from each edge.
    pub crt_native_path: bool,
    /// The 6x8 bitmap face is in use, so type quantizes to 8 or 16 px.
    pub bitmap_type: bool,
    /// Rotated (TATE) layout: percentage helpers read the other axis.
    pub swap_percentage_axes: bool,
    pub interface_profile: InterfaceProfile,
}

impl Default for Inputs {
    /// Matches `Sizing.qml`'s own property defaults, so the first QML binding
    /// pass agrees with Rust before the scene has pushed real dimensions.
    fn default() -> Self {
        Self {
            screen_width: 640.0,
            screen_height: 480.0,
            crt_native_path: false,
            bitmap_type: false,
            swap_percentage_axes: false,
            interface_profile: InterfaceProfile::Standard,
        }
    }
}

impl Inputs {
    /// Percentage of the height axis, in whole pixels.
    pub fn pct_h(&self, percent: f64) -> i32 {
        let axis = if self.swap_percentage_axes {
            self.screen_width
        } else {
            self.screen_height
        };
        js_round(axis * percent / 100.0) as i32
    }

    /// Percentage of the width axis, in whole pixels.
    pub fn pct_w(&self, percent: f64) -> i32 {
        let axis = if self.swap_percentage_axes {
            self.screen_height
        } else {
            self.screen_width
        };
        js_round(axis * percent / 100.0) as i32
    }

    /// At least one physical pixel, so a hairline never rounds away.
    fn stroke(&self, percent: f64) -> i32 {
        self.pct_h(percent).max(1)
    }

    /// In TATE the scene dimensions are swapped, so read the original
    /// framebuffer's height axis rather than promoting a 720p portrait scene
    /// into the 1080 tier.
    pub fn effective_height(&self) -> i32 {
        let value = if self.swap_percentage_axes {
            self.screen_width
        } else {
            self.screen_height
        };
        value as i32
    }

    /// The framebuffer's resolution axis, recovered from a CRT scene's
    /// action-safe canvas. Keyed off the rendering path so a future 540p or
    /// 720p CRT mode is not demoted into a lower tier by the safe-area trim.
    pub fn resolution_height(&self) -> i32 {
        let effective = self.effective_height();
        if self.crt_native_path {
            js_round(f64::from(effective) / 0.9) as i32
        } else {
            effective
        }
    }

    pub fn tier(&self) -> Tier {
        Tier::from_resolution_height(self.resolution_height())
    }

    pub fn handheld(&self) -> bool {
        self.interface_profile == InterfaceProfile::Handheld
    }

    /// Minimum 8 px to stay legible at 240p. The bitmap face only has two
    /// usable sizes, so it snaps rather than scaling.
    pub fn font_size(&self, percent: f64) -> i32 {
        let size = self.pct_h(percent).max(8);
        if !self.bitmap_type {
            return size;
        }
        if size < 12 {
            8
        } else {
            16
        }
    }

    fn font_role(&self, role: usize, bitmap_percent: f64) -> i32 {
        if self.bitmap_type {
            self.font_size(bitmap_percent)
        } else {
            self.tier().font_ladder()[role]
        }
    }
}

/// A page's cell count. Two scalars rather than a map, because every invokable
/// across the cxx-qt bridge in this project returns a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridShape {
    pub columns: i32,
    pub rows: i32,
}

impl GridShape {
    fn new(columns: i32, rows: i32) -> Self {
        Self { columns, rows }
    }

    /// Transposed in a rotated layout, so a page keeps its cell count.
    fn oriented(self, swap: bool) -> Self {
        if swap {
            Self::new(self.rows, self.columns)
        } else {
            self
        }
    }
}

/// Bounds the adaptive scorer works inside. Systems and games solve the same
/// viewport-fit problem, so the shared limits live in one place and each
/// surface overrides only what is materially different.
#[derive(Debug, Clone, Copy)]
struct GridConfig {
    min_cell_width: f64,
    min_cell_height: f64,
    preferred_page_size: f64,
    min_columns: i32,
    max_columns: i32,
    min_rows: i32,
    max_rows: i32,
    target_aspect: f64,
}

fn systems_grid_config(inputs: &Inputs) -> GridConfig {
    let crt = inputs.crt_native_path;
    GridConfig {
        min_cell_width: if crt { 72.0 } else { 160.0 },
        min_cell_height: if crt { 72.0 } else { 140.0 },
        preferred_page_size: if crt { 9.0 } else { 12.0 },
        min_columns: 2,
        max_columns: if crt { 3 } else { 5 },
        min_rows: 2,
        max_rows: if crt { 3 } else { 5 },
        // System logos are squarer than box art, so they target a slightly
        // wider cell while keeping the same preferred page size.
        target_aspect: 1.25,
    }
}

fn games_grid_config(inputs: &Inputs) -> GridConfig {
    let crt = inputs.crt_native_path;
    GridConfig {
        min_cell_width: if crt { 72.0 } else { 160.0 },
        // A 1080p MiSTer output renders through a 960x540 framebuffer. At that
        // logical height 31.5% resolves to 170 px, which preserves the normal
        // five-column two-row page instead of falling back to 2x2.
        min_cell_height: if crt {
            96.0
        } else {
            f64::from(inputs.pct_h(31.5))
        },
        preferred_page_size: if crt { 6.0 } else { 10.0 },
        min_columns: 2,
        max_columns: if crt { 3 } else { 5 },
        min_rows: 2,
        max_rows: if crt { 3 } else { 5 },
        target_aspect: if crt { 0.78 } else { 0.71 },
    }
}

/// Pick the (columns, rows) whose cell aspect best matches the target while
/// staying above the minimum readable cell size, with a small penalty for
/// straying from the preferred page size.
fn select_grid_shape(viewport_width: f64, viewport_height: f64, options: &GridConfig) -> GridShape {
    let safe_width = viewport_width.max(1.0);
    let safe_height = viewport_height.max(1.0);
    let mut best = GridShape::new(options.min_columns, options.min_rows);
    let mut best_score = f64::MAX;

    for columns in options.min_columns..=options.max_columns {
        let cell_width = safe_width / f64::from(columns);
        if cell_width < options.min_cell_width {
            continue;
        }
        for rows in options.min_rows..=options.max_rows {
            let cell_height = safe_height / f64::from(rows);
            if cell_height < options.min_cell_height {
                continue;
            }
            let aspect = cell_width / cell_height;
            let aspect_error = (aspect / options.target_aspect).ln().abs();
            let page_penalty =
                (f64::from(columns * rows) - options.preferred_page_size).abs() * 0.04;
            let score = aspect_error + page_penalty;
            if score < best_score {
                best_score = score;
                best = GridShape::new(columns, rows);
            }
        }
    }

    best
}

/// Which browse surface a grid shape is being resolved for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridKind {
    Systems,
    Games,
}

impl GridKind {
    /// Mirrors the QML string argument, so the facade can forward verbatim.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "systems" => Some(Self::Systems),
            "games" => Some(Self::Games),
            _ => None,
        }
    }
}

/// The scene matches a common framebuffer size, within a two-pixel tolerance.
/// Those sizes have hand-declared page geometry rather than a scored fit.
fn common_digital_scene(inputs: &Inputs) -> Option<Tier> {
    let (width, height) = if inputs.swap_percentage_axes {
        (inputs.screen_height, inputs.screen_width)
    } else {
        (inputs.screen_width, inputs.screen_height)
    };
    let close = |actual: f64, expected: f64| (actual - expected).abs() <= 2.0;
    let matches = |w: f64, h: f64| close(width, w) && close(height, h);

    if matches(320.0, 240.0) {
        Some(Tier::T240)
    } else if matches(640.0, 480.0) {
        Some(Tier::T480)
    } else if matches(960.0, 540.0) {
        Some(Tier::T540)
    } else if matches(1280.0, 720.0) || matches(1366.0, 768.0) {
        Some(Tier::T720)
    } else if matches(1920.0, 1080.0) {
        Some(Tier::T1080)
    } else {
        None
    }
}

/// Hand-declared page geometry for CRT and for the common framebuffer sizes.
/// `None` means "no declared shape, run the adaptive scorer".
pub fn declared_grid_shape(inputs: &Inputs, kind: GridKind) -> Option<GridShape> {
    let swap = inputs.swap_percentage_axes;

    if inputs.crt_native_path {
        return Some(match kind {
            GridKind::Systems => GridShape::new(3, 3).oriented(swap),
            GridKind::Games => GridShape::new(3, 2).oriented(swap),
        });
    }

    let common = common_digital_scene(inputs)?;
    let shape = match (kind, common) {
        (GridKind::Systems, Tier::T240) => GridShape::new(2, 2),
        (GridKind::Systems, Tier::T480) => GridShape::new(3, 3),
        (GridKind::Systems, _) => GridShape::new(4, 3),
        (GridKind::Games, Tier::T240) => GridShape::new(3, 2),
        (GridKind::Games, Tier::T480) => GridShape::new(4, 2),
        (GridKind::Games, _) => GridShape::new(5, 2),
    };
    Some(shape.oriented(swap))
}

/// Games page shape for a viewport. Comes from the logical viewport rather
/// than from height-only breakpoints, so rotating the scene changes how many
/// tiles fit without stretching the cards into a different shape.
pub fn games_grid_shape(inputs: &Inputs, viewport_width: f64, viewport_height: f64) -> GridShape {
    declared_grid_shape(inputs, GridKind::Games).unwrap_or_else(|| {
        select_grid_shape(viewport_width, viewport_height, &games_grid_config(inputs))
    })
}

/// Systems page shape for a viewport. System logos need more vertical capacity
/// than the compact 240p landscape grid provides, so both rotated layouts use
/// a fixed 2x3 page.
pub fn systems_grid_shape(inputs: &Inputs, viewport_width: f64, viewport_height: f64) -> GridShape {
    if inputs.swap_percentage_axes {
        return GridShape::new(2, 3);
    }
    declared_grid_shape(inputs, GridKind::Systems).unwrap_or_else(|| {
        select_grid_shape(
            viewport_width,
            viewport_height,
            &systems_grid_config(inputs),
        )
    })
}

/// Hub page shape. Deliberately a fixed per-tier table rather than the
/// adaptive scorer Systems and Games use: a hand-arranged Hub layout must not
/// reflow when the window or display changes, only when the discrete tier
/// itself does. An adaptive fit would silently scramble hand-placed tiles on
/// a display or CRT switch. Pagination is the intended overflow path.
pub fn hub_grid_shape(inputs: &Inputs) -> GridShape {
    let tier = inputs.tier();
    let compact = tier == Tier::T240 || tier == Tier::T480;
    let base = if inputs.handheld() && !compact {
        // Handheld changes page density, not persisted order: the same linear
        // Hub slots reflow into fewer columns. The low tiers are already
        // compact enough.
        GridShape::new(4, 3)
    } else if compact {
        GridShape::new(4, 2)
    } else {
        GridShape::new(7, 3)
    };
    base.oriented(inputs.swap_percentage_axes)
}

/// Cover decode tiers, mirroring Core's resize ladder. A cover's source size
/// must be one of these so the decoded texture matches the WebP Core delivers
/// with no resample, and so small resolution wobble does not move the tier.
pub fn snap_cover_tier(px: f64) -> i32 {
    if px <= 128.0 {
        128
    } else if px <= 256.0 {
        256
    } else if px <= 512.0 {
        512
    } else {
        768
    }
}

/// Header-logo asset ladder (`resources/images/logo/logo-<variant>-<w>.png`).
/// Pre-sized rungs so the header decodes close to its painted width instead of
/// bilinearly downscaling the 600 px master at paint time.
pub fn snap_logo_width(px: f64) -> i32 {
    if px <= 96.0 {
        96
    } else if px <= 144.0 {
        144
    } else if px <= 192.0 {
        192
    } else if px <= 256.0 {
        256
    } else if px <= 384.0 {
        384
    } else {
        600
    }
}

/// Largest cover tier not wider than the viewport itself. A decode wider than
/// the scene can only ever be shown downscaled, so requesting it wastes
/// resample time and decoded-cache bytes.
pub fn max_expressible_cover_tier(viewport_width: f64) -> i32 {
    if viewport_width >= 768.0 {
        768
    } else if viewport_width >= 512.0 {
        512
    } else if viewport_width >= 256.0 {
        256
    } else {
        128
    }
}

/// Raw painted height of a games-grid cover in caption mode: the raised face
/// height minus top padding and the bottom caption band. Box art is portrait,
/// so this height is the bounding side. A pure function of the resolution, and
/// therefore the same value for every tile, which is what lets callers use it
/// as a stable decode-size input instead of the live painted height (which
/// fluctuates per layout and would force a reload on every change).
pub fn games_grid_cover_box(inputs: &Inputs, viewport_width: f64, viewport_height: f64) -> i32 {
    let shape = games_grid_shape(inputs, viewport_width, viewport_height);
    let tile_height = (viewport_height.max(1.0) / f64::from(shape.rows.max(1))).ceil() as i32;
    let cover_box = tile_height
        - derived_press_edge_height(inputs)
        - inputs.pct_h(2.0)
        - (inputs.pct_h(5.5) + inputs.pct_h(0.4));
    cover_box.max(1)
}

/// Stable per-view cover decode size, snapped to a Core tier. One source of
/// truth for both the Core fetch request and the grid tile's decode size, so
/// request size equals decode size.
pub fn games_grid_cover_source_size(
    inputs: &Inputs,
    viewport_width: f64,
    viewport_height: f64,
) -> i32 {
    snap_cover_tier(f64::from(games_grid_cover_box(
        inputs,
        viewport_width,
        viewport_height,
    )))
}

/// Detail-pane cover decode size: roughly twice the grid cover, snapped to its
/// own tier, then capped at the largest tier the viewport can actually
/// express. Without the cap a CRT-native scene requests 512-wide decodes the
/// framebuffer can never display, at roughly 1.1 MB of decoded cache each.
pub fn detail_cover_source_size(inputs: &Inputs, viewport_width: f64, viewport_height: f64) -> i32 {
    let doubled = snap_cover_tier(f64::from(
        games_grid_cover_box(inputs, viewport_width, viewport_height) * 2,
    ));
    doubled.min(max_expressible_cover_tier(viewport_width))
}

fn derived_press_edge_height(inputs: &Inputs) -> i32 {
    inputs.stroke(0.8)
}

/// Every value the QML facade republishes as a notifying property. Computed
/// once per input change rather than per read, which is also why the adapter
/// can expose them as plain properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub effective_height: i32,
    pub resolution_height: i32,
    pub tier: Tier,
    pub handheld_profile: bool,

    pub radius_md: i32,
    pub radius_sm: i32,
    pub corner_antialiasing: bool,

    pub font_hero: i32,
    pub font_title: i32,
    pub font_section: i32,
    pub font_body: i32,
    pub font_caption: i32,
    pub font_small: i32,

    pub card_border_width: i32,
    pub focus_border_width: i32,
    pub focus_ring_width: i32,
    pub press_edge_height: i32,

    pub help_bar_height: i32,
    pub help_bar_clearance: i32,
    pub visible_covers: i32,

    pub hub_grid_columns: i32,
    pub hub_grid_rows: i32,
    pub hub_active_label_height: i32,
    pub hub_grid_top_margin: i32,
    pub hub_grid_height_budget: i32,
    pub hub_grid_side_inset: i32,
    pub hub_grid_column_gap: i32,
    pub hub_grid_top_inset: i32,
    pub hub_grid_bottom_inset: i32,
    pub hub_grid_row_gap: i32,
    pub hub_grid_width_fit: i32,
    pub hub_grid_height_fit: i32,
    pub hub_tile_size: i32,
    pub hub_tile_width: i32,
    pub hub_tile_height: i32,

    pub systems_grid_columns: i32,
    pub systems_grid_rows: i32,
    pub games_grid_columns: i32,
    pub games_grid_rows: i32,

    pub header_row_height: i32,
    pub header_stack_gap: i32,
    pub header_top_margin: i32,
    pub header_side_margin: i32,
    pub header_height: i32,
    pub header_bottom: i32,
}

/// The Hub's resolved page geometry: its fixed per-tier shape plus the insets,
/// gaps and cell fits derived from it. Settings reuses these to render a Hub
/// preview without a live `HubScreen`, which is why they are resolved here
/// rather than inside the screen.
#[derive(Debug, Clone, Copy)]
struct HubMetrics {
    columns: i32,
    rows: i32,
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
    tile_width: i32,
    tile_height: i32,
}

fn derive_hub(inputs: &Inputs, header_bottom: i32, help_bar_height: i32) -> HubMetrics {
    let is_240 = inputs.tier() == Tier::T240;
    let header_side_margin = inputs.pct_w(2.0);
    let shape = hub_grid_shape(inputs);
    let hub_active_label_height = if is_240 { 8 } else { inputs.pct_h(7.0) };
    let hub_grid_top_margin = if is_240 { inputs.pct_h(1.0) } else { 0 };
    // Kept in f64 to the end: the QML does this arithmetic in JavaScript
    // numbers and only truncates when the result lands in an `int` property.
    let hub_reserved = f64::from(header_bottom + help_bar_height + hub_active_label_height)
        + if is_240 {
            f64::from(hub_grid_top_margin)
        } else {
            f64::from(3 * inputs.pct_h(2.0))
        };
    let hub_grid_height_budget = (inputs.screen_height - hub_reserved).max(0.0) as i32;
    let hub_grid_side_inset = if is_240 {
        header_side_margin
    } else {
        inputs.pct_w(3.0)
    };
    let hub_grid_column_gap = if is_240 { 4 } else { inputs.pct_w(2.0) };
    let hub_grid_top_inset = if is_240 { 2 } else { inputs.pct_h(2.0) };
    let hub_grid_bottom_inset = if is_240 { 4 } else { inputs.pct_h(2.0) };
    let hub_grid_row_gap = if is_240 { 4 } else { inputs.pct_h(4.0) };

    let hub_grid_width_fit = ((inputs.screen_width
        - f64::from(2 * hub_grid_side_inset + (shape.columns - 1) * hub_grid_column_gap))
        / f64::from(shape.columns))
    .floor()
    .max(0.0) as i32;
    let hub_grid_height_fit = (f64::from(
        hub_grid_height_budget
            - hub_grid_top_inset
            - hub_grid_bottom_inset
            - (shape.rows - 1) * hub_grid_row_gap,
    ) / f64::from(shape.rows))
    .floor()
    .max(0.0) as i32;
    let hub_tile_size = hub_grid_width_fit.min(hub_grid_height_fit);

    HubMetrics {
        columns: shape.columns,
        rows: shape.rows,
        hub_active_label_height,
        hub_grid_top_margin,
        hub_grid_height_budget,
        hub_grid_side_inset,
        hub_grid_column_gap,
        hub_grid_top_inset,
        hub_grid_bottom_inset,
        hub_grid_row_gap,
        hub_grid_width_fit,
        hub_grid_height_fit,
        hub_tile_size,
        // Four-column low-resolution Hub pages are width-bound, so let tile
        // height use its independent fit and keep the gaps balanced. Larger
        // tiers keep the established square tile.
        tile_width: if is_240 {
            hub_grid_width_fit
        } else {
            hub_tile_size
        },
        tile_height: if is_240 {
            hub_grid_height_fit
        } else {
            hub_tile_size
        },
    }
}

/// Resolve every derived value for one set of inputs.
pub fn derive(inputs: &Inputs) -> Derived {
    let tier = inputs.tier();
    let is_240 = tier == Tier::T240;

    // Header first: the Hub's vertical budget is measured from `header_bottom`.
    let header_row_height = inputs.font_size(3.4);
    let header_stack_gap = inputs.pct_h(0.8);
    let header_top_margin = inputs.pct_h(2.0);
    let header_side_margin = inputs.pct_w(2.0);
    let header_height = 2 * header_row_height + header_stack_gap;
    let header_bottom = header_top_margin + header_height;

    // Compact screens may need two atomic icon-plus-label help groups, so 240p
    // reserves two rows; larger tiers keep the single-line footer.
    let help_bar_height = if is_240 {
        inputs.pct_h(10.0)
    } else {
        inputs.pct_h(6.0)
    };

    let radius_md = tier.radius_md();

    let hub = derive_hub(inputs, header_bottom, help_bar_height);

    let systems_shape = systems_grid_shape(inputs, inputs.screen_width, inputs.screen_height);
    let games_shape = games_grid_shape(inputs, inputs.screen_width, inputs.screen_height);

    Derived {
        effective_height: inputs.effective_height(),
        resolution_height: inputs.resolution_height(),
        tier,
        handheld_profile: inputs.handheld(),

        radius_md,
        radius_sm: js_round(f64::from(radius_md) / 2.0).max(1.0) as i32,
        corner_antialiasing: tier.corner_antialiasing(),

        font_hero: inputs.font_role(0, 4.0),
        font_title: inputs.font_role(1, 3.2),
        font_section: inputs.font_role(2, 2.9),
        font_body: inputs.font_role(3, 2.6),
        font_caption: inputs.font_role(4, 2.4),
        font_small: inputs.font_role(5, 2.2),

        card_border_width: inputs.stroke(0.2),
        focus_border_width: inputs.stroke(0.4),
        focus_ring_width: inputs.stroke(0.6),
        press_edge_height: derived_press_edge_height(inputs),

        help_bar_height,
        help_bar_clearance: help_bar_height + inputs.pct_h(2.0),
        // Fewer visible covers at very low resolution, to avoid crowding.
        visible_covers: if inputs.effective_height() < 300 {
            3
        } else {
            5
        },

        hub_grid_columns: hub.columns,
        hub_grid_rows: hub.rows,
        hub_active_label_height: hub.hub_active_label_height,
        hub_grid_top_margin: hub.hub_grid_top_margin,
        hub_grid_height_budget: hub.hub_grid_height_budget,
        hub_grid_side_inset: hub.hub_grid_side_inset,
        hub_grid_column_gap: hub.hub_grid_column_gap,
        hub_grid_top_inset: hub.hub_grid_top_inset,
        hub_grid_bottom_inset: hub.hub_grid_bottom_inset,
        hub_grid_row_gap: hub.hub_grid_row_gap,
        hub_grid_width_fit: hub.hub_grid_width_fit,
        hub_grid_height_fit: hub.hub_grid_height_fit,
        hub_tile_size: hub.hub_tile_size,
        // Four-column low-resolution Hub pages are width-bound, so let tile
        // height use its independent fit and keep the gaps balanced. Larger
        // tiers keep the established square tile.
        hub_tile_width: hub.tile_width,
        hub_tile_height: hub.tile_height,

        systems_grid_columns: systems_shape.columns,
        systems_grid_rows: systems_shape.rows,
        games_grid_columns: games_shape.columns,
        games_grid_rows: games_shape.rows,

        header_row_height,
        header_stack_gap,
        header_top_margin,
        header_side_margin,
        header_height,
        header_bottom,
    }
}
