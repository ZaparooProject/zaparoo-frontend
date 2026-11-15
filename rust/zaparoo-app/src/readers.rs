// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Readers page and the pages inside it: the order reader types are
//! listed in, the three ways a reader type can be set and the driver
//! configuration each one writes, the steps of adding a reader, the
//! option lists for scanning and the launch guard, and the rows each page
//! shows. Every user-visible string stays a key the view translates.

use crate::settings::{Control, Row};

/// The pages, none of them a root tile.
pub const READERS_PAGE: &str = "pageReaders";
pub const DETAIL_PAGE: &str = "pageReaderDetail";
pub const ADD_PAGE: &str = "pageAddReader";
pub const ADD_PORT_PAGE: &str = "pageAddReaderPort";
pub const ADD_OPTIONS_PAGE: &str = "pageAddReaderOptions";
pub const TYPES_PAGE: &str = "pageReaderTypes";
pub const LAUNCH_GUARD_PAGE: &str = "pageLaunchGuard";
pub const MAPPINGS_PAGE: &str = "pageMappings";
pub const INSPECTOR_PAGE: &str = "pageTokenInspector";
/// The systems a token removal never exits, one switch per system.
pub const IGNORED_SYSTEMS_PAGE: &str = "pageIgnoredSystems";

/// Every page this module lays out.
pub const PAGES: &[&str] = &[
    READERS_PAGE,
    DETAIL_PAGE,
    ADD_PAGE,
    ADD_PORT_PAGE,
    ADD_OPTIONS_PAGE,
    TYPES_PAGE,
    LAUNCH_GUARD_PAGE,
    MAPPINGS_PAGE,
    INSPECTOR_PAGE,
    IGNORED_SYSTEMS_PAGE,
];

/// Reader types in the order they are listed: the common NFC readers
/// first, then the other NFC drivers, serial and barcode readers, drives,
/// the software sources, and the display last. A driver Core reports that
/// is not here follows them, by id.
pub const DRIVER_ORDER: &[&str] = &[
    "pn532",
    "ccid",
    "libnfc",
    "libnfcacr122",
    "legacypn532uart",
    "legacypn532i2c",
    "simpleserial",
    "simpleserial_ble",
    "rs232barcode",
    "opticaldrive",
    "externaldrive",
    "file",
    "mqtt",
    "tty2oled",
];

/// Drivers Core ships that are left off the reader type lists: they have
/// no documented name yet.
pub const HIDDEN_DRIVERS: &[&str] = &["libnfc", "simpleserial_ble"];

/// Whether a driver is listed on Add a reader and Reader types.
pub fn listed(id: &str) -> bool {
    !HIDDEN_DRIVERS.contains(&id)
}

/// Where a driver sits in the list: its place in `DRIVER_ORDER`, or after
/// every known one.
fn rank(id: &str) -> usize {
    DRIVER_ORDER
        .iter()
        .position(|known| *known == id)
        .unwrap_or(DRIVER_ORDER.len())
}

/// Sort a driver list into the order it is shown in. A display always
/// goes last, whatever its id.
pub fn sort_drivers<T>(items: &mut [T], id: impl Fn(&T) -> &str, display: impl Fn(&T) -> bool) {
    items.sort_by(|a, b| (display(a), rank(id(a)), id(a)).cmp(&(display(b), rank(id(b)), id(b))));
}

/// How a reader type is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Core never uses it.
    Off,
    /// Core uses it only on a port it was added on.
    Manual,
    /// Core also looks for it on its own.
    Auto,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::Manual, Mode::Auto];

    pub fn token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }

    pub fn parse(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.token() == token)
    }
}

/// What Core says about one driver's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Defaults {
    pub enabled: bool,
    pub auto_detect: bool,
    pub can_auto_detect: bool,
}

/// A driver's configuration as Core stores it: a field that is not set
/// takes the driver's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config {
    pub enabled: Option<bool>,
    pub auto_detect: Option<bool>,
}

/// The mode a driver's defaults give it.
pub fn default_mode(defaults: Defaults) -> Mode {
    mode(Config::default(), defaults)
}

/// The mode a driver is in.
pub fn mode(config: Config, defaults: Defaults) -> Mode {
    if !config.enabled.unwrap_or(defaults.enabled) {
        Mode::Off
    } else if defaults.can_auto_detect && config.auto_detect.unwrap_or(defaults.auto_detect) {
        Mode::Auto
    } else {
        Mode::Manual
    }
}

/// The configuration that puts a driver in `mode`. A field equal to the
/// driver's default is left out, so Core drops it from its config file and
/// a later change of default reaches it.
pub fn config_for(mode: Mode, defaults: Defaults) -> Config {
    let keep = |value: bool, default: bool| (value != default).then_some(value);
    match mode {
        Mode::Off => Config {
            enabled: keep(false, defaults.enabled),
            auto_detect: None,
        },
        Mode::Manual => Config {
            enabled: keep(true, defaults.enabled),
            auto_detect: keep(false, defaults.auto_detect),
        },
        Mode::Auto => Config {
            enabled: keep(true, defaults.enabled),
            auto_detect: keep(true, defaults.auto_detect),
        },
    }
}

/// Whether a mode can be picked for a driver: Auto-detect only where the
/// driver can detect its readers, and only while detection is on at all.
pub fn mode_available(mode: Mode, defaults: Defaults, auto_detect_on: bool) -> bool {
    mode != Mode::Auto || (defaults.can_auto_detect && auto_detect_on)
}

/// What a driver needs to know where its reader is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// A serial port, picked from the ports Core finds.
    Serial,
    /// A device node (a drive), picked the same way.
    Device,
    /// Something typed: an MQTT broker, a file path.
    Text,
    /// Nothing: the driver finds its reader itself.
    None,
}

impl PathKind {
    /// Core's `pathKind`. An unknown kind asks for nothing.
    pub fn parse(token: &str) -> Self {
        match token {
            "serial" => Self::Serial,
            "device" => Self::Device,
            "text" => Self::Text,
            _ => Self::None,
        }
    }
}

/// The step after the reader type is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddStep {
    /// Pick a port from Core's list.
    Port,
    /// Type the address or path on the keyboard.
    Entry,
    /// Go straight to the options and Save.
    Options,
}

pub fn first_step(kind: PathKind) -> AddStep {
    match kind {
        PathKind::Serial | PathKind::Device => AddStep::Port,
        PathKind::Text => AddStep::Entry,
        PathKind::None => AddStep::Options,
    }
}

/// What a typed entry asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The MQTT broker's address.
    Address,
    /// The file the file reader watches.
    Path,
}

pub fn entry_for(driver: &str) -> Entry {
    if driver == "mqtt" {
        Entry::Address
    } else {
        Entry::Path
    }
}

/// Scan modes, as Core names them.
pub const SCAN_MODES: &[&str] = &["tap", "hold"];
/// Seconds a held token's game keeps running after it is removed.
pub const EXIT_DELAYS: &[&str] = &["0", "1", "2", "3", "5", "10"];
/// Seconds a token scanned during play waits for confirmation; -1 waits
/// for ever.
pub const GUARD_TIMEOUTS: &[&str] = &["5", "10", "15", "30", "60", "-1"];
/// Seconds before a confirming scan counts.
pub const GUARD_DELAYS: &[&str] = &["0", "1", "2", "3", "5"];
/// The ID source choices an optical drive offers come from Core; these
/// are the ones it has today, for the view's labels.
pub const ID_SOURCES: &[&str] = &["merged", "uuid", "label"];

/// The canonical token for a number of seconds Core reports: whole
/// numbers without a fraction, anything else as written.
pub fn seconds_token(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e6 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// The seconds a picked token stands for.
pub fn seconds_value(token: &str) -> Option<f32> {
    token.parse().ok()
}

/// The option list of one of this module's picker rows.
pub fn options(id: &str) -> Option<&'static [&'static str]> {
    Some(match id {
        "readersScanMode" => SCAN_MODES,
        "readersScanExitDelay" => EXIT_DELAYS,
        "launchGuardTimeout" => GUARD_TIMEOUTS,
        "launchGuardDelay" => GUARD_DELAYS,
        _ => return None,
    })
}

/// How a reader came to be listed, for its detail page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Core found it on its own.
    #[default]
    Auto,
    /// Someone added it on a port.
    Manual,
}

/// The reader a detail page is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Detail {
    pub source: Source,
    pub has_path: bool,
    pub has_error: bool,
}

/// What the token inspector shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Inspector {
    /// No reader is connected: nothing can be read.
    #[default]
    NoReader,
    /// A reader is connected and nothing has been read since the page
    /// opened.
    Waiting,
    /// A token was read.
    Scanned,
}

/// The read token, as far as the actions care.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about one token"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Token {
    /// Core resolved it to something it would run, and did not hide it.
    pub launchable: bool,
    /// The reader it was read on can write.
    pub writable: bool,
    /// It has an ID a mapping can match.
    pub has_id: bool,
    /// A mapping from Core's database matches it.
    pub database_mapping: bool,
    /// Any mapping matches it.
    pub mapped: bool,
}

/// Everything the Readers pages' rows depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about Core's answers"
)]
pub struct Inputs {
    /// Core answered `settings`: the scanning rows have values to show.
    pub loaded: bool,
    /// Readers Core lists, connected or configured.
    pub entries: usize,
    /// Scan mode is Hold.
    pub hold_mode: bool,
    /// Reader types Core reports.
    pub drivers: usize,
    /// How many of them are displays, sorted last by `sort_drivers`.
    pub display_drivers: usize,
    /// Ports found for the reader being added.
    pub ports: usize,
    /// The reader being added has a port or address.
    pub draft_path: bool,
    /// The reader being added offers a choice of ID source.
    pub id_sources: bool,
    pub detail: Detail,
    /// The launch guard is on.
    pub guard_on: bool,
    /// Scanning the same token again switches games (Core's
    /// `launchGuardRequireConfirm` off).
    pub guard_scan_again: bool,
    /// Core answered `mappings`.
    pub mappings_loaded: bool,
    pub mappings: usize,
    /// Systems Core knows, for the ignored systems page.
    pub systems: usize,
    pub inspector: Inspector,
    pub token: Token,
}

fn field(id: &'static str, control: Control) -> Row {
    Row::Field { id, control }
}

/// The reader types to add, with the displays last under their own
/// header: they show what is playing and never read tokens.
fn add_rows(inputs: &Inputs) -> Vec<Row> {
    let displays = inputs.display_drivers.min(inputs.drivers);
    let readers = inputs.drivers - displays;
    let mut rows: Vec<Row> = items("addReaderType", readers, Control::Navigate).collect();
    if displays > 0 {
        rows.push(Row::Header("displays"));
        rows.extend((readers..inputs.drivers).map(|index| Row::Item {
            id: "addReaderType",
            index,
            control: Control::Navigate,
        }));
    }
    rows
}

fn items(id: &'static str, count: usize, control: Control) -> impl Iterator<Item = Row> {
    (0..count).map(move |index| Row::Item { id, index, control })
}

/// The rows of one of this module's pages, or `None` for any other page.
pub fn page_rows(page: &str, inputs: &Inputs) -> Option<Vec<Row>> {
    Some(match page {
        READERS_PAGE => readers_rows(inputs),
        DETAIL_PAGE => detail_rows(inputs.detail),
        ADD_PAGE => add_rows(inputs),
        ADD_PORT_PAGE => {
            if inputs.ports == 0 {
                vec![field("noPorts", Control::Info)]
            } else {
                items("addReaderPort", inputs.ports, Control::Navigate).collect()
            }
        }
        ADD_OPTIONS_PAGE => {
            let mut rows = vec![field("addReaderDriver", Control::Info)];
            if inputs.draft_path {
                rows.push(field("addReaderPath", Control::Info));
            }
            if inputs.id_sources {
                rows.push(field("addReaderIdSource", Control::Picker));
            }
            rows.push(field("saveReader", Control::Action));
            rows
        }
        TYPES_PAGE => {
            let mut rows = vec![field("readersAutoDetect", Control::Toggle)];
            if inputs.drivers > 0 {
                rows.push(Row::Header("readerTypes"));
                rows.extend(items("readerType", inputs.drivers, Control::Picker));
            }
            rows
        }
        LAUNCH_GUARD_PAGE => {
            let mut rows = vec![field("launchGuardEnabled", Control::Toggle)];
            if inputs.guard_on {
                rows.extend([
                    field("launchGuardTimeout", Control::Picker),
                    field("launchGuardScanAgain", Control::Toggle),
                ]);
                // The wait only matters while a second scan switches.
                if inputs.guard_scan_again {
                    rows.push(field("launchGuardDelay", Control::Picker));
                }
            }
            rows
        }
        MAPPINGS_PAGE => {
            if !inputs.mappings_loaded {
                Vec::new()
            } else if inputs.mappings == 0 {
                vec![field("noMappings", Control::Info)]
            } else {
                items("mapping", inputs.mappings, Control::Device).collect()
            }
        }
        INSPECTOR_PAGE => inspector_rows(inputs),
        IGNORED_SYSTEMS_PAGE => items("ignoredSystem", inputs.systems, Control::Toggle).collect(),
        _ => return None,
    })
}

/// Readers: reading a token leads, then the readers Core has and the ways
/// to add or allow more, then how scanning behaves, then the two pages
/// about what a scan does.
fn readers_rows(inputs: &Inputs) -> Vec<Row> {
    let mut rows = vec![
        field("readToken", Control::Navigate),
        Row::Header("connectedReaders"),
    ];
    if inputs.entries == 0 {
        rows.push(field("noReaders", Control::Info));
    } else {
        rows.extend(items("connectedReader", inputs.entries, Control::Navigate));
    }
    rows.extend([
        field("addReader", Control::Navigate),
        field("readerTypes", Control::Navigate),
    ]);
    if inputs.loaded {
        rows.push(Row::Header("scanning"));
        rows.push(field("readersScanMode", Control::Picker));
        // Exit delay and the systems a removal never exits only mean
        // something when removing the token ends the game.
        if inputs.hold_mode {
            rows.push(field("readersScanExitDelay", Control::Picker));
            rows.push(field("readersScanIgnoreSystems", Control::Navigate));
        }
        rows.extend([
            field("readersScanIgnoreOnConnect", Control::Toggle),
            field("audioScanFeedback", Control::Toggle),
        ]);
    }
    rows.extend([
        Row::Header("duringPlay"),
        field("launchGuard", Control::Navigate),
        field("mappings", Control::Navigate),
    ]);
    rows
}

/// One reader: what it is, where, what it can do, and the one way to stop
/// using it that fits how it was found.
fn detail_rows(detail: Detail) -> Vec<Row> {
    let mut rows = vec![field("readerDriver", Control::Info)];
    if detail.has_path {
        rows.push(field("readerPort", Control::Info));
    }
    rows.extend([
        field("readerWrite", Control::Info),
        field("readerScanMode", Control::Info),
    ]);
    if detail.has_error {
        rows.push(field("readerError", Control::Info));
    }
    rows.push(match detail.source {
        Source::Manual => field("removeReader", Control::Action),
        Source::Auto => field("stopDetecting", Control::Action),
    });
    rows
}

/// The token inspector: the reader line, then what to do or what was
/// read, then what it resolves to and the actions that fit it.
fn inspector_rows(inputs: &Inputs) -> Vec<Row> {
    // With no reader the status says so; a reader count of none would
    // say it twice.
    if inputs.inspector == Inspector::NoReader {
        return vec![field("inspectorState", Control::Info)];
    }
    let mut rows = vec![field("inspectorReaders", Control::Info)];
    if inputs.inspector != Inspector::Scanned {
        rows.push(field("inspectorState", Control::Info));
        return rows;
    }
    rows.extend([
        Row::Header("token"),
        field("tokenTime", Control::Info),
        field("tokenId", Control::Info),
        field("tokenType", Control::Info),
        field("tokenValue", Control::Info),
        field("tokenReader", Control::Info),
        Row::Header("resolvesTo"),
        field("tokenResolves", Control::Info),
    ]);
    let token = inputs.token;
    if token.mapped {
        rows.push(field("tokenMapping", Control::Info));
    }
    if token.launchable {
        rows.push(field("launchToken", Control::Action));
    }
    if token.writable {
        rows.push(field("writeToken", Control::Action));
    }
    if token.has_id && !token.database_mapping {
        rows.push(field("mapToken", Control::Action));
    }
    if token.database_mapping {
        rows.push(field("removeMapping", Control::Action));
    }
    rows
}

/// The page a row on these pages opens.
pub fn opens(id: &str) -> Option<&'static str> {
    Some(match id {
        "readToken" => INSPECTOR_PAGE,
        "connectedReader" => DETAIL_PAGE,
        "addReader" => ADD_PAGE,
        "readerTypes" => TYPES_PAGE,
        "launchGuard" => LAUNCH_GUARD_PAGE,
        "mappings" => MAPPINGS_PAGE,
        "readersScanIgnoreSystems" => IGNORED_SYSTEMS_PAGE,
        _ => return None,
    })
}

/// The page Back returns to, and the row it seats on there.
pub fn parent(page: &str) -> Option<(&'static str, &'static str)> {
    Some(match page {
        DETAIL_PAGE => (READERS_PAGE, "connectedReader"),
        ADD_PAGE => (READERS_PAGE, "addReader"),
        ADD_PORT_PAGE | ADD_OPTIONS_PAGE => (ADD_PAGE, "addReaderType"),
        TYPES_PAGE => (READERS_PAGE, "readerTypes"),
        LAUNCH_GUARD_PAGE => (READERS_PAGE, "launchGuard"),
        MAPPINGS_PAGE => (READERS_PAGE, "mappings"),
        INSPECTOR_PAGE => (READERS_PAGE, "readToken"),
        IGNORED_SYSTEMS_PAGE => (READERS_PAGE, "readersScanIgnoreSystems"),
        _ => return None,
    })
}

/// The inspector's reader line: how many readers are connected, and,
/// when there is one, which driver.
pub fn reader_count_line<'a>(drivers: &[&'a str]) -> (usize, Option<&'a str>) {
    match drivers {
        [one] => (1, Some(one)),
        many => (many.len(), None),
    }
}

/// Add a connection to the list Core keeps, unless the same driver is
/// already on the same path. Answers whether anything changed.
pub fn add_connection<T>(list: &mut Vec<T>, entry: T, key: impl Fn(&T) -> (&str, &str)) -> bool {
    let (driver, path) = key(&entry);
    if list.iter().any(|existing| key(existing) == (driver, path)) {
        return false;
    }
    list.push(entry);
    true
}

/// Take a connection off the list. Answers whether it was there.
pub fn remove_connection<T>(
    list: &mut Vec<T>,
    driver: &str,
    path: &str,
    key: impl Fn(&T) -> (&str, &str),
) -> bool {
    let before = list.len();
    list.retain(|existing| key(existing) != (driver, path));
    list.len() != before
}

/// Turn one system on or off in the ignored list, keeping its order.
pub fn toggle_system(list: &mut Vec<String>, system: &str) {
    if let Some(at) = list.iter().position(|s| s == system) {
        list.remove(at);
    } else {
        list.push(system.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|row| row.id()).collect()
    }

    #[test]
    fn drivers_list_nfc_first_and_the_display_last() {
        let mut list = vec![
            ("tty2oled", true),
            ("zzz", false),
            ("mqtt", false),
            ("pn532", false),
            ("ccid", false),
            ("aaa", false),
            ("simpleserial", false),
        ];
        sort_drivers(&mut list, |d| d.0, |d| d.1);
        let order: Vec<&str> = list.iter().map(|d| d.0).collect();
        assert_eq!(
            order,
            [
                "pn532",
                "ccid",
                "simpleserial",
                "mqtt",
                "aaa",
                "zzz",
                "tty2oled"
            ]
        );
    }

    #[test]
    fn undocumented_drivers_are_not_listed() {
        assert!(!listed("libnfc"));
        assert!(!listed("simpleserial_ble"));
        assert!(listed("pn532"));
        assert!(listed("someday"));
    }

    #[test]
    fn add_a_reader_puts_displays_under_their_own_header() {
        let inputs = Inputs {
            drivers: 3,
            display_drivers: 1,
            ..Inputs::default()
        };
        let rows = page_rows(ADD_PAGE, &inputs).unwrap_or_default();
        let item = |index| Row::Item {
            id: "addReaderType",
            index,
            control: Control::Navigate,
        };
        assert_eq!(rows, [item(0), item(1), Row::Header("displays"), item(2)]);
        let none = Inputs {
            drivers: 2,
            ..Inputs::default()
        };
        assert_eq!(
            page_rows(ADD_PAGE, &none).unwrap_or_default(),
            [item(0), item(1)]
        );
    }

    #[test]
    fn modes_read_the_config_over_the_defaults() {
        let nfc = Defaults {
            enabled: true,
            auto_detect: true,
            can_auto_detect: true,
        };
        assert_eq!(default_mode(nfc), Mode::Auto);
        assert_eq!(
            mode(
                Config {
                    enabled: None,
                    auto_detect: Some(false)
                },
                nfc
            ),
            Mode::Manual
        );
        assert_eq!(
            mode(
                Config {
                    enabled: Some(false),
                    auto_detect: Some(true)
                },
                nfc
            ),
            Mode::Off
        );
        // A driver that cannot detect is never in Auto, whatever is set.
        let serial = Defaults {
            enabled: true,
            auto_detect: true,
            can_auto_detect: false,
        };
        assert_eq!(default_mode(serial), Mode::Manual);
        let display = Defaults {
            enabled: false,
            auto_detect: true,
            can_auto_detect: true,
        };
        assert_eq!(default_mode(display), Mode::Off);
    }

    #[test]
    fn each_mode_writes_only_what_differs_from_the_default() {
        let nfc = Defaults {
            enabled: true,
            auto_detect: true,
            can_auto_detect: true,
        };
        assert_eq!(config_for(Mode::Auto, nfc), Config::default());
        assert_eq!(
            config_for(Mode::Manual, nfc),
            Config {
                enabled: None,
                auto_detect: Some(false)
            }
        );
        assert_eq!(
            config_for(Mode::Off, nfc),
            Config {
                enabled: Some(false),
                auto_detect: None
            }
        );
        let display = Defaults {
            enabled: false,
            auto_detect: true,
            can_auto_detect: true,
        };
        assert_eq!(config_for(Mode::Off, display), Config::default());
        assert_eq!(
            config_for(Mode::Manual, display),
            Config {
                enabled: Some(true),
                auto_detect: Some(false)
            }
        );
        assert_eq!(
            config_for(Mode::Auto, display),
            Config {
                enabled: Some(true),
                auto_detect: None
            }
        );
        // Every mode reads back as itself.
        for defaults in [nfc, display] {
            for m in Mode::ALL {
                assert_eq!(mode(config_for(m, defaults), defaults), m);
            }
        }
    }

    #[test]
    fn auto_detect_is_offered_only_where_it_works() {
        let can = Defaults {
            enabled: true,
            auto_detect: true,
            can_auto_detect: true,
        };
        let cannot = Defaults {
            can_auto_detect: false,
            ..can
        };
        assert!(mode_available(Mode::Auto, can, true));
        assert!(!mode_available(Mode::Auto, can, false));
        assert!(!mode_available(Mode::Auto, cannot, true));
        assert!(mode_available(Mode::Manual, cannot, false));
        assert!(mode_available(Mode::Off, cannot, false));
    }

    #[test]
    fn adding_a_reader_asks_for_what_its_driver_needs() {
        assert_eq!(first_step(PathKind::parse("serial")), AddStep::Port);
        assert_eq!(first_step(PathKind::parse("device")), AddStep::Port);
        assert_eq!(first_step(PathKind::parse("text")), AddStep::Entry);
        assert_eq!(first_step(PathKind::parse("none")), AddStep::Options);
        assert_eq!(first_step(PathKind::parse("new")), AddStep::Options);
        assert_eq!(entry_for("mqtt"), Entry::Address);
        assert_eq!(entry_for("file"), Entry::Path);
    }

    #[test]
    fn hold_mode_shows_its_own_rows() {
        let tap = Inputs {
            loaded: true,
            ..Inputs::default()
        };
        let rows = ids(&page_rows(READERS_PAGE, &tap).unwrap_or_default());
        assert_eq!(
            rows,
            [
                "readToken",
                "connectedReaders",
                "noReaders",
                "addReader",
                "readerTypes",
                "scanning",
                "readersScanMode",
                "readersScanIgnoreOnConnect",
                "audioScanFeedback",
                "duringPlay",
                "launchGuard",
                "mappings",
            ]
        );
        let hold = Inputs {
            hold_mode: true,
            entries: 2,
            ..tap
        };
        let rows = ids(&page_rows(READERS_PAGE, &hold).unwrap_or_default());
        assert_eq!(&rows[2..4], ["connectedReader", "connectedReader"]);
        assert!(rows.contains(&"readersScanExitDelay"));
        assert!(rows.contains(&"readersScanIgnoreSystems"));
        // Before Core answers, nothing shows a value it does not have.
        let cold = ids(&page_rows(READERS_PAGE, &Inputs::default()).unwrap_or_default());
        assert!(!cold.contains(&"readersScanMode"));
    }

    #[test]
    fn a_reader_offers_the_removal_that_fits_how_it_was_found() {
        let manual = Inputs {
            detail: Detail {
                source: Source::Manual,
                has_path: true,
                has_error: true,
            },
            ..Inputs::default()
        };
        let rows = ids(&page_rows(DETAIL_PAGE, &manual).unwrap_or_default());
        assert_eq!(
            rows,
            [
                "readerDriver",
                "readerPort",
                "readerWrite",
                "readerScanMode",
                "readerError",
                "removeReader"
            ]
        );
        let auto = ids(&page_rows(DETAIL_PAGE, &Inputs::default()).unwrap_or_default());
        assert_eq!(auto.last(), Some(&"stopDetecting"));
        assert!(!auto.contains(&"readerPort"));
    }

    #[test]
    fn launch_guard_rows_follow_the_switch() {
        let off = ids(&page_rows(LAUNCH_GUARD_PAGE, &Inputs::default()).unwrap_or_default());
        assert_eq!(off, ["launchGuardEnabled"]);
        let on = Inputs {
            guard_on: true,
            ..Inputs::default()
        };
        let rows = ids(&page_rows(LAUNCH_GUARD_PAGE, &on).unwrap_or_default());
        assert_eq!(rows.last(), Some(&"launchGuardScanAgain"));
        let again = Inputs {
            guard_on: true,
            guard_scan_again: true,
            ..Inputs::default()
        };
        let rows = ids(&page_rows(LAUNCH_GUARD_PAGE, &again).unwrap_or_default());
        assert_eq!(rows.last(), Some(&"launchGuardDelay"));
    }

    #[test]
    fn the_inspector_offers_only_what_fits_the_token() {
        let waiting = Inputs {
            inspector: Inspector::Waiting,
            ..Inputs::default()
        };
        assert_eq!(
            ids(&page_rows(INSPECTOR_PAGE, &waiting).unwrap_or_default()),
            ["inspectorReaders", "inspectorState"]
        );
        let none = Inputs {
            inspector: Inspector::NoReader,
            ..Inputs::default()
        };
        assert_eq!(
            ids(&page_rows(INSPECTOR_PAGE, &none).unwrap_or_default()),
            ["inspectorState"]
        );
        let redacted = Inputs {
            inspector: Inspector::Scanned,
            token: Token {
                has_id: true,
                ..Token::default()
            },
            ..Inputs::default()
        };
        let rows = ids(&page_rows(INSPECTOR_PAGE, &redacted).unwrap_or_default());
        assert!(!rows.contains(&"launchToken"));
        assert!(!rows.contains(&"writeToken"));
        assert!(rows.contains(&"mapToken"));
        let mapped = Inputs {
            inspector: Inspector::Scanned,
            token: Token {
                launchable: true,
                writable: true,
                has_id: true,
                database_mapping: true,
                mapped: true,
            },
            ..Inputs::default()
        };
        let rows = ids(&page_rows(INSPECTOR_PAGE, &mapped).unwrap_or_default());
        assert!(rows.contains(&"tokenMapping"));
        assert!(rows.contains(&"launchToken"));
        assert!(rows.contains(&"writeToken"));
        assert!(rows.contains(&"removeMapping"));
        assert!(!rows.contains(&"mapToken"));
    }

    #[test]
    fn every_page_has_a_parent_and_other_pages_are_not_ours() {
        for page in PAGES {
            assert!(page_rows(page, &Inputs::default()).is_some());
            if *page != READERS_PAGE {
                assert!(parent(page).is_some(), "{page}");
            }
        }
        assert!(page_rows("pageAppearance", &Inputs::default()).is_none());
    }

    #[test]
    fn connections_are_unique_by_driver_and_path() {
        let mut list = vec![("pn532", "/dev/ttyUSB0")];
        assert!(!add_connection(&mut list, ("pn532", "/dev/ttyUSB0"), |e| (
            e.0, e.1
        )));
        assert!(add_connection(&mut list, ("pn532", "/dev/ttyACM0"), |e| (
            e.0, e.1
        )));
        assert!(remove_connection(&mut list, "pn532", "/dev/ttyUSB0", |e| (
            e.0, e.1
        )));
        assert_eq!(list, [("pn532", "/dev/ttyACM0")]);
        assert!(!remove_connection(&mut list, "mqtt", "", |e| (e.0, e.1)));
    }

    #[test]
    fn seconds_round_trip_and_systems_toggle() {
        assert_eq!(seconds_token(15.0), "15");
        assert_eq!(seconds_token(-1.0), "-1");
        assert_eq!(seconds_token(1.5), "1.5");
        assert_eq!(seconds_value("2"), Some(2.0));
        let mut systems = vec!["SNES".to_string()];
        toggle_system(&mut systems, "NES");
        toggle_system(&mut systems, "SNES");
        assert_eq!(systems, ["NES"]);
        assert_eq!(reader_count_line(&["pn532"]), (1, Some("pn532")));
        assert_eq!(reader_count_line(&["pn532", "mqtt"]), (2, None));
    }
}
