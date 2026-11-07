// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Game controllers and Bluetooth pairing, as Core reports them: what a
//! controller card reads, what the header shows of each controller's
//! battery and when it warns about one running flat, how controllers and
//! devices are named, whether this Core can pair one, and the Controllers
//! page's search, its found list and its layout.
//! Core's wire tokens are decoded here, with anything unknown read as
//! unknown rather than guessed at.

/// A controller battery's coarse level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Empty,
    Low,
    Medium,
    Full,
    Unknown,
}

impl Level {
    pub fn from_token(token: &str) -> Self {
        match token {
            "empty" => Self::Empty,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "full" => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// Whether a battery is taking charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl Charge {
    /// Core's `chargeState`; `None` (unknown, or a Core that predates
    /// it) is `Unknown`.
    pub fn from_token(token: Option<&str>) -> Self {
        match token {
            Some("charging") => Self::Charging,
            Some("discharging") => Self::Discharging,
            Some("full") => Self::Full,
            Some("notCharging") => Self::NotCharging,
            _ => Self::Unknown,
        }
    }
}

/// A controller's battery, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: Option<u8>,
    pub level: Level,
    pub charge: Charge,
}

impl Battery {
    pub fn new(percent: Option<i64>, level: &str, charge: Option<&str>) -> Self {
        Self {
            percent: percent.map(|p| u8::try_from(p.clamp(0, 100)).unwrap_or(0)),
            level: Level::from_token(level),
            charge: Charge::from_token(charge),
        }
    }

    /// Running low and not being charged: the header warns about it.
    pub fn is_low(&self) -> bool {
        matches!(self.level, Level::Empty | Level::Low)
            && !matches!(self.charge, Charge::Charging | Charge::Full)
    }
}

/// What a controller row says about its power, on its right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// On a cable, with no battery of its own.
    Wired,
    /// Not on a cable, and its driver reports no battery: nothing to say.
    Unreported,
    /// Taking charge, with the charge so far when Core knows it.
    Charging(Option<u8>),
    /// Charged and still on the cable.
    Charged,
    /// On battery, with a percentage.
    Percent(u8),
    /// On battery, with only a level to go on.
    Level(Level),
}

/// `connection` is Core's for the controller: only a USB one without a
/// battery is wired. A Bluetooth pad whose driver keeps its charge to
/// itself (an Xbox pad over Bluetooth LE) is not.
pub fn reading(battery: Option<&Battery>, connection: &str) -> Reading {
    let Some(battery) = battery else {
        return if connection == "usb" {
            Reading::Wired
        } else {
            Reading::Unreported
        };
    };
    match battery.charge {
        Charge::Charging => Reading::Charging(battery.percent),
        Charge::Full => Reading::Charged,
        Charge::Discharging | Charge::NotCharging | Charge::Unknown => battery
            .percent
            .map_or(Reading::Level(battery.level), Reading::Percent),
    }
}

/// Whether the compact header (240p and the CRT profile) shows its one
/// low controller battery glyph: some controller is low or empty and
/// nothing is charging it.
pub fn any_low<'a>(batteries: impl IntoIterator<Item = &'a Battery>) -> bool {
    batteries.into_iter().any(Battery::is_low)
}

/// How much charge a header pad glyph shows inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    Empty,
    Low,
    Half,
    Full,
}

impl Fill {
    /// The same steps the header's own battery glyph takes.
    fn from_percent(percent: u8) -> Self {
        match percent {
            75.. => Self::Full,
            40..=74 => Self::Half,
            15..=39 => Self::Low,
            _ => Self::Empty,
        }
    }
}

/// One controller's battery in the header, where there is room for one
/// glyph per controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderPad {
    pub fill: Fill,
    /// A charging mark follows the glyph.
    pub charging: bool,
    /// Drawn as a warning: low or empty and not charging.
    pub low: bool,
    /// Charging marks before this glyph, so the view can place it.
    pub marks_before: usize,
}

/// The most controllers the header shows a battery for.
pub const HEADER_PADS: usize = 4;

/// The header's controller batteries, in Core's order: one per controller
/// with a battery whose charge Core can read, up to [`HEADER_PADS`].
/// `None` is a controller without one (wired, or a driver that keeps its
/// charge to itself), which gets no glyph.
pub fn header_pads<'a>(batteries: impl IntoIterator<Item = Option<&'a Battery>>) -> Vec<HeaderPad> {
    let mut marks = 0;
    batteries
        .into_iter()
        .flatten()
        .filter_map(|battery| {
            let fill = Fill::from_percent(bar_percent(battery)?);
            let charging = battery.charge == Charge::Charging;
            let pad = HeaderPad {
                fill,
                charging,
                low: battery.is_low(),
                marks_before: marks,
            };
            marks += usize::from(charging);
            Some(pad)
        })
        .take(HEADER_PADS)
        .collect()
}

/// The value `capabilities.actions` gives an action this client may call.
pub const SUPPORTED: &str = "supported";

/// Whether to offer pairing: Core lets this client pair, and there is an
/// adapter to pair through.
pub fn pairing_available(pair_action: Option<&str>, adapter_present: bool) -> bool {
    pair_action == Some(SUPPORTED) && adapter_present
}

/// Why a pairing attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError {
    /// The device wants a PIN or a passkey, which Core does not enter.
    PinRequired,
    NotPowered,
    Timeout,
    Rejected,
    NotFound,
    Failed,
}

impl PairError {
    /// Core's `pairing.error`; anything else is a plain failure.
    pub fn from_token(token: Option<&str>) -> Self {
        match token {
            Some("pinRequired") => Self::PinRequired,
            Some("notPowered") => Self::NotPowered,
            Some("timeout") => Self::Timeout,
            Some("rejected") => Self::Rejected,
            Some("notFound") => Self::NotFound,
            _ => Self::Failed,
        }
    }

    /// Whether asking again can change the outcome. A controller that wants
    /// a PIN, or an adapter that is off, fails the same way every time.
    pub fn retries(self) -> bool {
        !matches!(self, Self::PinRequired | Self::NotPowered)
    }

    /// A refused `device.bluetooth.*` request, by Core's
    /// `error.data.category`: `unavailable` is an adapter that is off.
    pub fn from_request_error(category: Option<&str>) -> Self {
        if category == Some("unavailable") {
            Self::NotPowered
        } else {
            Self::Failed
        }
    }
}

/// How full a battery bar is drawn, in percent: the reading when Core
/// has one, otherwise a fixed share for the level, so a pad that reports
/// only a level still shows a bar. `None` draws no bar.
pub fn bar_percent(battery: &Battery) -> Option<u8> {
    battery.percent.or(match battery.level {
        Level::Empty => Some(5),
        Level::Low => Some(25),
        Level::Medium => Some(60),
        Level::Full => Some(100),
        Level::Unknown => None,
    })
}

/// The name a controller or device goes by: Core's friendly model name
/// when it knows the model, otherwise the name the device reports. Blank
/// names count as none.
pub fn display_name<'a>(display_name: Option<&'a str>, name: Option<&'a str>) -> Option<&'a str> {
    let named = |n: Option<&'a str>| n.map(str::trim).filter(|n| !n.is_empty());
    named(display_name).or_else(|| named(name))
}

/// How one entry is named on screen. `name` is `None` for an entry with
/// no name at all, which the view calls "Controller {n}" after its place.
/// `tail` tells apart entries that share a name: the last five characters
/// of the device's Bluetooth address, e.g. `79:CA`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Label {
    pub name: Option<String>,
    pub tail: Option<String>,
}

/// The last five characters of an address: its last two bytes.
pub fn address_tail(address: &str) -> Option<String> {
    let chars: Vec<char> = address.trim().chars().collect();
    if chars.len() < 5 {
        return None;
    }
    Some(chars[chars.len() - 5..].iter().collect())
}

/// Name the entries shown together, each a `(name, address)` pair. Where
/// two share a name, each one that has an address gets its tail; that is
/// the device's own data, not a guess at which is which.
pub fn labels(entries: &[(Option<&str>, Option<&str>)]) -> Vec<Label> {
    entries
        .iter()
        .map(|&(name, address)| {
            let shared =
                name.is_some_and(|n| entries.iter().filter(|(m, _)| *m == Some(n)).count() > 1);
            Label {
                name: name.map(str::to_string),
                tail: if shared {
                    address.and_then(address_tail)
                } else {
                    None
                },
            }
        })
        .collect()
}

/// The Controllers page's connected slots. A controller keeps its slot for
/// as long as the page is open, so a card never moves: one that goes away
/// leaves its slot empty rather than pulling its neighbors along, and one
/// that arrives takes the first empty slot. Controllers past the last slot
/// are counted, not shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slots {
    ids: Vec<Option<String>>,
}

/// How many cards the Connected area holds: four side by side, or two
/// lines of two where cards do not fit.
pub const SLOTS: usize = 4;

impl Default for Slots {
    fn default() -> Self {
        Self {
            ids: vec![None; SLOTS],
        }
    }
}

impl Slots {
    /// Seat the controllers connected now, in Core's order. Returns how
    /// many did not fit.
    pub fn place(&mut self, connected: &[&str]) -> usize {
        for slot in &mut self.ids {
            if slot.as_deref().is_some_and(|id| !connected.contains(&id)) {
                *slot = None;
            }
        }
        let mut overflow = 0;
        for id in connected {
            if self.ids.iter().any(|slot| slot.as_deref() == Some(*id)) {
                continue;
            }
            match self.ids.iter_mut().find(|slot| slot.is_none()) {
                Some(slot) => *slot = Some((*id).to_string()),
                None => overflow += 1,
            }
        }
        overflow
    }

    /// Which controller each slot holds.
    pub fn ids(&self) -> &[Option<String>] {
        &self.ids
    }
}

/// The Paired controllers page's order: as first seen, newly paired
/// devices appended, forgotten ones dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PairedOrder {
    ids: Vec<String>,
}

impl PairedOrder {
    /// Fold in the devices Core lists as paired now. An adapter that is off
    /// or gone lists nothing, which says nothing about what is paired: `BlueZ`
    /// lists them again once it is back. Only a powered adapter's list
    /// (`listing`) drops a row, so the cursor on one stays put through a
    /// dongle reset.
    pub fn observe(&mut self, paired: &[&str], listing: bool) {
        if listing {
            self.ids.retain(|id| paired.contains(&id.as_str()));
        }
        for id in paired {
            if !self.ids.iter().any(|p| p == id) {
                self.ids.push((*id).to_string());
            }
        }
    }

    /// The page's rows, in order.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }
}

/// Core's latest pairing attempt, as it reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt<'a> {
    pub id: &'a str,
    /// Core numbers pair requests in the order it accepted them.
    pub number: u64,
    /// `pairing`, `paired` or `failed`.
    pub state: &'a str,
    pub error: Option<&'a str>,
}

/// One Bluetooth device as Core reports it right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sighting<'a> {
    pub id: &'a str,
    /// Already resolved through `display_name`.
    pub name: Option<&'a str>,
    pub paired: bool,
    pub connected: bool,
}

/// One device the search turned up, as the page last heard of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: String,
    pub name: Option<String>,
    pub paired: bool,
    pub connected: bool,
    /// Why this page's last attempt on it failed, until it is tried again
    /// or pairs.
    failure: Option<PairError>,
}

/// Where the search itself stands, for the status line over the found
/// list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Search {
    #[default]
    Searching,
    /// A device is pairing; the search waits for it.
    Pairing,
    /// Core said the adapter is off.
    Off,
    /// Core would not search, or the adapter would not start.
    Failed,
}

/// The pairing half of the Controllers page, for as long as the page is
/// open. Found devices are only ever appended, in the order they turned
/// up, and stay listed until the page closes even when Core stops
/// reporting them, so nothing moves under the cursor. Core keeps only its
/// latest attempt, so each row's own outcome is held here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PairSession {
    found: Vec<Found>,
    /// Core's search answered for this page. Until then a device it lists
    /// unpaired is from an earlier search and is not news.
    listening: bool,
    search: Search,
    /// The device this page asked Core to pair, while the attempt runs.
    asked: Option<String>,
    /// The number Core gave this page's running attempt, once it accepted
    /// the request. Only that attempt's outcome is this page's.
    ours: Option<u64>,
    /// The attempt failed in a way asking again can fix: the page asks
    /// again, until it pairs or the user stops it.
    retry_due: bool,
    /// The first found row the list shows.
    top: usize,
    /// Core's own word on whether the adapter is searching, from its last
    /// device list; `None` while it has none (no link, no adapter).
    scanning: Option<bool>,
    requests: Requests,
}

/// The page's own search requests to Core.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Requests {
    /// A search request is waiting on Core's answer.
    asking: bool,
    /// The page should ask Core to search now.
    due: bool,
    /// A search that ended under the page was asked for again since the
    /// last renewal. A radio that keeps dropping its search waits for the
    /// renewal rather than being asked in a loop.
    rescued: bool,
}

impl PairSession {
    pub fn found(&self) -> &[Found] {
        &self.found
    }

    pub fn search(&self) -> Search {
        if self.busy() {
            Search::Pairing
        } else {
            self.search
        }
    }

    pub fn top(&self) -> usize {
        self.top
    }

    /// A search request is going to Core: the page's first, or a renewal.
    pub fn search_requested(&mut self, renewal: bool) {
        self.requests.asking = true;
        self.requests.due = false;
        if renewal {
            self.requests.rescued = false;
        }
    }

    /// Whether to ask Core to search now: the search ended under the page,
    /// a pairing attempt finished, or Core came back. Marks the request
    /// sent.
    pub fn take_search(&mut self) -> bool {
        if !self.requests.due || self.requests.asking || self.busy() {
            return false;
        }
        self.requests.asking = true;
        self.requests.due = false;
        true
    }

    /// Core answered a search request: what it lists unpaired from here
    /// on was found by this page's search. An adapter that is still not
    /// searching after that could not start.
    pub fn search_answered(&mut self, scanning: bool) {
        self.listening = true;
        self.requests.asking = false;
        self.requests.due = false;
        self.scanning = Some(scanning);
        self.search = if scanning {
            Search::Searching
        } else {
            Search::Failed
        };
    }

    /// Core refused a search request.
    pub fn search_refused(&mut self, error: PairError) {
        self.requests.asking = false;
        self.search = if error == PairError::NotPowered {
            Search::Off
        } else {
            Search::Failed
        };
    }

    /// Fold in Core's search state. `None` means it lists no devices at
    /// all right now; the first list after that is a fresh start.
    fn follow_scanning(&mut self, scanning: Option<bool>, pairing_ended: bool) {
        let before = std::mem::replace(&mut self.scanning, scanning);
        let Some(scanning) = scanning else {
            return;
        };
        if scanning {
            self.search = Search::Searching;
            return;
        }
        if self.requests.asking || self.busy() {
            return;
        }
        if pairing_ended || before.is_none() && self.listening {
            self.requests.due = true;
        } else if before == Some(true) {
            if self.requests.rescued {
                self.search = Search::Failed;
            } else {
                self.requests.rescued = true;
                self.requests.due = true;
            }
        }
    }

    /// Fold in what Core reports: its devices, its latest attempt and
    /// whether it is searching (`None` when it lists nothing).
    pub fn observe<'a>(
        &mut self,
        devices: impl IntoIterator<Item = Sighting<'a>>,
        attempt: Option<Attempt<'_>>,
        scanning: Option<bool>,
    ) {
        let was_busy = self.busy();
        self.fold(devices, attempt);
        let pairing_ended = was_busy && !self.busy();
        self.follow_scanning(scanning, pairing_ended);
    }

    fn fold<'a>(
        &mut self,
        devices: impl IntoIterator<Item = Sighting<'a>>,
        attempt: Option<Attempt<'_>>,
    ) {
        for seen in devices {
            if let Some(entry) = self.found.iter_mut().find(|f| f.id == seen.id) {
                entry.name = seen.name.map(str::to_string).or(entry.name.take());
                entry.paired = seen.paired;
                entry.connected = seen.connected;
                if seen.paired {
                    entry.failure = None;
                }
            } else if self.listening && !seen.paired {
                self.found.push(Found {
                    id: seen.id.to_string(),
                    name: seen.name.map(str::to_string),
                    paired: false,
                    connected: seen.connected,
                    failure: None,
                });
            }
        }
        let (Some(asked), Some(ours), Some(attempt)) = (self.asked.clone(), self.ours, attempt)
        else {
            return;
        };
        if attempt.number > ours {
            // Someone else started pairing: ours is over.
            self.asked = None;
            self.ours = None;
            self.retry_due = false;
            return;
        }
        if attempt.number < ours || attempt.id != asked {
            return;
        }
        match attempt.state {
            "paired" => {
                self.asked = None;
                self.ours = None;
                if let Some(entry) = self.entry(&asked) {
                    entry.failure = None;
                }
            }
            "failed" => self.failed(PairError::from_token(attempt.error)),
            _ => {}
        }
    }

    /// Core accepted the page's pair request as attempt `number`.
    pub fn pair_accepted(&mut self, number: u64) {
        if self.asked.is_some() {
            self.ours = Some(number);
        }
    }

    fn entry(&mut self, id: &str) -> Option<&mut Found> {
        self.found.iter_mut().find(|f| f.id == id)
    }

    /// The state found row `index` shows.
    pub fn state(&self, index: usize) -> Option<DeviceState> {
        let entry = self.found.get(index)?;
        Some(if self.asked.as_deref() == Some(entry.id.as_str()) {
            DeviceState::Pairing
        } else if entry.connected {
            DeviceState::Connected
        } else if entry.paired {
            DeviceState::Paired
        } else if let Some(error) = entry.failure {
            DeviceState::Failed(error)
        } else {
            DeviceState::Found
        })
    }

    /// Whether an attempt is underway, so another cannot start.
    pub fn busy(&self) -> bool {
        self.asked.is_some()
    }

    /// Accept on found row `index`: the device to ask Core to pair, if it
    /// can be paired and nothing else is pairing.
    pub fn pair_requested(&mut self, index: usize) -> Option<String> {
        if self.busy() || !self.state(index)?.can_pair() {
            return None;
        }
        let id = self.found[index].id.clone();
        if let Some(entry) = self.entry(&id) {
            entry.failure = None;
        }
        self.asked = Some(id.clone());
        self.ours = None;
        Some(id)
    }

    /// Core refused the pair request itself.
    pub fn pair_refused(&mut self, error: PairError) {
        let was_busy = self.busy();
        self.failed(error);
        if was_busy && !self.busy() {
            self.search_after_pairing();
        }
    }

    /// Pairing is over: search again unless Core still is.
    fn search_after_pairing(&mut self) {
        if self.scanning != Some(true) {
            self.requests.due = true;
        }
    }

    /// The running attempt failed: try again when that can help, otherwise
    /// give up and show why.
    fn failed(&mut self, error: PairError) {
        if self.asked.is_none() {
            return;
        }
        self.ours = None;
        if error.retries() {
            self.retry_due = true;
            return;
        }
        if let Some(id) = self.asked.take() {
            if let Some(entry) = self.entry(&id) {
                entry.failure = Some(error);
            }
        }
    }

    /// The device to ask Core to pair again, once per failed attempt.
    pub fn take_retry(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.retry_due) {
            return None;
        }
        self.asked.clone()
    }

    /// Whether the page is still trying to pair `id`.
    pub fn pairing(&self, id: &str) -> bool {
        self.asked.as_deref() == Some(id)
    }

    /// Stop trying to pair. An attempt Core is already running finishes on
    /// its own; the row then shows what the device is.
    pub fn stop_pairing(&mut self) -> bool {
        self.retry_due = false;
        self.ours = None;
        let stopped = self.asked.take().is_some();
        if stopped {
            self.search_after_pairing();
        }
        stopped
    }

    /// Keep found row `focus` in a list `visible` rows tall, scrolling as
    /// little as it takes. Nothing but the cursor moves the list.
    pub fn follow(&mut self, focus: usize, visible: usize) {
        self.top = keep_in_view(self.top, focus, visible);
    }
}

/// The first row a list `visible` rows tall shows so that row `focus`
/// is on it, moved from `top` as little as possible.
pub fn keep_in_view(top: usize, focus: usize, visible: usize) -> usize {
    if focus < top {
        focus
    } else if visible > 0 && focus >= top + visible {
        focus + 1 - visible
    } else {
        top
    }
}

/// What one device row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Found by the search and not paired: Accept pairs it.
    Found,
    Pairing,
    /// Paired, not connected right now.
    Paired,
    Connected,
    Failed(PairError),
}

impl DeviceState {
    /// Accept pairs this row.
    pub fn can_pair(self) -> bool {
        matches!(self, Self::Found | Self::Failed(_))
    }
}

/// How often the page asks Core to keep searching, and for how long each
/// time. Asked again well before the last one runs out, a search never
/// ends while the page is open.
pub const SEARCH_SECONDS: u32 = 60;
pub const RENEW_SECONDS: u64 = 45;

/// How long the page waits after a failed pairing attempt before asking
/// Core to try again.
pub const RETRY_SECONDS: u64 = 2;

/// The Controllers page laid out in whole pixels inside the settings
/// card's rows band (`settings::page_geometry`). Every figure is fixed per
/// scene, never per content: nothing on the page moves when controllers
/// come and go or the search finds something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLayout {
    /// One line per controller instead of cards, at 240p and on the
    /// bitmap face.
    pub compact: bool,
    /// The band's width, the card's less its lips.
    pub width: i32,
    /// The connected cards lead the band, unheaded: a card with a pad and
    /// a battery on it says what it is, and a heading would cost the found
    /// list a row at every tier.
    pub slots_y: i32,
    pub slot_columns: i32,
    pub slot_width: i32,
    pub slot_height: i32,
    pub slot_gap: i32,
    /// The search's status line, a heading over the found list.
    pub status_y: i32,
    pub status_height: i32,
    pub list_y: i32,
    pub row_height: i32,
    pub visible_rows: i32,
    /// The Paired controllers row, pinned to the band's bottom.
    pub paired_y: i32,
}

pub fn page_layout(inputs: &crate::sizing::Inputs) -> PageLayout {
    use crate::settings::{hint_line_height, line_height, page_geometry};
    let derived = crate::sizing::derive(inputs);
    let page = page_geometry(inputs);
    let compact = derived.tier == crate::sizing::Tier::T240 || inputs.bitmap_type;
    let width = (page.card_width - 2 * page.pad).max(0);
    let gap = page.pad;
    let slot_gap = inputs.pct_min(1.0).max(2);
    let (slot_columns, slot_rows, slot_height) = if compact {
        // A line per controller: the face's own height and a pixel of air
        // either side.
        (2, 2, hint_line_height(inputs, derived.font_body) + 2)
    } else {
        let inner = inputs.pct_min(1.2);
        (
            4,
            1,
            2 * inner
                + line_height(derived.font_body)
                + inputs.pct_h(0.8)
                + line_height(derived.font_caption),
        )
    };
    let slots_y = 0;
    let slot_width = ((width - (slot_columns - 1) * slot_gap) / slot_columns).max(0);
    let slots_bottom = slots_y + slot_rows * slot_height + (slot_rows - 1) * (slot_gap / 2);
    let status_y = slots_bottom + if compact { 1 } else { gap };
    let status_height = page.header_height;
    let list_y = status_y + status_height;
    let row_height = page.row_height;
    let paired_y = (page.rows_viewport - row_height).max(list_y);
    // A hairline over the Paired controllers row, and the list stops one
    // clear line above it.
    let visible_rows = ((paired_y - 2 - list_y) / row_height.max(1)).max(0);
    PageLayout {
        compact,
        width,
        slots_y,
        slot_columns,
        slot_width,
        slot_height,
        slot_gap,
        status_y,
        status_height,
        list_y,
        row_height,
        visible_rows,
        paired_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_rows_ride_out_an_adapter_reset_and_a_forget_drops_one() {
        let mut order = PairedOrder::default();
        order.observe(&["A", "B"], true);
        // The dongle drops off the bus: Core reports it off with nothing on it.
        order.observe(&[], false);
        assert_eq!(order.ids(), ["A", "B"]);
        // Back again, BlueZ lists them in its own order; the page keeps its.
        order.observe(&["B", "A"], true);
        assert_eq!(order.ids(), ["A", "B"]);
        order.observe(&["B"], true);
        assert_eq!(order.ids(), ["B"]);
        order.observe(&["B", "C"], true);
        assert_eq!(order.ids(), ["B", "C"]);
    }

    #[test]
    fn a_row_reads_its_charge_or_says_wired() {
        let b = |p, level, charge| Battery::new(p, level, charge);
        assert_eq!(reading(None, "usb"), Reading::Wired);
        assert_eq!(reading(None, "bluetooth"), Reading::Unreported);
        assert_eq!(reading(None, "unknown"), Reading::Unreported);
        assert_eq!(
            reading(Some(&b(Some(80), "full", Some("charging"))), "bluetooth"),
            Reading::Charging(Some(80))
        );
        assert_eq!(
            reading(Some(&b(None, "low", Some("charging"))), "bluetooth"),
            Reading::Charging(None)
        );
        assert_eq!(
            reading(Some(&b(Some(100), "full", Some("full"))), "bluetooth"),
            Reading::Charged
        );
        assert_eq!(
            reading(
                Some(&b(Some(42), "medium", Some("discharging"))),
                "bluetooth"
            ),
            Reading::Percent(42)
        );
        assert_eq!(
            reading(Some(&b(Some(42), "medium", None)), "bluetooth"),
            Reading::Percent(42)
        );
        // XInput: a level and nothing else.
        assert_eq!(
            reading(Some(&b(None, "low", None)), "bluetooth"),
            Reading::Level(Level::Low)
        );
        assert_eq!(
            reading(Some(&b(Some(250), "full", None)), "bluetooth"),
            Reading::Percent(100)
        );
    }

    #[test]
    fn the_header_warns_only_about_a_controller_running_flat() {
        let b = |level, charge| Battery::new(None, level, charge);
        assert!(b("low", Some("discharging")).is_low());
        assert!(b("empty", None).is_low());
        assert!(b("low", Some("notCharging")).is_low());
        assert!(!b("low", Some("charging")).is_low());
        assert!(!b("empty", Some("full")).is_low());
        assert!(!b("medium", Some("discharging")).is_low());
        assert!(!b("bogus", None).is_low());
        let pads = [b("full", None), b("low", Some("charging"))];
        assert!(!any_low(&pads));
        let pads = [b("full", None), b("low", Some("discharging"))];
        assert!(any_low(&pads));
        assert!(!any_low(&[]));
    }

    #[test]
    fn pairing_needs_the_action_and_an_adapter() {
        assert!(pairing_available(Some("supported"), true));
        assert!(!pairing_available(Some("notPermitted"), true));
        assert!(!pairing_available(None, true));
        assert!(!pairing_available(Some("supported"), false));
    }

    #[test]
    fn pair_errors_map_from_core_and_from_a_refused_request() {
        assert_eq!(
            PairError::from_token(Some("pinRequired")),
            PairError::PinRequired
        );
        assert_eq!(
            PairError::from_token(Some("notPowered")),
            PairError::NotPowered
        );
        assert_eq!(PairError::from_token(Some("timeout")), PairError::Timeout);
        assert_eq!(PairError::from_token(Some("rejected")), PairError::Rejected);
        assert_eq!(PairError::from_token(Some("notFound")), PairError::NotFound);
        assert_eq!(PairError::from_token(Some("failed")), PairError::Failed);
        assert_eq!(PairError::from_token(Some("new")), PairError::Failed);
        assert_eq!(PairError::from_token(None), PairError::Failed);
        assert_eq!(
            PairError::from_request_error(Some("unavailable")),
            PairError::NotPowered
        );
        assert_eq!(
            PairError::from_request_error(Some("busy")),
            PairError::Failed
        );
        assert_eq!(PairError::from_request_error(None), PairError::Failed);
    }

    #[test]
    fn a_bar_shows_the_reading_or_the_level() {
        let b = |p, level| Battery::new(p, level, None);
        assert_eq!(bar_percent(&b(Some(42), "medium")), Some(42));
        assert_eq!(bar_percent(&b(None, "low")), Some(25));
        assert_eq!(bar_percent(&b(None, "full")), Some(100));
        assert_eq!(bar_percent(&b(None, "bogus")), None);
    }

    #[test]
    fn names_prefer_cores_model_name_then_the_reported_one() {
        assert_eq!(
            display_name(Some("DualShock 4"), Some("Wireless Controller")),
            Some("DualShock 4")
        );
        assert_eq!(
            display_name(None, Some("Wireless Controller")),
            Some("Wireless Controller")
        );
        assert_eq!(display_name(Some("  "), Some("Pad")), Some("Pad"));
        assert_eq!(display_name(None, Some("")), None);
        assert_eq!(display_name(None, None), None);
    }

    #[test]
    fn entries_sharing_a_name_carry_their_address_tail() {
        let got = labels(&[
            (Some("Wireless Controller"), Some("A4:AE:12:34:79:CA")),
            (Some("Pro Controller"), Some("98:B6:E9:00:00:01")),
            (Some("Wireless Controller"), Some("A4:AE:12:34:11:0F")),
            (Some("Wireless Controller"), None),
            (None, Some("00:11:22:33:44:55")),
            (None, None),
        ]);
        assert_eq!(got[0].tail.as_deref(), Some("79:CA"));
        assert_eq!(got[0].name.as_deref(), Some("Wireless Controller"));
        assert_eq!(got[1].tail, None, "a name of its own needs nothing more");
        assert_eq!(got[2].tail.as_deref(), Some("11:0F"));
        assert_eq!(got[3].tail, None, "no address, nothing to add");
        // Unnamed entries are numbered by the view, not by their address.
        assert_eq!(got[4], Label::default());
        assert_eq!(got[5], Label::default());
        assert_eq!(address_tail("1:2"), None);
    }

    #[test]
    fn a_card_keeps_its_slot_while_others_come_and_go() {
        let mut slots = Slots::default();
        assert_eq!(slots.place(&["a", "b", "c"]), 0);
        let ids = |s: &Slots| -> Vec<Option<String>> { s.ids().to_vec() };
        let some = |id: &str| Some(id.to_string());
        assert_eq!(ids(&slots), vec![some("a"), some("b"), some("c"), None]);
        // `b` goes: its neighbors stay where they are.
        slots.place(&["a", "c"]);
        assert_eq!(ids(&slots), vec![some("a"), None, some("c"), None]);
        // A newcomer takes the first empty slot; Core's order does not
        // reshuffle the rest.
        slots.place(&["c", "d", "a"]);
        assert_eq!(ids(&slots), vec![some("a"), some("d"), some("c"), None]);
        assert_eq!(slots.place(&["c", "d", "a", "e", "f"]), 1);
        assert_eq!(ids(&slots)[3], some("e"));
    }

    fn seen(id: &str, paired: bool, connected: bool) -> Sighting<'_> {
        Sighting {
            id,
            name: Some(id),
            paired,
            connected,
        }
    }

    fn attempt<'a>(
        id: &'a str,
        number: u64,
        state: &'a str,
        error: Option<&'a str>,
    ) -> Attempt<'a> {
        Attempt {
            id,
            number,
            state,
            error,
        }
    }

    #[test]
    fn the_header_shows_a_pad_per_battery_up_to_four() {
        let charging = Battery::new(Some(30), "low", Some("charging"));
        let low = Battery::new(Some(10), "empty", Some("discharging"));
        let level_only = Battery::new(None, "medium", None);
        let unreadable = Battery::new(None, "unknown", None);
        let full = Battery::new(Some(90), "full", Some("full"));
        let pads = header_pads([
            Some(&charging),
            None,
            Some(&low),
            Some(&unreadable),
            Some(&level_only),
            Some(&full),
            Some(&full),
        ]);
        assert_eq!(
            pads,
            [
                HeaderPad {
                    fill: Fill::Low,
                    charging: true,
                    low: false,
                    marks_before: 0,
                },
                HeaderPad {
                    fill: Fill::Empty,
                    charging: false,
                    low: true,
                    marks_before: 1,
                },
                HeaderPad {
                    fill: Fill::Half,
                    charging: false,
                    low: false,
                    marks_before: 1,
                },
                HeaderPad {
                    fill: Fill::Full,
                    charging: false,
                    low: false,
                    marks_before: 1,
                },
            ],
            "no glyph without a battery or a readable charge, four at most"
        );
        assert!(header_pads([]).is_empty());
    }

    #[test]
    fn found_devices_are_appended_and_never_dropped() {
        let mut s = PairSession::default();
        // Before Core answers the search, an unpaired device is left over
        // from an earlier one.
        s.observe([seen("old", false, false)], None, Some(true));
        assert!(s.found().is_empty());
        s.search_answered(true);
        s.observe(
            [seen("p", true, true), seen("a", false, false)],
            None,
            Some(true),
        );
        s.observe(
            [seen("b", false, false), seen("a", false, false)],
            None,
            Some(true),
        );
        let ids: Vec<&str> = s.found().iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            ["a", "b"],
            "paired devices are not finds; order is discovery"
        );
        // Core stops listing `a` (its search ended): the row stays.
        s.observe([seen("b", false, false)], None, Some(true));
        assert_eq!(s.found().len(), 2);
        assert_eq!(s.state(0), Some(DeviceState::Found));
        // A late arrival goes on the end.
        s.observe([seen("c", false, false)], None, Some(true));
        assert_eq!(s.found()[2].id, "c");
        assert_eq!(s.state(3), None);
    }

    #[test]
    fn each_row_keeps_its_own_outcome() {
        let mut s = PairSession::default();
        s.search_answered(true);
        s.observe(
            [seen("a", false, false), seen("b", false, false)],
            None,
            Some(true),
        );
        assert_eq!(s.pair_requested(0).as_deref(), Some("a"));
        assert!(s.busy());
        assert_eq!(s.pair_requested(1), None, "one attempt at a time");
        assert_eq!(s.state(0), Some(DeviceState::Pairing));
        // A result from before Core accepted this attempt is not its result.
        s.observe(
            [],
            Some(attempt("a", 1, "failed", Some("timeout"))),
            Some(true),
        );
        assert_eq!(s.state(0), Some(DeviceState::Pairing));
        s.pair_accepted(2);
        s.observe(
            [],
            Some(attempt("a", 1, "failed", Some("timeout"))),
            Some(true),
        );
        assert_eq!(s.state(0), Some(DeviceState::Pairing), "an older attempt");
        s.observe(
            [],
            Some(attempt("a", 2, "failed", Some("pinRequired"))),
            Some(true),
        );
        assert_eq!(
            s.state(0),
            Some(DeviceState::Failed(PairError::PinRequired))
        );
        assert!(!s.busy());
        // The next attempt leaves the first row's failure where it is.
        assert_eq!(s.pair_requested(1).as_deref(), Some("b"));
        s.pair_accepted(3);
        s.observe([], Some(attempt("b", 3, "pairing", None)), Some(true));
        assert_eq!(
            s.state(0),
            Some(DeviceState::Failed(PairError::PinRequired))
        );
        s.observe(
            [seen("b", true, true)],
            Some(attempt("b", 3, "paired", None)),
            Some(true),
        );
        assert_eq!(s.state(1), Some(DeviceState::Connected));
        s.observe([seen("b", true, false)], None, Some(true));
        assert_eq!(s.state(1), Some(DeviceState::Paired));
        // A failed row can be tried again, and a refusal fails it at once.
        assert_eq!(s.pair_requested(0).as_deref(), Some("a"));
        s.pair_refused(PairError::NotPowered);
        assert_eq!(s.state(0), Some(DeviceState::Failed(PairError::NotPowered)));
        assert_eq!(s.pair_requested(1), None, "a paired row does not pair");
    }

    #[test]
    fn a_failed_attempt_is_tried_again_until_stopped() {
        let mut s = PairSession::default();
        s.search_answered(true);
        s.observe([seen("a", false, false)], None, Some(true));
        assert_eq!(s.pair_requested(0).as_deref(), Some("a"));
        assert_eq!(s.take_retry(), None, "nothing has failed yet");
        s.pair_accepted(1);
        s.observe(
            [],
            Some(attempt("a", 1, "failed", Some("timeout"))),
            Some(true),
        );
        assert_eq!(s.state(0), Some(DeviceState::Pairing), "still trying");
        assert_eq!(s.take_retry().as_deref(), Some("a"));
        assert_eq!(s.take_retry(), None, "one retry per failure");
        // Core still reports the old failure until the new attempt starts.
        s.observe(
            [],
            Some(attempt("a", 1, "failed", Some("timeout"))),
            Some(true),
        );
        assert_eq!(s.take_retry(), None);
        // The same failure again on the next attempt is a new failure.
        s.pair_accepted(2);
        s.observe(
            [],
            Some(attempt("a", 2, "failed", Some("timeout"))),
            Some(true),
        );
        assert_eq!(s.take_retry().as_deref(), Some("a"));
        // A refused request is tried again too.
        s.pair_refused(PairError::Failed);
        assert_eq!(s.take_retry().as_deref(), Some("a"));
        s.pair_accepted(3);
        s.observe(
            [],
            Some(attempt("a", 3, "failed", Some("rejected"))),
            Some(true),
        );
        assert!(s.pairing("a"));
        assert!(s.stop_pairing());
        assert_eq!(s.take_retry(), None, "stopped");
        assert_eq!(s.state(0), Some(DeviceState::Found));
        assert!(!s.stop_pairing());
        // It pairs on a later attempt.
        s.pair_requested(0);
        s.pair_accepted(4);
        s.observe(
            [],
            Some(attempt("a", 4, "failed", Some("timeout"))),
            Some(true),
        );
        s.take_retry();
        s.pair_accepted(5);
        s.observe(
            [seen("a", true, true)],
            Some(attempt("a", 5, "paired", None)),
            Some(true),
        );
        assert_eq!(s.state(0), Some(DeviceState::Connected));
        assert!(!s.busy());
    }

    #[test]
    fn another_clients_attempt_ends_ours() {
        let mut s = PairSession::default();
        s.search_answered(true);
        s.observe([seen("a", false, false)], None, Some(true));
        s.pair_requested(0);
        s.pair_accepted(1);
        s.observe([], Some(attempt("a", 1, "pairing", None)), Some(true));
        s.observe([], Some(attempt("z", 2, "pairing", None)), Some(true));
        assert!(!s.busy());
        assert_eq!(s.state(0), Some(DeviceState::Found));
    }

    #[test]
    fn the_search_reports_an_adapter_that_is_off() {
        let mut s = PairSession::default();
        assert_eq!(s.search(), Search::Searching);
        s.search_refused(PairError::NotPowered);
        assert_eq!(s.search(), Search::Off);
        s.search_refused(PairError::Failed);
        assert_eq!(s.search(), Search::Failed);
        s.search_answered(true);
        assert_eq!(s.search(), Search::Searching);
    }

    fn searching() -> PairSession {
        let mut s = PairSession::default();
        s.search_requested(false);
        s.search_answered(true);
        s.observe([seen("a", false, false)], None, Some(true));
        s
    }

    #[test]
    fn an_adapter_that_will_not_search_shows_failed() {
        let mut s = PairSession::default();
        s.search_requested(false);
        s.observe([], None, Some(false));
        assert_eq!(s.search(), Search::Searching, "the answer is still due");
        assert!(!s.take_search());
        s.search_answered(false);
        assert_eq!(s.search(), Search::Failed);
        s.observe([], None, Some(false));
        assert!(!s.take_search(), "no edge, no new request");
        s.observe([], None, Some(true));
        assert_eq!(s.search(), Search::Searching, "follows the adapter");
    }

    #[test]
    fn a_search_that_ends_is_asked_for_once_per_renewal() {
        let mut s = searching();
        s.observe([], None, Some(false));
        assert!(s.take_search());
        assert!(!s.take_search(), "once per edge");
        s.search_answered(true);
        s.observe([], None, Some(false));
        assert!(!s.take_search(), "a second drop waits for the renewal");
        assert_eq!(s.search(), Search::Failed);
        s.search_requested(true);
        s.search_answered(true);
        s.observe([], None, Some(false));
        assert!(s.take_search(), "the renewal resets it");
    }

    #[test]
    fn pairing_holds_the_search_and_restarts_it_after() {
        let mut s = searching();
        assert_eq!(s.pair_requested(0).as_deref(), Some("a"));
        assert_eq!(s.search(), Search::Pairing);
        s.pair_accepted(1);
        s.observe([], Some(attempt("a", 1, "pairing", None)), Some(false));
        assert!(!s.take_search(), "nothing is asked while pairing");
        assert_eq!(s.search(), Search::Pairing);
        s.observe(
            [seen("a", true, true)],
            Some(attempt("a", 1, "paired", None)),
            Some(false),
        );
        assert_eq!(s.search(), Search::Searching);
        assert!(s.take_search(), "pairing over, search again");

        let mut s = searching();
        s.pair_requested(0);
        s.observe([], None, Some(false));
        s.stop_pairing();
        assert!(s.take_search(), "a cancel searches again too");

        let mut s = searching();
        s.pair_requested(0);
        s.pair_refused(PairError::PinRequired);
        assert!(!s.take_search(), "Core is still searching");
    }

    #[test]
    fn coming_back_to_core_searches_again() {
        let mut s = searching();
        s.observe([], None, None);
        assert!(!s.take_search(), "nothing to ask while the link is down");
        s.observe([], None, Some(false));
        assert!(s.take_search());
    }

    #[test]
    fn the_list_scrolls_only_to_keep_the_cursor_on_it() {
        assert_eq!(keep_in_view(0, 2, 3), 0);
        assert_eq!(keep_in_view(0, 3, 3), 1);
        assert_eq!(keep_in_view(4, 2, 3), 2);
        assert_eq!(keep_in_view(2, 3, 3), 2);
        assert_eq!(keep_in_view(5, 9, 0), 5, "no rows, no scrolling");
        let mut s = PairSession::default();
        s.follow(4, 2);
        assert_eq!(s.top(), 3);
    }

    #[test]
    fn the_page_fits_at_every_tier_and_nothing_overlaps() {
        let scenes = [
            (960.0, 540.0, false),
            (1280.0, 720.0, false),
            (1920.0, 1080.0, false),
            (316.0, 216.0, true),
        ];
        for (w, h, crt) in scenes {
            for two_rows in [false, true] {
                let inputs = crate::sizing::Inputs {
                    screen_width: w,
                    screen_height: h,
                    crt_native_path: crt,
                    bitmap_type: crt,
                    help_bar_two_rows: two_rows,
                    ..crate::sizing::Inputs::default()
                };
                let l = page_layout(&inputs);
                let page = crate::settings::page_geometry(&inputs);
                assert_eq!(l.compact, crt, "{w}x{h}");
                assert!(l.visible_rows >= 1, "{w}x{h}: {l:?}");
                assert!(l.list_y + l.visible_rows * l.row_height <= l.paired_y - 2);
                assert_eq!(l.paired_y + l.row_height, page.rows_viewport);
                assert!(l.status_y >= l.slots_y + l.slot_height);
                assert!(
                    l.slot_columns * l.slot_width + (l.slot_columns - 1) * l.slot_gap <= l.width
                );
            }
        }
    }
}
