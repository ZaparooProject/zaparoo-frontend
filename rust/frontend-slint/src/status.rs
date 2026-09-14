// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Driver for the header status line: feeds `zaparoo_app::status_line`'s
// ladder from the Core link, the media-status resource and classified
// notifications, and pushes the resolved message into the `Status`
// global. Every mutation happens on the UI thread; the dwell timers are
// tokio sleeps that wake the ladder when a held message expires.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, SharedString};
use zaparoo_app::status_line::{Event, Ladder, Link, LinkInput, Output, TaskInput};
use zaparoo_core::client::{ConnectionState, Notification};
use zaparoo_core::store::MediaStatusState;

use crate::{App, Status};

#[derive(Debug)]
pub struct Driver {
    ladder: Ladder,
    link: LinkInput,
    task: TaskInput,
    /// Language tag for count formatting in the terminal messages.
    language: String,
    /// Bumped on every push so a stale dwell timer re-pushes nothing.
    generation: u64,
}

pub type Shared = Arc<Mutex<Driver>>;

pub fn new(language: &str) -> Shared {
    Arc::new(Mutex::new(Driver {
        ladder: Ladder::new(),
        link: LinkInput::default(),
        task: TaskInput::default(),
        language: language.to_string(),
        generation: 0,
    }))
}

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Driver> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn link_of(state: &ConnectionState) -> Link {
    match state {
        ConnectionState::Disconnected => Link::Disconnected,
        ConnectionState::Connecting => Link::Connecting,
        ConnectionState::Connected => Link::Connected,
        ConnectionState::Reconnecting => Link::Reconnecting,
        ConnectionState::Unreachable(_) => Link::Unreachable,
    }
}

pub fn task_of(s: &MediaStatusState) -> TaskInput {
    TaskInput {
        indexing: s.indexing,
        optimizing: s.optimizing,
        scraping: s.scraping,
        paused: s.paused,
        scrape_paused: s.scrape_paused,
        current_step: s.current_step,
        total_steps: s.total_steps,
        current_step_display: s.current_step_display.clone(),
        scrape_current_step: s.scrape_current_step,
        scrape_total_steps: s.scrape_total_steps,
        scrape_current_step_display: s.scrape_current_step_display.clone(),
        total_files: s.total_files,
        scrape_state: s.scrape_state.clone(),
        scrape_error: s.scrape_error.clone(),
        scrape_matched: s.scrape_matched,
        scrape_total: s.scrape_total,
    }
}

/// The Qt `status_events.rs` classifier: the notifications the line
/// surfaces, with their single plain-text argument.
pub fn classify(notification: &Notification) -> Option<Event> {
    let field = |key: &str| {
        notification
            .params
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    match notification.method.as_str() {
        "tokens.added" => Some(Event::TokenScanned(
            field("text")
                .filter(|s| !s.is_empty())
                .or_else(|| field("uid"))
                .unwrap_or_default(),
        )),
        "playtime.limit.warning" => Some(Event::PlaytimeWarning(
            field("remaining").unwrap_or_default(),
        )),
        "inbox.added" => Some(Event::InboxMessage(field("title").unwrap_or_default())),
        _ => None,
    }
}

/// Turn the task tiers on (the Qt build waits for the first frame; the
/// Slint build enables them as soon as the media status is seeded).
pub fn enable_media_activity(shared: &Shared, app: &App, handle: &tokio::runtime::Handle) {
    {
        let mut d = lock(shared);
        let task = d.task.clone();
        d.ladder.enable_media_activity(&task);
    }
    push(shared, app, handle);
}

pub fn set_language(shared: &Shared, language: &str) {
    lock(shared).language = language.to_string();
}

pub fn set_link(
    shared: &Shared,
    app: &App,
    handle: &tokio::runtime::Handle,
    link: Link,
    last_error: Option<&str>,
) {
    {
        let mut d = lock(shared);
        d.link.link = link;
        if let Some(error) = last_error {
            d.link.last_error = error.to_string();
        }
    }
    push(shared, app, handle);
}

pub fn set_catalog_error(
    shared: &Shared,
    app: &App,
    handle: &tokio::runtime::Handle,
    error: Option<&str>,
) {
    {
        let mut d = lock(shared);
        d.link.catalog_error = error.is_some();
        d.link.last_error = error.unwrap_or_default().to_string();
    }
    push(shared, app, handle);
}

pub fn set_task(shared: &Shared, app: &App, handle: &tokio::runtime::Handle, task: TaskInput) {
    {
        let mut d = lock(shared);
        let language = d.language.clone();
        let count = move |n: i32| zaparoo_app::format::count(i64::from(n), &language);
        d.ladder.observe_task(&task, Instant::now(), &count);
        d.task = task;
    }
    push(shared, app, handle);
}

pub fn observe_event(shared: &Shared, app: &App, handle: &tokio::runtime::Handle, event: &Event) {
    {
        let mut d = lock(shared);
        let (link, task) = (d.link.clone(), d.task.clone());
        d.ladder.observe_event(&link, &task, event, Instant::now());
    }
    push(shared, app, handle);
}

/// Render the ladder into the `Status` global and arm the next dwell
/// wake-up, if any.
pub fn push(shared: &Shared, app: &App, handle: &tokio::runtime::Handle) {
    let (output, deadline, generation) = {
        let mut d = lock(shared);
        d.generation += 1;
        let now = Instant::now();
        (
            d.ladder.render(&d.link, &d.task, now),
            d.ladder.next_deadline(),
            d.generation,
        )
    };
    apply(app, &output);
    if let Some(deadline) = deadline {
        let delay = deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1);
        let weak = app.as_weak();
        let shared = shared.clone();
        let handle_clone = handle.clone();
        handle.spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = weak.upgrade_in_event_loop(move |app| {
                if lock(&shared).generation == generation {
                    push(&shared, &app, &handle_clone);
                }
            });
        });
    }
}

fn apply(app: &App, output: &Output) {
    let status = app.global::<Status>();
    status.set_kind(output.message.kind.into());
    status.set_arg(SharedString::from(output.message.arg.as_str()));
    status.set_arg2(SharedString::from(output.message.arg2.as_str()));
    status.set_is_error(output.is_error);
    status.set_show_track(output.show_track);
    status.set_paused(output.paused);
    status.set_total_known(output.total_known);
    status.set_current_step(output.current_step);
    status.set_total_steps(output.total_steps);
    status.set_percent(output.percent.unwrap_or(-1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_matches_the_qt_model() {
        let n = |method: &str, params: serde_json::Value| Notification {
            method: method.into(),
            params,
        };
        assert_eq!(
            classify(&n("tokens.added", json!({"uid": "04a1", "text": "Mario"}))),
            Some(Event::TokenScanned("Mario".into()))
        );
        assert_eq!(
            classify(&n("tokens.added", json!({"uid": "04a1", "text": ""}))),
            Some(Event::TokenScanned("04a1".into()))
        );
        assert_eq!(
            classify(&n("playtime.limit.warning", json!({"remaining": "4m58s"}))),
            Some(Event::PlaytimeWarning("4m58s".into()))
        );
        assert_eq!(
            classify(&n("inbox.added", json!({"title": "Update available"}))),
            Some(Event::InboxMessage("Update available".into()))
        );
        assert_eq!(classify(&n("media.indexing", json!({}))), None);
    }
}
