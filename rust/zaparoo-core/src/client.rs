// Zaparoo Frontend
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
    HealthResult, LaunchersResult, LogDownloadResult, MediaBrowseIndexParams,
    MediaBrowseIndexResult, MediaBrowseParams, MediaBrowseResult, MediaHistoryLatestResult,
    MediaHistoryParams, MediaHistoryResult, MediaImageParams, MediaImageResult, MediaIndexParams,
    MediaMetaParams, MediaMetaResult, MediaMetaUpdateParams, MediaResult, MediaScrapeParams,
    MediaSearchParams, MediaSearchResult, MediaTagsUpdateParams, MediaTagsUpdateResult,
    PairStartResult, ReadersResult, ReadersWriteParams, RunParams, ScrapersResult,
    ScrapingStatusResponse, SettingsResult, SystemsParams, SystemsResult, TokensHistoryResult,
    TokensResult, UpdateSettingsParams, VersionResult,
};
use crate::transport::Transport;
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
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
/// frontend pick up Core within a quarter-second of HTTP bind instead
/// of waiting up to 16 s for the next exponential backoff slot.
const BOOT_RETRY: Duration = Duration::from_millis(250);

/// Rolling state of the WebSocket link, published via `watch` so late
/// subscribers (UI bindings that attach after the client has already
/// connected) read the current value rather than silently missing
/// transitions.
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
    data: Option<Value>,
}

/// The error category Core stamps on a launch it could not carry out.
const LAUNCH_REPAIR_CATEGORY: &str = "launch_repair";

/// Ceiling on Core's own sentence, used only when it names a reason this
/// build does not know.
const MAX_REPAIR_MESSAGE: usize = 1024;

/// Ceiling on a display name Core attaches to a launch failure. A launcher
/// or plugin name longer than this is not a name, and the alert has one
/// panel's worth of room for it.
const MAX_DISPLAY_NAME: usize = 64;

/// Core's own spelling for "no structured reason". It is the same case as
/// a reason this build has never heard of, and takes the same path: Core's
/// `message` if it sent one, the generic launch copy otherwise. Deliberately
/// not a `LaunchReason`, because it carries no meaning to word.
const UNSPECIFIED_REASON: &str = "unspecified";

/// Why Core could not start something, in its own closed vocabulary. The
/// frontend owns every word the user reads; this only says which of them
/// to say. A value outside this set is not an error — Core's vocabulary
/// may grow ahead of this build — and falls back to `repair_message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchReason {
    /// The launcher application is not installed.
    LauncherNotInstalled,
    /// Installed, but the entry point it declares is gone or disabled.
    LauncherComponentMissing,
    /// Installed, but its plugin for this system is absent.
    LauncherPluginMissing,
    /// Several usable launchers and no reviewed default; the user chooses.
    /// Reserved: no producer in Core yet, since selection resolves by
    /// catalog precedence. Nothing may depend on it firing.
    LauncherAmbiguous,
    /// This launcher cannot play the selected media entry.
    LauncherUnsupportedMedia,
    /// The launch options requested are not supported by this launcher.
    LauncherOptionsUnsupported,
    /// The installed build of the launcher cannot be used for this media,
    /// for instance because its storage model is unsupported. A different
    /// build of the same launcher is what fixes it.
    LauncherVersionUnsupported,
    /// The launcher lacks the storage permission it needs.
    StoragePermissionRequired,
    /// The media lives on a provider this launcher cannot read.
    StorageProviderUnsupported,
    /// The storage holding the media is not present.
    StorageUnavailable,
    /// The media file cannot be resolved or opened.
    MediaUnavailable,
    /// The host's launch service is not answering.
    HostUnavailable,
    /// The launch needs the user to return to the app first.
    HostForegroundRequired,
    /// Dispatched, but the result could not be confirmed.
    OutcomeUnknown,
    /// The operating system refused the request.
    Refused,
    /// The launch was called off before it started, usually by the app
    /// itself rather than by anything the user did.
    Cancelled,
}

impl LaunchReason {
    /// Every reason Core can name. `unspecified` is absent on purpose; see
    /// `UNSPECIFIED_REASON`.
    pub const ALL: [Self; 16] = [
        Self::LauncherNotInstalled,
        Self::LauncherComponentMissing,
        Self::LauncherPluginMissing,
        Self::LauncherAmbiguous,
        Self::LauncherUnsupportedMedia,
        Self::LauncherOptionsUnsupported,
        Self::LauncherVersionUnsupported,
        Self::StoragePermissionRequired,
        Self::StorageProviderUnsupported,
        Self::StorageUnavailable,
        Self::MediaUnavailable,
        Self::HostUnavailable,
        Self::HostForegroundRequired,
        Self::OutcomeUnknown,
        Self::Refused,
        Self::Cancelled,
    ];

    /// Core's wire spelling. An API token, not routing state.
    pub fn token(self) -> &'static str {
        match self {
            Self::LauncherNotInstalled => "launcher_not_installed",
            Self::LauncherComponentMissing => "launcher_component_missing",
            Self::LauncherPluginMissing => "launcher_plugin_missing",
            Self::LauncherAmbiguous => "launcher_ambiguous",
            Self::LauncherUnsupportedMedia => "launcher_unsupported_media",
            Self::LauncherOptionsUnsupported => "launcher_options_unsupported",
            Self::LauncherVersionUnsupported => "launcher_version_unsupported",
            Self::StoragePermissionRequired => "storage_permission_required",
            Self::StorageProviderUnsupported => "storage_provider_unsupported",
            Self::StorageUnavailable => "storage_unavailable",
            Self::MediaUnavailable => "media_unavailable",
            Self::HostUnavailable => "host_unavailable",
            Self::HostForegroundRequired => "host_foreground_required",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(token: &str) -> Option<Self> {
        if token == UNSPECIFIED_REASON {
            return None;
        }
        Self::ALL.into_iter().find(|reason| reason.token() == token)
    }
}

/// A launch failure Core explained: the reason plus the display names the
/// copy may put in its sentence. Both names are already screened for
/// anything that cannot go in one, and either may be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRepair {
    pub reason: LaunchReason,
    pub launcher: String,
    pub plugin: String,
}

/// The `launch_repair` half of an error, before the reason is known to be
/// one this build can word itself.
#[derive(Debug, Default)]
struct RepairData {
    reason: Option<LaunchReason>,
    launcher: String,
    plugin: String,
}

#[derive(Debug)]
pub struct ClientError {
    pub message: String,
    /// `Some` exactly when Core categorized the failure `launch_repair`.
    repair: Option<RepairData>,
}

impl ClientError {
    /// An error with no structured metadata from Core.
    pub(crate) fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            repair: None,
        }
    }

    /// Core's structured account of a launch that did not start, when the
    /// reason it named is one this build knows how to word.
    pub fn launch_repair(&self) -> Option<LaunchRepair> {
        let data = self.repair.as_ref()?;
        Some(LaunchRepair {
            reason: data.reason?,
            launcher: data.launcher.clone(),
            plugin: data.plugin.clone(),
        })
    }

    /// Core's own sentence, and only for a launch failure whose reason this
    /// build does not know: the documented fallback for a vocabulary that
    /// grew ahead of us. Ordinary errors retain the generic alert; they may
    /// contain private details.
    pub fn repair_message(&self) -> Option<&str> {
        let data = self.repair.as_ref()?;
        if data.reason.is_some() {
            return None;
        }
        display_safe(&self.message, MAX_REPAIR_MESSAGE)
    }
}

/// Text from Core that may go straight into a sentence: present, short
/// enough for an alert, and free of control characters that would break
/// the line or smuggle formatting through.
fn display_safe(value: &str, max: usize) -> Option<&str> {
    (!value.is_empty() && value.len() <= max && !value.chars().any(char::is_control))
        .then_some(value)
}

/// Read the `launch_repair` metadata off an RPC error's `data` object.
/// Answers `None` for every other category, which keeps the generic alert.
fn parse_repair(data: Option<&Value>) -> Option<RepairData> {
    let data = data?;
    if data.get("category").and_then(Value::as_str) != Some(LAUNCH_REPAIR_CATEGORY) {
        return None;
    }
    let params = data.get("params");
    let name = |key: &str| {
        params
            .and_then(|params| params.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .and_then(|value| display_safe(value, MAX_DISPLAY_NAME))
            .unwrap_or_default()
            .to_owned()
    };
    Some(RepairData {
        reason: data
            .get("reason")
            .and_then(Value::as_str)
            .and_then(LaunchReason::parse),
        launcher: name("launcher"),
        plugin: name("plugin"),
    })
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, ClientError>>>>>;

/// Removes a pending RPC when its `call()` future is canceled or times out.
/// Normal responses remove the same key first, making this drop a no-op.
struct PendingRequestGuard {
    id: String,
    pending: PendingMap,
}

impl Drop for PendingRequestGuard {
    #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.id);
    }
}

fn deserialize_timed<T: DeserializeOwned>(
    method: &'static str,
    val: Value,
) -> Result<T, ClientError> {
    let started = Instant::now();
    let result = serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()));
    debug!(
        method,
        duration_ms = started.elapsed().as_millis(),
        ok = result.is_ok(),
        "rpc deserialize",
    );
    result
}

/// Session-scoped outbound sender. `None` means no live ws session, so
/// `call()` fails fast with `not connected` instead of queueing into a
/// channel that might be drained against a later session.
type OutboundSlot = Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>;

#[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
fn teardown_session(tx_slot: &OutboundSlot, pending: &PendingMap) {
    let drained: Vec<_> = {
        // Keep session invalidation and pending-request draining in one
        // critical section. `call()` uses the same lock while registering and
        // sending, so teardown cannot miss a request from the ending session.
        let mut sender = tx_slot.lock().unwrap();
        *sender = None;
        let drained = pending.lock().unwrap().drain().collect();
        drop(sender);
        drained
    };
    for (_, response) in drained {
        let _ = response.send(Err(ClientError::plain("disconnected")));
    }
}

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
                    repair: parse_repair(err.data.as_ref()),
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

enum SessionEnd {
    Disconnected,
    Replaced,
    Closed,
}

/// A session owns its outbound queue; no message crosses an endpoint change.
async fn serve_session(
    socket: std::pin::Pin<Box<dyn crate::transport::Socket>>,
    transport: &Transport,
    target: &mut watch::Receiver<Option<Transport>>,
    mut outgoing: mpsc::UnboundedReceiver<String>,
    pending: &PendingMap,
    notifications: &broadcast::Sender<Notification>,
) -> SessionEnd {
    let (mut write, mut read) = socket.split();
    loop {
        tokio::select! {
            changed = target.changed() => {
                return if changed.is_ok() { SessionEnd::Replaced } else { SessionEnd::Closed };
            }
            message = outgoing.recv() => {
                let Some(text) = message else {
                    // Host invalidation retires the sender before notifying the watch.
                    return SessionEnd::Disconnected;
                };
                tokio::select! {
                    biased;
                    changed = target.changed() => {
                        return if changed.is_ok() { SessionEnd::Replaced } else { SessionEnd::Closed };
                    }
                    result = write.send(Message::Text(text.into())) => {
                        if let Err(error) = result {
                            warn!("ws send error: {error}");
                            return SessionEnd::Disconnected;
                        }
                    }
                }
            }
            message = read.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(response) = serde_json::from_str::<RpcResponse>(text.as_str()) {
                            let current = target.borrow();
                            if current.as_ref() == Some(transport)
                                && !target.has_changed().unwrap_or(true)
                            {
                                handle_incoming(response, pending, notifications);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        debug!("ws closed");
                        return SessionEnd::Disconnected;
                    }
                    Some(Err(error)) => {
                        warn!("ws read error: {error}");
                        return SessionEnd::Disconnected;
                    }
                    _ => {}
                }
            }
        }
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
    transport: watch::Sender<Option<Transport>>,
}

impl Client {
    pub fn new(endpoint: String, runtime: &tokio::runtime::Handle) -> Arc<Self> {
        Self::with_transport(Transport::tcp(endpoint, None), runtime)
    }

    pub fn with_transport(transport: Transport, runtime: &tokio::runtime::Handle) -> Arc<Self> {
        Self::with_optional_transport(Some(transport), runtime)
    }

    /// Wait for a host-owned endpoint rather than probing a desktop default.
    pub fn waiting(runtime: &tokio::runtime::Handle) -> Arc<Self> {
        Self::with_optional_transport(None, runtime)
    }

    fn with_optional_transport(
        transport: Option<Transport>,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Self> {
        let (transport_tx, mut transport_rx) = watch::channel(transport);
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
            transport: transport_tx,
        });

        runtime.spawn(async move {
            let mut fsm = ConnectionFsm::default();
            let process_start = Instant::now();

            loop {
                let target = { transport_rx.borrow_and_update().clone() };
                let Some(transport) = target else {
                    if fsm.ever_connected() {
                        connection_clone.send_replace(ConnectionState::Reconnecting);
                    }
                    if transport_rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                };
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
                // user". The frontend connects to exactly one trusted
                // Core on the LAN, so that threat model doesn't apply —
                // and a legitimate bulk `media.image` response can
                // exceed 16 MiB, which kills the session and cascades
                // every in-flight watcher into "disconnected". Disable
                // both incoming caps; outbound traffic is small JSON
                // requests that never approach the limits.
                let ws_config = WebSocketConfig::default()
                    .max_message_size(None)
                    .max_frame_size(None);
                let connected = tokio::select! {
                    biased;
                    changed = transport_rx.changed() => {
                        if changed.is_err() { return; }
                        continue;
                    }
                    result = transport.connect(ws_config) => result,
                };
                match connected {
                    Ok(ws_stream) => {
                        info!(?transport, "connected to core");

                        // Fresh outbound channel per session — see the
                        // OutboundSlot doc comment for why this isn't
                        // shared across reconnects.
                        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<String>();
                        {
                            // Fence endpoint replacement against installing a session
                            // whose handshake completed just as its credential expired.
                            let current = transport_rx.borrow();
                            if current.as_ref() != Some(&transport)
                                || transport_rx.has_changed().unwrap_or(true)
                            {
                                continue;
                            }
                            #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
                            {
                                *tx_slot_clone.lock().unwrap() = Some(msg_tx);
                            }
                            connection_clone.send_replace(fsm.on_connected());
                        }

                        let ended = serve_session(
                            ws_stream, &transport, &mut transport_rx, msg_rx,
                            &pending_clone, &notification_tx_clone,
                        ).await;
                        teardown_session(&tx_slot_clone, &pending_clone);
                        match ended {
                            SessionEnd::Replaced => continue,
                            SessionEnd::Closed => return,
                            SessionEnd::Disconnected => {}
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
                tokio::select! {
                    biased;
                    changed = transport_rx.changed() => {
                        if changed.is_err() { return; }
                    }
                    () = tokio::time::sleep(backoff_delay(fsm.current_failures(), boot_window)) => {}
                }
            }
        });

        client
    }

    /// Invalidates queued/in-flight work before publishing replacement credentials.
    /// Identical updates are idempotent; None retires the session until readiness.
    pub fn set_transport(&self, transport: Option<Transport>) {
        self.transport.send_if_modified(|current| {
            if *current == transport {
                return false;
            }
            teardown_session(&self.tx, &self.pending);
            self.connection.send_if_modified(|state| {
                if matches!(state, ConnectionState::Connected) {
                    *state = ConnectionState::Reconnecting;
                    true
                } else {
                    false
                }
            });
            *current = transport;
            true
        });
    }

    pub fn is_local(&self) -> bool {
        self.transport
            .borrow()
            .as_ref()
            .is_some_and(Transport::is_local)
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
        let text = serde_json::to_string(&req).map_err(|e| ClientError::plain(e.to_string()))?;
        let started = Instant::now();

        let (resp_tx, resp_rx) = oneshot::channel();
        #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
        let send_result = {
            // Register and send while holding the session lock also used by
            // teardown. This makes the sender snapshot and pending entry one
            // session-scoped operation: teardown either runs before all three
            // steps or drains the newly registered request afterward.
            let sender = self.tx.lock().unwrap();
            let sender = sender
                .as_ref()
                .ok_or_else(|| ClientError::plain("not connected"))?;
            self.pending.lock().unwrap().insert(id.clone(), resp_tx);
            sender.send(text)
        };
        let _pending_guard = PendingRequestGuard {
            id: id.clone(),
            pending: self.pending.clone(),
        };

        if send_result.is_err() {
            // Receiver dropped unexpectedly while the session still owned its
            // sender. Clean up the pending entry so it doesn't leak.
            #[allow(clippy::unwrap_used, reason = "mutex poisoning is unrecoverable")]
            {
                self.pending.lock().unwrap().remove(&id);
            }
            debug!(
                method,
                request_id = %id,
                duration_ms = started.elapsed().as_millis(),
                error = "not connected",
                "rpc round trip",
            );
            return Err(ClientError::plain("not connected"));
        }

        let result = resp_rx
            .await
            .map_err(|_| ClientError::plain("channel closed"))?;
        match result {
            Ok(val) => {
                let payload_bytes = serde_json::to_vec(&val).map_or(0, |bytes| bytes.len());
                debug!(
                    method,
                    request_id = %id,
                    duration_ms = started.elapsed().as_millis(),
                    payload_bytes,
                    "rpc round trip",
                );
                Ok(val)
            }
            Err(e) => {
                debug!(
                    method,
                    request_id = %id,
                    duration_ms = started.elapsed().as_millis(),
                    error = %e.message,
                    "rpc round trip",
                );
                Err(e)
            }
        }
    }

    pub async fn systems(&self, params: SystemsParams) -> Result<SystemsResult, ClientError> {
        let val = self.call("systems", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn readers(&self) -> Result<ReadersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("readers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn health(&self) -> Result<HealthResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("health", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn tokens(&self) -> Result<TokensResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("tokens", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn tokens_history(&self) -> Result<TokensHistoryResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("tokens.history", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn settings(&self) -> Result<SettingsResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("settings", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn settings_update(&self, params: UpdateSettingsParams) -> Result<(), ClientError> {
        self.call("settings.update", &params).await?;
        Ok(())
    }

    pub async fn settings_logs_download(&self) -> Result<LogDownloadResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("settings.logs.download", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Start pairing a device. Core only accepts this from a local
    /// client, and answers the PIN to read out plus the deadline it stops
    /// working at. It can refuse, for instance when the paired-client cap
    /// is already reached.
    pub async fn clients_pair_start(&self) -> Result<PairStartResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("clients.pair.start", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Drop the pending pairing. Core accepts this whether or not one is
    /// still open, so it is safe on every exit path.
    pub async fn clients_pair_cancel(&self) -> Result<(), ClientError> {
        #[derive(Serialize)]
        struct P {}
        self.call("clients.pair.cancel", &P {}).await?;
        Ok(())
    }

    pub async fn launchers(&self) -> Result<LaunchersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("launchers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    pub async fn launchers_refresh(&self) -> Result<(), ClientError> {
        #[derive(Serialize)]
        struct P {}
        self.call("launchers.refresh", &P {}).await?;
        Ok(())
    }

    pub async fn media_search(
        &self,
        params: MediaSearchParams,
    ) -> Result<MediaSearchResult, ClientError> {
        let val = self.call("media.search", &params).await?;
        deserialize_timed("media.search", val)
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
            root_view = ?params.root_view,
            "media.browse request",
        );
        let val = self.call("media.browse", &params).await?;
        let entries_len = val
            .get("entries")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let total_files = val.get("totalFiles").and_then(Value::as_u64).unwrap_or(0);
        debug!(entries_len, total_files, "media.browse response");
        deserialize_timed("media.browse", val)
    }

    pub async fn media_browse_index(
        &self,
        params: MediaBrowseIndexParams,
    ) -> Result<MediaBrowseIndexResult, ClientError> {
        debug!(
            path = %params.path,
            systems = ?params.systems,
            sort = ?params.sort,
            "media.browse.index request",
        );
        let val = self.call("media.browse.index", &params).await?;
        deserialize_timed("media.browse.index", val)
    }

    /// Fetches a single best-match cover image for the given media row.
    /// Identified by `mediaId` when available, otherwise `(system,
    /// path)` where `path` is the canonical indexed media path returned
    /// by `media.search` or `media.browse`. Returns the `media.image`
    /// payload: actual delivery mode, content type, file extension (when
    /// derivable), inline base64 bytes or an opaque local thumbnail path,
    /// and the resolved property type tag.
    pub async fn media_image(
        &self,
        params: MediaImageParams,
    ) -> Result<MediaImageResult, ClientError> {
        let val = self.call("media.image", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Fetches the full metadata graph for a single media row —
    /// ROM-level + title-level tags and scraped properties. Identified
    /// by `mediaId` when available, otherwise `(system, path)` with the
    /// canonical indexed media path from `media.search`/`media.browse`.
    /// Property values
    /// surface their MIME type and extension when binary-backed, so
    /// callers can render or cache them without sniffing.
    pub async fn media_meta(
        &self,
        params: MediaMetaParams,
    ) -> Result<MediaMetaResult, ClientError> {
        let val = self.call("media.meta", &params).await?;
        deserialize_timed("media.meta", val)
    }

    /// Sets or clears the per-media launcher override, then returns the
    /// updated metadata graph (same response shape as `media_meta`). Core
    /// validates the launcher exists and supports the row's system before
    /// saving it.
    pub async fn media_meta_update(
        &self,
        params: MediaMetaUpdateParams,
    ) -> Result<MediaMetaResult, ClientError> {
        let val = self.call("media.meta.update", &params).await?;
        deserialize_timed("media.meta.update", val)
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
        deserialize_timed("media.history", val)
    }

    /// Fetches the latest play-history row without media DB enrichment.
    pub async fn media_history_latest(&self) -> Result<MediaHistoryLatestResult, ClientError> {
        let val = self
            .call("media.history.latest", &serde_json::json!({}))
            .await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Adds or removes mutable user tags for one indexed media item.
    pub async fn media_tags_update(
        &self,
        params: MediaTagsUpdateParams,
    ) -> Result<MediaTagsUpdateResult, ClientError> {
        let val = self.call("media.tags.update", &params).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Snapshot of Core's media state — database build status plus the
    /// active-media list. The frontend uses the `database` block to seed
    /// the status pill / first-run gate; later notifications
    /// (`media.indexing`) supersede the seed.
    pub async fn media(&self) -> Result<MediaResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("media", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
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
    /// is running, so a fresh frontend seeing an idle Core has no other
    /// way to learn the cumulative `total_scraped`.
    pub async fn media_scrape_status(&self) -> Result<ScrapingStatusResponse, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("media.scrape.status", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }

    /// Lists the scrapers Core knows how to run. Used to resolve a
    /// default `scraperId` for the "Run scraper" Settings action.
    pub async fn scrapers(&self) -> Result<ScrapersResult, ClientError> {
        #[derive(Serialize)]
        struct P {}
        let val = self.call("scrapers", &P {}).await?;
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
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
        serde_json::from_value(val).map_err(|e| ClientError::plain(e.to_string()))
    }
}

/// Delay before the next connect attempt.
///
/// Two regimes:
///
/// - **Boot window** (`boot_window == true`): fixed [`BOOT_RETRY`].
///   Used while the frontend has never successfully connected and the
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
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[allow(clippy::unwrap_used, reason = "bounded in-memory RPC fixtures")]
    async fn dispatch_error(data: Value) -> ClientError {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().unwrap().insert("repair".into(), tx);
        let (notifications, _) = broadcast::channel(1);
        let response: RpcResponse = serde_json::from_value(serde_json::json!({
            "id": "repair", "error": {"message": "Core said something", "data": data}
        }))
        .unwrap();
        handle_incoming(response, &pending, &notifications);
        rx.await.unwrap().unwrap_err()
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used, reason = "bounded in-memory RPC fixtures")]
    async fn repair_metadata_survives_rpc_dispatch_without_exposing_generic_errors() {
        for (category, message, expected) in [
            (Some("launch_repair"), "Check player storage access", true),
            (
                Some("execution_failed"),
                "/private/path from a legacy error",
                false,
            ),
            (None, "/private/path without structured metadata", false),
            (Some("unknown"), "Unexpected category", false),
            (Some("launch_repair"), "line\nbreak", false),
            (Some("launch_repair"), "", false),
        ] {
            let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
            let (tx, rx) = oneshot::channel();
            pending.lock().unwrap().insert("repair".into(), tx);
            let (notifications, _) = broadcast::channel(1);
            let response: RpcResponse = serde_json::from_value(serde_json::json!({
                "id": "repair", "error": {"message": message, "data": {"category": category}}
            }))
            .unwrap();
            handle_incoming(response, &pending, &notifications);
            let error = rx.await.unwrap().unwrap_err();
            assert_eq!(error.message, message);
            assert_eq!(error.repair_message().is_some(), expected);
            assert!(error.launch_repair().is_none());
        }
        let oversized = ClientError {
            message: "x".repeat(1025),
            repair: Some(RepairData::default()),
        };
        assert!(oversized.repair_message().is_none());
    }

    #[test]
    fn every_reason_has_its_own_token_and_parses_back() {
        let mut seen = Vec::new();
        for reason in LaunchReason::ALL {
            let token = reason.token();
            assert!(!token.is_empty());
            assert!(!seen.contains(&token), "duplicate reason token {token}");
            seen.push(token);
            assert_eq!(LaunchReason::parse(token), Some(reason));
        }
        assert_eq!(LaunchReason::parse("launcher_on_fire"), None);
        assert_eq!(LaunchReason::parse(""), None);
        // Core's explicit "no reason" spells the same degradation path as
        // a reason this build has never heard of.
        assert_eq!(LaunchReason::parse(UNSPECIFIED_REASON), None);
    }

    #[tokio::test]
    async fn an_unspecified_reason_keeps_cores_own_sentence() {
        let error = dispatch_error(serde_json::json!({
            "category": "launch_repair",
            "reason": UNSPECIFIED_REASON,
        }))
        .await;
        assert!(error.launch_repair().is_none());
        assert_eq!(error.repair_message(), Some("Core said something"));
    }

    #[tokio::test]
    async fn a_named_reason_arrives_typed_with_its_display_names() {
        let error = dispatch_error(serde_json::json!({
            "category": "launch_repair",
            "reason": "launcher_plugin_missing",
            "params": {"launcher": " RetroArch ", "plugin": "Mesen"},
        }))
        .await;
        assert_eq!(
            error.launch_repair(),
            Some(LaunchRepair {
                reason: LaunchReason::LauncherPluginMissing,
                launcher: "RetroArch".into(),
                plugin: "Mesen".into(),
            })
        );
        // The typed reason owns the wording, so Core's sentence stops here.
        assert_eq!(error.repair_message(), None);
    }

    #[tokio::test]
    async fn a_reason_this_build_does_not_know_keeps_cores_own_sentence() {
        let error = dispatch_error(serde_json::json!({
            "category": "launch_repair",
            "reason": "launcher_possessed",
            "params": {"launcher": "RetroArch"},
        }))
        .await;
        assert!(error.launch_repair().is_none());
        assert_eq!(error.repair_message(), Some("Core said something"));
    }

    #[tokio::test]
    async fn unusable_display_names_are_dropped_rather_than_carried_into_a_sentence() {
        for params in [
            serde_json::json!({}),
            serde_json::json!({"launcher": "", "plugin": "   "}),
            serde_json::json!({"launcher": "line\nbreak", "plugin": 7}),
            serde_json::json!({"launcher": "x".repeat(MAX_DISPLAY_NAME + 1)}),
        ] {
            let error = dispatch_error(serde_json::json!({
                "category": "launch_repair",
                "reason": "launcher_not_installed",
                "params": params,
            }))
            .await;
            assert_eq!(
                error.launch_repair(),
                Some(LaunchRepair {
                    reason: LaunchReason::LauncherNotInstalled,
                    launcher: String::new(),
                    plugin: String::new(),
                })
            );
        }
    }

    #[tokio::test]
    async fn params_without_a_launch_repair_category_are_ignored() {
        let error = dispatch_error(serde_json::json!({
            "category": "execution_failed",
            "reason": "launcher_not_installed",
            "params": {"launcher": "RetroArch"},
        }))
        .await;
        assert!(error.launch_repair().is_none());
        assert!(error.repair_message().is_none());
    }

    struct BackpressuredSocket(Option<oneshot::Sender<()>>);

    impl futures_util::Stream for BackpressuredSocket {
        type Item = Result<Message, tokio_tungstenite::tungstenite::Error>;
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    impl futures_util::Sink<Message> for BackpressuredSocket {
        type Error = tokio_tungstenite::tungstenite::Error;
        fn poll_ready(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            if let Some(waiting) = self.0.take() {
                let _ = waiting.send(());
            }
            Poll::Pending
        }
        fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
            Ok(())
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used, reason = "bounded fake-transport regression test")]
    async fn endpoint_change_cancels_a_backpressured_write() {
        tokio::time::timeout(Duration::from_secs(2), async {
            let transport = Transport::tcp("ws://localhost/api/v0.1".into(), None);
            let (target, mut target_rx) = watch::channel(Some(transport.clone()));
            let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
            let (waiting, blocked) = oneshot::channel();
            let session = tokio::spawn(async move {
                let pending = PendingMap::default();
                let (notifications, _) = broadcast::channel(1);
                serve_session(
                    Box::pin(BackpressuredSocket(Some(waiting))),
                    &transport,
                    &mut target_rx,
                    outgoing_rx,
                    &pending,
                    &notifications,
                )
                .await
            });
            outgoing.send("queued request".into()).unwrap();
            blocked.await.unwrap();
            target.send_replace(None);
            assert!(matches!(session.await.unwrap(), SessionEnd::Replaced));
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used, reason = "bounded transport regression test")]
    async fn endpoint_replacement_fails_pending_work_and_drop_closes_session() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let first_target = Transport::tcp(
                format!("ws://{}/api/v0.1", first.local_addr().unwrap()),
                None,
            );
            let second_target = Transport::tcp(
                format!("ws://{}/api/v0.1", second.local_addr().unwrap()),
                None,
            );
            let (received_tx, received_rx) = oneshot::channel();
            let first_server = tokio::spawn(async move {
                let (stream, _) = first.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                assert!(matches!(socket.next().await, Some(Ok(Message::Text(_)))));
                received_tx.send(()).unwrap();
                // Replacement must drop this socket, not replay its queued request.
                assert!(!matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            });
            let second_server = tokio::spawn(async move {
                let (stream, _) = second.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let request = socket.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
                assert_eq!(request["method"], "replacement");
                socket
                    .send(Message::Text(
                        serde_json::json!({
                            "jsonrpc": "2.0", "id": request["id"], "result": "new-session"
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                    .unwrap();
                // Dropping the last Client must stop its background connection task.
                assert!(!matches!(socket.next().await, Some(Ok(Message::Text(_)))));
            });
            let client = Client::waiting(&tokio::runtime::Handle::current());
            assert!(!client.is_local());
            assert!(client.call("offline", &Value::Null).await.is_err());
            let mut connection = client.connection.subscribe();
            client.set_transport(Some(first_target.clone()));
            drop(
                connection
                    .wait_for(|state| matches!(state, ConnectionState::Connected))
                    .await
                    .unwrap(),
            );
            assert!(client.is_local());
            let pending = tokio::spawn({
                let client = client.clone();
                async move { client.call("old-pending", &Value::Null).await }
            });
            received_rx.await.unwrap();
            client.set_transport(Some(first_target)); // Same identity must not tear down.
            assert!(matches!(*connection.borrow(), ConnectionState::Connected));
            client.set_transport(None);
            assert!(!client.is_local());
            assert_eq!(pending.await.unwrap().unwrap_err().message, "disconnected");
            assert!(client.call("offline", &Value::Null).await.is_err());
            first_server.await.unwrap();
            client.set_transport(Some(second_target));
            drop(
                connection
                    .wait_for(|state| matches!(state, ConnectionState::Connected))
                    .await
                    .unwrap(),
            );
            assert_eq!(
                client.call("replacement", &Value::Null).await.unwrap(),
                "new-session"
            );
            drop(client);
            second_server.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "test mutex must remain healthy")]
    fn pending_request_guard_cleans_up_canceled_call() {
        let pending = PendingMap::default();
        let (sender, _receiver) = oneshot::channel();
        pending.lock().unwrap().insert("request".into(), sender);
        {
            let _guard = PendingRequestGuard {
                id: "request".into(),
                pending: pending.clone(),
            };
        }
        assert!(pending.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(
        clippy::expect_used,
        reason = "concurrency regression test should fail fast on task or timeout errors"
    )]
    async fn concurrent_call_and_teardown_never_strands_response() {
        let (msg_tx, _msg_rx) = mpsc::unbounded_channel();
        let tx_slot: OutboundSlot = Arc::new(Mutex::new(Some(msg_tx)));
        let pending = PendingMap::default();
        let (notifications, _) = broadcast::channel(1);
        let (connection, _) = watch::channel(ConnectionState::Connected);
        let client = Arc::new(Client {
            tx: tx_slot.clone(),
            pending: pending.clone(),
            notifications,
            connection: Arc::new(connection),
            transport: watch::channel(None).0,
        });
        let barrier = Arc::new(tokio::sync::Barrier::new(2));

        let call_task = tokio::spawn({
            let client = client.clone();
            let barrier = barrier.clone();
            async move {
                barrier.wait().await;
                client.call("test.concurrent", &Value::Null).await
            }
        });
        let teardown_task = tokio::spawn(async move {
            barrier.wait().await;
            teardown_session(&tx_slot, &pending);
        });

        teardown_task.await.expect("teardown task should complete");
        let call_result = tokio::time::timeout(Duration::from_secs(1), call_task)
            .await
            .expect("call must not remain pending after teardown")
            .expect("call task should complete");
        let error = call_result.expect_err("teardown cannot produce a successful response");
        assert!(matches!(
            error.message.as_str(),
            "disconnected" | "not connected"
        ));
    }

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
        // the frontend pick up Core within ~250 ms of HTTP bind on a
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

    #[tokio::test]
    async fn offline_test_endpoint_is_rejected_before_transport() {
        // The UI harness relies on a scheme with no transport/default port.
        // Keep this rejection locked when upgrading the WebSocket dependency.
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            tokio_tungstenite::connect_async("zaparoo-offline://localhost"),
        )
        .await;
        assert!(matches!(
            result,
            Ok(Err(tokio_tungstenite::tungstenite::Error::Url(
                tokio_tungstenite::tungstenite::error::UrlError::UnsupportedUrlScheme
            )))
        ));
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
        // successful connect once it's up. The frontend should publish
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
