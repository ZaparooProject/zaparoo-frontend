// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Async WebSocket JSON-RPC 2.0 client. Mirrors ZaparooClient.{cpp,h}.
// Runs on a tokio runtime; auto-reconnects with exponential backoff
// (1→2→4→8→16s, capped at 30s).
//
// `ConnectionState` is an explicit state machine published via `watch`
// (see the variant docs below for transitions). The outbound channel is
// session-scoped — every successful connect installs a fresh
// `mpsc<String>` so RPCs queued while the link is down can't replay
// against the next session, and `call()` fails fast with `not connected`
// instead of disappearing into a queue.

use crate::media_types::{
    LaunchersResult, MediaBrowseParams, MediaBrowseResult, MediaHistoryParams, MediaHistoryResult,
    MediaHistoryTopParams, MediaHistoryTopResult, MediaImageBulkParams, MediaImageBulkResult,
    MediaImageParams, MediaImageResult, MediaIndexParams, MediaLookupParams, MediaLookupResult,
    MediaMetaParams, MediaMetaResult, MediaResult, MediaScrapeParams, MediaSearchParams,
    MediaSearchResult, MediaTagsParams, MediaTagsResult, MediaTagsUpdateParams,
    MediaTagsUpdateResult, ReadersResult, ReadersWriteParams, RunParams, ScrapersResult,
    ScrapingStatusResponse, SettingsResult, SystemsParams, SystemsResult, UpdateSettingsParams,
    VersionResult,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Consecutive connect failures after which the client advertises
/// `ConnectionState::Unreachable` to subscribers. The outer loop keeps
/// retrying past this point — the threshold exists so the UI can
/// escalate a transient drop into a "Core unreachable" banner rather
/// than cycling endlessly through "Reconnecting…".
const RETRY_ERROR_THRESHOLD: u32 = 10;

/// Ceiling on reconnect backoff. Chosen so a laptop waking from sleep
/// after hours still reconnects within half a minute.
const MAX_BACKOFF_SECS: u64 = 30;

/// Cold-boot window during which we retry the WebSocket every
/// `BOOT_RETRY` instead of using the exponential curve. Sized to cover
/// the worst observed Core cold-start on `MiSTer` (~22 s service-binary
/// prep + database open with a large media DB) plus headroom. While the
/// window is open and we have never connected, connect failures don't
/// count toward `RETRY_ERROR_THRESHOLD` — they're expected, not a
/// reachability problem.
const BOOT_WINDOW: Duration = Duration::from_secs(45);

/// Retry interval inside the boot window. A connect-refused on a
/// not-yet-bound port is one TCP SYN + RST, so 250 ms × 180 attempts
/// across the window is negligible CPU/network on `MiSTer` and lets the
/// launcher pick up Core within a quarter-second of HTTP bind instead
/// of waiting up to 16 s for the next exponential backoff slot.
const BOOT_RETRY: Duration = Duration::from_millis(250);

/// Rolling state of the WebSocket link, published via `watch` so late
/// subscribers (QML singletons whose `initialize()` runs after the QML
/// engine boots, post-connect) read the current value rather than
/// silently missing transitions.
///
/// State machine:
///
/// ```text
///                        ┌──────── (success) ──────────┐
///                        ↓                              │
///       Disconnected ──→ Connecting ──→ Connected      │
///                          ↑                │           │
///                          │     (drop) ────┘           │
///                          │                            │
///        (recover) ←───────┴──── Reconnecting ──────────┘
///                                      │
///                                      │ (failures ≥ RETRY_ERROR_THRESHOLD)
///                                      ↓
///                                Unreachable(last_err)
///                                      │
///                                      └─ (eventual success → Connected;
///                                          stays Unreachable until then)
/// ```
///
/// Invariants the connect loop preserves:
/// - `Connecting` is published exactly once, on the first attempt before
///   any successful connect.
/// - After a successful connect that drops, the next published state is
///   `Reconnecting`, not `Connecting`. UI code can rely on `Connecting`
///   meaning "first-ever attempt" and `Reconnecting` meaning "lost a
///   live link."
/// - Once `Unreachable(msg)` is published, the loop keeps retrying
///   internally but does not republish `Reconnecting`. Recovery is
///   signalled by a single transition to `Connected`.
/// - `Disconnected` is the initial state only — the loop never returns
///   to it after the first attempt begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// No connect attempt has been made yet. Initial value before the
    /// connect loop's first iteration.
    Disconnected,
    /// First-ever `connect_async` in flight. Replaced by `Connected` on
    /// success or `Unreachable` after `RETRY_ERROR_THRESHOLD` consecutive
    /// failures; UI code may treat this as "boot-time wait."
    Connecting,
    /// ws link up and service loop running.
    Connected,
    /// Lost a previously-live link and the loop is retrying. Distinct
    /// from `Connecting` so UI can show "lost connection, retrying"
    /// without flickering "connecting…" every backoff cycle.
    Reconnecting,
    /// Threshold of consecutive connect failures hit. The inner string
    /// is the most recent connect error. Sticky: subsequent retries do
    /// not republish `Connecting` or `Reconnecting`; only a successful
    /// connect (→ `Connected`) clears it.
    Unreachable(String),
}

#[derive(Debug, Clone, Serialize)]
struct RpcRequest<'a, T: Serialize> {
    jsonrpc: &'a str,
    method: &'a str,
    params: &'a T,
    id: String,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    id: Option<String>,
    result: Option<Value>,
    error: Option<RpcError>,
    method: Option<String>,
    params: Option<Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct RpcError {
    message: String,
}

#[derive(Debug)]
pub struct ClientError {
    pub message: String,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, ClientError>>>>>;

/// Session-scoped outbound sender. `None` means no live ws session, so
/// `call()` fails fast with `not connected` instead of queueing into a
/// channel that might be drained against a later session.
type OutboundSlot = Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

#[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
fn handle_incoming(
    resp: RpcResponse,
    pending: &PendingMap,
    notifications: &broadcast::Sender<Notification>,
) {
    if let Some(id) = resp.id {
        let sender = pending.lock().unwrap().remove(&id);
        if let Some(tx) = sender {
            let result = if let Some(err) = resp.error {
                Err(ClientError {
                    message: err.message,
                })
            } else {
                Ok(resp.result.unwrap_or(Value::Null))
            };
            let _ = tx.send(result);
        }
    } else if let Some(method) = resp.method {
        let _ = notifications.send(Notification {
            method,
            params: resp.params.unwrap_or(Value::Null),
        });
    }
}

/// Bookkeeping for the connection state machine. Extracted from the
/// connect loop so the transition rules can be unit-tested without
/// driving real WebSocket I/O.
#[derive(Debug, Default)]
struct ConnectionFsm {
    /// Consecutive connect failures since the last successful handshake.
    /// Resets on `on_connected`.
    failures: u32,
    /// Set on the first `Connected` and never cleared. Drives
    /// `Connecting` (first-ever attempt) vs `Reconnecting` (post-drop).
    ever_connected: bool,
    /// Latches the moment `Unreachable` is published so subsequent retry
    /// attempts stay silent until a successful connect clears it. Without
    /// this, the loop would clobber `Unreachable` with `Reconnecting` on
    /// every backoff cycle.
    unreachable_published: bool,
}

impl ConnectionFsm {
    /// Returns the state to publish before the next connect attempt, or
    /// `None` if no transition is needed (already at the right state, or
    /// `Unreachable` has latched).
    fn before_attempt(&self, current: &ConnectionState) -> Option<ConnectionState> {
        if self.unreachable_published {
            return None;
        }
        let trying = if self.ever_connected {
            ConnectionState::Reconnecting
        } else {
            ConnectionState::Connecting
        };
        if *current == trying {
            None
        } else {
            Some(trying)
        }
    }

    /// Records a successful connect and returns the state to publish.
    /// Always `Connected`; the loop should always publish this so a
    /// recovery from `Unreachable` produces a visible transition.
    fn on_connected(&mut self) -> ConnectionState {
        self.failures = 0;
        self.ever_connected = true;
        self.unreachable_published = false;
        ConnectionState::Connected
    }

    /// Records a failed connect attempt and returns the state to publish,
    /// or `None` if the failure shouldn't change the published state
    /// (below threshold, already `Unreachable`, or still inside the
    /// cold-boot window before the first successful connect).
    ///
    /// `boot_window` is true while we have never connected and the
    /// process is younger than [`BOOT_WINDOW`]. During that period
    /// connect failures are expected (Core is starting), so they don't
    /// increment the failure counter or escalate to `Unreachable`.
    fn on_attempt_failed(&mut self, err: String, boot_window: bool) -> Option<ConnectionState> {
        if boot_window {
            return None;
        }
        self.failures = self.failures.saturating_add(1);
        if self.failures >= RETRY_ERROR_THRESHOLD && !self.unreachable_published {
            self.unreachable_published = true;
            Some(ConnectionState::Unreachable(err))
        } else {
            None
        }
    }

    fn current_failures(&self) -> u32 {
        self.failures
    }

    fn ever_connected(&self) -> bool {
        self.ever_connected
    }
}

#[derive(Clone, Debug)]
pub struct Client {
    tx: OutboundSlot,
    pending: PendingMap,
    notifications: broadcast::Sender<Notification>,
    pub connection: Arc<watch::Sender<ConnectionState>>,
}

impl Client {
    pub fn new(endpoint: String, runtime: &tokio::runtime::Handle) -> Arc<Self> {
        let (connection_tx, _) = watch::channel(ConnectionState::Disconnected);
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();
        let connection_arc = Arc::new(connection_tx);
        let connection_clone = connection_arc.clone();
        let tx_slot: OutboundSlot = Arc::new(Mutex::new(None));
        let tx_slot_clone = tx_slot.clone();
        let (notification_tx, _) = broadcast::channel(64);
        let notification_tx_clone = notification_tx.clone();

        let client = Arc::new(Self {
            tx: tx_slot,
            pending,
            notifications: notification_tx,
            connection: connection_arc,
        });

        runtime.spawn(async move {
            let mut fsm = ConnectionFsm::default();
            let process_start = std::time::Instant::now();

            loop {
                // Cold-boot fast-retry window: while we have never
                // connected and the process is young, Core is probably
                // still starting up. Retry tightly so we pick up Core's
                // HTTP bind within ~250 ms instead of waiting up to
                // 16 s for the next exponential slot.
                let boot_window =
                    !fsm.ever_connected() && process_start.elapsed() < BOOT_WINDOW;

                // Bind the borrow to a local so it's dropped before
                // `send_replace` — temporaries in the scrutinee of an
                // `if let` outlive the body, so inlining the borrow
                // here would hold a read lock across `send_replace`'s
                // write lock and deadlock the connection task.
                let next = {
                    let current = connection_clone.borrow();
                    fsm.before_attempt(&current)
                };
                if let Some(next) = next {
                    connection_clone.send_replace(next);
                }

                // tungstenite's default 16 MiB frame / 64 MiB message
                // caps exist to "prevent memory eating by a malicious
                // user". The launcher connects to exactly one trusted
                // Core on the LAN, so that threat model doesn't apply —
                // and a legitimate bulk `media.image` response can
                // exceed 16 MiB, which kills the session and cascades
                // every in-flight watcher into "disconnected". Disable
                // both incoming caps; outbound traffic is small JSON
                // requests that never approach the limits.
                let ws_config = WebSocketConfig::default()
                    .max_message_size(None)
                    .max_frame_size(None);
                match connect_async_with_config(&endpoint, Some(ws_config), false).await {
                    Ok((ws_stream, _)) => {
                        info!("connected to core at {endpoint}");

                        // Fresh outbound channel per session — see the
                        // OutboundSlot doc comment for why this isn't
                        // shared across reconnects.
                        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<String>();
                        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
                        {
                            *tx_slot_clone.lock().unwrap() = Some(msg_tx);
                        }
                        connection_clone.send_replace(fsm.on_connected());

                        let (mut write, mut read) = ws_stream.split();

                        loop {
                            tokio::select! {
                                msg = msg_rx.recv() => {
                                    match msg {
                                        Some(text) => {
                                            if let Err(e) = write.send(Message::Text(text.into())).await {
                                                warn!("ws send error: {e}");
                                                break;
                                            }
                                        }
                                        None => return, // Outbound channel dropped — only happens on shutdown.
                                    }
                                }
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(resp) = serde_json::from_str::<RpcResponse>(text.as_str()) {
                                                handle_incoming(resp, &pending_clone, &notification_tx_clone);
                                            }
                                        }
                                        Some(Ok(Message::Close(_))) | None => {
                                            debug!("ws closed");
                                            break;
                                        }
                                        Some(Err(e)) => {
                                            warn!("ws read error: {e}");
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        // Tear down the session: drop the outbound sender
                        // (msg_rx is dropped at scope-exit, taking any
                        // queued-but-unsent messages with it) and fail
                        // every pending RPC. The next iteration publishes
                        // `Reconnecting` automatically.
                        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
                        {
                            *tx_slot_clone.lock().unwrap() = None;
                        }
                        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
                        let drained: Vec<_> = pending_clone.lock().unwrap().drain().collect();
                        for (_, tx) in drained {
                            let _ = tx.send(Err(ClientError { message: "disconnected".into() }));
                        }
                    }
                    Err(e) => {
                        if let Some(next) = fsm.on_attempt_failed(e.to_string(), boot_window) {
                            connection_clone.send_replace(next);
                        }
                        debug!(
                            "ws connect failed (attempt {}, boot_window={boot_window}): {e}",
                            fsm.current_failures()
                        );
                    }
                }
                tokio::time::sleep(backoff_delay(fsm.current_failures(), boot_window)).await;
            }
        });

        client
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<Notification> {
        self.notifications.subscribe()
    }

    async fn call<P: Serialize>(&self, method: &str, params: &P) -> Result<Value, ClientError> {
        let id = Uuid::new_v4().to_string();
        let req = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id: id.clone(),
        };
        let text = serde_json::to_string(&req).map_err(|e| ClientError {
            message: e.to_string(),
        })?;

        // Snapshot the current session's sender. If `None`, no live link —
        // fail immediately rather than queueing into a channel that will
        // be dropped at the next disconnect or, worse, drained by the
        // wrong session.
        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
        let sender = self.tx.lock().unwrap().clone().ok_or_else(|| ClientError {
            message: "not connected".into(),
        })?;

        let (resp_tx, resp_rx) = oneshot::channel();
        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
        {
            self.pending.lock().unwrap().insert(id.clone(), resp_tx);
        }

        if sender.send(text).is_err() {
            // Receiver was dropped between the snapshot and the send —
            // session ended in flight. Clean up the pending entry so it
            // doesn't leak.
            #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
            {
                self.pending.lock().unwrap().remove(&id);
            }
            return Err(ClientError {
                message: "not connected".into(),
            });
        }

        resp_rx.await.map_err(|_| ClientError {
            message: "channel closed".into(),
        })?
    }

    pub async fn systems(&self, params: SystemsParams) -> Result<SystemsResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let _ = params;
        let val = self.call("systems", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn readers(&self) -> Result<ReadersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("readers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn settings(&self) -> Result<SettingsResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("settings", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn settings_update(&self, params: UpdateSettingsParams) -> Result<(), ClientError> {
        self.call("settings.update", &params).await?;
        Ok(())
    }

    pub async fn launchers(&self) -> Result<LaunchersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("launchers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn media_search(
        &self,
        params: MediaSearchParams,
    ) -> Result<MediaSearchResult, ClientError> {
        let val = self.call("media.search", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn media_browse(
        &self,
        params: MediaBrowseParams,
    ) -> Result<MediaBrowseResult, ClientError> {
        debug!(
            path = %params.path,
            systems = ?params.systems,
            max_results = ?params.max_results,
            cursor_set = params.cursor.is_some(),
            letter = ?params.letter,
            sort = ?params.sort,
            "media.browse request",
        );
        let val = self.call("media.browse", &params).await?;
        let entries_len = val
            .get("entries")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let total_files = val.get("totalFiles").and_then(Value::as_u64).unwrap_or(0);
        debug!(entries_len, total_files, "media.browse response");
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Fetches a single best-match cover image for the given media row.
    /// Identified by `(system, path)` — `path` is the canonical indexed
    /// media path returned by `media.search` or `media.browse`. Returns
    /// the `media.image` payload: content type, file extension (when
    /// derivable), base64 image bytes, and the resolved property type
    /// tag.
    pub async fn media_image(
        &self,
        params: MediaImageParams,
    ) -> Result<MediaImageResult, ClientError> {
        let val = self.call("media.image", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Batched `media.image`: resolves up to `MEDIA_IMAGE_BATCH_MAX`
    /// (50) covers in a single JSON-RPC call. Core dispatches by
    /// request shape, so the wire method name is the same as the
    /// single-shot wrapper. Per-item failures (system not found, no
    /// image) come back inside the response as an `error` string —
    /// the call itself only fails on transport / RPC-level errors.
    pub async fn media_image_bulk(
        &self,
        params: MediaImageBulkParams,
    ) -> Result<MediaImageBulkResult, ClientError> {
        let val = self.call("media.image", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Fetches the full metadata graph for a single media row —
    /// ROM-level + title-level tags and scraped properties. Identified
    /// by `(system, path)`; the canonical indexed media path from
    /// `media.search`/`media.browse` is required. Property values
    /// surface their MIME type and extension when binary-backed, so
    /// callers can render or cache them without sniffing.
    pub async fn media_meta(
        &self,
        params: MediaMetaParams,
    ) -> Result<MediaMetaResult, ClientError> {
        let val = self.call("media.meta", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn media_history(
        &self,
        params: MediaHistoryParams,
    ) -> Result<MediaHistoryResult, ClientError> {
        debug!(
            limit = ?params.limit,
            systems = ?params.systems,
            cursor_set = params.cursor.is_some(),
            "media.history request",
        );
        let val = self.call("media.history", &params).await?;
        let entries_len = val
            .get("entries")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        debug!(entries_len, "media.history response");
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Most-played aggregates over the session log. Optionally scoped to
    /// `systems` and/or windowed by `since` (RFC3339).
    pub async fn media_history_top(
        &self,
        params: MediaHistoryTopParams,
    ) -> Result<MediaHistoryTopResult, ClientError> {
        let val = self.call("media.history.top", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Resolves a `(system, name)` pair to a single best-match media row.
    /// Core returns `{match: null}` (success, no match) for `ErrNoMatch`
    /// / `ErrLowConfidence` rather than a JSON-RPC error, so callers
    /// pattern-match on `result.match_` rather than `Err(...)`.
    pub async fn media_lookup(
        &self,
        params: MediaLookupParams,
    ) -> Result<MediaLookupResult, ClientError> {
        let val = self.call("media.lookup", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Lists the available tag index, optionally scoped to a system
    /// filter. Useful for any future filter UI; the launcher does not
    /// currently call this, but the wrapper is here so it's available.
    pub async fn media_tags(
        &self,
        params: MediaTagsParams,
    ) -> Result<MediaTagsResult, ClientError> {
        let val = self.call("media.tags", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Adds or removes mutable user tags for one indexed media item.
    pub async fn media_tags_update(
        &self,
        params: MediaTagsUpdateParams,
    ) -> Result<MediaTagsUpdateResult, ClientError> {
        let val = self.call("media.tags.update", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Snapshot of Core's media state — database build status plus the
    /// active-media list. The launcher uses the `database` block to seed
    /// the status pill / first-run gate; later notifications
    /// (`media.indexing`) supersede the seed.
    pub async fn media(&self) -> Result<MediaResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("media", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Triggers a (re)build of Core's media database. With an empty
    /// `params`, the build runs across every configured system. Core
    /// emits `media.indexing` notifications during the run.
    pub async fn media_generate(&self, params: MediaIndexParams) -> Result<(), ClientError> {
        self.call("media.generate", &params).await?;
        Ok(())
    }

    /// Cancels an in-flight `media.generate`. Core's response is null on
    /// success and an error if no build is running, which surfaces
    /// through `ClientError`.
    pub async fn media_generate_cancel(&self) -> Result<(), ClientError> {
        #[derive(Serialize)]
        struct P {}
        self.call("media.generate.cancel", &P {}).await?;
        Ok(())
    }

    /// Runs a scraper across (a subset of) Core's media database.
    /// `scraper_id` is required server-side — pick one from `scrapers()`.
    /// Core emits `media.scraping` notifications during the run.
    pub async fn media_scrape(&self, params: MediaScrapeParams) -> Result<(), ClientError> {
        self.call("media.scrape", &params).await?;
        Ok(())
    }

    /// Cancels an in-flight `media.scrape`. As with the indexer, Core
    /// returns an error when no scraper is running.
    pub async fn media_scrape_cancel(&self) -> Result<(), ClientError> {
        #[derive(Serialize)]
        struct P {}
        self.call("media.scrape.cancel", &P {}).await?;
        Ok(())
    }

    /// One-shot scraper status. Mirrors the TUI's `getScrapeStatus` —
    /// the `media.scraping` notification stream only fires while a scrape
    /// is running, so a fresh launcher seeing an idle Core has no other
    /// way to learn the cumulative `total_scraped`.
    pub async fn media_scrape_status(&self) -> Result<ScrapingStatusResponse, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("media.scrape.status", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    /// Lists the scrapers Core knows how to run. Used to resolve a
    /// default `scraperId` for the "Run scraper" Settings action.
    pub async fn scrapers(&self) -> Result<ScrapersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("scrapers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }

    pub async fn run(&self, params: RunParams) -> Result<(), ClientError> {
        // `RunParams` already derives `Serialize`; forwarding it as-is
        // means new optional fields the upstream API gains (`type`,
        // `uid`, `data`, `unsafe`) flow through with no client edits.
        // Upstream returns null on success; swallow it.
        self.call("run", &params).await?;
        Ok(())
    }

    pub async fn readers_write(&self, params: ReadersWriteParams) -> Result<(), ClientError> {
        self.call("readers.write", &params).await?;
        Ok(())
    }

    pub async fn version(&self) -> Result<VersionResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("version", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError {
            message: e.to_string(),
        })
    }
}

/// Delay before the next connect attempt.
///
/// Two regimes:
///
/// - **Boot window** (`boot_window == true`): fixed [`BOOT_RETRY`].
///   Used while the launcher has never successfully connected and the
///   process is younger than [`BOOT_WINDOW`]. Lets us pick up Core
///   within ~250 ms of its HTTP bind on cold `MiSTer` boots, instead of
///   waiting up to 16 s for the next exponential slot.
///
/// - **Steady state**: exponential backoff capped at
///   `MAX_BACKOFF_SECS`. `failures == 0` represents "we just
///   disconnected from a successful session" and yields a 1 s retry so
///   a brief drop doesn't feel sluggish; each subsequent consecutive
///   failure doubles the delay until the cap.
///
///   Sequence: 0→1, 1→1, 2→2, 3→4, 4→8, 5→16, 6→30, 7+→30 (seconds).
///
/// `pub(crate)` so `remote_resource` can reuse the same curve for
/// in-session RPC retry without duplicating the math. Callers in the
/// RPC retry path always pass `boot_window: false` — that fast-retry
/// regime applies only to the connect loop.
pub(crate) fn backoff_delay(failures: u32, boot_window: bool) -> Duration {
    if boot_window {
        return BOOT_RETRY;
    }
    let exp = failures.saturating_sub(1).min(5);
    let secs = 1u64 << exp;
    Duration::from_secs(secs.min(MAX_BACKOFF_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_exponential_curve_then_caps() {
        assert_eq!(backoff_delay(0, false), Duration::from_secs(1));
        assert_eq!(backoff_delay(1, false), Duration::from_secs(1));
        assert_eq!(backoff_delay(2, false), Duration::from_secs(2));
        assert_eq!(backoff_delay(3, false), Duration::from_secs(4));
        assert_eq!(backoff_delay(4, false), Duration::from_secs(8));
        assert_eq!(backoff_delay(5, false), Duration::from_secs(16));
        assert_eq!(
            backoff_delay(6, false),
            Duration::from_secs(MAX_BACKOFF_SECS)
        );
        assert_eq!(
            backoff_delay(7, false),
            Duration::from_secs(MAX_BACKOFF_SECS)
        );
        assert_eq!(
            backoff_delay(u32::MAX, false),
            Duration::from_secs(MAX_BACKOFF_SECS)
        );
    }

    #[test]
    fn backoff_uses_boot_retry_inside_boot_window_regardless_of_failures() {
        // While `boot_window` is true, the failure count is ignored and
        // we always return the fast-retry interval. This is what lets
        // the launcher pick up Core within ~250 ms of HTTP bind on a
        // cold MiSTer boot, instead of being stuck in a 16 s
        // exponential slot.
        assert_eq!(backoff_delay(0, true), BOOT_RETRY);
        assert_eq!(backoff_delay(1, true), BOOT_RETRY);
        assert_eq!(backoff_delay(5, true), BOOT_RETRY);
        assert_eq!(backoff_delay(u32::MAX, true), BOOT_RETRY);
    }

    /// Replays a scripted sequence of connect outcomes through
    /// `ConnectionFsm` and returns the distinct sequence of states
    /// published to the watch — i.e. the public-visible transitions.
    /// `outcomes` is `Ok(())` for a successful connect or `Err(msg)` for
    /// a failed connect attempt. Models the post-boot-window steady
    /// state; for boot-window scenarios use [`replay_with_window`].
    fn replay(outcomes: &[Result<(), &str>]) -> Vec<ConnectionState> {
        replay_with_window(&outcomes.iter().map(|o| (false, *o)).collect::<Vec<_>>())
    }

    /// Like [`replay`] but each outcome carries an explicit
    /// `boot_window` flag, so tests can model the cold-boot fast-retry
    /// regime (where failures don't escalate to `Unreachable`) and the
    /// transition out of it.
    fn replay_with_window(outcomes: &[(bool, Result<(), &str>)]) -> Vec<ConnectionState> {
        let mut fsm = ConnectionFsm::default();
        let mut current = ConnectionState::Disconnected;
        let mut log = Vec::new();
        for (boot_window, outcome) in outcomes {
            if let Some(next) = fsm.before_attempt(&current) {
                current = next.clone();
                log.push(next);
            }
            match outcome {
                Ok(()) => {
                    let next = fsm.on_connected();
                    current = next.clone();
                    log.push(next);
                }
                Err(e) => {
                    if let Some(next) = fsm.on_attempt_failed((*e).to_string(), *boot_window) {
                        current = next.clone();
                        log.push(next);
                    }
                }
            }
        }
        log
    }

    #[test]
    fn first_ever_attempt_publishes_connecting_then_connected() {
        assert_eq!(
            replay(&[Ok(())]),
            vec![ConnectionState::Connecting, ConnectionState::Connected],
        );
    }

    #[test]
    fn drop_after_connected_transitions_through_reconnecting_not_connecting() {
        let log = replay(&[Ok(()), Err("dropped"), Ok(())]);
        assert_eq!(
            log,
            vec![
                ConnectionState::Connecting,
                ConnectionState::Connected,
                // The failed second attempt does not republish; the
                // pre-attempt of the third attempt publishes Reconnecting.
                ConnectionState::Reconnecting,
                ConnectionState::Connected,
            ],
        );
    }

    #[test]
    fn unreachable_is_sticky_until_recovery() {
        // Ten failed first-attempts, then a successful one. Unreachable
        // is published exactly once, and recovery clears it.
        let mut script: Vec<Result<(), &str>> = (0..RETRY_ERROR_THRESHOLD)
            .map(|_| Err("conn refused"))
            .collect();
        script.push(Ok(()));
        let log = replay(&script);
        assert_eq!(
            log,
            vec![
                ConnectionState::Connecting,
                ConnectionState::Unreachable("conn refused".into()),
                ConnectionState::Connected,
            ],
        );
    }

    #[test]
    fn unreachable_does_not_republish_reconnecting_during_extended_outage() {
        // 20 failures (twice the threshold). Unreachable should appear
        // exactly once; no Reconnecting/Connecting flapping in between.
        let script: Vec<Result<(), &str>> = (0..20).map(|_| Err("conn refused")).collect();
        let log = replay(&script);
        assert_eq!(
            log,
            vec![
                ConnectionState::Connecting,
                ConnectionState::Unreachable("conn refused".into()),
            ],
        );
    }

    #[test]
    fn second_unreachable_window_after_recovery_publishes_again() {
        // Drop after Connected, fail past threshold a second time. The
        // second Unreachable goes out because the recovery cleared the
        // latch.
        let mut script: Vec<Result<(), &str>> = vec![Ok(()), Err("first drop")];
        for _ in 0..RETRY_ERROR_THRESHOLD {
            script.push(Err("flaky"));
        }
        script.push(Ok(()));
        let log = replay(&script);
        assert_eq!(
            log,
            vec![
                ConnectionState::Connecting,
                ConnectionState::Connected,
                ConnectionState::Reconnecting,
                ConnectionState::Unreachable("flaky".into()),
                ConnectionState::Connected,
            ],
        );
    }

    #[test]
    fn boot_window_failures_do_not_escalate_to_unreachable() {
        // Many failed attempts entirely inside the boot window. We
        // should publish `Connecting` once and then stay silent —
        // boot-window failures are expected (Core is starting up) and
        // must not flip the UI to "Core unreachable".
        let script: Vec<(bool, Result<(), &str>)> = (0..(RETRY_ERROR_THRESHOLD * 2) as usize)
            .map(|_| (true, Err("conn refused")))
            .collect();
        let log = replay_with_window(&script);
        assert_eq!(log, vec![ConnectionState::Connecting]);
    }

    #[test]
    fn boot_window_fast_path_then_steady_state_unreachable() {
        // Boot-window fast retries fail without escalating. Once we
        // exit the boot window (timer expired without a successful
        // connect), the steady-state regime takes over and the next
        // RETRY_ERROR_THRESHOLD failures publish Unreachable.
        let mut script: Vec<(bool, Result<(), &str>)> =
            (0..50).map(|_| (true, Err("conn refused"))).collect();
        for _ in 0..RETRY_ERROR_THRESHOLD {
            script.push((false, Err("conn refused")));
        }
        let log = replay_with_window(&script);
        assert_eq!(
            log,
            vec![
                ConnectionState::Connecting,
                ConnectionState::Unreachable("conn refused".into()),
            ],
        );
    }

    #[test]
    fn boot_window_then_successful_connect_is_clean() {
        // The expected MiSTer cold-boot path: a burst of refused
        // connects while Core's HTTP server is still binding, then a
        // successful connect once it's up. The launcher should publish
        // exactly Connecting → Connected with no Unreachable in
        // between.
        let mut script: Vec<(bool, Result<(), &str>)> =
            (0..30).map(|_| (true, Err("conn refused"))).collect();
        script.push((true, Ok(())));
        let log = replay_with_window(&script);
        assert_eq!(
            log,
            vec![ConnectionState::Connecting, ConnectionState::Connected],
        );
    }
}
