// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `MediaStatusResource` — the live snapshot of Core's media-database
// build state and scraper state, surfaced as a single
// `tokio::sync::watch` channel.
//
// Why this isn't an `Endpoint`: endpoints are pull-only — Core decides
// the value, the resource fetches and caches it. Media status is
// half-pull/half-push: the initial value comes from a `media` query, but
// every subsequent change is pushed through the
// `media.indexing`/`media.scraping` notification streams. Folding those
// notifications into the same shape and republishing keeps the QML
// singleton's binding cost flat — one `watch::Receiver` and a single
// projection function.
//
// `watch` (not `broadcast`) so a freshly-mounted modal sees the current
// state, not just the next edge — see `feedback_broadcast_vs_watch` in
// the project memory.

use crate::client::{Client, ClientError, ConnectionState, Notification};
use crate::media_types::{
    IndexingStatusResponse, MediaIndexParams, MediaScrapeParams, ScrapingStatusResponse,
};
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

/// Snapshot of every field the frontend renders from `media`,
/// `media.indexing`, and `media.scraping`. Cloned out of the watch
/// channel by the QML projection function — keep field counts modest
/// and types cheap to clone (no nested `Vec`s of large payloads).
///
/// `seeded` flips true after the first successful `media` query so the
/// UI can distinguish "we don't know yet" from "we know there's no
/// database." Without it, the empty-DB first-run gate would trip on the
/// initial `Default::default()` value before the seed lands.
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags; one bool per Core status field"
)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaStatusState {
    pub seeded: bool,

    // From `media.database` and `media.indexing`.
    pub exists: bool,
    pub indexing: bool,
    pub optimizing: bool,
    pub paused: bool,
    pub current_step: i32,
    pub total_steps: i32,
    pub current_step_display: String,
    pub total_files: i32,
    pub total_media: i32,

    // From `media.scraping`.
    pub scraping: bool,
    pub scrape_done: bool,
    pub scrape_paused: bool,
    pub scrape_processed: i32,
    pub scrape_total: i32,
    pub scrape_matched: i32,
    pub scrape_skipped: i32,
    pub scrape_total_scraped: i32,
    pub scrape_system_id: String,
    pub scrape_scraper_id: String,
}

impl MediaStatusState {
    fn apply_indexing(&mut self, status: &IndexingStatusResponse) {
        self.exists = status.exists;
        self.indexing = status.indexing;
        self.optimizing = status.optimizing;
        self.paused = status.paused;
        self.current_step = status.current_step.unwrap_or(0);
        self.total_steps = status.total_steps.unwrap_or(0);
        status
            .current_step_display
            .as_deref()
            .unwrap_or_default()
            .clone_into(&mut self.current_step_display);
        self.total_files = status.total_files.unwrap_or(0);
        self.total_media = status.total_media.unwrap_or(0);
    }

    fn apply_scraping(&mut self, status: &ScrapingStatusResponse) {
        self.scraping = status.scraping;
        self.scrape_done = status.done;
        self.scrape_paused = status.paused;
        self.scrape_processed = status.processed;
        self.scrape_total = status.total;
        self.scrape_matched = status.matched;
        self.scrape_skipped = status.skipped;
        self.scrape_total_scraped = status.total_scraped;
        self.scrape_system_id.clone_from(&status.system_id);
        self.scrape_scraper_id.clone_from(&status.scraper_id);
    }
}

/// Live media-status publisher. One per `Store`; subscribers go through
/// `subscribe()` to receive a `watch::Receiver` whose initial value is
/// whatever the publisher last set (always either the `Default` sentinel
/// or a real seeded value).
pub struct MediaStatusResource {
    state: Arc<watch::Sender<MediaStatusState>>,
    client: Arc<Client>,
}

impl std::fmt::Debug for MediaStatusResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaStatusResource")
            .finish_non_exhaustive()
    }
}

impl MediaStatusResource {
    pub fn new(client: &Arc<Client>, runtime: &Handle) -> Arc<Self> {
        let (state_tx, _) = watch::channel(MediaStatusState::default());
        let state_arc = Arc::new(state_tx);

        let resource = Arc::new(Self {
            state: state_arc.clone(),
            client: client.clone(),
        });

        // Drive the seed on connect and fold notifications into the
        // shared state. The task lives for the lifetime of the runtime;
        // it exits when the connection watch is closed (i.e. the
        // `Client` is dropped).
        let connection_rx = client.connection.subscribe();
        let notifications_rx = client.subscribe_notifications();
        let client_for_task = client.clone();
        let state_for_task = state_arc;
        runtime.spawn(async move {
            run_task(
                client_for_task,
                connection_rx,
                notifications_rx,
                state_for_task,
            )
            .await;
        });

        resource
    }

    pub fn subscribe(&self) -> watch::Receiver<MediaStatusState> {
        self.state.subscribe()
    }

    /// Trigger `media.generate`. The Core notification stream — not
    /// this call's return value — drives the resulting state changes;
    /// the caller only sees whether the request reached Core.
    pub async fn start_index(&self, params: MediaIndexParams) -> Result<(), ClientError> {
        self.client.media_generate(params).await
    }

    pub async fn cancel_index(&self) -> Result<(), ClientError> {
        self.client.media_generate_cancel().await
    }

    pub async fn start_scrape(&self, params: MediaScrapeParams) -> Result<(), ClientError> {
        self.client.media_scrape(params).await
    }

    pub async fn cancel_scrape(&self) -> Result<(), ClientError> {
        self.client.media_scrape_cancel().await
    }
}

/// Re-seed delay after a failed `media` query while the connection is
/// still up. Keeps a flapping Core from hammering the RPC layer; long
/// enough that a real recovery (Core finishing its own startup) fits
/// inside one cycle.
const SEED_RETRY_DELAY: Duration = Duration::from_secs(2);

async fn run_task(
    client: Arc<Client>,
    mut connection_rx: watch::Receiver<ConnectionState>,
    mut notifications_rx: broadcast::Receiver<Notification>,
    state: Arc<watch::Sender<MediaStatusState>>,
) {
    // Connection-driven outer loop. On every `Connected` transition,
    // re-seed via `media`; while connected, fold notifications into
    // the watch state. A drop / reconnect cycle exits the inner loop
    // and re-seeds — Core's view of the DB may have changed during the
    // outage.
    loop {
        let current = connection_rx.borrow_and_update().clone();
        if matches!(current, ConnectionState::Connected) {
            seed_now(&client, &state).await;

            // Inner notification-fold loop. Exits on connection-state
            // change (caught by the outer `changed().await` below).
            loop {
                tokio::select! {
                    notification = notifications_rx.recv() => {
                        match notification {
                            Ok(n) => fold_notification(&n, &state),
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                // Lossy broadcast caught up — re-seed
                                // so we don't drift from Core.
                                warn!(skipped, "media_status: notifications lagged; re-seeding");
                                seed_now(&client, &state).await;
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                    changed = connection_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        // Re-evaluate outer loop on every connection
                        // transition, including spurious reconnects.
                        break;
                    }
                }
            }
        } else if connection_rx.changed().await.is_err() {
            return;
        }
    }
}

async fn seed_now(client: &Arc<Client>, state: &Arc<watch::Sender<MediaStatusState>>) {
    match client.media().await {
        Ok(media) => {
            state.send_modify(|s| {
                s.apply_indexing(&media.database);
                s.seeded = true;
            });
            debug!("media_status: seeded from media query");
        }
        Err(e) => {
            // Don't hammer the RPC layer — wait a beat and let the
            // outer connection loop re-trigger the seed on the next
            // change. If the link is still up after the delay, the
            // notification stream will drive any state change; if it
            // dropped, the outer loop catches the disconnect.
            warn!("media_status: media seed failed: {e}");
            sleep(SEED_RETRY_DELAY).await;
            return;
        }
    }
    // Scrape state has its own one-shot endpoint — `media.scraping`
    // notifications only fire while a scrape is running, so without
    // this call a fresh frontend seeing an idle Core would always
    // report `total_scraped = 0`. Mirrors the TUI's `getScrapeStatus`.
    match client.media_scrape_status().await {
        Ok(status) => {
            state.send_modify(|s| s.apply_scraping(&status));
            debug!("media_status: seeded scrape status");
        }
        Err(e) => warn!("media_status: scrape status seed failed: {e}"),
    }
}

fn fold_notification(notification: &Notification, state: &Arc<watch::Sender<MediaStatusState>>) {
    match notification.method.as_str() {
        "media.indexing" => {
            match serde_json::from_value::<IndexingStatusResponse>(notification.params.clone()) {
                Ok(status) => state.send_modify(|s| {
                    s.apply_indexing(&status);
                    s.seeded = true;
                }),
                Err(e) => warn!("media_status: media.indexing decode failed: {e}"),
            }
        }
        "media.scraping" => {
            match serde_json::from_value::<ScrapingStatusResponse>(notification.params.clone()) {
                Ok(status) => state.send_modify(|s| s.apply_scraping(&status)),
                Err(e) => warn!("media_status: media.scraping decode failed: {e}"),
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "tests should fail-fast on unexpected errors"
)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_indexing_unwraps_optional_counters() {
        let mut state = MediaStatusState::default();
        let status = IndexingStatusResponse {
            total_steps: Some(4),
            current_step: Some(2),
            current_step_display: Some("Indexing SNES".into()),
            total_files: Some(1234),
            total_media: Some(567),
            exists: true,
            indexing: true,
            optimizing: false,
            paused: false,
        };
        state.apply_indexing(&status);
        assert!(state.exists);
        assert!(state.indexing);
        assert_eq!(state.current_step, 2);
        assert_eq!(state.total_steps, 4);
        assert_eq!(state.current_step_display, "Indexing SNES");
        assert_eq!(state.total_files, 1234);
        assert_eq!(state.total_media, 567);
    }

    #[test]
    fn apply_indexing_treats_missing_counters_as_zero() {
        // Idle Core sends `*int omitempty` with the counters absent —
        // the wire-level `None` lands as a numeric zero, not as a
        // sentinel that breaks the QML bindings.
        let mut state = MediaStatusState::default();
        state.apply_indexing(&IndexingStatusResponse {
            exists: true,
            ..IndexingStatusResponse::default()
        });
        assert!(state.exists);
        assert_eq!(state.current_step, 0);
        assert_eq!(state.total_steps, 0);
        assert_eq!(state.current_step_display, "");
    }

    #[test]
    fn fold_notification_routes_indexing_into_state() {
        let (tx, rx) = watch::channel(MediaStatusState::default());
        let tx = Arc::new(tx);
        let payload = json!({
            "totalSteps": 3, "currentStep": 1, "currentStepDisplay": "Discovering files",
            "totalFiles": 100, "totalMedia": 50,
            "exists": true, "indexing": true, "optimizing": false, "paused": false
        });
        fold_notification(
            &Notification {
                method: "media.indexing".into(),
                params: payload,
            },
            &tx,
        );
        let snapshot = rx.borrow().clone();
        assert!(snapshot.indexing);
        assert!(snapshot.seeded);
        assert_eq!(snapshot.current_step, 1);
        assert_eq!(snapshot.current_step_display, "Discovering files");
    }

    #[test]
    fn fold_notification_routes_scraping_into_state() {
        let (tx, rx) = watch::channel(MediaStatusState::default());
        let tx = Arc::new(tx);
        let payload = json!({
            "scraperId": "screenscraper", "systemId": "SNES",
            "processed": 12, "total": 200, "matched": 10, "skipped": 2,
            "totalScraped": 50, "scraping": true, "done": false, "paused": false
        });
        fold_notification(
            &Notification {
                method: "media.scraping".into(),
                params: payload,
            },
            &tx,
        );
        let snapshot = rx.borrow().clone();
        assert!(snapshot.scraping);
        assert_eq!(snapshot.scrape_processed, 12);
        assert_eq!(snapshot.scrape_total, 200);
        assert_eq!(snapshot.scrape_system_id, "SNES");
        // `scraped`-side notifications must not flip the indexing
        // `seeded` flag — that's the `media` query's job.
        assert!(!snapshot.seeded);
    }

    #[test]
    fn fold_notification_ignores_unrelated_methods() {
        let (tx, rx) = watch::channel(MediaStatusState::default());
        let tx = Arc::new(tx);
        let before = rx.borrow().clone();
        fold_notification(
            &Notification {
                method: "tokens.added".into(),
                params: json!({}),
            },
            &tx,
        );
        let after = rx.borrow().clone();
        assert_eq!(before, after);
    }

    #[test]
    fn fold_notification_swallows_decode_errors() {
        // A malformed `media.indexing` payload must not panic; the
        // existing state stays put and a warning is logged.
        let (tx, rx) = watch::channel(MediaStatusState {
            exists: true,
            indexing: false,
            ..MediaStatusState::default()
        });
        let tx = Arc::new(tx);
        let before = rx.borrow().clone();
        fold_notification(
            &Notification {
                method: "media.indexing".into(),
                params: json!("not an object"),
            },
            &tx,
        );
        let after = rx.borrow().clone();
        assert_eq!(before, after);
    }
}
