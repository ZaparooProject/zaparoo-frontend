// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Scripted `media.generate` / `media.scrape` sequences plus the `media`
// and `media.scrape.status` seed responses. Real Core drives indexing
// and scraper progress from actual filesystem/network work; the mock
// drives the same notification shapes from a timer so the redesigned
// header status line has something to render in `just run-dev` without
// a real library or network access.
//
// One global `Mutex<MediaState>` is enough here — mock-core is a
// single-process dev tool and in practice serves one frontend
// connection at a time. A `generation` counter per sequence (index /
// scrape) is the cancellation mechanism: starting or cancelling a
// sequence bumps its counter, and the running background task checks
// its own captured generation before every send, exiting quietly the
// moment it no longer matches. This deliberately does not replicate
// Core's reject-if-already-running behavior for `media.generate`/
// `media.scrape` restart — simplicity for a dev tool over fidelity to
// that particular error path.

use crate::fixtures::MOCK_SYSTEMS;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;

/// Per-system file count used for the illustrative `totalFiles` /
/// per-system scrape `total` counters. Not tied to `fixtures::ALL_GAMES`'s
/// real per-system counts — this module only needs plausible, steadily
/// growing numbers for the status line to render, not exact fidelity.
const FILES_PER_SYSTEM: i32 = 6;
const INDEX_STEP_DELAY: Duration = Duration::from_millis(500);
const INDEX_PAUSE_DELAY: Duration = Duration::from_millis(1500);
const OPTIMIZE_DELAY: Duration = Duration::from_millis(1200);
const SCRAPE_SUBSTEP_DELAY: Duration = Duration::from_millis(400);

/// Handle for pushing a JSON-RPC notification (no `id`) back over the
/// socket that requested a sequence. Cloned into every spawned
/// background task; the receiving end lives in `main.rs::serve`'s
/// select loop.
#[derive(Clone)]
pub struct Notifier {
    sender: UnboundedSender<Value>,
}

impl Notifier {
    pub fn new(sender: UnboundedSender<Value>) -> Self {
        Self { sender }
    }

    /// A `Notifier` with no live receiver — every `send` is a silent
    /// no-op. Used by `handler`'s unit tests, which exercise `dispatch`
    /// directly and have no socket-pump task to receive pushed
    /// notifications.
    #[cfg(test)]
    pub fn noop() -> Self {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        Self { sender }
    }

    pub(crate) fn send(&self, method: &str, params: &Value) {
        let payload = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        // Send failure just means the client disconnected mid-sequence;
        // the background task keeps running its steps harmlessly until
        // it finishes (cheap, bounded) rather than needing its own
        // cancellation path for socket closure.
        let _ = self.sender.send(payload);
    }
}

#[derive(Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags mirroring MediaStatusState's own bool surface"
)]
struct MediaState {
    exists: bool,
    indexing: bool,
    optimizing: bool,
    paused: bool,
    current_step: i32,
    total_steps: i32,
    current_step_display: String,
    total_files: i32,
    total_media: i32,
    index_generation: u64,

    scraping: bool,
    scrape_done: bool,
    scrape_paused: bool,
    scrape_processed: i32,
    scrape_total: i32,
    scrape_matched: i32,
    scrape_skipped: i32,
    scrape_total_scraped: i32,
    scrape_force: bool,
    scrape_system_id: String,
    scrape_scraper_id: String,
    scrape_state: String,
    scrape_error: String,
    scrape_current_step: i32,
    scrape_total_steps: i32,
    scrape_current_step_display: String,
    scrape_current_system_id: String,
    scrape_current_system_name: String,
    scrape_generation: u64,
}

fn state() -> &'static Mutex<MediaState> {
    static STATE: OnceLock<Mutex<MediaState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(MediaState::default()))
}

fn with_state<R>(f: impl FnOnce(&mut MediaState) -> R) -> R {
    let mut guard = state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// `media` RPC response — the seed the frontend's `MediaStatusResource`
/// fetches on every connect. `active` is always empty; the mock does not
/// model a currently-running-game session.
pub fn media_response() -> Value {
    with_state(|s| {
        json!({
            "database": {
                "exists": s.exists,
                "indexing": s.indexing,
                "optimizing": s.optimizing,
                "paused": s.paused,
                "totalSteps": s.total_steps,
                "currentStep": s.current_step,
                "currentStepDisplay": s.current_step_display,
                "totalFiles": s.total_files,
                "totalMedia": s.total_media,
            },
            "active": [],
        })
    })
}

/// `media.scrape.status` RPC response — the one-shot seed for scrape
/// state, mirroring the shape of a `media.scraping` notification.
pub fn scrape_status_response() -> Value {
    with_state(|s| {
        json!({
            "scraperId": s.scrape_scraper_id,
            "systemId": s.scrape_system_id,
            "state": s.scrape_state,
            "error": s.scrape_error,
            "processed": s.scrape_processed,
            "total": s.scrape_total,
            "matched": s.scrape_matched,
            "skipped": s.scrape_skipped,
            "totalScraped": s.scrape_total_scraped,
            "force": s.scrape_force,
            "scraping": s.scraping,
            "done": s.scrape_done,
            "paused": s.scrape_paused,
            "totalSteps": s.scrape_total_steps,
            "currentStep": s.scrape_current_step,
            "currentStepDisplay": s.scrape_current_step_display,
            "currentSystem": {
                "systemId": s.scrape_current_system_id,
                "systemName": s.scrape_current_system_name,
                "processed": s.scrape_processed,
                "total": s.scrape_total,
                "matched": s.scrape_matched,
                "skipped": s.scrape_skipped,
            },
        })
    })
}

fn index_notification() -> Value {
    with_state(|s| {
        json!({
            "exists": s.exists,
            "indexing": s.indexing,
            "optimizing": s.optimizing,
            "paused": s.paused,
            "totalSteps": s.total_steps,
            "currentStep": s.current_step,
            "currentStepDisplay": s.current_step_display,
            "totalFiles": s.total_files,
            "totalMedia": s.total_media,
        })
    })
}

fn scrape_notification() -> Value {
    scrape_status_response()
}

/// Starts (or restarts) the scripted indexing sequence. Always succeeds —
/// see the module doc comment on why the mock doesn't replicate Core's
/// reject-if-already-running error path.
pub fn start_index(notifier: &Notifier) -> Value {
    let generation = with_state(|s| {
        s.index_generation += 1;
        s.indexing = true;
        s.optimizing = false;
        s.paused = false;
        s.current_step = 0;
        s.total_steps = MOCK_SYSTEMS.len() as i32;
        s.current_step_display = String::new();
        s.total_files = 0;
        s.index_generation
    });
    notifier.send("media.indexing", &index_notification());
    tokio::spawn(run_index_sequence(generation, notifier.clone()));
    Value::Null
}

pub fn cancel_index(notifier: &Notifier) -> Result<Value, String> {
    let was_indexing = with_state(|s| {
        if !s.indexing {
            return false;
        }
        s.index_generation += 1;
        s.indexing = false;
        s.optimizing = false;
        s.paused = false;
        true
    });
    if !was_indexing {
        return Err("no index build is running".to_owned());
    }
    // Push the cleared state immediately rather than waiting for the
    // (now-invalidated) sequence task's next tick, so cancel feels
    // responsive in the dev loop.
    notifier.send("media.indexing", &index_notification());
    Ok(Value::Null)
}

async fn run_index_sequence(generation: u64, notifier: Notifier) {
    for (step, (_, display_name, _)) in MOCK_SYSTEMS.iter().enumerate() {
        sleep(INDEX_STEP_DELAY).await;
        let still_current = with_state(|s| {
            if s.index_generation != generation {
                return false;
            }
            s.current_step = step as i32 + 1;
            (*display_name).clone_into(&mut s.current_step_display);
            s.total_files += FILES_PER_SYSTEM;
            true
        });
        if !still_current {
            return;
        }
        notifier.send("media.indexing", &index_notification());

        // Partway through, simulate a game starting and Core pausing the
        // build around it — see `media.indexing` docs and
        // `docs/core/blog/core-v2.11.0` on the pause-while-playing policy.
        if step == MOCK_SYSTEMS.len() / 2 {
            let still_current = with_state(|s| {
                if s.index_generation != generation {
                    return false;
                }
                s.paused = true;
                true
            });
            if !still_current {
                return;
            }
            notifier.send("media.indexing", &index_notification());
            sleep(INDEX_PAUSE_DELAY).await;
            let still_current = with_state(|s| {
                if s.index_generation != generation {
                    return false;
                }
                s.paused = false;
                true
            });
            if !still_current {
                return;
            }
            notifier.send("media.indexing", &index_notification());
        }
    }

    sleep(INDEX_STEP_DELAY).await;
    let still_current = with_state(|s| {
        if s.index_generation != generation {
            return false;
        }
        s.indexing = false;
        s.optimizing = true;
        "vacuum".clone_into(&mut s.current_step_display);
        true
    });
    if !still_current {
        return;
    }
    notifier.send("media.indexing", &index_notification());

    sleep(OPTIMIZE_DELAY).await;
    with_state(|s| {
        if s.index_generation != generation {
            return;
        }
        s.optimizing = false;
        s.exists = true;
        s.total_media = s.total_files;
        s.current_step_display = String::new();
    });
    if with_state(|s| s.index_generation == generation) {
        notifier.send("media.indexing", &index_notification());
    }
}

/// Starts the scripted scraper sequence. `force` mirrors the real
/// "rescrape existing" toggle in Settings — as a deliberate dev-loop
/// convenience, a forced run is scripted to fail partway through so the
/// failure copy has a reliable, discoverable way to be exercised: leave
/// "rescrape existing" off for the happy path, turn it on to see the
/// failure frame.
pub fn start_scrape(force: bool, notifier: &Notifier) -> Value {
    let generation = with_state(|s| {
        s.scrape_generation += 1;
        s.scraping = true;
        s.scrape_done = false;
        s.scrape_paused = false;
        s.scrape_force = force;
        "gamelist.xml".clone_into(&mut s.scrape_scraper_id);
        "running".clone_into(&mut s.scrape_state);
        s.scrape_error.clear();
        s.scrape_current_step = 0;
        s.scrape_total_steps = MOCK_SYSTEMS.len() as i32;
        s.scrape_processed = 0;
        s.scrape_total = 0;
        s.scrape_matched = 0;
        s.scrape_skipped = 0;
        s.scrape_current_step_display.clear();
        s.scrape_current_system_id.clear();
        s.scrape_current_system_name.clear();
        s.scrape_generation
    });
    notifier.send("media.scraping", &scrape_notification());
    tokio::spawn(run_scrape_sequence(generation, force, notifier.clone()));
    Value::Null
}

pub fn cancel_scrape(notifier: &Notifier) -> Result<Value, String> {
    let was_scraping = with_state(|s| {
        if !s.scraping {
            return false;
        }
        s.scrape_generation += 1;
        s.scraping = false;
        s.scrape_done = true;
        s.scrape_paused = false;
        "cancelled".clone_into(&mut s.scrape_state);
        true
    });
    if !was_scraping {
        return Err("no scraper is running".to_owned());
    }
    notifier.send("media.scraping", &scrape_notification());
    Ok(Value::Null)
}

async fn run_scrape_sequence(generation: u64, force: bool, notifier: Notifier) {
    let fail_after_step = MOCK_SYSTEMS.len() / 2;
    for (step, (system_id, system_name, _)) in MOCK_SYSTEMS.iter().enumerate() {
        for (processed, matched, skipped) in [(0, 0, 0), (3, 2, 0), (6, 4, 1)] {
            sleep(SCRAPE_SUBSTEP_DELAY).await;
            let still_current = with_state(|s| {
                if s.scrape_generation != generation {
                    return false;
                }
                s.scrape_current_step = step as i32 + 1;
                (*system_name).clone_into(&mut s.scrape_current_step_display);
                (*system_id).clone_into(&mut s.scrape_system_id);
                (*system_id).clone_into(&mut s.scrape_current_system_id);
                (*system_name).clone_into(&mut s.scrape_current_system_name);
                s.scrape_processed = processed;
                s.scrape_total = 6;
                s.scrape_matched = matched;
                s.scrape_skipped = skipped;
                true
            });
            if !still_current {
                return;
            }
            notifier.send("media.scraping", &scrape_notification());
        }
        let matched_this_system = with_state(|s| s.scrape_matched);
        with_state(|s| {
            if s.scrape_generation == generation {
                s.scrape_total_scraped += matched_this_system;
            }
        });

        if force && step == fail_after_step {
            let still_current = with_state(|s| {
                if s.scrape_generation != generation {
                    return false;
                }
                s.scraping = false;
                s.scrape_done = true;
                "failed".clone_into(&mut s.scrape_state);
                "Simulated scraper failure (rescrape existing was on) — exercises the failure UI"
                    .clone_into(&mut s.scrape_error);
                true
            });
            if still_current {
                notifier.send("media.scraping", &scrape_notification());
            }
            return;
        }
    }

    let still_current = with_state(|s| {
        if s.scrape_generation != generation {
            return false;
        }
        s.scraping = false;
        s.scrape_done = true;
        "completed".clone_into(&mut s.scrape_state);
        true
    });
    if still_current {
        notifier.send("media.scraping", &scrape_notification());
    }
}
