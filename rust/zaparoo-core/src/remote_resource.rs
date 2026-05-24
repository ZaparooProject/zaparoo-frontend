// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// One shape for any RPC-backed value that the UI binds to.
//
// Every screen needs the same four states: nothing yet, fetching now,
// here's the data, fetch failed. Before this module each model invented
// its own `loading` / `error_message` / `has_*` triplet and wired it by
// hand in every match arm; that's how `GamesModel::set_system` ended up
// not clearing `has_next_page` on the error path. `ResourceStatus<T>`
// makes the four states one enum so the QML translation layer is the
// only place per-screen mapping lives, and pagination flags ride inside
// `Ready(T)` where they can't drift.
//
// `RemoteResource::driven_by` ties the resource to the connection state
// machine in `client::ConnectionState`:
//   - `Disconnected`            → `Idle`
//   - `Connecting`/`Reconnecting` → `Loading`
//   - `Unreachable(msg)`        → `Errored { retrying: false, .. }`
//   - `Connected`               → run `fetch`, retry while still
//                                  Connected with capped backoff,
//                                  publish `Ready(T)` on success
//
// Because the dispatch reads the *current* connection state on every
// loop iteration (via `borrow_and_update`), any transition into
// `Connected` — including from `Reconnecting` after a transient drop —
// triggers a refetch. Refresh-on-reconnect is a property of the
// abstraction, not a per-call-site obligation.
//
// `RemoteResource::refetch()` exposes an explicit invalidation pulse
// for tag-driven re-fetching. While connected, calling it cancels any
// in-flight fetch and starts a new one. While not connected, the
// notification queues and fires on the next reconnect (one extra fetch
// alongside the natural refresh-on-reconnect; harmless). Subscribers
// don't observe the difference between an explicit refetch and a
// reconnect refresh.

use crate::client::{backoff_delay, Client, ClientError, ConnectionState};
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{oneshot, watch, Notify};
use tracing::warn;

/// Lifecycle of a remote-fetched value as it appears to the UI.
///
/// `retrying` distinguishes "we hit an error but the socket is still
/// up so we'll try again" (yellow / spinner) from "we hit an error and
/// the socket has gone away" (red / static). UI code can choose to
/// render both the same; the distinction exists so it doesn't have to
/// ask the connection layer separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceStatus<T> {
    Idle,
    Loading,
    Ready(T),
    Errored { message: String, retrying: bool },
}

#[derive(Debug)]
pub struct RemoteResource<T: Clone + Send + Sync + 'static> {
    status: watch::Sender<ResourceStatus<T>>,
    // Pulse for explicit refetch invalidation. `notify_one` queues at
    // most one notification, so back-to-back invalidations collapse to
    // a single re-fetch. While the connection is not Connected the
    // pulse queues, then fires on the next reconnect (one extra fetch
    // atop the natural refresh-on-reconnect; harmless).
    refetch: Arc<Notify>,
    // Held only for its Drop side-effect: when the resource is dropped,
    // this sender drops, which fires the cancellation receiver inside
    // the spawned task and unwinds it (cancelling any in-flight fetch
    // future via tokio::select!). Necessary for resources with shorter
    // lifetimes than the `Client` — e.g. per-system search resources
    // that are replaced when the user picks a different system.
    _cancel_tx: oneshot::Sender<()>,
}

impl<T: Clone + Send + Sync + 'static> RemoteResource<T> {
    pub fn subscribe(&self) -> watch::Receiver<ResourceStatus<T>> {
        self.status.subscribe()
    }

    /// Trigger a refetch outside the natural connection-change cadence.
    /// While connected, this cancels any in-flight fetch and starts a
    /// new one; otherwise the notification queues and fires on the
    /// next reconnect. The store layer drives tag-based invalidation
    /// through this method; direct callers rarely need it.
    pub fn refetch(&self) {
        self.refetch.notify_one();
    }

    /// Spawn a task on `runtime` that drives the resource lifecycle off
    /// `client.connection`. `fetch` is invoked on every transition into
    /// `Connected` (and, while still Connected, on retry after error
    /// with capped exponential backoff).
    ///
    /// Lifecycle: dropping the returned `RemoteResource` cancels the
    /// spawned task and any in-flight fetch (the held `_cancel_tx`
    /// fires its receiver inside the `tokio::select!`). The task also
    /// exits when the connection watch is closed — i.e. when the
    /// `Client` is dropped — so callers don't have to manage either
    /// lifetime end explicitly. Both shutdown paths are exercised by
    /// `dropping_resource_cancels_spawned_task` and the surrounding
    /// tests.
    pub fn driven_by<F, Fut>(client: Arc<Client>, runtime: &Handle, fetch: F) -> Self
    where
        F: Fn(Arc<Client>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, ClientError>> + Send + 'static,
    {
        let connection_rx = client.connection.subscribe();
        let fetch = move || fetch(client.clone());
        Self::spawn_with(connection_rx, runtime, fetch)
    }

    /// Internal entry point used by `driven_by` and tests. Decoupled
    /// from `Client` so tests can drive a synthetic `ConnectionState`
    /// watch without standing up a real WebSocket.
    pub(crate) fn spawn_with<F, Fut>(
        mut connection_rx: watch::Receiver<ConnectionState>,
        runtime: &Handle,
        fetch: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, ClientError>> + Send + 'static,
    {
        // Seed the watch from the *current* connection state so a
        // subscriber that calls `borrow()` before the spawned task
        // runs sees the right value. Without this seed a singleton
        // constructed after the WebSocket has already advanced past
        // `Disconnected` would observe the default `Idle` on the
        // first frame and stay there until the next state transition
        // (the MiSTer "screen never updates if Core connects before
        // QML loads" race).
        let initial_status = match &*connection_rx.borrow() {
            ConnectionState::Disconnected => ResourceStatus::Idle,
            ConnectionState::Connecting
            | ConnectionState::Reconnecting
            | ConnectionState::Connected => ResourceStatus::Loading,
            ConnectionState::Unreachable(msg) => ResourceStatus::Errored {
                message: msg.clone(),
                retrying: false,
            },
        };
        let (status_tx, _) = watch::channel(initial_status);
        let status_for_task = status_tx.clone();
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let refetch = Arc::new(Notify::new());
        let refetch_for_task = refetch.clone();

        runtime.spawn(async move {
            loop {
                let conn = connection_rx.borrow_and_update().clone();
                match conn {
                    ConnectionState::Disconnected => {
                        status_for_task.send_replace(ResourceStatus::Idle);
                    }
                    ConnectionState::Connecting | ConnectionState::Reconnecting => {
                        status_for_task.send_replace(ResourceStatus::Loading);
                    }
                    ConnectionState::Unreachable(msg) => {
                        status_for_task.send_replace(ResourceStatus::Errored {
                            message: msg,
                            retrying: false,
                        });
                    }
                    ConnectionState::Connected => {
                        status_for_task.send_replace(ResourceStatus::Loading);
                        // Race the connected loop against cancellation
                        // so dropping the resource mid-fetch aborts the
                        // in-flight RPC.
                        tokio::select! {
                            biased;
                            _ = &mut cancel_rx => return,
                            () = run_connected(
                                &fetch,
                                &mut connection_rx,
                                &status_for_task,
                                &refetch_for_task,
                            ) => {}
                        }
                        // run_connected returns when the connection
                        // state has changed; loop and dispatch the new
                        // state.
                        continue;
                    }
                }

                tokio::select! {
                    biased;
                    _ = &mut cancel_rx => return,
                    res = connection_rx.changed() => {
                        if res.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Self {
            status: status_tx,
            refetch,
            _cancel_tx: cancel_tx,
        }
    }
}

/// Inner loop active while the connection is `Connected`. Returns when
/// the connection state changes (so the outer loop can dispatch the new
/// state).
async fn run_connected<T, F, Fut>(
    fetch: &F,
    connection_rx: &mut watch::Receiver<ConnectionState>,
    status: &watch::Sender<ResourceStatus<T>>,
    refetch: &Arc<Notify>,
) where
    T: Clone + Send + Sync + 'static,
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ClientError>> + Send,
{
    let mut rpc_failures: u32 = 0;
    loop {
        // Race the fetch against (a) a connection-state change — abort
        // and let the outer loop publish the appropriate state — or
        // (b) an explicit refetch — drop the in-flight attempt and
        // start over (RTK-Query-style invalidation).
        let attempt = fetch();
        tokio::pin!(attempt);
        let result = tokio::select! {
            biased;
            _ = connection_rx.changed() => return,
            () = refetch.notified() => continue,
            r = &mut attempt => r,
        };

        match result {
            Ok(value) => {
                status.send_replace(ResourceStatus::Ready(value));
                // Reset the failure count: a subsequent refetch-driven
                // retry should not inherit the previous backoff.
                rpc_failures = 0;
                // Ready stays sticky until either (a) the connection
                // transitions — outer loop re-dispatches — or (b) an
                // explicit refetch — loop and fetch again. Dropping
                // out of the refetch arm falls through to the next
                // iteration of the enclosing `loop`.
                tokio::select! {
                    biased;
                    _ = connection_rx.changed() => return,
                    () = refetch.notified() => {}
                }
            }
            Err(e) => {
                rpc_failures = rpc_failures.saturating_add(1);
                warn!(
                    "RemoteResource fetch failed (attempt {rpc_failures}): {}",
                    e.message
                );
                status.send_replace(ResourceStatus::Errored {
                    message: e.message,
                    retrying: true,
                });
                // RPC-level retries always use the steady-state curve;
                // the connect-loop's boot-window fast retry doesn't
                // apply here because we only reach this path after a
                // successful WebSocket session is up.
                let delay = backoff_delay(rpc_failures, false);
                // refetch wins over the backoff timer: explicit
                // invalidation should retry immediately, not wait out
                // the existing schedule. Falling through either of the
                // non-return arms loops back to retry.
                tokio::select! {
                    biased;
                    _ = connection_rx.changed() => return,
                    () = refetch.notified() => {}
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::manual_assert,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Wait for the resource status to satisfy a predicate, with a
    /// generous timeout so a stuck task fails the test rather than
    /// hanging.
    async fn wait_for<T, P>(
        rx: &mut watch::Receiver<ResourceStatus<T>>,
        mut pred: P,
    ) -> ResourceStatus<T>
    where
        T: Clone + Send + Sync + 'static + std::fmt::Debug,
        P: FnMut(&ResourceStatus<T>) -> bool,
    {
        timeout(Duration::from_secs(5), async {
            loop {
                {
                    let cur = rx.borrow_and_update().clone();
                    if pred(&cur) {
                        return cur;
                    }
                }
                if rx.changed().await.is_err() {
                    panic!("watch channel closed before predicate matched");
                }
            }
        })
        .await
        .expect("timed out waiting for resource status")
    }

    #[test]
    fn idle_until_first_connect() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let res =
                RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), || async { Ok(42) });
            let sub = res.subscribe();
            // Disconnected -> Idle is both the seeded initial value
            // and the task's first publish; either way the observable
            // status is Idle.
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(matches!(*sub.borrow(), ResourceStatus::Idle));
            drop(conn_tx);
        });
    }

    #[test]
    fn connected_triggers_fetch_and_publishes_ready() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let res =
                RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), || async { Ok(42) });
            let mut sub = res.subscribe();
            conn_tx.send_replace(ConnectionState::Connected);
            let final_status = wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(42))).await;
            assert!(matches!(final_status, ResourceStatus::Ready(42)));
            drop(conn_tx);
        });
    }

    #[test]
    fn reconnect_triggers_refetch() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let calls = Arc::new(AtomicUsize::new(0));
            let calls_clone = calls.clone();
            let res = RemoteResource::<usize>::spawn_with(conn_rx, runtime.handle(), move || {
                let n = calls_clone.fetch_add(1, Ordering::SeqCst) + 1;
                async move { Ok(n) }
            });
            let mut sub = res.subscribe();

            conn_tx.send_replace(ConnectionState::Connected);
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(1))).await;

            conn_tx.send_replace(ConnectionState::Reconnecting);
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Loading)).await;

            conn_tx.send_replace(ConnectionState::Connected);
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(2))).await;

            assert_eq!(calls.load(Ordering::SeqCst), 2);
            drop(conn_tx);
        });
    }

    #[test]
    fn error_with_socket_up_publishes_retrying_then_recovers() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let calls = Arc::new(AtomicUsize::new(0));
            let calls_clone = calls.clone();
            let res =
                RemoteResource::<&'static str>::spawn_with(conn_rx, runtime.handle(), move || {
                    let n = calls_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        if n == 1 {
                            Err(ClientError {
                                message: "transient".into(),
                            })
                        } else {
                            Ok("ok")
                        }
                    }
                });
            let mut sub = res.subscribe();

            conn_tx.send_replace(ConnectionState::Connected);
            // First attempt errors with retrying=true; the second
            // succeeds (the same Connected window keeps trying after
            // backoff).
            wait_for(&mut sub, |s| {
                matches!(s, ResourceStatus::Errored { retrying: true, .. })
            })
            .await;
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready("ok"))).await;
            drop(conn_tx);
        });
    }

    #[test]
    fn dropping_resource_cancels_spawned_task() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Connected);
            let calls = Arc::new(AtomicUsize::new(0));
            let calls_clone = calls.clone();
            let res = RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), move || {
                calls_clone.fetch_add(1, Ordering::SeqCst);
                async move { Ok(0) }
            });
            let mut sub = res.subscribe();
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(0))).await;
            let baseline = calls.load(Ordering::SeqCst);

            // Drop the resource — its task should exit and stop reacting
            // to subsequent connection-state changes.
            drop(res);
            // Give the runtime a beat to observe the cancellation.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Cycle the connection to prove the cancelled task no longer
            // refetches.
            conn_tx.send_replace(ConnectionState::Reconnecting);
            conn_tx.send_replace(ConnectionState::Connected);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(
                calls.load(Ordering::SeqCst),
                baseline,
                "fetch must not run after the resource is dropped"
            );
        });
    }

    #[test]
    fn unreachable_publishes_non_retrying_error() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let res = RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), || async {
                Ok(0) // never called — Unreachable doesn't trigger fetch
            });
            let mut sub = res.subscribe();
            conn_tx.send_replace(ConnectionState::Unreachable("dead".into()));
            let status = wait_for(&mut sub, |s| {
                matches!(
                    s,
                    ResourceStatus::Errored {
                        retrying: false,
                        ..
                    }
                )
            })
            .await;
            match status {
                ResourceStatus::Errored {
                    message,
                    retrying: false,
                } => assert_eq!(message, "dead"),
                other => panic!("unexpected status: {other:?}"),
            }
            drop(conn_tx);
        });
    }

    #[test]
    fn seed_loading_when_already_connected() {
        let runtime = rt();
        runtime.block_on(async {
            let (_conn_tx, conn_rx) = watch::channel(ConnectionState::Connected);
            // Long-running fetch keeps the spawned task in Loading; it
            // never publishes Ready while we're checking the seed.
            let res = RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), || async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(0)
            });
            let sub = res.subscribe();
            // Without the seed fix the watch is initialised with Idle
            // and a borrow() before the spawned task runs would observe
            // Idle (the original MiSTer race). With the fix the seed
            // reflects the current ConnectionState — Connected -> Loading
            // — so borrow() is correct from the first frame.
            let initial = sub.borrow().clone();
            assert!(
                matches!(initial, ResourceStatus::Loading),
                "seed must reflect Connected state, got {initial:?}"
            );
        });
    }

    #[test]
    fn seed_errored_when_unreachable() {
        let runtime = rt();
        runtime.block_on(async {
            let (_conn_tx, conn_rx) =
                watch::channel(ConnectionState::Unreachable("startup-failure".into()));
            let res =
                RemoteResource::<i32>::spawn_with(conn_rx, runtime.handle(), || async { Ok(0) });
            let sub = res.subscribe();
            let initial = sub.borrow().clone();
            match initial {
                ResourceStatus::Errored {
                    message,
                    retrying: false,
                } => assert_eq!(message, "startup-failure"),
                other => panic!("expected Errored, got {other:?}"),
            }
        });
    }

    #[test]
    fn refetch_notify_triggers_refetch() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let calls = Arc::new(AtomicUsize::new(0));
            let calls_clone = calls.clone();
            let res = RemoteResource::<usize>::spawn_with(conn_rx, runtime.handle(), move || {
                let n = calls_clone.fetch_add(1, Ordering::SeqCst) + 1;
                async move { Ok(n) }
            });
            let mut sub = res.subscribe();

            conn_tx.send_replace(ConnectionState::Connected);
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(1))).await;
            assert_eq!(calls.load(Ordering::SeqCst), 1);

            // Explicit refetch — fetch is invoked again, Ready advances.
            res.refetch();
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(2))).await;
            assert_eq!(calls.load(Ordering::SeqCst), 2);

            drop(conn_tx);
        });
    }

    /// Regression: pulsing `refetch()` while the connection has not
    /// yet come up must not fire the fetch closure (there's nothing
    /// to fetch over). The pulse is held by the `Notify` and consumed
    /// once the dispatcher reaches the connected branch — at which
    /// point at least one fetch must run. The risk this guards
    /// against is a future rewire that adds `refetch.notified()` to
    /// the disconnected arm of the outer select and silently fires
    /// fetch with no socket.
    #[test]
    fn refetch_pulse_while_disconnected_does_not_fetch_until_connected() {
        let runtime = rt();
        runtime.block_on(async {
            let (conn_tx, conn_rx) = watch::channel(ConnectionState::Disconnected);
            let calls = Arc::new(AtomicUsize::new(0));
            let calls_clone = calls.clone();
            let res = RemoteResource::<usize>::spawn_with(conn_rx, runtime.handle(), move || {
                let n = calls_clone.fetch_add(1, Ordering::SeqCst) + 1;
                async move { Ok(n) }
            });
            let mut sub = res.subscribe();

            // Pulse while still Disconnected — must not invoke fetch.
            res.refetch();
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "fetch must not run while disconnected"
            );
            assert!(matches!(*sub.borrow_and_update(), ResourceStatus::Idle));

            // Transition to Connected — the queued pulse and the
            // first connect both prompt fetches. We only assert at
            // least one fetch ran (not the exact count) because the
            // queued pulse can race the first fetch to completion.
            conn_tx.send_replace(ConnectionState::Connected);
            wait_for(&mut sub, |s| matches!(s, ResourceStatus::Ready(_))).await;
            assert!(
                calls.load(Ordering::SeqCst) >= 1,
                "fetch must run at least once after connecting"
            );

            drop(conn_tx);
        });
    }
}
