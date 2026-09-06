// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Settings registry: the six root categories, the rows each page
//! shows at the current runtime, what control each row carries, the
//! canonical option lists, and the cursor rules over rows that include
//! non-navigable headers. Ported from `SettingsScreen.qml` and
//! `models/settings.rs`. Every user-visible string stays a key the view
//! translates.

/// The control a row carries (`_fieldControl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// A value the user cycles or picks from a list.
    Picker,
    /// An on/off switch.
    Toggle,
    /// A one-shot the row performs (its label says what).
    Action,
    /// Opens another page or screen.
    Navigate,
}

/// A row on a settings page: a group header, or a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A non-navigable group label; the id is its vocabulary key.
    Header(&'static str),
    Field {
        id: &'static str,
        control: Control,
    },
}

impl Row {
    pub fn id(self) -> &'static str {
        match self {
            Self::Header(id) | Self::Field { id, .. } => id,
        }
    }

    pub fn is_field(self) -> bool {
        matches!(self, Self::Field { .. })
    }
}

/// One root category tile: the page it opens and its glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub id: &'static str,
    pub glyph: &'static str,
}

/// The root grid, in the order the console-style menu shows it:
/// Appearance is how it looks, Library the collection and its
/// maintenance, Display the video output, Controls the input, Language
/// the locale, About the identity and diagnostics.
pub const PAGES: &[Page] = &[
    Page {
        id: "pageAppearance",
        glyph: "icons/Appearance",
    },
    Page {
        id: "pageLibraryData",
        glyph: "icons/Library",
    },
    Page {
        id: "pageDisplayInterface",
        glyph: "icons/Display",
    },
    Page {
        id: "pageControlsInput",
        glyph: "icons/Controls",
    },
    Page {
        id: "pageLanguage",
        glyph: "icons/Language",
    },
    Page {
        id: "pageSupportAbout",
        glyph: "icons/Support",
    },
];

/// What the registry needs to know about the machine it runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Inputs {
    /// `MiSTer`: the resolution and analog-video rows only exist there.
    pub is_mister: bool,
    /// The frontend is already running the native CRT path.
    pub crt_enabled: bool,
    /// A debug build offers the one-second screensaver for testing.
    pub debug_build: bool,
}

const TOGGLES: &[&str] = &[
    "mouseEnabled",
    "showHidden",
    "showOriginalFilenames",
    "debugLogging",
    "reduceMotion",
    "crtEnabled",
    "swapConfirmCancel",
    "swapOptionsView",
];

const NAVIGATES: &[&str] = &[
    "aboutLicense",
    "documentation",
    "crtCalibration",
    "pageAppearance",
    "pageDisplayInterface",
    "pageLanguage",
    "pageControlsInput",
    "pageLibraryData",
    "pageSupportAbout",
];

const ACTIONS: &[&str] = &["updateMediaDb", "runScraper", "uploadLog"];

/// `_fieldControl`: everything that is not a toggle, a navigation or a
/// one-shot is a picker.
pub fn control(id: &str) -> Control {
    if TOGGLES.contains(&id) {
        Control::Toggle
    } else if NAVIGATES.contains(&id) {
        Control::Navigate
    } else if ACTIONS.contains(&id) {
        Control::Action
    } else {
        Control::Picker
    }
}

fn field(id: &'static str) -> Row {
    Row::Field {
        id,
        control: control(id),
    }
}

/// The rows of one page; the empty page id is the root category grid.
pub fn page_rows(page: &str, inputs: &Inputs) -> Vec<Row> {
    match page {
        "" => PAGES.iter().map(|p| field(p.id)).collect(),
        "pageAppearance" => vec![
            field("colorScheme"),
            field("colorIntensity"),
            field("systemLogoStyle"),
            field("reduceMotion"),
            field("screensaverTimeout"),
        ],
        "pageDisplayInterface" => {
            let mut rows = Vec::new();
            // The interface resolution is the HDMI path's own; under the
            // CRT path the video standard sets it instead.
            if inputs.is_mister && !inputs.crt_enabled {
                rows.push(field("resolution"));
            }
            rows.push(field("interfaceProfile"));
            rows.push(field("orientation"));
            if inputs.is_mister {
                // The toggle is always offered so an HDMI user can switch
                // in; the standard and the calibration pattern only mean
                // something once the frontend runs with --crt.
                rows.push(Row::Header("analogVideo"));
                rows.push(field("crtEnabled"));
                if inputs.crt_enabled {
                    rows.push(field("crtVideoStandard"));
                    rows.push(field("crtCalibration"));
                }
            }
            rows
        }
        "pageLanguage" => vec![field("language"), field("region"), field("clockFormat")],
        "pageControlsInput" => vec![
            field("buttonLayout"),
            field("swapConfirmCancel"),
            field("swapOptionsView"),
            field("mouseEnabled"),
        ],
        // Maintenance leads: these are rows a user comes here to do, not
        // one-time preferences.
        "pageLibraryData" => vec![
            Row::Header("maintenance"),
            field("updateMediaDb"),
            field("runScraper"),
            Row::Header("browsing"),
            field("systemsLayout"),
            field("gamesLayout"),
            field("mediaImageType"),
            field("showHidden"),
            field("showOriginalFilenames"),
        ],
        "pageSupportAbout" => vec![
            field("aboutLicense"),
            field("documentation"),
            field("debugLogging"),
            field("uploadLog"),
        ],
        _ => Vec::new(),
    }
}

/// The page a row opens, for the navigation rows that are category tiles.
pub fn opens_page(id: &str) -> Option<&'static str> {
    PAGES.iter().find(|p| p.id == id).map(|p| p.id)
}

pub const LANGUAGES: &[&str] = &[
    "auto", "ar", "eu", "zh_CN", "zh_TW", "nl", "en", "fr", "de", "el", "he", "hi", "it_IT", "ja",
    "ko", "ro", "sk", "es", "uk",
];
pub const CLOCK_FORMATS: &[&str] = &["auto", "12h", "24h"];
pub const REGIONS: &[&str] = &["auto", "us", "eu", "jp"];
pub const INTERFACE_PROFILES: &[&str] = &["device", "standard", "handheld"];
pub const ORIENTATIONS: &[&str] = &["horizontal", "cw", "ccw"];
pub const BROWSE_LAYOUTS: &[&str] = &["grid", "list"];
pub const FAVORITES_GROUPINGS: &[&str] = &["none", "system"];
pub const SYSTEM_LOGO_STYLES: &[&str] = &["tinted", "color"];
pub const COLOR_INTENSITIES: &[&str] = &["subtle", "vivid"];
pub const BUTTON_LAYOUTS: &[&str] = &[
    "auto", "style_a", "style_b", "style_c", "style_d", "style_e",
];
pub const SCREENSAVER_TIMEOUTS: &[&str] = &["off", "60", "120", "300", "600", "900", "1800"];
pub const MEDIA_IMAGE_TYPES: &[&str] = &[
    "auto",
    "image",
    "thumbnail",
    "boxart",
    "boxart3d",
    "screenshot",
    "wheel",
    "titleshot",
    "map",
    "marquee",
    "fanart",
    "boxartside",
    "boxartback",
];
pub const CRT_VIDEO_STANDARDS: &[&str] = &["ntsc", "pal"];

/// The option list of a picker row, when it is a fixed one. Resolutions
/// and color schemes come from the host (the output modes it detected,
/// and the palette table), so they answer `None`.
pub fn options(id: &str, inputs: &Inputs) -> Option<Vec<&'static str>> {
    let fixed: &[&'static str] = match id {
        "language" => LANGUAGES,
        "clockFormat" => CLOCK_FORMATS,
        "region" => REGIONS,
        "interfaceProfile" => INTERFACE_PROFILES,
        "orientation" => ORIENTATIONS,
        "systemsLayout" | "gamesLayout" => BROWSE_LAYOUTS,
        "favoritesGrouping" => FAVORITES_GROUPINGS,
        "systemLogoStyle" => SYSTEM_LOGO_STYLES,
        "colorIntensity" => COLOR_INTENSITIES,
        "buttonLayout" => BUTTON_LAYOUTS,
        "mediaImageType" => MEDIA_IMAGE_TYPES,
        "crtVideoStandard" => CRT_VIDEO_STANDARDS,
        "screensaverTimeout" => {
            let mut values = SCREENSAVER_TIMEOUTS.to_vec();
            // A debug build can watch the screensaver arm immediately.
            if inputs.debug_build {
                values.insert(1, "1");
            }
            return Some(values);
        }
        _ => return None,
    };
    Some(fixed.to_vec())
}

/// The label an action row's control shows (`focusedActionLabel`), as a
/// vocabulary key.
pub fn action_label_key(id: &str, busy: bool) -> &'static str {
    match id {
        "updateMediaDb" | "runScraper" => {
            if busy {
                "cancel"
            } else {
                "configure"
            }
        }
        "uploadLog" => "upload",
        _ => "open",
    }
}

/// A media job the row would start is already running.
pub fn action_busy(id: &str, index_busy: bool, scrape_busy: bool) -> bool {
    match id {
        "updateMediaDb" => index_busy,
        "runScraper" => scrape_busy,
        _ => false,
    }
}

/// The other media job is running, so this row cannot start its own.
pub fn action_disabled(id: &str, index_busy: bool, scrape_busy: bool) -> bool {
    match id {
        "updateMediaDb" => scrape_busy,
        "runScraper" => index_busy,
        _ => false,
    }
}

/// Rows whose change only takes effect after the frontend restarts; the
/// view says so in their description and the host stages a confirm.
pub fn restarts(id: &str) -> bool {
    matches!(
        id,
        "resolution" | "crtEnabled" | "crtVideoStandard" | "language" | "debugLogging"
    )
}

/// The first row a cursor may sit on (`_firstNavigableIndex`).
pub fn first_navigable(rows: &[Row]) -> usize {
    rows.iter().position(|r| r.is_field()).unwrap_or(0)
}

/// The next navigable row in `dir`, wrapping and skipping headers
/// (`_seekNavigable`).
pub fn seek_navigable(rows: &[Row], from: usize, dir: i64) -> usize {
    let len = rows.len();
    if len == 0 {
        return from;
    }
    let mut i = from as i64;
    for _ in 0..len {
        i += dir;
        if i < 0 {
            i = len as i64 - 1;
        } else if i >= len as i64 {
            i = 0;
        }
        if rows[i as usize].is_field() {
            return i as usize;
        }
    }
    from
}

/// The root category grid: two rows of three, transposed when the scene
/// is rotated.
pub fn root_grid_shape(count: usize, rotated: bool) -> (usize, usize) {
    let rows = 2;
    let columns = count.div_ceil(rows).max(1);
    if rotated {
        (rows, columns)
    } else {
        (columns, rows)
    }
}

/// A cardinal move on the root grid: horizontal wraps inside its row,
/// vertical wraps around the column (`_moveRootGrid`).
pub fn root_grid_move(index: usize, count: usize, columns: usize, dx: i64, dy: i64) -> usize {
    if count == 0 {
        return index;
    }
    let columns = columns.max(1);
    let row = index / columns;
    let col = index % columns;
    if dx != 0 {
        let row_start = row * columns;
        let row_end = (row_start + columns - 1).min(count - 1);
        let next = index as i64 + dx;
        return if next < row_start as i64 {
            row_end
        } else if next > row_end as i64 {
            row_start
        } else {
            next as usize
        };
    }
    if dy != 0 {
        let next = index as i64 + dy * columns as i64;
        return if next < 0 {
            let last_row = (count - 1) / columns;
            (last_row * columns + col).min(count - 1)
        } else if next >= count as i64 {
            col.min(count - 1)
        } else {
            next as usize
        };
    }
    index
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a missing option list is a test failure either way"
)]
mod tests {
    use super::*;

    fn mister(crt: bool) -> Inputs {
        Inputs {
            is_mister: true,
            crt_enabled: crt,
            debug_build: false,
        }
    }

    #[test]
    fn field_control_covers_every_row_kind() {
        assert_eq!(control("showHidden"), Control::Toggle);
        assert_eq!(control("reduceMotion"), Control::Toggle);
        assert_eq!(control("aboutLicense"), Control::Navigate);
        assert_eq!(control("documentation"), Control::Navigate);
        assert_eq!(control("crtCalibration"), Control::Navigate);
        assert_eq!(control("pageDisplayInterface"), Control::Navigate);
        assert_eq!(control("updateMediaDb"), Control::Action);
        assert_eq!(control("runScraper"), Control::Action);
        assert_eq!(control("uploadLog"), Control::Action);
        assert_eq!(control("resolution"), Control::Picker);
        assert_eq!(control("colorScheme"), Control::Picker);
    }

    #[test]
    fn root_page_lists_the_six_categories() {
        let rows = page_rows("", &Inputs::default());
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|r| r.is_field()));
        assert_eq!(rows[0].id(), "pageAppearance");
        assert_eq!(rows[5].id(), "pageSupportAbout");
        assert_eq!(opens_page("pageLibraryData"), Some("pageLibraryData"));
        assert_eq!(opens_page("colorScheme"), None);
    }

    #[test]
    fn display_page_gates_the_mister_and_crt_rows() {
        let desktop = page_rows("pageDisplayInterface", &Inputs::default());
        let ids: Vec<&str> = desktop.iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec!["interfaceProfile", "orientation"]);

        let hdmi: Vec<&str> = page_rows("pageDisplayInterface", &mister(false))
            .iter()
            .map(|r| r.id())
            .collect();
        assert_eq!(
            hdmi,
            vec![
                "resolution",
                "interfaceProfile",
                "orientation",
                "analogVideo",
                "crtEnabled"
            ]
        );

        let crt: Vec<&str> = page_rows("pageDisplayInterface", &mister(true))
            .iter()
            .map(|r| r.id())
            .collect();
        assert_eq!(
            crt,
            vec![
                "interfaceProfile",
                "orientation",
                "analogVideo",
                "crtEnabled",
                "crtVideoStandard",
                "crtCalibration"
            ]
        );
    }

    #[test]
    fn library_page_leads_with_maintenance() {
        let rows = page_rows("pageLibraryData", &Inputs::default());
        assert_eq!(rows[0], Row::Header("maintenance"));
        assert_eq!(rows[1].id(), "updateMediaDb");
        assert_eq!(rows[3], Row::Header("browsing"));
        assert_eq!(rows[4].id(), "systemsLayout");
        assert_eq!(rows[5].id(), "gamesLayout");
        // Round 10 retired the arcade alternate-versions toggle.
        assert!(!rows
            .iter()
            .any(|r| r.id() == "discoverArcadeAlternateVersions"));
        assert!(!rows.iter().any(|r| r.id() == "browseLayout"));
    }

    #[test]
    fn unknown_page_has_no_rows() {
        assert!(page_rows("pageNope", &Inputs::default()).is_empty());
    }

    #[test]
    fn option_lists_are_the_canonical_ones() {
        assert_eq!(
            options("clockFormat", &Inputs::default()).unwrap(),
            CLOCK_FORMATS
        );
        assert_eq!(
            options("gamesLayout", &Inputs::default()).unwrap(),
            BROWSE_LAYOUTS
        );
        assert_eq!(options("resolution", &Inputs::default()), None);
        assert_eq!(options("colorScheme", &Inputs::default()), None);
        assert_eq!(options("showHidden", &Inputs::default()), None);
    }

    #[test]
    fn a_debug_build_offers_the_one_second_screensaver() {
        let release = options("screensaverTimeout", &Inputs::default()).unwrap();
        assert_eq!(release, SCREENSAVER_TIMEOUTS);
        let debug = options(
            "screensaverTimeout",
            &Inputs {
                debug_build: true,
                ..Inputs::default()
            },
        )
        .unwrap();
        assert_eq!(debug[0], "off");
        assert_eq!(debug[1], "1");
        assert_eq!(debug.len(), SCREENSAVER_TIMEOUTS.len() + 1);
    }

    #[test]
    fn media_jobs_share_their_row_labels_and_gates() {
        assert_eq!(action_label_key("updateMediaDb", false), "configure");
        assert_eq!(action_label_key("updateMediaDb", true), "cancel");
        assert_eq!(action_label_key("runScraper", true), "cancel");
        assert_eq!(action_label_key("uploadLog", false), "upload");
        assert_eq!(action_label_key("aboutLicense", false), "open");
        assert!(action_busy("updateMediaDb", true, false));
        assert!(!action_busy("updateMediaDb", false, true));
        assert!(action_disabled("updateMediaDb", false, true));
        assert!(action_disabled("runScraper", true, false));
        assert!(!action_disabled("uploadLog", true, true));
    }

    #[test]
    fn restart_rows_are_the_documented_ones() {
        for id in [
            "resolution",
            "crtEnabled",
            "crtVideoStandard",
            "language",
            "debugLogging",
        ] {
            assert!(restarts(id), "{id} should restart");
        }
        for id in ["orientation", "colorScheme", "showHidden"] {
            assert!(!restarts(id), "{id} should not restart");
        }
    }

    #[test]
    fn the_cursor_skips_headers_and_wraps() {
        let rows = page_rows("pageLibraryData", &Inputs::default());
        assert_eq!(first_navigable(&rows), 1);
        // Down off the last maintenance row lands past the next header.
        assert_eq!(seek_navigable(&rows, 2, 1), 4);
        // Up off the first field wraps to the last.
        assert_eq!(seek_navigable(&rows, 1, -1), rows.len() - 1);
        assert_eq!(seek_navigable(&rows, rows.len() - 1, 1), 1);
        assert_eq!(seek_navigable(&[], 0, 1), 0);
        assert_eq!(first_navigable(&[Row::Header("x")]), 0);
    }

    #[test]
    fn root_grid_is_two_rows_transposed_when_rotated() {
        assert_eq!(root_grid_shape(6, false), (3, 2));
        assert_eq!(root_grid_shape(6, true), (2, 3));
    }

    #[test]
    fn root_grid_moves_wrap_within_the_row_and_around_the_column() {
        // 6 tiles, 3 columns: 0 1 2 / 3 4 5.
        assert_eq!(root_grid_move(0, 6, 3, 1, 0), 1);
        assert_eq!(root_grid_move(2, 6, 3, 1, 0), 0);
        assert_eq!(root_grid_move(3, 6, 3, -1, 0), 5);
        assert_eq!(root_grid_move(1, 6, 3, 0, 1), 4);
        assert_eq!(root_grid_move(4, 6, 3, 0, 1), 1);
        assert_eq!(root_grid_move(1, 6, 3, 0, -1), 4);
        assert_eq!(root_grid_move(0, 0, 3, 1, 0), 0);
        // A short last row: the column with a tile below steps into it,
        // the one without wraps back to the top of its own column.
        assert_eq!(root_grid_move(0, 5, 3, 0, 1), 3);
        assert_eq!(root_grid_move(2, 5, 3, 0, 1), 2);
    }
}
