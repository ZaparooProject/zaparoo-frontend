// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The music player's rules: the transport row and what each control
//! sends to Core, which of them can act right now and why not, the
//! `ZapScript` that starts a folder, the playback clock the progress bar
//! reads between Core's answers, and the player's geometry. Core's
//! background slot is the only slot it drives.
//!
//! The row is the one players share (Apple Music, Spotify): Shuffle,
//! Previous, Play/Pause, Next, Repeat. Stop is a Menu row, and the open
//! playlist is a page of its own rather than a second focus zone.

use std::fmt::Write as _;

use crate::sizing::{Derived, Tier};
use crate::zapscript::escape_arg;

/// The `ZapScript` suffix every music command carries.
pub const SLOT_ARG: &str = "slot=background";

/// How often the time text and the progress bar move while music plays.
pub const TICK_MS: u64 = 1000;
/// How often Core is asked where the track really is, as a backup to its
/// own notifications, while the player is on screen and playing. A Core
/// that sends `media.playback` gets the long interval.
pub const RESYNC_MS: u64 = 5000;
pub const BACKUP_RESYNC_MS: u64 = 30_000;
/// How soon after a control press Core is asked again, so a pause shows
/// without waiting for the next resync.
pub const AFTER_ACTION_MS: u64 = 300;
/// How long a background `media.stopped` is held before the player
/// believes the slot is idle: a track change stops one track and starts
/// the next, and the card must not blank in between.
pub const STOP_GRACE_MS: u64 = 750;

/// The transport row, in the order it shows. Focus starts on Play/Pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Control {
    Shuffle,
    Previous,
    #[default]
    PlayPause,
    Next,
    Repeat,
}

pub const CONTROLS: [Control; 5] = [
    Control::Shuffle,
    Control::Previous,
    Control::PlayPause,
    Control::Next,
    Control::Repeat,
];

impl Control {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shuffle => "shuffle",
            Self::Previous => "previous",
            Self::PlayPause => "play-pause",
            Self::Next => "next",
            Self::Repeat => "repeat",
        }
    }

    /// A persisted token; anything unknown (an older build's control
    /// included) seats on Play/Pause.
    pub fn parse(token: &str) -> Self {
        CONTROLS
            .into_iter()
            .find(|control| control.as_str() == token)
            .unwrap_or(Self::PlayPause)
    }

    pub fn index(self) -> usize {
        CONTROLS
            .iter()
            .position(|control| *control == self)
            .unwrap_or(2)
    }

    pub fn at(index: usize) -> Self {
        CONTROLS.get(index).copied().unwrap_or(Self::PlayPause)
    }

    /// The control beside this one, `None` past either end of the row.
    pub fn step(self, right: bool) -> Option<Self> {
        let index = self.index();
        let next = if right {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        CONTROLS.get(next).copied()
    }
}

/// An open playlist's repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    /// Core's `PlaylistState.repeat`. Anything unknown reads as off.
    pub fn from_core(value: &str) -> Self {
        match value {
            "all" => Self::All,
            "one" => Self::One,
            _ => Self::Off,
        }
    }

    /// The spelling `playlists.update` takes.
    pub fn core_token(self) -> &'static str {
        match self {
            Self::Off => "none",
            Self::All => "all",
            Self::One => "one",
        }
    }

    /// The spelling the `ZapScript` `repeat` argument takes.
    pub fn zapscript_token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }

    /// What one press of Repeat moves to.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// What the background slot is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playback {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl Playback {
    /// Core's `playbackState` for a slot with a track on it. A track with
    /// no state reported is playing: Core only reports it for launchers
    /// that can tell.
    pub fn from_core(has_track: bool, state: &str) -> Self {
        if !has_track {
            return Self::Stopped;
        }
        match state {
            "paused" => Self::Paused,
            "stopped" => Self::Stopped,
            _ => Self::Playing,
        }
    }
}

/// Why a control cannot act right now. The view words it under the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unavailable {
    #[default]
    None,
    /// Nothing is on the background slot.
    NothingPlaying,
    /// The control works on a playlist and none is open.
    NoPlaylist,
    /// Core or the track's launcher does not offer it.
    NotSupported,
    /// A track is starting: Core takes no other launch until it has, so
    /// a second press would only be refused.
    Starting,
}

/// What the player knows about the background slot, enough to decide
/// every control.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts Core reports about the slot"
)]
pub struct Now {
    pub has_track: bool,
    pub playback: Playback,
    /// The track launcher's controls (`launcherControls`).
    pub controls: Vec<String>,
    pub has_playlist: bool,
    /// The open playlist is playing rather than paused.
    pub playlist_playing: bool,
    pub playlist_total: usize,
    /// Core reports the playlist's shuffle state, so it can change it.
    pub can_update_playlist: bool,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// A track this player asked for is still starting (`TrackStart`).
    pub starting: bool,
}

impl Now {
    fn offers(&self, action: &str) -> bool {
        self.controls.iter().any(|control| control == action)
    }

    /// The music is audibly playing, which decides Play or Pause.
    pub fn playing(&self) -> bool {
        if self.has_playlist && !self.playlist_playing {
            return false;
        }
        self.playback == Playback::Playing
    }
}

/// Whether `control` can act now, and if not, why.
pub fn unavailable(control: Control, now: &Now) -> Unavailable {
    match control {
        Control::Shuffle | Control::Repeat => {
            if !now.has_playlist {
                Unavailable::NoPlaylist
            } else if !now.can_update_playlist {
                Unavailable::NotSupported
            } else {
                Unavailable::None
            }
        }
        Control::Previous | Control::Next | Control::PlayPause if now.starting => {
            Unavailable::Starting
        }
        Control::Previous | Control::Next => {
            if now.has_playlist && now.playlist_total > 0 {
                Unavailable::None
            } else if now.has_track {
                Unavailable::NoPlaylist
            } else {
                Unavailable::NothingPlaying
            }
        }
        Control::PlayPause => {
            if now.has_playlist {
                Unavailable::None
            } else if !now.has_track {
                Unavailable::NothingPlaying
            } else if now.offers("toggle_pause") {
                Unavailable::None
            } else {
                Unavailable::NotSupported
            }
        }
    }
}

/// How long a track this player asked for may take to start before the
/// controls come back anyway: Core answers a refused or failed launch, but
/// a lost answer must not leave them waiting for good.
pub const START_TIMEOUT_MS: u64 = 5000;

/// A track this player asked for (Previous, Next, a playlist row, new
/// music) between the request and Core reporting it. Core refuses another
/// launch until the first has started, so the controls that launch wait.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackStart {
    /// The track on the slot when the request went out.
    from: Option<String>,
    pending: bool,
    seq: u64,
}

impl TrackStart {
    /// A request went out while `current` was on the slot; the sequence
    /// number for its timeout.
    pub fn begin(&mut self, current: Option<&str>) -> u64 {
        self.from = current.map(str::to_string);
        self.pending = true;
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Core reported the slot: a different track (or the first one) means
    /// the request landed. True when that ended the wait.
    pub fn heard(&mut self, current: Option<&str>) -> bool {
        if self.pending && current.is_some() && current.map(str::to_string) != self.from {
            self.pending = false;
            return true;
        }
        false
    }

    /// The request failed, or its timeout `seq` ran out while still the
    /// latest. True when that ended the wait.
    pub fn end(&mut self, seq: Option<u64>) -> bool {
        if !self.pending || seq.is_some_and(|seq| seq != self.seq) {
            return false;
        }
        self.pending = false;
        true
    }
}

/// What a control press sends to Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `ZapScript` through `run`.
    Run(String),
    /// `media.control` on the background slot.
    Control(&'static str),
    /// `playlists.update` on the background slot.
    Update {
        shuffle: Option<bool>,
        repeat: Option<Repeat>,
    },
}

fn playlist_command(name: &str) -> Command {
    Command::Run(format!("**playlist.{name}?{SLOT_ARG}"))
}

/// What pressing `control` sends, or `None` when it cannot act. Play and
/// Pause go through the playlist when one is open, so its `playing` flag
/// stays true to what is heard; a lone track uses the launcher control.
pub fn transport_command(control: Control, now: &Now) -> Option<Command> {
    if unavailable(control, now) != Unavailable::None {
        return None;
    }
    Some(match control {
        Control::PlayPause if now.has_playlist => {
            if now.playing() {
                playlist_command("pause")
            } else if now.playlist_playing
                && now.playback == Playback::Paused
                && now.offers("toggle_pause")
            {
                // Paused outside the playlist (for a game): `play` would
                // skip, so resume the track itself.
                Command::Control("toggle_pause")
            } else {
                // With no argument, play resumes the open playlist. Sent
                // only while it is paused: on a playing one it skips.
                playlist_command("play")
            }
        }
        Control::PlayPause => Command::Control("toggle_pause"),
        Control::Previous => playlist_command("previous"),
        Control::Next => playlist_command("next"),
        Control::Shuffle => Command::Update {
            shuffle: Some(!now.shuffle),
            repeat: None,
        },
        Control::Repeat => Command::Update {
            shuffle: None,
            repeat: Some(now.repeat.next()),
        },
    })
}

/// The Menu's Stop: the playlist, or a lone track. `None` when the slot
/// is idle.
pub fn stop_command(now: &Now) -> Option<Command> {
    if now.has_playlist {
        Some(playlist_command("stop"))
    } else if now.has_track {
        Some(Command::Control("stop"))
    } else {
        None
    }
}

/// The track after the current one, as the playlist will play it: the
/// next item, the first again under Repeat all, the same one under Repeat
/// one, and nothing after the last otherwise.
pub fn up_next(index: usize, total: usize, repeat: Repeat) -> Option<usize> {
    if total == 0 {
        return None;
    }
    match repeat {
        Repeat::One => Some(index.min(total - 1)),
        _ if index + 1 < total => Some(index + 1),
        Repeat::All => Some(0),
        Repeat::Off => None,
    }
}

/// Jump to playlist item `index` (counted from 0) and play it. A paused
/// playlist only moves its place on `goto`, so it is resumed after; a
/// playing one launches the item itself, and `play` there would skip it.
pub fn goto_script(index: usize, playing: bool) -> String {
    let goto = format!("**playlist.goto:{}?{SLOT_ARG}", index + 1);
    if playing {
        goto
    } else {
        format!("{goto}||**playlist.play?{SLOT_ARG}")
    }
}

/// Play one folder as a playlist on the background slot, from its track
/// `start` (counted from 0 among the folder's tracks) when one was picked.
/// Core lists a folder playlist exactly as a filename-ordered browse of it
/// (see Core's `media.browse` docs), so track n of the listing the user saw
/// is item n of the playlist. The script names the folder, so it stays
/// small however many tracks the folder holds.
pub fn play_folder_script(
    folder: &str,
    shuffle: bool,
    repeat: Repeat,
    start: Option<usize>,
) -> String {
    // Filtered to Audio, as the library's browse is, so Core plays the
    // listing the user saw.
    let mut script = format!(
        "**playlist.play:{}?{SLOT_ARG}&system=Audio&repeat={}",
        escape_arg(folder),
        repeat.zapscript_token()
    );
    if shuffle {
        script.push_str("&mode=shuffle");
    }
    if let Some(start) = start {
        let _ = write!(script, "&start={}", start + 1);
    }
    script
}

/// Where `picked` sits among a listing's tracks, counted from 0.
pub fn track_start(rows: &[(EntryKind, &str)], picked: &str) -> Option<usize> {
    rows.iter()
        .filter(|(kind, _)| *kind == EntryKind::Track)
        .position(|(_, path)| *path == picked)
}

/// Where the track is between Core's answers: the last position Core
/// reported, carried forward by the wall clock while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackClock {
    position_ms: u64,
    duration_ms: Option<u64>,
    playing: bool,
    anchor_ms: u64,
}

impl PlaybackClock {
    /// Core reported the track at `position_ms` at time `now_ms`.
    pub fn sync(
        &mut self,
        position_ms: Option<u64>,
        duration_ms: Option<u64>,
        playing: bool,
        now_ms: u64,
    ) {
        self.position_ms = position_ms.unwrap_or(0);
        self.duration_ms = duration_ms.filter(|duration| *duration > 0);
        self.playing = playing;
        self.anchor_ms = now_ms;
    }

    /// Stop moving where it is (a pause seen before Core confirms it).
    pub fn hold(&mut self, now_ms: u64) {
        self.position_ms = self.position_at(now_ms);
        self.playing = false;
        self.anchor_ms = now_ms;
    }

    pub fn position_at(&self, now_ms: u64) -> u64 {
        let moved = if self.playing {
            now_ms.saturating_sub(self.anchor_ms)
        } else {
            0
        };
        let position = self.position_ms.saturating_add(moved);
        self.duration_ms
            .map_or(position, |duration| position.min(duration))
    }

    pub fn duration(&self) -> Option<u64> {
        self.duration_ms
    }
}

/// A time as the player shows it: `m:ss`, or `h:mm:ss` from an hour.
pub fn clock_text(ms: u64) -> String {
    let seconds = ms / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// How far through the track `position` is, 0 to 1000; 0 when the length
/// is unknown.
pub fn progress_milli(position_ms: u64, duration_ms: Option<u64>) -> i32 {
    match duration_ms {
        Some(duration) if duration > 0 => {
            let milli = u128::from(position_ms.min(duration)) * 1000 / u128::from(duration);
            i32::try_from(milli).unwrap_or(1000).min(1000)
        }
        _ => 0,
    }
}

/// Whether the player should be ticking and asking Core where the track
/// is: only while it is on screen, the frontend is awake, and music plays.
pub fn should_poll(on_screen: bool, awake: bool, playing: bool) -> bool {
    on_screen && awake && playing
}

/// Where a restored or refreshed playlist focus seats: on the saved row
/// when that row still holds the saved item, else on the current track.
pub fn reseat_row(
    saved_index: usize,
    saved_item: &str,
    items: &[String],
    current_track: usize,
) -> usize {
    if items
        .get(saved_index)
        .is_some_and(|item| item == saved_item)
    {
        return saved_index;
    }
    if let Some(index) = items.iter().position(|item| item == saved_item) {
        return index;
    }
    current_track.min(items.len().saturating_sub(1))
}

/// What a library row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// One of Core's index routes for the Audio system.
    Route,
    Folder,
    Track,
}

/// The player in whole pixels, relative to the top-left corner of its
/// card. The card keeps the Settings pages' margins and hugs the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub padding: i32,
    pub art_x: i32,
    pub art_y: i32,
    pub art_size: i32,
    /// The text column beside the art, centered on it.
    pub text_x: i32,
    pub text_y: i32,
    pub text_width: i32,
    pub title_height: i32,
    pub line_height: i32,
    /// The album gets its own line; at 240p it shares the artist's.
    pub album_line: bool,
    /// The progress row spans the card's inner width: the elapsed time,
    /// the progress track's cells, the length.
    pub progress_y: i32,
    pub progress_height: i32,
    pub time_width: i32,
    pub bar_x: i32,
    pub bar_width: i32,
    pub controls_x: i32,
    pub controls_y: i32,
    pub button_size: i32,
    pub button_gap: i32,
    /// The line under the row: the focused control and its state, or why
    /// it cannot act. As far below the row as the row is below the
    /// progress row.
    pub label_y: i32,
    pub label_height: i32,
    /// The card's height around its content: the block and the padding
    /// above and below it, never more than the room it was given.
    pub height: i32,
}

/// Lay the player out in a card `width` wide with up to `height` of room,
/// with the card's own inner padding. The card hugs what it holds
/// (`Geometry::height`), so a tall scene does not frame a band of empty
/// card above and below the player.
pub fn geometry(derived: &Derived, width: i32, height: i32, padding: i32) -> Geometry {
    let compact = derived.tier == Tier::T240;
    let line = |font: i32| font + (font * 3 / 10).max(2);
    // The detail title's rung, as on Game Info and a browse pane.
    let title_height = line(derived.font_title);
    let line_height = line(derived.font_body);
    // The line under the row is a row of the card in its own right, at
    // the artist line's size.
    let label_height = line(derived.font_body);
    let album_line = !compact;
    let gap = if compact {
        4
    } else {
        (derived.font_body * 3 / 4).max(6)
    };
    // Title, artist (and album), and the up-next line.
    let text_lines = title_height + line_height * if album_line { 3 } else { 2 };
    let inner = (width - 2 * padding).max(0);
    let count = CONTROLS.len() as i32;
    let wanted = if compact {
        20
    } else {
        derived.font_body * 2 + derived.font_body / 2
    };
    // The row is spaced the way players space it: a button's width apart.
    let fits = inner * 2 / (3 * count - 1);
    let button_size = wanted.min(fits).max(8);
    let button_gap = button_size / 2;
    let row_width = count * button_size + (count - 1) * button_gap;
    let progress_height = line(derived.font_caption);
    let rest = gap + progress_height + gap + button_size + gap + label_height;
    // The cover is as tall as the text beside it, at every tier, so the
    // card hugs its content and never frames a band of empty art box.
    let art_size = text_lines;
    let block = art_size + rest;
    let top = padding;
    let time_width = derived.font_caption * 3;
    let bar_gap = if compact { 4 } else { derived.font_caption / 2 };
    let progress_y = top + art_size + gap;
    let controls_y = progress_y + progress_height + gap;
    Geometry {
        padding,
        art_x: padding,
        art_y: top,
        art_size,
        text_x: padding + art_size + padding,
        text_y: top + (art_size - text_lines) / 2,
        text_width: (width - 2 * padding - art_size - padding).max(0),
        title_height,
        line_height,
        album_line,
        progress_y,
        progress_height,
        time_width,
        bar_x: padding + time_width + bar_gap,
        bar_width: (inner - 2 * (time_width + bar_gap)).max(0),
        controls_x: padding + (inner - row_width).max(0) / 2,
        controls_y,
        button_size,
        button_gap,
        label_y: controls_y + button_size + gap,
        label_height,
        height: (block + 2 * padding).min(height),
    }
}

/// How far one step of the volume slider moves it.
pub const VOLUME_STEP: i32 = 10;
/// The slider's top. Core takes up to 200; a volume above 100 reads as full
/// on the slider and steps back down from 100.
pub const VOLUME_MAX: i32 = 100;

/// Where one step of the slider takes `current`: the next multiple of ten
/// above or below, kept within 0 to 100.
pub fn volume_step(current: i32, forward: bool) -> i32 {
    if current > VOLUME_MAX {
        return VOLUME_MAX;
    }
    let current = current.max(0);
    if forward {
        ((current / VOLUME_STEP + 1) * VOLUME_STEP).min(VOLUME_MAX)
    } else if current % VOLUME_STEP == 0 {
        (current - VOLUME_STEP).max(0)
    } else {
        current / VOLUME_STEP * VOLUME_STEP
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests should fail fast")]
mod tests {
    use super::*;

    #[test]
    fn a_starting_track_holds_the_launching_controls_until_core_reports_it() {
        let mut start = TrackStart::default();
        let seq = start.begin(Some("/m/1.mp3"));
        assert!(start.pending());
        assert!(!start.heard(Some("/m/1.mp3")), "the same track is not it");
        assert!(!start.heard(None), "a gap between tracks is not it");
        assert!(start.heard(Some("/m/2.mp3")));
        assert!(!start.pending());
        assert!(!start.end(Some(seq)), "a late timeout ends nothing");
        let old = start.begin(None);
        let new = start.begin(None);
        assert!(!start.end(Some(old)), "an older request's timeout");
        assert!(start.end(Some(new)));
        start.begin(None);
        assert!(start.end(None), "a failed request ends the wait at once");

        let now = Now {
            has_track: true,
            has_playlist: true,
            playlist_total: 3,
            can_update_playlist: true,
            starting: true,
            ..Now::default()
        };
        for control in [Control::Previous, Control::PlayPause, Control::Next] {
            assert_eq!(unavailable(control, &now), Unavailable::Starting);
            assert_eq!(transport_command(control, &now), None);
        }
        assert_eq!(unavailable(Control::Shuffle, &now), Unavailable::None);
        assert_eq!(unavailable(Control::Repeat, &now), Unavailable::None);
    }
    use crate::sizing::{derive, Inputs};

    fn playlist_now() -> Now {
        Now {
            has_track: true,
            playback: Playback::Playing,
            controls: vec!["toggle_pause".into(), "stop".into()],
            has_playlist: true,
            playlist_playing: true,
            playlist_total: 12,
            can_update_playlist: true,
            shuffle: false,
            repeat: Repeat::All,
            starting: false,
        }
    }

    #[test]
    fn the_row_is_the_standard_five_and_unknowns_seat_on_play() {
        assert_eq!(
            CONTROLS.map(Control::as_str),
            ["shuffle", "previous", "play-pause", "next", "repeat"]
        );
        for control in CONTROLS {
            assert_eq!(Control::parse(control.as_str()), control);
            assert_eq!(Control::at(control.index()), control);
        }
        assert_eq!(
            Control::parse("rewind"),
            Control::PlayPause,
            "an older build's control"
        );
        assert_eq!(Control::PlayPause.step(true), Some(Control::Next));
        assert_eq!(Control::Shuffle.step(false), None, "no wrap");
        assert_eq!(Control::Repeat.step(true), None, "no wrap");
    }

    #[test]
    fn repeat_maps_both_spellings_and_cycles() {
        assert_eq!(Repeat::from_core("none"), Repeat::Off);
        assert_eq!(Repeat::from_core("off"), Repeat::Off);
        assert_eq!(Repeat::from_core("all"), Repeat::All);
        assert_eq!(Repeat::from_core("one"), Repeat::One);
        assert_eq!(Repeat::from_core("sideways"), Repeat::Off);
        assert_eq!(Repeat::Off.core_token(), "none");
        assert_eq!(Repeat::Off.zapscript_token(), "off");
        assert_eq!(Repeat::Off.next(), Repeat::All);
        assert_eq!(Repeat::All.next(), Repeat::One);
        assert_eq!(Repeat::One.next(), Repeat::Off);
    }

    #[test]
    fn a_playing_playlist_pauses_through_the_playlist() {
        let now = playlist_now();
        assert_eq!(
            transport_command(Control::PlayPause, &now),
            Some(Command::Run("**playlist.pause?slot=background".into()))
        );
        let paused = Now {
            playlist_playing: false,
            playback: Playback::Paused,
            ..playlist_now()
        };
        assert_eq!(
            transport_command(Control::PlayPause, &paused),
            Some(Command::Run("**playlist.play?slot=background".into()))
        );
        assert_eq!(
            transport_command(Control::Next, &now),
            Some(Command::Run("**playlist.next?slot=background".into()))
        );
        assert_eq!(
            stop_command(&now),
            Some(Command::Run("**playlist.stop?slot=background".into()))
        );
    }

    #[test]
    fn a_track_paused_outside_the_playlist_resumes_as_a_track() {
        let now = Now {
            playlist_playing: true,
            playback: Playback::Paused,
            controls: vec!["toggle_pause".into()],
            ..playlist_now()
        };
        assert_eq!(
            transport_command(Control::PlayPause, &now),
            Some(Command::Control("toggle_pause"))
        );
    }

    #[test]
    fn a_lone_track_uses_the_launcher_controls() {
        let now = Now {
            has_playlist: false,
            playlist_playing: false,
            playlist_total: 0,
            ..playlist_now()
        };
        assert_eq!(
            transport_command(Control::PlayPause, &now),
            Some(Command::Control("toggle_pause"))
        );
        assert_eq!(stop_command(&now), Some(Command::Control("stop")));
        assert_eq!(unavailable(Control::Next, &now), Unavailable::NoPlaylist);
        assert_eq!(unavailable(Control::Shuffle, &now), Unavailable::NoPlaylist);
        assert_eq!(transport_command(Control::Next, &now), None);
        let no_toggle = Now {
            controls: Vec::new(),
            ..now
        };
        assert_eq!(
            unavailable(Control::PlayPause, &no_toggle),
            Unavailable::NotSupported
        );
    }

    #[test]
    fn shuffle_and_repeat_flip_through_playlists_update() {
        let now = playlist_now();
        assert_eq!(
            transport_command(Control::Shuffle, &now),
            Some(Command::Update {
                shuffle: Some(true),
                repeat: None
            })
        );
        assert_eq!(
            transport_command(Control::Repeat, &now),
            Some(Command::Update {
                shuffle: None,
                repeat: Some(Repeat::One)
            })
        );
        let old_core = Now {
            can_update_playlist: false,
            ..playlist_now()
        };
        assert_eq!(
            unavailable(Control::Shuffle, &old_core),
            Unavailable::NotSupported
        );
        assert_eq!(
            unavailable(Control::Repeat, &old_core),
            Unavailable::NotSupported
        );
    }

    #[test]
    fn nothing_on_the_slot_leaves_every_control_unavailable() {
        let idle = Now::default();
        for control in CONTROLS {
            assert_ne!(
                unavailable(control, &idle),
                Unavailable::None,
                "{control:?}"
            );
            assert_eq!(transport_command(control, &idle), None);
        }
        assert_eq!(stop_command(&idle), None);
        assert_eq!(
            unavailable(Control::PlayPause, &idle),
            Unavailable::NothingPlaying
        );
    }

    #[test]
    fn up_next_follows_the_repeat_mode() {
        assert_eq!(up_next(1, 3, Repeat::Off), Some(2));
        assert_eq!(up_next(2, 3, Repeat::Off), None, "the last plays alone");
        assert_eq!(up_next(2, 3, Repeat::All), Some(0), "around again");
        assert_eq!(up_next(2, 3, Repeat::One), Some(2), "the same again");
        assert_eq!(up_next(0, 0, Repeat::All), None);
    }

    #[test]
    fn goto_counts_from_one() {
        assert_eq!(goto_script(0, true), "**playlist.goto:1?slot=background");
        assert_eq!(goto_script(4, true), "**playlist.goto:5?slot=background");
        assert_eq!(
            goto_script(2, false),
            "**playlist.goto:3?slot=background||**playlist.play?slot=background",
            "a paused playlist is resumed on the new item"
        );
    }

    #[test]
    fn a_folder_plays_by_its_path_whatever_its_size() {
        assert_eq!(
            play_folder_script("/music/Crises, Live?", false, Repeat::All, Some(1)),
            "**playlist.play:/music/Crises^, Live^??slot=background&system=Audio&repeat=all&start=2"
        );
        assert_eq!(
            play_folder_script("/m", true, Repeat::Off, None),
            "**playlist.play:/m?slot=background&system=Audio&repeat=off&mode=shuffle"
        );
    }

    #[test]
    fn the_clock_runs_only_while_playing_and_stops_at_the_end() {
        let mut clock = PlaybackClock::default();
        clock.sync(Some(1000), Some(5000), true, 10_000);
        assert_eq!(clock.position_at(10_000), 1000);
        assert_eq!(clock.position_at(12_500), 3500);
        assert_eq!(clock.position_at(99_000), 5000, "never past the end");
        clock.hold(11_000);
        assert_eq!(clock.position_at(20_000), 2000, "held where it was");
        clock.sync(Some(2000), Some(0), false, 0);
        assert_eq!(clock.duration(), None, "a zero length is unknown");
        assert_eq!(clock.position_at(5000), 2000);
    }

    #[test]
    fn times_read_as_minutes_or_hours() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(61_999), "1:01");
        assert_eq!(clock_text(3_600_000), "1:00:00");
        assert_eq!(clock_text(3_725_000), "1:02:05");
    }

    #[test]
    fn progress_reads_in_thousandths() {
        assert_eq!(progress_milli(0, Some(100)), 0);
        assert_eq!(progress_milli(50, Some(100)), 500);
        assert_eq!(progress_milli(200, Some(100)), 1000);
        assert_eq!(progress_milli(50, None), 0);
    }

    #[test]
    fn polling_needs_the_screen_an_awake_frontend_and_music() {
        assert!(should_poll(true, true, true));
        assert!(!should_poll(false, true, true));
        assert!(!should_poll(true, false, true));
        assert!(!should_poll(true, true, false));
    }

    #[test]
    fn a_saved_row_holds_only_while_it_holds_the_same_item() {
        let items: Vec<String> = ["a", "b", "c"].iter().map(|s| (*s).to_string()).collect();
        assert_eq!(reseat_row(1, "b", &items, 0), 1);
        assert_eq!(reseat_row(0, "c", &items, 0), 2, "found where it moved");
        assert_eq!(reseat_row(1, "z", &items, 2), 2, "else the current track");
        assert_eq!(reseat_row(0, "z", &[], 4), 0);
    }

    #[test]
    fn a_picked_track_counts_among_the_tracks_only() {
        let rows = [
            (EntryKind::Folder, "/m/Live"),
            (EntryKind::Track, "/m/1.mp3"),
            (EntryKind::Track, "/m/2.mp3"),
        ];
        assert_eq!(track_start(&rows, "/m/2.mp3"), Some(1));
        assert_eq!(track_start(&rows, "/m/Live"), None);
        assert_eq!(track_start(&rows, "/m/9.mp3"), None);
    }

    #[test]
    fn the_volume_slider_steps_by_tens_within_its_range() {
        assert_eq!(volume_step(80, true), 90);
        assert_eq!(volume_step(80, false), 70);
        assert_eq!(volume_step(100, true), 100, "full stays full");
        assert_eq!(volume_step(0, false), 0, "silent stays silent");
        assert_eq!(
            volume_step(65, true),
            70,
            "an odd value goes to the next ten"
        );
        assert_eq!(volume_step(65, false), 60);
        assert_eq!(
            volume_step(150, false),
            100,
            "above the slider it steps to full"
        );
        assert_eq!(volume_step(150, true), 100);
    }

    #[test]
    fn the_player_fits_its_card_on_whole_pixels_at_every_tier() {
        for (w, h, crt) in [
            (320.0, 240.0, true),
            (640.0, 480.0, false),
            (960.0, 540.0, false),
            (1280.0, 720.0, false),
            (1920.0, 1080.0, false),
            (720.0, 1280.0, false),
        ] {
            let inputs = Inputs {
                screen_width: w,
                screen_height: h,
                crt_native_path: crt,
                bitmap_type: crt,
                ..Inputs::default()
            };
            let derived = derive(&inputs);
            let page = crate::settings::page_geometry(&inputs);
            let g = geometry(&derived, page.card_width, page.card_height, page.pad);
            let row = 5 * g.button_size + 4 * g.button_gap;
            assert!(
                g.controls_x + row <= page.card_width,
                "{w}x{h}: controls overflow"
            );
            assert!(g.button_size >= 8, "{w}x{h}");
            assert!(g.text_width > 0, "{w}x{h}: no room for text");
            assert!(
                g.bar_width > page.card_width / 2,
                "{w}x{h}: the bar spans the card"
            );
            assert!(
                g.label_y + g.label_height <= g.height,
                "{w}x{h}: the block fits the card"
            );
            assert!(g.height <= page.card_height, "{w}x{h}: within the room");
            assert_eq!(
                g.height - (g.label_y + g.label_height),
                g.art_y,
                "{w}x{h}: the card hugs the block, padding above and below"
            );
            assert_eq!(g.album_line, derived.tier != Tier::T240);
            assert_eq!(
                g.art_size,
                g.title_height + g.line_height * if g.album_line { 3 } else { 2 },
                "{w}x{h}: the art is as tall as the text beside it at every tier"
            );
        }
    }
}
