// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MusicMediaResource`: the music Core plays and the playlists open on its
// slots, as one `watch` channel. The music player and the header's music
// icon read it. Two slots carry music: the background slot, which plays
// alongside a game, and the primary slot while native audio plays there
// in place of a game (an album, an audio CD). A game on the primary slot
// is not music and reads as nothing here.
//
// Seeded from `media` on every connect. After that, `playlists.changed`
// carries the playlists, `media.playback` the playing track, and a
// `media.started`/`media.stopped` on either slot asks `media.active` for
// that slot's track, which also brings its tags and position. A Core that
// predates `playlists.changed` gets a full `media` read on each track
// change instead, so its playlist index still moves.
//
// Kept apart from `MediaStatusResource` so a track change does not wake
// the status line, Settings and the Hub.

use crate::client::{Client, ClientError, ConnectionState, Notification};
use crate::media_types::{
    is_background_slot, is_native_audio, is_primary_slot, ActiveMediaInfo, MediaResult,
    PlaylistState, PlaylistsChanged, SLOT_BACKGROUND, SLOT_PRIMARY,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

/// A slot that carries music.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MusicSlot {
    #[default]
    Background,
    Primary,
}

impl MusicSlot {
    /// The slot as Core names it on the wire.
    pub fn token(self) -> &'static str {
        match self {
            Self::Background => SLOT_BACKGROUND,
            Self::Primary => SLOT_PRIMARY,
        }
    }

    /// The music slot Core's `slot` names, if it names one.
    pub fn of(slot: &str) -> Option<Self> {
        if is_background_slot(slot) {
            Some(Self::Background)
        } else if is_primary_slot(slot) {
            Some(Self::Primary)
        } else {
            None
        }
    }
}

/// One slot's music: its track and its open playlist.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlotMedia {
    /// The track on the slot, `None` when it plays no music.
    pub track: Option<ActiveMediaInfo>,
    /// The playlist open on the slot. Shared, so cloning the state out of
    /// the channel never copies a long item list.
    pub playlist: Option<Arc<PlaylistState>>,
}

impl SlotMedia {
    /// Music is coming out of the speakers from this slot right now. A
    /// paused playlist wins over a track update that has not landed yet,
    /// as the player's own controls read it.
    pub fn playing(&self) -> bool {
        if self.playlist.as_ref().is_some_and(|p| !p.playing) {
            return false;
        }
        self.track.as_ref().is_some_and(|track| {
            track.playback_state != "paused" && track.playback_state != "stopped"
        })
    }
}

/// The music slots as the frontend last heard them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MusicMediaState {
    /// The first `media` read has landed since the last connect.
    pub seeded: bool,
    pub background: SlotMedia,
    /// Native audio on the primary slot. A game there is not kept.
    pub primary: SlotMedia,
    /// This Core pushes `media.playback`, so a reader only needs to ask
    /// for the track now and then as a backup.
    pub pushes_playback: bool,
}

impl MusicMediaState {
    /// Music is coming out of the speakers from either slot right now.
    pub fn playing(&self) -> bool {
        self.background.playing() || self.primary.playing()
    }

    /// The slot the player shows. Primary music wins while it plays; a
    /// paused album stays on show (so Play is in reach) unless the
    /// background slot is playing.
    pub fn shown_slot(&self) -> MusicSlot {
        if self.primary.track.is_some() && (self.primary.playing() || !self.background.playing()) {
            MusicSlot::Primary
        } else {
            MusicSlot::Background
        }
    }

    /// Music started on the primary slot since `before`: an album or a
    /// disc, not the next track of one already playing. Nothing starts
    /// before the first answer, so a cold start mid-album opens nothing.
    pub fn primary_started(&self, before: &Self) -> bool {
        before.seeded && before.primary.track.is_none() && self.primary.track.is_some()
    }

    /// The slot the player shows, as it stands.
    pub fn shown(&self) -> &SlotMedia {
        self.slot(self.shown_slot())
    }

    pub fn slot(&self, slot: MusicSlot) -> &SlotMedia {
        match slot {
            MusicSlot::Background => &self.background,
            MusicSlot::Primary => &self.primary,
        }
    }

    fn slot_mut(&mut self, slot: MusicSlot) -> &mut SlotMedia {
        match slot {
            MusicSlot::Background => &mut self.background,
            MusicSlot::Primary => &mut self.primary,
        }
    }

    /// Set one slot's track as Core reported it. Media on the primary slot
    /// that is not native audio is a game, and reads as none. Returns
    /// whether the state changed.
    fn set_track(&mut self, slot: MusicSlot, track: Option<ActiveMediaInfo>) -> bool {
        let track = track.filter(|t| slot == MusicSlot::Background || is_native_audio(t));
        let current = &mut self.slot_mut(slot).track;
        if *current == track {
            return false;
        }
        *current = track;
        true
    }

    /// A `media.playback` push: the slot's whole track, replaced as Core
    /// sent it. Returns whether the state changed.
    fn apply_playback(&mut self, track: ActiveMediaInfo) -> bool {
        let Some(slot) = MusicSlot::of(&track.slot) else {
            return false;
        };
        let first = !self.pushes_playback;
        self.pushes_playback = true;
        self.set_track(slot, Some(track)) || first
    }

    /// Both slots' playlists, from `media` or `playlists.changed`.
    /// Returns whether the state changed.
    fn apply_playlists(&mut self, playlists: &[PlaylistState]) -> bool {
        let mut changed = false;
        for slot in [MusicSlot::Background, MusicSlot::Primary] {
            let playlist = slot_playlist(playlists, slot);
            let current = &mut self.slot_mut(slot).playlist;
            if *current != playlist {
                *current = playlist;
                changed = true;
            }
        }
        changed
    }

    fn apply_media(&mut self, media: &MediaResult) {
        for slot in [MusicSlot::Background, MusicSlot::Primary] {
            self.set_track(slot, slot_track(&media.active, slot));
        }
        self.apply_playlists(&media.playlists);
        self.seeded = true;
    }
}

fn slot_track(active: &[ActiveMediaInfo], slot: MusicSlot) -> Option<ActiveMediaInfo> {
    active
        .iter()
        .find(|item| MusicSlot::of(&item.slot) == Some(slot))
        .cloned()
}

fn slot_playlist(playlists: &[PlaylistState], slot: MusicSlot) -> Option<Arc<PlaylistState>> {
    playlists
        .iter()
        .find(|playlist| MusicSlot::of(&playlist.slot) == Some(slot))
        .cloned()
        .map(Arc::new)
}

/// Live music-slot publisher. One per `Store`.
pub struct MusicMediaResource {
    state: Arc<watch::Sender<MusicMediaState>>,
    client: Arc<Client>,
}

impl std::fmt::Debug for MusicMediaResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicMediaResource").finish_non_exhaustive()
    }
}

impl MusicMediaResource {
    pub fn new(client: &Arc<Client>, runtime: &Handle) -> Arc<Self> {
        let (state_tx, _) = watch::channel(MusicMediaState::default());
        let state = Arc::new(state_tx);
        let resource = Arc::new(Self {
            state: state.clone(),
            client: client.clone(),
        });
        let connection_rx = client.connection.subscribe();
        let notifications_rx = client.subscribe_notifications();
        let client = client.clone();
        runtime.spawn(async move {
            run_task(client, connection_rx, notifications_rx, state).await;
        });
        resource
    }

    pub fn subscribe(&self) -> watch::Receiver<MusicMediaState> {
        self.state.subscribe()
    }

    /// The state as it stands now.
    pub fn current(&self) -> MusicMediaState {
        self.state.borrow().clone()
    }

    /// Ask Core for the shown slot's track again: its position, and
    /// whether it is paused, which an older Core does not push.
    pub async fn resync(&self) -> Result<(), ClientError> {
        let slot = self.state.borrow().shown_slot();
        refresh_track(&self.client, &self.state, slot).await
    }
}

/// Re-seed delay after a failed `media` read while still connected.
const SEED_RETRY_DELAY: Duration = Duration::from_secs(2);

async fn run_task(
    client: Arc<Client>,
    mut connection_rx: watch::Receiver<ConnectionState>,
    mut notifications_rx: broadcast::Receiver<Notification>,
    state: Arc<watch::Sender<MusicMediaState>>,
) {
    // Whether this Core sends `playlists.changed`; until it does, a track
    // change reads the whole `media` answer to keep the playlist current.
    let notifies = AtomicBool::new(false);
    loop {
        let current = connection_rx.borrow_and_update().clone();
        if matches!(current, ConnectionState::Connected) {
            while !seed_now(&client, &state).await
                && matches!(*connection_rx.borrow(), ConnectionState::Connected)
            {}
            loop {
                tokio::select! {
                    notification = notifications_rx.recv() => {
                        match notification {
                            Ok(n) => fold(&client, &state, &notifies, &n).await,
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!(skipped, "music_media: notifications lagged; re-seeding");
                                seed_now(&client, &state).await;
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                    changed = connection_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        } else {
            // Nothing is known about a Core we are not connected to.
            state.send_if_modified(|s| {
                let was = s.clone();
                *s = MusicMediaState::default();
                *s != was
            });
            if connection_rx.changed().await.is_err() {
                return;
            }
        }
    }
}

async fn fold(
    client: &Arc<Client>,
    state: &Arc<watch::Sender<MusicMediaState>>,
    notifies: &AtomicBool,
    notification: &Notification,
) {
    match notification.method.as_str() {
        "playlists.changed" => {
            notifies.store(true, Ordering::Relaxed);
            match serde_json::from_value::<PlaylistsChanged>(notification.params.clone()) {
                Ok(changed) => {
                    state.send_if_modified(|s| s.apply_playlists(&changed.playlists));
                }
                Err(e) => {
                    warn!("music_media: playlists.changed did not decode: {e}");
                    seed_now(client, state).await;
                }
            }
        }
        "media.playback" => {
            match serde_json::from_value::<ActiveMediaInfo>(notification.params.clone()) {
                Ok(track) => {
                    state.send_if_modified(|s| s.apply_playback(track));
                }
                Err(e) => warn!("music_media: media.playback did not decode: {e}"),
            }
        }
        "media.started" | "media.stopped" => {
            let slot = notification
                .params
                .get("slot")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let Some(slot) = MusicSlot::of(slot) else {
                return;
            };
            if notification.method == "media.stopped" {
                // A track change stops one track and starts the next. The
                // slot is only idle if nothing has started once the grace
                // runs out, so the card never blanks in between.
                let reason = notification
                    .params
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if reason == "replaced" {
                    return;
                }
                let client = client.clone();
                let state = state.clone();
                let notified = notifies.load(Ordering::Relaxed);
                tokio::spawn(async move {
                    sleep(STOP_GRACE).await;
                    refresh_after_change(&client, &state, notified, slot).await;
                });
                return;
            }
            refresh_after_change(client, state, notifies.load(Ordering::Relaxed), slot).await;
        }
        _ => {}
    }
}

/// How long a `media.stopped` waits for the next track before the slot
/// reads as idle.
const STOP_GRACE: Duration = Duration::from_millis(750);

/// Read the slot again after a track started or stopped: just the track
/// where Core sends `playlists.changed`, the whole `media` answer where it
/// does not, so the playlist's place still moves.
async fn refresh_after_change(
    client: &Arc<Client>,
    state: &Arc<watch::Sender<MusicMediaState>>,
    notifies: bool,
    slot: MusicSlot,
) {
    if notifies {
        if let Err(e) = refresh_track(client, state, slot).await {
            debug!("music_media: media.active failed, reading media: {e}");
            seed_now(client, state).await;
        }
    } else {
        seed_now(client, state).await;
    }
}

async fn refresh_track(
    client: &Arc<Client>,
    state: &Arc<watch::Sender<MusicMediaState>>,
    slot: MusicSlot,
) -> Result<(), ClientError> {
    let track = client.media_active(slot.token()).await?;
    state.send_if_modified(|s| s.set_track(slot, track));
    Ok(())
}

/// Seed from `media`; false when the read failed.
async fn seed_now(client: &Arc<Client>, state: &Arc<watch::Sender<MusicMediaState>>) -> bool {
    match client.media().await {
        Ok(media) => {
            state.send_if_modified(|s| {
                let was = s.clone();
                s.apply_media(&media);
                *s != was
            });
            debug!("music_media: seeded from media query");
            true
        }
        Err(e) => {
            warn!("music_media: media seed failed: {e}");
            sleep(SEED_RETRY_DELAY).await;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media_types::NATIVE_AUDIO_LAUNCHER;

    fn track(slot: &str, state: &str) -> ActiveMediaInfo {
        ActiveMediaInfo {
            slot: slot.to_string(),
            launcher_id: NATIVE_AUDIO_LAUNCHER.to_string(),
            media_name: "Moonlight Shadow".to_string(),
            playback_state: state.to_string(),
            ..ActiveMediaInfo::default()
        }
    }

    fn game(slot: &str) -> ActiveMediaInfo {
        ActiveMediaInfo {
            launcher_id: "Steam".to_string(),
            system_id: "PC".to_string(),
            ..track(slot, "")
        }
    }

    fn playlist(slot: &str) -> PlaylistState {
        PlaylistState {
            id: format!("/music/{slot}"),
            slot: slot.to_string(),
            total: 2,
            ..PlaylistState::default()
        }
    }

    #[test]
    fn the_seed_keeps_each_music_slot() {
        let media = MediaResult {
            active: vec![track("primary", "playing"), track("background", "paused")],
            playlists: vec![
                PlaylistState {
                    playing: true,
                    ..playlist("primary")
                },
                playlist("background"),
            ],
            ..MediaResult::default()
        };
        let mut state = MusicMediaState::default();
        state.apply_media(&media);
        assert!(state.seeded);
        assert_eq!(
            state.background.track.as_ref().map(|t| t.slot.as_str()),
            Some("background")
        );
        assert_eq!(
            state.background.playlist.as_ref().map(|p| p.slot.as_str()),
            Some("background")
        );
        assert_eq!(
            state.primary.track.as_ref().map(|t| t.slot.as_str()),
            Some("primary")
        );
        assert!(!state.background.playing(), "a paused track is not playing");
        assert!(state.playing(), "the primary album plays");
        assert_eq!(state.shown_slot(), MusicSlot::Primary);
        assert_eq!(
            state.shown().playlist.as_ref().map(|p| p.slot.as_str()),
            Some("primary")
        );
    }

    #[test]
    fn a_game_on_the_primary_slot_is_not_music() {
        let media = MediaResult {
            active: vec![game(""), track("background", "playing")],
            ..MediaResult::default()
        };
        let mut state = MusicMediaState::default();
        state.apply_media(&media);
        assert!(state.primary.track.is_none());
        assert_eq!(state.shown_slot(), MusicSlot::Background);
        state.pushes_playback = true;
        assert!(!state.apply_playback(game("primary")), "no change");
        assert!(state.primary.track.is_none());
    }

    #[test]
    fn an_idle_background_slot_reads_as_nothing() {
        let media = MediaResult {
            active: vec![game("")],
            ..MediaResult::default()
        };
        let mut state = MusicMediaState::default();
        state.apply_media(&media);
        assert!(state.background.track.is_none());
        assert!(state.background.playlist.is_none());
        assert!(!state.playing());
        assert_eq!(state.shown_slot(), MusicSlot::Background);
    }

    #[test]
    fn primary_music_wins_while_it_plays() {
        let mut state = MusicMediaState {
            background: SlotMedia {
                track: Some(track("background", "playing")),
                playlist: None,
            },
            ..MusicMediaState::default()
        };
        assert_eq!(state.shown_slot(), MusicSlot::Background);
        state.set_track(MusicSlot::Primary, Some(track("primary", "playing")));
        assert_eq!(state.shown_slot(), MusicSlot::Primary);
        state.set_track(MusicSlot::Primary, Some(track("primary", "paused")));
        assert_eq!(
            state.shown_slot(),
            MusicSlot::Background,
            "the background music is what is heard"
        );
        state.set_track(MusicSlot::Background, Some(track("background", "paused")));
        assert_eq!(
            state.shown_slot(),
            MusicSlot::Primary,
            "a paused album stays in reach"
        );
        state.set_track(MusicSlot::Primary, None);
        assert_eq!(state.shown_slot(), MusicSlot::Background);
    }

    #[test]
    fn a_paused_primary_playlist_yields_to_background_music() {
        let mut state = MusicMediaState {
            background: SlotMedia {
                track: Some(track("background", "playing")),
                playlist: None,
            },
            ..MusicMediaState::default()
        };
        state.set_track(MusicSlot::Primary, Some(track("primary", "playing")));
        let primary = PlaylistState {
            playing: true,
            ..playlist("primary")
        };
        assert!(state.apply_playlists(std::slice::from_ref(&primary)));
        assert_eq!(state.shown_slot(), MusicSlot::Primary, "its playlist plays");
        // Paused by playlists.changed before the track update arrives.
        assert!(state.apply_playlists(&[PlaylistState {
            playing: false,
            ..primary
        }]));
        assert!(!state.primary.playing());
        assert_eq!(
            state.shown_slot(),
            MusicSlot::Background,
            "the background music is what is heard"
        );
    }

    #[test]
    fn music_starting_on_the_primary_slot_is_an_edge() {
        let unseeded = MusicMediaState::default();
        let mut idle = MusicMediaState {
            seeded: true,
            ..MusicMediaState::default()
        };
        let mut playing = idle.clone();
        playing.set_track(MusicSlot::Primary, Some(track("primary", "playing")));
        assert!(playing.primary_started(&idle));
        assert!(
            !playing.primary_started(&unseeded),
            "a cold start mid-album is not a start"
        );
        let mut next = playing.clone();
        next.set_track(MusicSlot::Primary, Some(track("primary", "paused")));
        assert!(!next.primary_started(&playing), "the same album moving on");
        idle.set_track(MusicSlot::Primary, Some(game("primary")));
        let mut game_then_music = idle.clone();
        game_then_music.set_track(MusicSlot::Primary, Some(track("primary", "playing")));
        assert!(
            game_then_music.primary_started(&idle),
            "a game is not music"
        );
        let mut background = idle.clone();
        background.set_track(MusicSlot::Background, Some(track("background", "playing")));
        assert!(!background.primary_started(&idle));
    }

    #[test]
    fn a_playback_push_replaces_its_slot_and_marks_the_core() {
        let mut state = MusicMediaState::default();
        assert!(state.apply_playback(track("primary", "playing")));
        assert!(state.pushes_playback);
        assert!(state.primary.playing());
        assert!(state.background.track.is_none());
        assert!(state.apply_playback(track("background", "paused")));
        assert!(!state.background.playing());
        assert!(
            !state.apply_playback(track("background", "paused")),
            "no change"
        );
        assert!(state.apply_playback(track("background", "playing")));
        assert!(state.background.playing());
        assert!(!state.apply_playback(track("elsewhere", "playing")));
    }

    #[test]
    fn playlists_land_on_their_own_slots() {
        let mut state = MusicMediaState::default();
        assert!(state.apply_playlists(&[playlist("")]));
        assert_eq!(
            state.primary.playlist.as_ref().map(|p| p.id.as_str()),
            Some("/music/")
        );
        assert!(state.background.playlist.is_none());
        assert!(!state.apply_playlists(&[playlist("")]), "no change");
        assert!(state.apply_playlists(&[playlist("bg")]));
        assert!(state.primary.playlist.is_none());
        assert!(state.background.playlist.is_some());
    }

    #[test]
    fn slots_parse_with_their_aliases() {
        assert_eq!(MusicSlot::of("bg"), Some(MusicSlot::Background));
        assert_eq!(MusicSlot::of("background"), Some(MusicSlot::Background));
        assert_eq!(MusicSlot::of(""), Some(MusicSlot::Primary));
        assert_eq!(MusicSlot::of("primary"), Some(MusicSlot::Primary));
        assert_eq!(MusicSlot::of("other"), None);
        assert_eq!(MusicSlot::Primary.token(), "primary");
        assert_eq!(MusicSlot::Background.token(), "background");
    }

    #[test]
    fn a_track_with_no_state_plays() {
        let state = MusicMediaState {
            background: SlotMedia {
                track: Some(track("background", "")),
                playlist: None,
            },
            ..MusicMediaState::default()
        };
        assert!(state.playing());
    }
}
