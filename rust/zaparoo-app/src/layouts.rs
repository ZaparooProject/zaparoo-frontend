// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Port of `src/ui/theme/BrowseLayouts.qml`: the built-in browse layout
// profiles. The QML kept the tables as string tokens (`"pctH:7"`,
// `"sum(pctH:6,8)"`) resolved by a small interpreter at bind time; here the
// tables are typed values resolved through `sizing::Inputs`, which is what
// the tokens meant. Pinned to `tests/fixtures/layout_golden.txt`, dumped
// from the QML at every geometry the sizing fixture covers, for both themes
// and all six views.
//
// Two themes: `default` for digital tiers and `crt` for the 240p tier, the
// way `BrowseLayouts.currentThemeId` picks them. Six views: the systems and
// games screens as a grid, a list with a detail pane, and the rotated
// (TATE) list. Within a theme the grid views share their tables and the
// list views differ only in whether the detail metadata sits at the bottom
// of the pane.

use crate::sizing::{self, Inputs, Tier};

/// Which table set a screen resolves against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Default,
    Crt,
}

impl ThemeId {
    /// `BrowseLayouts.currentThemeId`: the CRT tables at the 240p tier,
    /// the default tables everywhere else.
    pub fn current(inputs: &Inputs) -> Self {
        if inputs.tier() == Tier::T240 {
            Self::Crt
        } else {
            Self::Default
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "default" => Some(Self::Default),
            "crt" => Some(Self::Crt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    SystemsGrid,
    SystemsList,
    SystemsListTate,
    GamesGrid,
    GamesList,
    GamesListTate,
}

impl View {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "systemsGrid" => Some(Self::SystemsGrid),
            "systemsList" => Some(Self::SystemsList),
            "systemsListTate" => Some(Self::SystemsListTate),
            "gamesGrid" => Some(Self::GamesGrid),
            "gamesList" => Some(Self::GamesList),
            "gamesListTate" => Some(Self::GamesListTate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// A geometry token as the QML tables spell it, resolved against `Inputs`.
#[derive(Debug, Clone, Copy)]
enum Tok {
    Px(i32),
    PctW(f64),
    PctH(f64),
    RadiusMd,
    RadiusSm,
    HeaderSideMargin,
    Sum(&'static [Tok]),
    Min(&'static Tok, &'static Tok),
}

impl Tok {
    fn resolve(self, inputs: &Inputs, derived: &sizing::Derived) -> i32 {
        match self {
            Self::Px(value) => value,
            Self::PctW(percent) => inputs.pct_w(percent),
            Self::PctH(percent) => inputs.pct_h(percent),
            Self::RadiusMd => derived.radius_md,
            Self::RadiusSm => derived.radius_sm,
            Self::HeaderSideMargin => derived.header_side_margin,
            Self::Sum(terms) => terms.iter().map(|t| t.resolve(inputs, derived)).sum(),
            Self::Min(a, b) => a.resolve(inputs, derived).min(b.resolve(inputs, derived)),
        }
    }
}

const fn px(value: i32) -> Tok {
    Tok::Px(value)
}
const fn pw(percent: f64) -> Tok {
    Tok::PctW(percent)
}
const fn ph(percent: f64) -> Tok {
    Tok::PctH(percent)
}

// Unresolved tables.

#[derive(Debug, Clone, Copy)]
struct HeaderT {
    title_in_header: bool,
    hud_bottom_aligned: bool,
    status_pill_pinned_top: bool,
}

#[derive(Debug, Clone, Copy)]
struct StatusT {
    top_strip_visible: bool,
    strip_height: Tok,
    slot_margin: Tok,
    top_margin: Tok,
}

#[derive(Debug, Clone, Copy)]
struct GridT {
    left_inset: Tok,
    right_inset: Tok,
    column_gap: Tok,
    top_inset: Tok,
    bottom_inset: Tok,
    row_gap: Tok,
    page_chevron_size: Tok,
}

#[derive(Debug, Clone, Copy)]
struct GridFooterT {
    page_cue_in_footer: bool,
    active_label_height: Tok,
    active_label_bottom_margin: Tok,
    bottom_status_left_margin: Tok,
    bottom_status_right_margin: Tok,
    grid_bottom_margin: Tok,
    bottom_unsafe_height: Tok,
}

#[derive(Debug, Clone, Copy)]
struct ListT {
    content_axis: Axis,
    list_share: i32,
    detail_share: i32,
    divider_width: i32,
    divider_margin: i32,
    card_side_margin: Tok,
    card_top_margin: Tok,
    card_bottom_margin: Tok,
    card_padding_left: Tok,
    card_padding_right: Tok,
    card_padding_top: Tok,
    card_padding_bottom: Tok,
    row_height: i32,
    row_spacing: Tok,
    center_slot: i32,
    row_text_left_padding: Tok,
    row_text_right_padding: Tok,
    favorite_right_padding: Tok,
    overlay_bottom_margin: Tok,
}

#[derive(Debug, Clone, Copy)]
struct DetailT {
    content_axis: Axis,
    section_gap: Tok,
    image_share: i32,
    metadata_share: i32,
    image_height_ratio_with_title: i32,
    image_reserved_width: i32,
    image_reserved_height: i32,
    image_bottom_margin: i32,
    pane_padding_left: Tok,
    pane_padding_right: Tok,
    pane_padding_top: Tok,
    pane_padding_bottom: Tok,
    image_padding_left: Tok,
    image_padding_right: Tok,
    image_padding_top: Tok,
    image_padding_bottom: Tok,
    metadata_padding_left: Tok,
    metadata_padding_right: Tok,
    metadata_padding_top: Tok,
    metadata_padding_bottom: Tok,
    metadata_top_margin: i32,
    metadata_left_margin: i32,
    metadata_right_margin: i32,
    metadata_height_adjustment: i32,
    /// Only the CRT non-rotated lists cap the label column.
    metadata_label_max_width: Option<i32>,
    title_bottom_margin: Tok,
    tag_row_height: Tok,
    tag_row_spacing: Tok,
}

#[derive(Debug, Clone, Copy)]
struct ThemeT {
    header: HeaderT,
    status: StatusT,
    grid: GridT,
    grid_footer: GridFooterT,
    list: ListT,
    list_tate: ListT,
    detail: DetailT,
    detail_tate: DetailT,
    list_bottom_unsafe_height: Tok,
}

const SURFACE: (Tok, Tok) = (Tok::RadiusMd, Tok::RadiusSm);

const DEFAULT: ThemeT = ThemeT {
    header: HeaderT {
        title_in_header: false,
        hud_bottom_aligned: false,
        status_pill_pinned_top: false,
    },
    status: StatusT {
        top_strip_visible: true,
        strip_height: ph(7.0),
        slot_margin: pw(3.0),
        top_margin: ph(1.0),
    },
    grid: GridT {
        left_inset: pw(2.0),
        right_inset: pw(2.0),
        column_gap: pw(2.0),
        top_inset: ph(2.0),
        bottom_inset: ph(2.0),
        row_gap: ph(3.0),
        page_chevron_size: Tok::Min(&pw(3.0), &ph(4.0)),
    },
    grid_footer: GridFooterT {
        page_cue_in_footer: false,
        active_label_height: ph(7.0),
        active_label_bottom_margin: ph(8.0),
        bottom_status_left_margin: pw(3.0),
        bottom_status_right_margin: pw(3.0),
        grid_bottom_margin: Tok::Sum(&[ph(8.0), ph(7.0)]),
        bottom_unsafe_height: Tok::Sum(&[ph(6.0), ph(2.0)]),
    },
    list: ListT {
        content_axis: Axis::Horizontal,
        list_share: 2,
        detail_share: 1,
        divider_width: 1,
        divider_margin: 0,
        card_side_margin: pw(3.0),
        card_top_margin: ph(2.0),
        card_bottom_margin: ph(8.0),
        card_padding_left: pw(2.0),
        card_padding_right: pw(2.0),
        card_padding_top: ph(2.0),
        card_padding_bottom: ph(2.0),
        row_height: 0,
        row_spacing: ph(0.7),
        center_slot: -1,
        row_text_left_padding: pw(1.6),
        row_text_right_padding: pw(1.6),
        favorite_right_padding: pw(1.6),
        overlay_bottom_margin: ph(15.0),
    },
    list_tate: ListT {
        content_axis: Axis::Vertical,
        list_share: 11,
        detail_share: 5,
        divider_width: 1,
        divider_margin: 0,
        card_side_margin: pw(3.0),
        card_top_margin: ph(2.0),
        card_bottom_margin: ph(8.0),
        card_padding_left: pw(2.0),
        card_padding_right: pw(2.0),
        card_padding_top: ph(2.0),
        card_padding_bottom: ph(2.0),
        row_height: 0,
        row_spacing: ph(0.3),
        center_slot: -1,
        row_text_left_padding: pw(1.6),
        row_text_right_padding: pw(1.6),
        favorite_right_padding: pw(1.6),
        overlay_bottom_margin: ph(15.0),
    },
    detail: DetailT {
        content_axis: Axis::Vertical,
        section_gap: ph(2.0),
        image_share: 2,
        metadata_share: 1,
        image_height_ratio_with_title: 48,
        image_reserved_width: 0,
        image_reserved_height: 0,
        image_bottom_margin: 0,
        pane_padding_left: pw(2.0),
        pane_padding_right: pw(2.0),
        pane_padding_top: ph(2.0),
        pane_padding_bottom: ph(2.0),
        image_padding_left: px(0),
        image_padding_right: px(0),
        image_padding_top: px(0),
        image_padding_bottom: px(0),
        metadata_padding_left: px(0),
        metadata_padding_right: px(0),
        metadata_padding_top: px(0),
        metadata_padding_bottom: px(0),
        metadata_top_margin: 12,
        metadata_left_margin: 0,
        metadata_right_margin: 0,
        metadata_height_adjustment: 0,
        metadata_label_max_width: None,
        title_bottom_margin: ph(2.0),
        tag_row_height: ph(3.0),
        tag_row_spacing: ph(0.55),
    },
    detail_tate: DetailT {
        content_axis: Axis::Horizontal,
        section_gap: pw(3.0),
        image_share: 4,
        metadata_share: 8,
        image_height_ratio_with_title: 48,
        image_reserved_width: 0,
        image_reserved_height: 0,
        image_bottom_margin: 0,
        pane_padding_left: pw(3.0),
        pane_padding_right: pw(3.0),
        pane_padding_top: ph(1.2),
        pane_padding_bottom: ph(1.2),
        image_padding_left: px(0),
        image_padding_right: pw(1.0),
        image_padding_top: ph(0.8),
        image_padding_bottom: ph(0.8),
        metadata_padding_left: pw(1.5),
        metadata_padding_right: pw(0.5),
        metadata_padding_top: ph(0.8),
        metadata_padding_bottom: ph(0.8),
        metadata_top_margin: 12,
        metadata_left_margin: 0,
        metadata_right_margin: 0,
        metadata_height_adjustment: 0,
        metadata_label_max_width: None,
        title_bottom_margin: ph(1.0),
        tag_row_height: ph(2.6),
        tag_row_spacing: ph(0.35),
    },
    list_bottom_unsafe_height: Tok::Sum(&[ph(6.0), ph(2.0)]),
};

const CRT: ThemeT = ThemeT {
    header: HeaderT {
        title_in_header: true,
        hud_bottom_aligned: true,
        status_pill_pinned_top: true,
    },
    status: StatusT {
        top_strip_visible: false,
        strip_height: px(0),
        slot_margin: Tok::HeaderSideMargin,
        top_margin: ph(1.0),
    },
    grid: GridT {
        left_inset: Tok::HeaderSideMargin,
        right_inset: Tok::HeaderSideMargin,
        column_gap: px(4),
        top_inset: px(2),
        bottom_inset: px(4),
        row_gap: px(4),
        page_chevron_size: px(8),
    },
    grid_footer: GridFooterT {
        page_cue_in_footer: true,
        active_label_height: px(8),
        active_label_bottom_margin: ph(6.0),
        bottom_status_left_margin: Tok::HeaderSideMargin,
        bottom_status_right_margin: Tok::HeaderSideMargin,
        grid_bottom_margin: Tok::Sum(&[ph(6.0), px(8)]),
        bottom_unsafe_height: px(16),
    },
    list: ListT {
        content_axis: Axis::Horizontal,
        list_share: 2,
        detail_share: 1,
        divider_width: 1,
        divider_margin: -16,
        card_side_margin: Tok::HeaderSideMargin,
        card_top_margin: px(2),
        card_bottom_margin: Tok::Sum(&[ph(6.0), px(12)]),
        card_padding_left: px(3),
        card_padding_right: px(2),
        card_padding_top: px(3),
        card_padding_bottom: px(2),
        row_height: 12,
        row_spacing: px(0),
        center_slot: 7,
        row_text_left_padding: px(4),
        row_text_right_padding: px(2),
        favorite_right_padding: px(2),
        overlay_bottom_margin: ph(14.0),
    },
    list_tate: ListT {
        content_axis: Axis::Vertical,
        list_share: 11,
        detail_share: 5,
        divider_width: 1,
        divider_margin: 0,
        card_side_margin: Tok::HeaderSideMargin,
        card_top_margin: px(2),
        card_bottom_margin: Tok::Sum(&[ph(6.0), px(12)]),
        card_padding_left: px(3),
        card_padding_right: px(2),
        card_padding_top: px(3),
        card_padding_bottom: px(2),
        row_height: 12,
        row_spacing: px(0),
        center_slot: 7,
        row_text_left_padding: px(4),
        row_text_right_padding: px(2),
        favorite_right_padding: px(2),
        overlay_bottom_margin: ph(14.0),
    },
    detail: DetailT {
        content_axis: Axis::Vertical,
        section_gap: px(2),
        image_share: 2,
        metadata_share: 1,
        image_height_ratio_with_title: 48,
        image_reserved_width: 16,
        image_reserved_height: 0,
        image_bottom_margin: 2,
        pane_padding_left: px(1),
        pane_padding_right: px(1),
        pane_padding_top: px(3),
        pane_padding_bottom: px(2),
        image_padding_left: px(2),
        image_padding_right: px(2),
        image_padding_top: px(0),
        image_padding_bottom: px(2),
        metadata_padding_left: px(2),
        metadata_padding_right: px(1),
        metadata_padding_top: px(0),
        metadata_padding_bottom: px(0),
        metadata_top_margin: 0,
        metadata_left_margin: 2,
        metadata_right_margin: 1,
        metadata_height_adjustment: 2,
        metadata_label_max_width: Some(24),
        title_bottom_margin: px(2),
        tag_row_height: px(9),
        tag_row_spacing: px(0),
    },
    detail_tate: DetailT {
        content_axis: Axis::Horizontal,
        section_gap: px(2),
        image_share: 4,
        metadata_share: 8,
        image_height_ratio_with_title: 48,
        image_reserved_width: 16,
        image_reserved_height: 0,
        image_bottom_margin: 2,
        pane_padding_left: px(2),
        pane_padding_right: px(1),
        pane_padding_top: px(1),
        pane_padding_bottom: px(1),
        image_padding_left: px(1),
        image_padding_right: px(2),
        image_padding_top: px(0),
        image_padding_bottom: px(1),
        metadata_padding_left: px(2),
        metadata_padding_right: px(1),
        metadata_padding_top: px(1),
        metadata_padding_bottom: px(0),
        metadata_top_margin: 12,
        metadata_left_margin: 0,
        metadata_right_margin: 0,
        metadata_height_adjustment: 0,
        metadata_label_max_width: None,
        title_bottom_margin: px(1),
        tag_row_height: px(8),
        tag_row_spacing: px(0),
    },
    list_bottom_unsafe_height: px(16),
};

// Resolved profiles: every length in whole pixels.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub title_in_header: bool,
    pub hud_bottom_aligned: bool,
    pub status_pill_pinned_top: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub top_strip_visible: bool,
    pub strip_height: i32,
    pub slot_margin: i32,
    pub top_margin: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub left_inset: i32,
    pub right_inset: i32,
    pub column_gap: i32,
    pub top_inset: i32,
    pub bottom_inset: i32,
    pub row_gap: i32,
    pub page_chevron_size: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridFooter {
    pub page_cue_in_footer: bool,
    pub active_label_height: i32,
    pub active_label_bottom_margin: i32,
    pub bottom_status_left_margin: i32,
    pub bottom_status_right_margin: i32,
    pub grid_bottom_margin: i32,
    pub bottom_unsafe_height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct List {
    pub content_axis: Axis,
    pub list_share: i32,
    pub detail_share: i32,
    pub divider_width: i32,
    pub divider_margin: i32,
    pub card_side_margin: i32,
    pub card_top_margin: i32,
    pub card_bottom_margin: i32,
    pub card_padding_left: i32,
    pub card_padding_right: i32,
    pub card_padding_top: i32,
    pub card_padding_bottom: i32,
    pub row_height: i32,
    pub row_spacing: i32,
    pub center_slot: i32,
    pub row_text_left_padding: i32,
    pub row_text_right_padding: i32,
    pub favorite_right_padding: i32,
    pub overlay_bottom_margin: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detail {
    pub content_axis: Axis,
    pub section_gap: i32,
    pub image_share: i32,
    pub metadata_share: i32,
    pub image_height_ratio_with_title: i32,
    pub image_reserved_width: i32,
    pub image_reserved_height: i32,
    pub image_bottom_margin: i32,
    pub pane_padding_left: i32,
    pub pane_padding_right: i32,
    pub pane_padding_top: i32,
    pub pane_padding_bottom: i32,
    pub image_padding_left: i32,
    pub image_padding_right: i32,
    pub image_padding_top: i32,
    pub image_padding_bottom: i32,
    pub metadata_padding_left: i32,
    pub metadata_padding_right: i32,
    pub metadata_padding_top: i32,
    pub metadata_padding_bottom: i32,
    pub metadata_top_margin: i32,
    pub metadata_left_margin: i32,
    pub metadata_right_margin: i32,
    pub metadata_height_adjustment: i32,
    pub metadata_bottom_aligned: bool,
    pub metadata_label_max_width: Option<i32>,
    pub title_bottom_margin: i32,
    pub tag_row_height: i32,
    pub tag_row_spacing: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    pub card_radius: i32,
    pub row_radius: i32,
}

/// The body of a profile: a grid, or a list with its detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Grid {
        grid: Grid,
        footer: GridFooter,
    },
    List {
        list: List,
        detail: Detail,
        bottom_unsafe_height: i32,
    },
}

/// A fully resolved layout profile for one view at one geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub header: Header,
    pub status: Status,
    pub body: Body,
    pub surface: Surface,
}

impl Profile {
    /// `footer.bottomUnsafeHeight`, the one key the shell reads for every view.
    pub fn bottom_unsafe_height(&self) -> i32 {
        match self.body {
            Body::Grid { footer, .. } => footer.bottom_unsafe_height,
            Body::List {
                bottom_unsafe_height,
                ..
            } => bottom_unsafe_height,
        }
    }

    /// Every value as `section.key=value` in the QML's own spelling, sorted,
    /// the shape the golden fixture uses.
    pub fn flatten(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        put(
            &mut out,
            "header.hudBottomAligned",
            self.header.hud_bottom_aligned,
        );
        put(
            &mut out,
            "header.statusPillPinnedTop",
            self.header.status_pill_pinned_top,
        );
        put(
            &mut out,
            "header.titleInHeader",
            self.header.title_in_header,
        );
        put(&mut out, "status.slotMargin", self.status.slot_margin);
        put(&mut out, "status.stripHeight", self.status.strip_height);
        put(&mut out, "status.topMargin", self.status.top_margin);
        put(
            &mut out,
            "status.topStripVisible",
            self.status.top_strip_visible,
        );
        put(&mut out, "surface.cardRadius", self.surface.card_radius);
        put(&mut out, "surface.rowRadius", self.surface.row_radius);
        match &self.body {
            Body::Grid { grid, footer } => {
                grid.flatten_into(&mut out);
                footer.flatten_into(&mut out);
            }
            Body::List {
                list,
                detail,
                bottom_unsafe_height,
            } => {
                list.flatten_into(&mut out);
                detail.flatten_into(&mut out);
                put(&mut out, "footer.bottomUnsafeHeight", *bottom_unsafe_height);
            }
        }
        out.sort();
        out
    }
}

fn put(out: &mut Vec<(String, String)>, key: &str, value: impl std::fmt::Display) {
    out.push((key.to_string(), value.to_string()));
}

impl Grid {
    fn flatten_into(&self, out: &mut Vec<(String, String)>) {
        put(out, "grid.bottomInset", self.bottom_inset);
        put(out, "grid.columnGap", self.column_gap);
        put(out, "grid.leftInset", self.left_inset);
        put(out, "grid.pageChevronSize", self.page_chevron_size);
        put(out, "grid.rightInset", self.right_inset);
        put(out, "grid.rowGap", self.row_gap);
        put(out, "grid.topInset", self.top_inset);
    }
}

impl GridFooter {
    fn flatten_into(&self, out: &mut Vec<(String, String)>) {
        put(
            out,
            "footer.activeLabelBottomMargin",
            self.active_label_bottom_margin,
        );
        put(out, "footer.activeLabelHeight", self.active_label_height);
        put(
            out,
            "footer.bottomStatusLeftMargin",
            self.bottom_status_left_margin,
        );
        put(
            out,
            "footer.bottomStatusRightMargin",
            self.bottom_status_right_margin,
        );
        put(out, "footer.bottomUnsafeHeight", self.bottom_unsafe_height);
        put(out, "footer.gridBottomMargin", self.grid_bottom_margin);
        put(out, "footer.pageCueInFooter", self.page_cue_in_footer);
    }
}

impl List {
    fn flatten_into(&self, out: &mut Vec<(String, String)>) {
        put(out, "list.cardBottomMargin", self.card_bottom_margin);
        put(out, "list.cardPaddingBottom", self.card_padding_bottom);
        put(out, "list.cardPaddingLeft", self.card_padding_left);
        put(out, "list.cardPaddingRight", self.card_padding_right);
        put(out, "list.cardPaddingTop", self.card_padding_top);
        put(out, "list.cardSideMargin", self.card_side_margin);
        put(out, "list.cardTopMargin", self.card_top_margin);
        put(out, "list.centerSlot", self.center_slot);
        put(out, "list.contentAxis", self.content_axis.as_str());
        put(out, "list.detailShare", self.detail_share);
        put(out, "list.dividerMargin", self.divider_margin);
        put(out, "list.dividerWidth", self.divider_width);
        put(
            out,
            "list.favoriteRightPadding",
            self.favorite_right_padding,
        );
        put(out, "list.listShare", self.list_share);
        put(out, "list.overlayBottomMargin", self.overlay_bottom_margin);
        put(out, "list.rowHeight", self.row_height);
        put(out, "list.rowSpacing", self.row_spacing);
        put(out, "list.rowTextLeftPadding", self.row_text_left_padding);
        put(out, "list.rowTextRightPadding", self.row_text_right_padding);
    }
}

impl Detail {
    fn flatten_into(&self, out: &mut Vec<(String, String)>) {
        put(out, "detail.contentAxis", self.content_axis.as_str());
        put(out, "detail.imageBottomMargin", self.image_bottom_margin);
        put(
            out,
            "detail.imageHeightRatioWithTitle",
            self.image_height_ratio_with_title,
        );
        put(out, "detail.imagePaddingBottom", self.image_padding_bottom);
        put(out, "detail.imagePaddingLeft", self.image_padding_left);
        put(out, "detail.imagePaddingRight", self.image_padding_right);
        put(out, "detail.imagePaddingTop", self.image_padding_top);
        put(
            out,
            "detail.imageReservedHeight",
            self.image_reserved_height,
        );
        put(out, "detail.imageReservedWidth", self.image_reserved_width);
        put(out, "detail.imageShare", self.image_share);
        put(
            out,
            "detail.metadataBottomAligned",
            self.metadata_bottom_aligned,
        );
        put(
            out,
            "detail.metadataHeightAdjustment",
            self.metadata_height_adjustment,
        );
        if let Some(width) = self.metadata_label_max_width {
            put(out, "detail.metadataLabelMaxWidth", width);
        }
        put(out, "detail.metadataLeftMargin", self.metadata_left_margin);
        put(
            out,
            "detail.metadataPaddingBottom",
            self.metadata_padding_bottom,
        );
        put(
            out,
            "detail.metadataPaddingLeft",
            self.metadata_padding_left,
        );
        put(
            out,
            "detail.metadataPaddingRight",
            self.metadata_padding_right,
        );
        put(out, "detail.metadataPaddingTop", self.metadata_padding_top);
        put(
            out,
            "detail.metadataRightMargin",
            self.metadata_right_margin,
        );
        put(out, "detail.metadataShare", self.metadata_share);
        put(out, "detail.metadataTopMargin", self.metadata_top_margin);
        put(out, "detail.panePaddingBottom", self.pane_padding_bottom);
        put(out, "detail.panePaddingLeft", self.pane_padding_left);
        put(out, "detail.panePaddingRight", self.pane_padding_right);
        put(out, "detail.panePaddingTop", self.pane_padding_top);
        put(out, "detail.sectionGap", self.section_gap);
        put(out, "detail.tagRowHeight", self.tag_row_height);
        put(out, "detail.tagRowSpacing", self.tag_row_spacing);
        put(out, "detail.titleBottomMargin", self.title_bottom_margin);
    }
}

fn theme_tables(theme: ThemeId) -> &'static ThemeT {
    match theme {
        ThemeId::Default => &DEFAULT,
        ThemeId::Crt => &CRT,
    }
}

/// A token resolver bound to one geometry.
struct Resolver<'a> {
    inputs: &'a Inputs,
    derived: sizing::Derived,
}

impl Resolver<'_> {
    fn px(&self, tok: Tok) -> i32 {
        tok.resolve(self.inputs, &self.derived)
    }

    fn grid(&self, t: &GridT) -> Grid {
        Grid {
            left_inset: self.px(t.left_inset),
            right_inset: self.px(t.right_inset),
            column_gap: self.px(t.column_gap),
            top_inset: self.px(t.top_inset),
            bottom_inset: self.px(t.bottom_inset),
            row_gap: self.px(t.row_gap),
            page_chevron_size: self.px(t.page_chevron_size),
        }
    }

    fn grid_footer(&self, t: &GridFooterT) -> GridFooter {
        GridFooter {
            page_cue_in_footer: t.page_cue_in_footer,
            active_label_height: self.px(t.active_label_height),
            active_label_bottom_margin: self.px(t.active_label_bottom_margin),
            bottom_status_left_margin: self.px(t.bottom_status_left_margin),
            bottom_status_right_margin: self.px(t.bottom_status_right_margin),
            grid_bottom_margin: self.px(t.grid_bottom_margin),
            bottom_unsafe_height: self.px(t.bottom_unsafe_height),
        }
    }

    fn list(&self, t: &ListT) -> List {
        List {
            content_axis: t.content_axis,
            list_share: t.list_share,
            detail_share: t.detail_share,
            divider_width: t.divider_width,
            divider_margin: t.divider_margin,
            card_side_margin: self.px(t.card_side_margin),
            card_top_margin: self.px(t.card_top_margin),
            card_bottom_margin: self.px(t.card_bottom_margin),
            card_padding_left: self.px(t.card_padding_left),
            card_padding_right: self.px(t.card_padding_right),
            card_padding_top: self.px(t.card_padding_top),
            card_padding_bottom: self.px(t.card_padding_bottom),
            row_height: t.row_height,
            row_spacing: self.px(t.row_spacing),
            center_slot: t.center_slot,
            row_text_left_padding: self.px(t.row_text_left_padding),
            row_text_right_padding: self.px(t.row_text_right_padding),
            favorite_right_padding: self.px(t.favorite_right_padding),
            overlay_bottom_margin: self.px(t.overlay_bottom_margin),
        }
    }

    fn detail(&self, t: &DetailT, metadata_bottom_aligned: bool) -> Detail {
        Detail {
            content_axis: t.content_axis,
            section_gap: self.px(t.section_gap),
            image_share: t.image_share,
            metadata_share: t.metadata_share,
            image_height_ratio_with_title: t.image_height_ratio_with_title,
            image_reserved_width: t.image_reserved_width,
            image_reserved_height: t.image_reserved_height,
            image_bottom_margin: t.image_bottom_margin,
            pane_padding_left: self.px(t.pane_padding_left),
            pane_padding_right: self.px(t.pane_padding_right),
            pane_padding_top: self.px(t.pane_padding_top),
            pane_padding_bottom: self.px(t.pane_padding_bottom),
            image_padding_left: self.px(t.image_padding_left),
            image_padding_right: self.px(t.image_padding_right),
            image_padding_top: self.px(t.image_padding_top),
            image_padding_bottom: self.px(t.image_padding_bottom),
            metadata_padding_left: self.px(t.metadata_padding_left),
            metadata_padding_right: self.px(t.metadata_padding_right),
            metadata_padding_top: self.px(t.metadata_padding_top),
            metadata_padding_bottom: self.px(t.metadata_padding_bottom),
            metadata_top_margin: t.metadata_top_margin,
            metadata_left_margin: t.metadata_left_margin,
            metadata_right_margin: t.metadata_right_margin,
            metadata_height_adjustment: t.metadata_height_adjustment,
            metadata_bottom_aligned,
            metadata_label_max_width: t.metadata_label_max_width,
            title_bottom_margin: self.px(t.title_bottom_margin),
            tag_row_height: self.px(t.tag_row_height),
            tag_row_spacing: self.px(t.tag_row_spacing),
        }
    }
}

/// `BrowseLayouts.themeProfile(themeId, viewId)` resolved at `inputs`.
pub fn profile(theme: ThemeId, view: View, inputs: &Inputs) -> Profile {
    let t = theme_tables(theme);
    let r = Resolver {
        inputs,
        derived: sizing::derive(inputs),
    };
    let header = Header {
        title_in_header: t.header.title_in_header,
        hud_bottom_aligned: t.header.hud_bottom_aligned,
        status_pill_pinned_top: t.header.status_pill_pinned_top,
    };
    let status = Status {
        top_strip_visible: t.status.top_strip_visible,
        strip_height: r.px(t.status.strip_height),
        slot_margin: r.px(t.status.slot_margin),
        top_margin: r.px(t.status.top_margin),
    };
    let surface = Surface {
        card_radius: r.px(SURFACE.0),
        row_radius: r.px(SURFACE.1),
    };
    let body = match view {
        View::SystemsGrid | View::GamesGrid => Body::Grid {
            grid: r.grid(&t.grid),
            footer: r.grid_footer(&t.grid_footer),
        },
        View::SystemsList | View::GamesList | View::SystemsListTate | View::GamesListTate => {
            let tate = matches!(view, View::SystemsListTate | View::GamesListTate);
            let (l, d) = if tate {
                (&t.list_tate, &t.detail_tate)
            } else {
                (&t.list, &t.detail)
            };
            // The games list pins its metadata to the bottom of the pane; the
            // systems list and both rotated lists do not.
            Body::List {
                list: r.list(l),
                detail: r.detail(d, view == View::GamesList),
                bottom_unsafe_height: r.px(t.list_bottom_unsafe_height),
            }
        }
    };
    Profile {
        header,
        status,
        body,
        surface,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crt_theme_only_at_the_240p_tier() {
        let hd = Inputs {
            screen_width: 1280.0,
            screen_height: 720.0,
            ..Inputs::default()
        };
        assert_eq!(ThemeId::current(&hd), ThemeId::Default);
        let crt = Inputs {
            screen_width: 352.0,
            screen_height: 240.0,
            ..Inputs::default()
        };
        assert_eq!(ThemeId::current(&crt), ThemeId::Crt);
    }

    #[test]
    fn games_list_pins_metadata_to_the_bottom() {
        let inputs = Inputs::default();
        let games = profile(ThemeId::Default, View::GamesList, &inputs);
        let systems = profile(ThemeId::Default, View::SystemsList, &inputs);
        let aligned = |p: &Profile| match p.body {
            Body::List { detail, .. } => detail.metadata_bottom_aligned,
            Body::Grid { .. } => unreachable!(),
        };
        assert!(aligned(&games));
        assert!(!aligned(&systems));
    }
}
