// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.MediaStatus` — QML-facing view of Core's media-database build
// state and scraper progress. Subscribes to the
// `MediaStatusResource` watch channel set up by the store, and exposes
// every field the frontend renders as a Q_PROPERTY plus the four action
// methods (`start_index`, `cancel_index`, `start_scrape`, `cancel_scrape`)
// as `qinvokable`s.
//
// Why this isn't `bind_to_endpoint!`: the resource is a plain
// `tokio::sync::watch::Sender<MediaStatusState>` rather than an
// `Endpoint`, because the underlying state is half-pull / half-push —
// see `rust/zaparoo-core/src/store/media_status.rs` for the rationale.
// The wiring is shaped exactly like `app_status.rs::bind_link_state`:
// seed via `borrow_and_update`, then loop on `changed().await`, queueing
// each projected snapshot back onto the QObject's Qt thread.
//
// Action methods (`start_index` / `cancel_index` / `start_scrape` /
// `cancel_scrape`) spawn fire-and-forget tasks on the global runtime —
// the resulting state changes flow back through Core's notification
// stream, which the resource folds into the same watch channel. Caller
// errors are logged but not surfaced as a Q_PROPERTY because the
// notification stream is the source of truth for what the UI renders.

use cxx_qt::{Initialize, Threading};
use cxx_qt_lib::QString;
use std::pin::Pin;
use tracing::warn;
use zaparoo_core::media_types::{MediaIndexParams, MediaScrapeParams};
use zaparoo_core::store::MediaStatusState;

#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags; one bool per Core status field exposed to QML"
)]
#[derive(Default)]
pub struct MediaStatusRust {
    seeded: bool,

    exists: bool,
    indexing: bool,
    optimizing: bool,
    paused: bool,
    current_step: i32,
    total_steps: i32,
    current_step_display: QString,
    total_files: i32,
    total_media: i32,

    scraping: bool,
    scrape_done: bool,
    scrape_paused: bool,
    scrape_processed: i32,
    scrape_total: i32,
    scrape_matched: i32,
    scrape_skipped: i32,
    scrape_total_scraped: i32,
    scrape_force: bool,
    scrape_force_known: bool,
    scrape_system_id: QString,
    scrape_scraper_id: QString,
    scrape_state: QString,
    scrape_error: QString,
    scrape_current_step: i32,
    scrape_total_steps: i32,
    scrape_current_step_display: QString,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, seeded)]
        #[qproperty(bool, exists)]
        #[qproperty(bool, indexing)]
        #[qproperty(bool, optimizing)]
        #[qproperty(bool, paused)]
        #[qproperty(i32, current_step)]
        #[qproperty(i32, total_steps)]
        #[qproperty(QString, current_step_display)]
        #[qproperty(i32, total_files)]
        #[qproperty(i32, total_media)]
        #[qproperty(bool, scraping)]
        #[qproperty(bool, scrape_done)]
        #[qproperty(bool, scrape_paused)]
        #[qproperty(i32, scrape_processed)]
        #[qproperty(i32, scrape_total)]
        #[qproperty(i32, scrape_matched)]
        #[qproperty(i32, scrape_skipped)]
        #[qproperty(i32, scrape_total_scraped)]
        #[qproperty(bool, scrape_force)]
        #[qproperty(bool, scrape_force_known)]
        #[qproperty(QString, scrape_system_id)]
        #[qproperty(QString, scrape_scraper_id)]
        #[qproperty(QString, scrape_state)]
        #[qproperty(QString, scrape_error)]
        #[qproperty(i32, scrape_current_step)]
        #[qproperty(i32, scrape_total_steps)]
        #[qproperty(QString, scrape_current_step_display)]
        type MediaStatus = super::MediaStatusRust;

        #[qinvokable]
        fn start_index(self: Pin<&mut MediaStatus>);

        #[qinvokable]
        fn cancel_index(self: Pin<&mut MediaStatus>);

        #[qinvokable]
        fn start_scrape(self: Pin<&mut MediaStatus>, force: bool);

        #[qinvokable]
        fn cancel_scrape(self: Pin<&mut MediaStatus>);
    }

    impl cxx_qt::Threading for MediaStatus {}
    impl cxx_qt::Initialize for MediaStatus {}
}

/// Mirror of `MediaStatusState` shaped for the closure-queue contract:
/// owned `String`s become `QString`s ahead of the Qt-thread hop so the
/// apply path doesn't need to allocate while holding the model pin.
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags; mirrors the QML singleton's bool surface"
)]
struct Snapshot {
    seeded: bool,

    exists: bool,
    indexing: bool,
    optimizing: bool,
    paused: bool,
    current_step: i32,
    total_steps: i32,
    current_step_display: QString,
    total_files: i32,
    total_media: i32,

    scraping: bool,
    scrape_done: bool,
    scrape_paused: bool,
    scrape_processed: i32,
    scrape_total: i32,
    scrape_matched: i32,
    scrape_skipped: i32,
    scrape_total_scraped: i32,
    scrape_force: bool,
    scrape_force_known: bool,
    scrape_system_id: QString,
    scrape_scraper_id: QString,
    scrape_state: QString,
    scrape_error: QString,
    scrape_current_step: i32,
    scrape_total_steps: i32,
    scrape_current_step_display: QString,
}

fn project(state: &MediaStatusState) -> Snapshot {
    Snapshot {
        seeded: state.seeded,
        exists: state.exists,
        indexing: state.indexing,
        optimizing: state.optimizing,
        paused: state.paused,
        current_step: state.current_step,
        total_steps: state.total_steps,
        current_step_display: QString::from(state.current_step_display.as_str()),
        total_files: state.total_files,
        total_media: state.total_media,
        scraping: state.scraping,
        scrape_done: state.scrape_done,
        scrape_paused: state.scrape_paused,
        scrape_processed: state.scrape_processed,
        scrape_total: state.scrape_total,
        scrape_matched: state.scrape_matched,
        scrape_skipped: state.scrape_skipped,
        scrape_total_scraped: state.scrape_total_scraped,
        scrape_force: state.scrape_force,
        scrape_force_known: state.scrape_force_known,
        scrape_system_id: QString::from(state.scrape_system_id.as_str()),
        scrape_scraper_id: QString::from(state.scrape_scraper_id.as_str()),
        scrape_state: QString::from(state.scrape_state.as_str()),
        scrape_error: QString::from(state.scrape_error.as_str()),
        scrape_current_step: state.scrape_current_step,
        scrape_total_steps: state.scrape_total_steps,
        scrape_current_step_display: QString::from(state.scrape_current_step_display.as_str()),
    }
}

impl Initialize for ffi::MediaStatus {
    fn initialize(mut self: Pin<&mut Self>) {
        let resource = crate::models::global_store().media_status();
        let mut rx = resource.subscribe();
        apply(self.as_mut(), project(&rx.borrow_and_update()));

        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            while rx.changed().await.is_ok() {
                let snapshot = project(&rx.borrow_and_update());
                let _ = qt_thread.queue(move |m| apply(m, snapshot));
            }
        });
    }
}

impl ffi::MediaStatus {
    fn start_index(self: Pin<&mut Self>) {
        let resource = crate::models::global_store().media_status();
        crate::models::global_handle().spawn(async move {
            if let Err(e) = resource.start_index(MediaIndexParams::default()).await {
                warn!("media_status: start_index failed: {}", e.message);
            }
        });
    }

    fn cancel_index(self: Pin<&mut Self>) {
        let resource = crate::models::global_store().media_status();
        crate::models::global_handle().spawn(async move {
            if let Err(e) = resource.cancel_index().await {
                warn!("media_status: cancel_index failed: {}", e.message);
            }
        });
    }

    fn start_scrape(self: Pin<&mut Self>, force: bool) {
        let resource = crate::models::global_store().media_status();
        crate::models::global_handle().spawn(async move {
            // ES gamelist.xml across every indexed system. `force` is
            // a one-shot UI toggle for re-scraping existing metadata.
            // Core ships this scraper in-tree (see `pkg/api/server.go`
            // wiring `gamelistxml.NewGamelistXMLScraper`) and validates
            // `scraperId` as `min=1` — there is no server-side "default"
            // alias, so the id has to be a real scraper. An empty
            // `systems` runs every system the scraper supports
            // (gamelist.xml supports all). Picking a different scraper
            // or a system subset is a future chooser-UI job.
            let params = MediaScrapeParams {
                scraper_id: "gamelist.xml".into(),
                systems: Vec::new(),
                force,
            };
            if let Err(e) = resource.start_scrape(params).await {
                warn!("media_status: start_scrape failed: {}", e.message);
            }
        });
    }

    fn cancel_scrape(self: Pin<&mut Self>) {
        let resource = crate::models::global_store().media_status();
        crate::models::global_handle().spawn(async move {
            if let Err(e) = resource.cancel_scrape().await {
                warn!("media_status: cancel_scrape failed: {}", e.message);
            }
        });
    }
}

#[allow(
    clippy::cognitive_complexity,
    reason = "21 fields × diff-and-set is mechanical; folding it loses the per-field NOTIFY suppression"
)]
fn apply(mut model: Pin<&mut ffi::MediaStatus>, s: Snapshot) {
    if model.seeded != s.seeded {
        model.as_mut().set_seeded(s.seeded);
    }
    if model.exists != s.exists {
        model.as_mut().set_exists(s.exists);
    }
    if model.indexing != s.indexing {
        model.as_mut().set_indexing(s.indexing);
    }
    if model.optimizing != s.optimizing {
        model.as_mut().set_optimizing(s.optimizing);
    }
    if model.paused != s.paused {
        model.as_mut().set_paused(s.paused);
    }
    if model.current_step != s.current_step {
        model.as_mut().set_current_step(s.current_step);
    }
    if model.total_steps != s.total_steps {
        model.as_mut().set_total_steps(s.total_steps);
    }
    if model.current_step_display != s.current_step_display {
        model
            .as_mut()
            .set_current_step_display(s.current_step_display);
    }
    if model.total_files != s.total_files {
        model.as_mut().set_total_files(s.total_files);
    }
    if model.total_media != s.total_media {
        model.as_mut().set_total_media(s.total_media);
    }
    if model.scraping != s.scraping {
        model.as_mut().set_scraping(s.scraping);
    }
    if model.scrape_done != s.scrape_done {
        model.as_mut().set_scrape_done(s.scrape_done);
    }
    if model.scrape_paused != s.scrape_paused {
        model.as_mut().set_scrape_paused(s.scrape_paused);
    }
    if model.scrape_processed != s.scrape_processed {
        model.as_mut().set_scrape_processed(s.scrape_processed);
    }
    if model.scrape_total != s.scrape_total {
        model.as_mut().set_scrape_total(s.scrape_total);
    }
    if model.scrape_matched != s.scrape_matched {
        model.as_mut().set_scrape_matched(s.scrape_matched);
    }
    if model.scrape_skipped != s.scrape_skipped {
        model.as_mut().set_scrape_skipped(s.scrape_skipped);
    }
    if model.scrape_total_scraped != s.scrape_total_scraped {
        model
            .as_mut()
            .set_scrape_total_scraped(s.scrape_total_scraped);
    }
    if model.scrape_force != s.scrape_force {
        model.as_mut().set_scrape_force(s.scrape_force);
    }
    if model.scrape_force_known != s.scrape_force_known {
        model.as_mut().set_scrape_force_known(s.scrape_force_known);
    }
    if model.scrape_system_id != s.scrape_system_id {
        model.as_mut().set_scrape_system_id(s.scrape_system_id);
    }
    if model.scrape_scraper_id != s.scrape_scraper_id {
        model.as_mut().set_scrape_scraper_id(s.scrape_scraper_id);
    }
    if model.scrape_state != s.scrape_state {
        model.as_mut().set_scrape_state(s.scrape_state);
    }
    if model.scrape_error != s.scrape_error {
        model.as_mut().set_scrape_error(s.scrape_error);
    }
    if model.scrape_current_step != s.scrape_current_step {
        model
            .as_mut()
            .set_scrape_current_step(s.scrape_current_step);
    }
    if model.scrape_total_steps != s.scrape_total_steps {
        model.as_mut().set_scrape_total_steps(s.scrape_total_steps);
    }
    if model.scrape_current_step_display != s.scrape_current_step_display {
        model
            .as_mut()
            .set_scrape_current_step_display(s.scrape_current_step_display);
    }
}

#[cfg(test)]
mod tests {
    use super::project;
    use cxx_qt_lib::QString;
    use zaparoo_core::store::MediaStatusState;

    #[test]
    fn project_copies_every_field() {
        let state = MediaStatusState {
            seeded: true,
            exists: true,
            indexing: true,
            optimizing: false,
            paused: false,
            current_step: 3,
            total_steps: 5,
            current_step_display: "Indexing SNES".into(),
            total_files: 1234,
            total_media: 567,
            scraping: false,
            scrape_done: false,
            scrape_paused: false,
            scrape_processed: 0,
            scrape_total: 0,
            scrape_matched: 0,
            scrape_skipped: 0,
            scrape_total_scraped: 0,
            scrape_force: false,
            scrape_force_known: false,
            scrape_system_id: String::new(),
            scrape_scraper_id: String::new(),
            scrape_state: String::new(),
            scrape_error: String::new(),
            scrape_current_step: 0,
            scrape_total_steps: 0,
            scrape_current_step_display: String::new(),
        };
        let snapshot = project(&state);
        assert!(snapshot.seeded);
        assert!(snapshot.exists);
        assert!(snapshot.indexing);
        assert_eq!(snapshot.current_step, 3);
        assert_eq!(snapshot.total_steps, 5);
        assert_eq!(
            snapshot.current_step_display,
            QString::from("Indexing SNES"),
        );
        assert_eq!(snapshot.total_files, 1234);
        assert_eq!(snapshot.total_media, 567);
    }

    #[test]
    fn project_preserves_scraping_state() {
        let state = MediaStatusState {
            scraping: true,
            scrape_processed: 12,
            scrape_total: 200,
            scrape_matched: 10,
            scrape_skipped: 2,
            scrape_total_scraped: 50,
            scrape_force: true,
            scrape_force_known: true,
            scrape_system_id: "SNES".into(),
            scrape_scraper_id: "screenscraper".into(),
            scrape_state: "running".into(),
            scrape_current_step: 2,
            scrape_total_steps: 5,
            scrape_current_step_display: "Super Nintendo".into(),
            ..MediaStatusState::default()
        };
        let snapshot = project(&state);
        assert!(snapshot.scraping);
        assert_eq!(snapshot.scrape_processed, 12);
        assert_eq!(snapshot.scrape_total, 200);
        assert_eq!(snapshot.scrape_matched, 10);
        assert_eq!(snapshot.scrape_skipped, 2);
        assert_eq!(snapshot.scrape_total_scraped, 50);
        assert!(snapshot.scrape_force);
        assert!(snapshot.scrape_force_known);
        assert_eq!(snapshot.scrape_system_id, QString::from("SNES"));
        assert_eq!(snapshot.scrape_scraper_id, QString::from("screenscraper"));
        assert_eq!(snapshot.scrape_state, QString::from("running"));
        assert_eq!(snapshot.scrape_current_step, 2);
        assert_eq!(snapshot.scrape_total_steps, 5);
        assert_eq!(
            snapshot.scrape_current_step_display,
            QString::from("Super Nintendo"),
        );
    }

    #[test]
    fn project_default_state_is_quiet() {
        let snapshot = project(&MediaStatusState::default());
        assert!(!snapshot.seeded);
        assert!(!snapshot.exists);
        assert!(!snapshot.indexing);
        assert!(!snapshot.scraping);
        assert_eq!(snapshot.current_step, 0);
        assert_eq!(snapshot.total_steps, 0);
        assert_eq!(snapshot.current_step_display, QString::default());
    }
}
