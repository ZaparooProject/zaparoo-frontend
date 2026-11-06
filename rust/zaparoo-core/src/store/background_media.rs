// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `BackgroundMediaResource`: what Core's background slot is playing and
// the playlist open on it, as one `watch` channel. The music player and
// the header's music icon read it.
//
// Seeded from `media` on every connect. After that, `playlists.changed`
// carries the playlist, and a background `media.started`/`media.stopped`
// asks `media.active` for the track, which also brings its tags and
// position. A Core that predates `playlists.changed` gets a full `media`
// read on each track change instead, so its playlist index still moves.
//
// Kept apart from `MediaStatusResource` so a track change does not wake
// the status line, Settings and the Hub, which only care about the
// primary slot.

use crate::client::{Client, ClientError, ConnectionState, Notification};
use crate::media_types::{
    is_background_slot, ActiveMediaInfo, MediaResult, PlaylistState, PlaylistsChanged,
    SLOT_BACKGROUND,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

/// The background slot as the frontend last heard it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackgroundMediaState {
    /// The first `media` read has landed since the last connect.
    pub seeded: bool,
    /// The track on the background slot, `None` when it is idle.
    pub track: Option<ActiveMediaInfo>,
    /// The playlist open on the background slot. Shared, so cloning the
    /// state out of the channel never copies a long item list.
    pub playlist: Option<Arc<PlaylistState>>,
    /// This Core pushes `media.playback`, so a reader only needs to ask
    /// for the track now and then as a backup.
    pub pushes_playback: bool,
}

impl BackgroundMediaState {
    /// Music is coming out of the speakers right now.
    pub fn playing(&self) -> bool {
        self.track.as_ref().is_some_and(|track| {
            track.playback_state != "paused" && track.playback_state != "stopped"
        })
    }

    /// A `media.playback` push: the slot's whole track, replaced as Core
    /// sent it. Another slot's push changes nothing. Returns whether the
    /// state changed.
    fn apply_playback(&mut self, track: ActiveMediaInfo) -> bool {
        if !is_background_slot(&track.slot) {
            return false;
        }
        let track = Some(track);
        let changed = self.track != track || !self.pushes_playback;
        self.track = track;
        self.pushes_playback = true;
        changed
    }

    fn apply_media(&mut self, media: &MediaResult) {
        self.track = background_track(&media.active);
        self.playlist = background_playlist(&media.playlists);
        self.seeded = true;
    }
}

fn background_track(active: &[ActiveMediaInfo]) -> Option<ActiveMediaInfo> {
    active
        .iter()
        .find(|item| is_background_slot(&item.slot))
        .cloned()
}

fn background_playlist(playlists: &[PlaylistState]) -> Option<Arc<PlaylistState>> {
    playlists
        .iter()
        .find(|playlist| is_background_slot(&playlist.slot))
        .cloned()
        .map(Arc::new)
}

/// Live background-slot publisher. One per `Store`.
pub struct BackgroundMediaResource {
    state: Arc<watch::Sender<BackgroundMediaState>>,
    client: Arc<Client>,
}

impl std::fmt::Debug for BackgroundMediaResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundMediaResource")
            .finish_non_exhaustive()
    }
}

impl BackgroundMediaResource {
    pub fn new(client: &Arc<Client>, runtime: &Handle) -> Arc<Self> {
        let (state_tx, _) = watch::channel(BackgroundMediaState::default());
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

    pub fn subscribe(&self) -> watch::Receiver<BackgroundMediaState> {
        self.state.subscribe()
    }

    /// The state as it stands now.
    pub fn current(&self) -> BackgroundMediaState {
        self.state.borrow().clone()
    }

    /// Ask Core for the track again: its position, and whether it is
    /// paused, which no notification reports.
    pub async fn resync(&self) -> Result<(), ClientError> {
        refresh_track(&self.client, &self.state).await
    }
}

/// Re-seed delay after a failed `media` read while still connected.
const SEED_RETRY_DELAY: Duration = Duration::from_secs(2);

async fn run_task(
    client: Arc<Client>,
    mut connection_rx: watch::Receiver<ConnectionState>,
    mut notifications_rx: broadcast::Receiver<Notification>,
    state: Arc<watch::Sender<BackgroundMediaState>>,
) {
    // Whether this Core sends `playlists.changed`; until it does, a track
    // change reads the whole `media` answer to keep the playlist current.
    let notifies = AtomicBool::new(false);
    loop {
        let current = connection_rx.borrow_and_update().clone();
        if matches!(current, ConnectionState::Connected) {
            seed_now(&client, &state).await;
            loop {
                tokio::select! {
                    notification = notifications_rx.recv() => {
                        match notification {
                            Ok(n) => fold(&client, &state, &notifies, &n).await,
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!(skipped, "background_media: notifications lagged; re-seeding");
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
                *s = BackgroundMediaState::default();
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
    state: &Arc<watch::Sender<BackgroundMediaState>>,
    notifies: &AtomicBool,
    notification: &Notification,
) {
    match notification.method.as_str() {
        "playlists.changed" => {
            notifies.store(true, Ordering::Relaxed);
            match serde_json::from_value::<PlaylistsChanged>(notification.params.clone()) {
                Ok(changed) => {
                    let playlist = background_playlist(&changed.playlists);
                    state.send_if_modified(|s| {
                        if s.playlist == playlist {
                            return false;
                        }
                        s.playlist = playlist;
                        true
                    });
                }
                Err(e) => {
                    warn!("background_media: playlists.changed did not decode: {e}");
                    seed_now(client, state).await;
                }
            }
        }
        "media.playback" => {
            match serde_json::from_value::<ActiveMediaInfo>(notification.params.clone()) {
                Ok(track) => {
                    state.send_if_modified(|s| s.apply_playback(track));
                }
                Err(e) => warn!("background_media: media.playback did not decode: {e}"),
            }
        }
        "media.started" | "media.stopped" => {
            let slot = notification
                .params
                .get("slot")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if !is_background_slot(slot) {
                return;
            }
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
                    refresh_after_change(&client, &state, notified).await;
                });
                return;
            }
            refresh_after_change(client, state, notifies.load(Ordering::Relaxed)).await;
        }
        _ => {}
    }
}

/// How long a background `media.stopped` waits for the next track before
/// the slot reads as idle.
const STOP_GRACE: Duration = Duration::from_millis(750);

/// Read the slot again after a track started or stopped: just the track
/// where Core sends `playlists.changed`, the whole `media` answer where it
/// does not, so the playlist's place still moves.
async fn refresh_after_change(
    client: &Arc<Client>,
    state: &Arc<watch::Sender<BackgroundMediaState>>,
    notifies: bool,
) {
    if notifies {
        if let Err(e) = refresh_track(client, state).await {
            debug!("background_media: media.active failed, reading media: {e}");
            seed_now(client, state).await;
        }
    } else {
        seed_now(client, state).await;
    }
}

async fn refresh_track(
    client: &Arc<Client>,
    state: &Arc<watch::Sender<BackgroundMediaState>>,
) -> Result<(), ClientError> {
    let track = client.media_active(SLOT_BACKGROUND).await?;
    state.send_if_modified(|s| {
        if s.track == track {
            return false;
        }
        s.track = track;
        true
    });
    Ok(())
}

async fn seed_now(client: &Arc<Client>, state: &Arc<watch::Sender<BackgroundMediaState>>) {
    match client.media().await {
        Ok(media) => {
            state.send_if_modified(|s| {
                let was = s.clone();
                s.apply_media(&media);
                *s != was
            });
            debug!("background_media: seeded from media query");
        }
        Err(e) => {
            warn!("background_media: media seed failed: {e}");
            sleep(SEED_RETRY_DELAY).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(slot: &str, state: &str) -> ActiveMediaInfo {
        ActiveMediaInfo {
            slot: slot.to_string(),
            media_name: "Moonlight Shadow".to_string(),
            playback_state: state.to_string(),
            ..ActiveMediaInfo::default()
        }
    }

    fn playlist(slot: &str) -> PlaylistState {
        PlaylistState {
            id: "/music/crises".to_string(),
            slot: slot.to_string(),
            total: 2,
            ..PlaylistState::default()
        }
    }

    #[test]
    fn the_seed_keeps_only_the_background_slot() {
        let media = MediaResult {
            active: vec![track("primary", "playing"), track("background", "paused")],
            playlists: vec![playlist("primary"), playlist("background")],
            ..MediaResult::default()
        };
        let mut state = BackgroundMediaState::default();
        state.apply_media(&media);
        assert!(state.seeded);
        assert_eq!(
            state.track.as_ref().map(|t| t.slot.as_str()),
            Some("background")
        );
        assert_eq!(
            state.playlist.as_ref().map(|p| p.slot.as_str()),
            Some("background")
        );
        assert!(!state.playing(), "a paused track is not playing");
    }

    #[test]
    fn an_idle_background_slot_reads_as_nothing() {
        let media = MediaResult {
            active: vec![track("", "playing")],
            ..MediaResult::default()
        };
        let mut state = BackgroundMediaState::default();
        state.apply_media(&media);
        assert!(state.track.is_none());
        assert!(state.playlist.is_none());
        assert!(!state.playing());
    }

    #[test]
    fn a_playback_push_replaces_the_track_and_marks_the_core() {
        let mut state = BackgroundMediaState::default();
        assert!(
            !state.apply_playback(track("primary", "playing")),
            "another slot"
        );
        assert!(state.track.is_none());
        assert!(state.apply_playback(track("background", "paused")));
        assert!(state.pushes_playback);
        assert!(!state.playing());
        assert!(
            !state.apply_playback(track("background", "paused")),
            "no change"
        );
        assert!(state.apply_playback(track("background", "playing")));
        assert!(state.playing());
    }

    #[test]
    fn the_bg_alias_counts_as_background() {
        assert!(is_background_slot("bg"));
        assert!(is_background_slot("background"));
        assert!(!is_background_slot(""));
        assert!(!is_background_slot("primary"));
    }

    #[test]
    fn a_track_with_no_state_plays() {
        let state = BackgroundMediaState {
            track: Some(track("background", "")),
            ..BackgroundMediaState::default()
        };
        assert!(state.playing());
    }
}
