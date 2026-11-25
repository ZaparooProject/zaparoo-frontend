// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaLevelsResource`: keeps this connection's `media.levels`
// subscription in step with whether a visualizer is on screen. The
// readings themselves go straight to `Client::subscribe_levels`; this
// only says when Core should send them.
//
// Core's subscription is per connection and ends with it, so a new
// connection subscribes again while the visualizer is wanted. A Core
// that predates the method answers method not found: the visualizer is
// then unsupported until the next connection, said once at debug level.

use crate::client::{Client, ClientError, ConnectionState};
use crate::media_types::MediaLevels;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};
use tracing::debug;

/// How long a failed subscribe or unsubscribe waits before trying again,
/// unless the connection or the wish changes first.
const RETRY_DELAY: Duration = Duration::from_secs(5);

/// How Core answered a `media.levels.subscribe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Done,
    /// Core does not have the method.
    Unsupported,
    Failed,
}

impl Answer {
    fn of(result: &Result<(), ClientError>) -> Self {
        match result {
            Ok(()) => Self::Done,
            Err(e) if e.is_method_not_found() => Self::Unsupported,
            Err(_) => Self::Failed,
        }
    }
}

/// The subscription as this connection stands. Pure, so its steps are
/// tested without a Core.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Fsm {
    connected: bool,
    /// What Core was last told and confirmed.
    subscribed: bool,
    unsupported: bool,
}

impl Fsm {
    /// The connection changed: every publish is a new session or the end
    /// of one, and either way Core holds no subscription for it.
    fn connection_changed(&mut self, connected: bool) {
        *self = Self {
            connected,
            ..Self::default()
        };
    }

    /// What to tell Core now, if anything.
    fn next(self, wanted: bool) -> Option<bool> {
        (self.connected && !self.unsupported && wanted != self.subscribed).then_some(wanted)
    }

    /// Core answered a request to set the subscription to `enabled`.
    /// Returns true the first time Core turns out not to have it.
    fn answered(&mut self, enabled: bool, answer: Answer) -> bool {
        match answer {
            Answer::Done => {
                self.subscribed = enabled;
                false
            }
            Answer::Unsupported => {
                let first = !self.unsupported;
                self.unsupported = true;
                self.subscribed = false;
                first
            }
            Answer::Failed => false,
        }
    }
}

/// Whether the level feed can run, as the visualizer needs to know.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LevelsSupport {
    /// The Core on this connection has no level feed.
    pub unsupported: bool,
}

/// The level feed's subscription. One per `Store`.
pub struct MediaLevelsResource {
    wanted: watch::Sender<bool>,
    support: Arc<watch::Sender<LevelsSupport>>,
    client: Arc<Client>,
}

impl std::fmt::Debug for MediaLevelsResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaLevelsResource")
            .finish_non_exhaustive()
    }
}

impl MediaLevelsResource {
    pub fn new(client: &Arc<Client>, runtime: &Handle) -> Arc<Self> {
        let (wanted, wanted_rx) = watch::channel(false);
        let support = Arc::new(watch::channel(LevelsSupport::default()).0);
        let resource = Arc::new(Self {
            wanted,
            support: support.clone(),
            client: client.clone(),
        });
        let connection_rx = client.connection.subscribe();
        let client = client.clone();
        runtime.spawn(async move {
            run_task(client, connection_rx, wanted_rx, support).await;
        });
        resource
    }

    /// Say whether a visualizer is on screen. Cheap and idempotent: call
    /// it on every render.
    pub fn set_wanted(&self, wanted: bool) {
        self.wanted.send_if_modified(|current| {
            if *current == wanted {
                return false;
            }
            *current = wanted;
            true
        });
    }

    pub fn subscribe_support(&self) -> watch::Receiver<LevelsSupport> {
        self.support.subscribe()
    }

    /// Whether the level feed can run on this connection, as it stands.
    pub fn support(&self) -> LevelsSupport {
        *self.support.borrow()
    }

    /// The latest readings of each slot.
    pub fn subscribe_levels(&self) -> watch::Receiver<MediaLevels> {
        self.client.subscribe_levels()
    }
}

async fn run_task(
    client: Arc<Client>,
    mut connection_rx: watch::Receiver<ConnectionState>,
    mut wanted_rx: watch::Receiver<bool>,
    support: Arc<watch::Sender<LevelsSupport>>,
) {
    let mut fsm = Fsm::default();
    let publish = |fsm: &Fsm| {
        let next = LevelsSupport {
            unsupported: fsm.unsupported,
        };
        support.send_if_modified(|current| {
            if *current == next {
                return false;
            }
            *current = next;
            true
        });
    };
    let mut first = true;
    loop {
        if first || connection_rx.has_changed().unwrap_or(false) {
            first = false;
            let connected = matches!(
                *connection_rx.borrow_and_update(),
                ConnectionState::Connected
            );
            fsm.connection_changed(connected);
            publish(&fsm);
        }
        let wanted = *wanted_rx.borrow_and_update();
        if let Some(enabled) = fsm.next(wanted) {
            let result = client.media_levels_subscribe(enabled).await;
            let answer = Answer::of(&result);
            if connection_rx.has_changed().unwrap_or(false) {
                // The answer belongs to a connection that has since ended.
                continue;
            }
            if fsm.answered(enabled, answer) {
                debug!("media_levels: Core has no level feed; the visualizer is off");
            }
            publish(&fsm);
            if answer != Answer::Failed {
                continue;
            }
            if let Err(e) = result {
                debug!("media_levels: subscribe({enabled}) failed: {}", e.message);
            }
            tokio::select! {
                changed = connection_rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    connection_rx.mark_changed();
                }
                changed = wanted_rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                () = sleep(RETRY_DELAY) => {}
            }
            continue;
        }
        tokio::select! {
            changed = connection_rx.changed() => {
                if changed.is_err() {
                    return;
                }
                connection_rx.mark_changed();
            }
            changed = wanted_rx.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribes_only_while_wanted_and_connected() {
        let mut fsm = Fsm::default();
        assert_eq!(fsm.next(true), None, "not connected");
        fsm.connection_changed(true);
        assert_eq!(fsm.next(false), None, "nothing to undo");
        assert_eq!(fsm.next(true), Some(true));
        assert!(!fsm.answered(true, Answer::Done));
        assert_eq!(fsm.next(true), None, "already on");
        assert_eq!(fsm.next(false), Some(false), "hidden: off");
        fsm.answered(false, Answer::Done);
        assert_eq!(fsm.next(false), None);
    }

    #[test]
    fn a_new_connection_subscribes_again() {
        let mut fsm = Fsm::default();
        fsm.connection_changed(true);
        fsm.answered(true, Answer::Done);
        fsm.connection_changed(false);
        assert_eq!(fsm.next(true), None);
        fsm.connection_changed(true);
        assert_eq!(
            fsm.next(true),
            Some(true),
            "Core forgot it with the old one"
        );
    }

    #[test]
    fn an_older_core_is_unsupported_until_the_next_connection() {
        let mut fsm = Fsm::default();
        fsm.connection_changed(true);
        assert!(fsm.answered(true, Answer::Unsupported), "said once");
        assert!(!fsm.answered(true, Answer::Unsupported));
        assert!(fsm.unsupported);
        assert_eq!(fsm.next(true), None, "never asked again");
        assert_eq!(fsm.next(false), None);
        fsm.connection_changed(true);
        assert!(!fsm.unsupported, "an upgraded Core gets another chance");
        assert_eq!(fsm.next(true), Some(true));
    }

    #[test]
    fn a_failed_request_is_asked_again() {
        let mut fsm = Fsm::default();
        fsm.connection_changed(true);
        assert!(!fsm.answered(true, Answer::Failed));
        assert_eq!(fsm.next(true), Some(true));
        fsm.answered(true, Answer::Done);
        fsm.answered(false, Answer::Failed);
        assert_eq!(fsm.next(false), Some(false), "an unsubscribe too");
    }
}
