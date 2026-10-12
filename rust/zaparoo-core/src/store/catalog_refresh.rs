// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Query the small systems catalog during indexing, not every cached game page.
//! A timer catches newly committed systems even when Core's progress is quiet.

use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{interval, MissedTickBehavior};

use super::MediaStatusState;

pub(super) const PERIOD: Duration = Duration::from_secs(2);

pub(super) async fn run(
    mut status: watch::Receiver<MediaStatusState>,
    period: Duration,
    mut refresh: impl FnMut() -> bool,
) {
    let mut timer = interval(period);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut was_active = false;
    loop {
        let active = {
            let current = status.borrow_and_update();
            current.indexing && !current.optimizing && !current.paused
        };
        if active && !was_active {
            timer.reset();
        }
        was_active = active;
        tokio::select! {
            result = status.changed() => {
                if result.is_err() {
                    return;
                }
            }
            _ = timer.tick(), if active => {
                if !refresh() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail fast on missing refreshes")]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::timeout;

    const TEST_PERIOD: Duration = Duration::from_millis(20);

    #[tokio::test]
    async fn polls_without_progress_notifications_and_stops_at_idle() {
        let (status, rx) = watch::channel(MediaStatusState::default());
        let (refresh, mut calls) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(rx, TEST_PERIOD, move || refresh.send(()).is_ok()));
        assert!(timeout(TEST_PERIOD * 3, calls.recv()).await.is_err());
        status.send_replace(MediaStatusState {
            indexing: true,
            ..MediaStatusState::default()
        });
        for _ in 0..2 {
            timeout(Duration::from_secs(5), calls.recv())
                .await
                .expect("periodic refresh")
                .expect("watcher alive");
        }
        status.send_replace(MediaStatusState::default());
        tokio::time::sleep(TEST_PERIOD).await;
        while calls.try_recv().is_ok() {}
        assert!(timeout(TEST_PERIOD * 3, calls.recv()).await.is_err());
        drop(status);
        task.await.expect("watcher stopped");
    }

    #[tokio::test]
    async fn skips_paused_optimizing_and_scraping_states_then_resumes() {
        let (status, rx) = watch::channel(MediaStatusState::default());
        let (refresh, mut calls) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(rx, TEST_PERIOD, move || refresh.send(()).is_ok()));
        for state in [
            MediaStatusState {
                indexing: true,
                paused: true,
                ..MediaStatusState::default()
            },
            MediaStatusState {
                indexing: true,
                optimizing: true,
                ..MediaStatusState::default()
            },
            MediaStatusState {
                scraping: true,
                ..MediaStatusState::default()
            },
        ] {
            status.send_replace(state);
            assert!(timeout(TEST_PERIOD * 3, calls.recv()).await.is_err());
        }
        status.send_replace(MediaStatusState {
            indexing: true,
            ..MediaStatusState::default()
        });
        timeout(Duration::from_secs(5), calls.recv())
            .await
            .expect("resumed refresh")
            .expect("watcher alive");
        drop(status);
        task.await.expect("watcher stopped");
    }

    #[tokio::test]
    async fn owner_disappearing_stops_active_polling() {
        let (_status, rx) = watch::channel(MediaStatusState {
            indexing: true,
            ..MediaStatusState::default()
        });
        timeout(Duration::from_secs(5), run(rx, TEST_PERIOD, || false))
            .await
            .expect("owner gone");
    }
}
