// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `bind_to_endpoint!` — boilerplate eliminator for QML singletons that
// surface a `Store` endpoint's `ResourceStatus<T>` to the UI.
//
// Without it, every singleton hand-rolled the same five-step dance:
// subscribe to the catalog watch, sync-seed from `borrow_and_update()`
// (the contract in `docs/cxx-qt-bridge.md`), capture a `qt_thread`,
// spawn a tokio task that listens for changes, and route each change
// through `qt_thread.queue` so property setters fire on the GUI
// thread. Forgetting the sync-seed step caused the recurring MiSTer
// "screen never updates if Core connects before QML loads" race.
//
// The macro takes two free functions:
//   `select`: project the resource status into a Send + 'static value
//             (e.g. `(i32, String)` for the connection-state banner).
//   `apply`:  consume the projected value plus `Pin<&mut Target>` and
//             update QProperties / call beginResetModel / etc.
//
// Both functions run twice: once synchronously during `initialize()`
// for the seed, then once per change inside the `qt_thread.queue`
// callback. Using free fn paths (rather than closures) keeps them
// `Copy` so they can be reused across the two call sites without any
// `Fn`/`Send`/`Sync` gymnastics.
//
// Per-arg endpoints (currently just `MediaSearchEndpoint`, used by
// `GamesModel::set_system`) are hand-rolled rather than driven by
// this macro. The shape is the same — sync-seed, qt_thread watcher
// — plus two extras: aborting the previous watcher's `JoinHandle`
// and a monotonic ticket so callbacks queued by the previous args
// don't apply to the new args (`JoinHandle::abort` does not drain
// callbacks already on the Qt event loop). With one such call site,
// a hand-rolled implementation with inline comments is clearer than
// a macro that bakes in guesses about which parts are generic.
// Revisit when a second per-arg endpoint appears (BrowseModel,
// NowPlayingForSystem, …) — at that point factor the
// abort+ticket+seed+spawn dance into `bind_to_arg_endpoint!` so the
// stale-callback bug class is closed structurally rather than by
// convention.

/// Generate `Initialize` for a cxx-qt QML singleton bound to a `Store`
/// endpoint. See module docs for the contract; the seed is automatic.
#[macro_export]
macro_rules! bind_to_endpoint {
    (
        for $target:ty,
        endpoint = $endpoint:ty,
        args = $args:expr,
        select = $select:path,
        apply = $apply:path $(,)?
    ) => {
        impl ::cxx_qt::Initialize for $target {
            fn initialize(mut self: ::std::pin::Pin<&mut Self>) {
                use ::cxx_qt::Threading;
                let mut rx = $crate::models::global_store()
                    .subscribe::<$endpoint>($args)
                    .subscribe();

                // Sync seed: project the current resource status and
                // apply it before spawning the watcher, so the first
                // QML frame sees real state rather than the
                // `Default::default()` placeholder.
                let projected = $select(&*rx.borrow_and_update());
                $apply(self.as_mut(), projected);

                let qt_thread = self.qt_thread();
                $crate::models::global_handle().spawn(async move {
                    while rx.changed().await.is_ok() {
                        let projected = $select(&*rx.borrow_and_update());
                        let _ = qt_thread.queue(move |m| $apply(m, projected));
                    }
                });
            }
        }
    };
}
