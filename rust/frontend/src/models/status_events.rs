// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.StatusEvents` — transient, non-task notifications the header
// status line can surface for a few seconds: a card scan, a playtime
// warning, an inbox message. These are genuinely ephemeral (Core retains
// the underlying record; the frontend just echoes that something
// happened while the UI was open), so unlike `MediaStatusResource` this
// has no seed step and no persisted state — it is a pure notification
// tap.
//
// Shaped like `action_error.rs`'s batched-event contract (scalar
// "latest" fields plus parallel `QStringList` history, sequence set
// last as the NOTIFY edge) but simpler to produce: `ActionError` needs a
// thread-safe `Mutex<VecDeque>` bus because `report_action_error` can be
// called from arbitrary call sites/threads. Here there is exactly one
// producer — the notification-fold loop below — so the rolling history
// is plain task-local state with no synchronization required.
//
// Exposes structured fields (`kind` + `detail`), not prose: QML owns the
// `qsTr()` templates so translators see real sentences instead of
// assembled fragments.

use cxx_qt::{Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use serde_json::Value;
use std::collections::VecDeque;
use std::pin::Pin;
use zaparoo_core::client::Notification;

/// Bounded so a burst of scans/warnings can't grow the parallel
/// `QStringList`s without limit; only recent history is relevant to a
/// line that shows one message at a time.
const EVENT_HISTORY_CAP: usize = 8;

#[derive(Clone)]
struct StatusEvent {
    sequence: i32,
    kind: String,
    detail: String,
}

#[derive(Default)]
pub struct StatusEventsRust {
    sequence: i32,
    kind: QString,
    detail: QString,
    event_sequences: QStringList,
    event_kinds: QStringList,
    event_details: QStringList,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, sequence)]
        #[qproperty(QString, kind)]
        #[qproperty(QString, detail)]
        #[qproperty(QStringList, event_sequences)]
        #[qproperty(QStringList, event_kinds)]
        #[qproperty(QStringList, event_details)]
        type StatusEvents = super::StatusEventsRust;
    }

    impl cxx_qt::Threading for StatusEvents {}
    impl cxx_qt::Initialize for StatusEvents {}
}

impl Initialize for ffi::StatusEvents {
    fn initialize(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        let mut notifications = crate::models::global_store().subscribe_notifications();
        crate::models::global_handle().spawn(async move {
            let mut next_sequence: i32 = 0;
            let mut history: VecDeque<StatusEvent> = VecDeque::new();
            loop {
                match notifications.recv().await {
                    Ok(notification) => {
                        let Some((kind, detail)) = classify(&notification) else {
                            continue;
                        };
                        next_sequence = next_sequence.wrapping_add(1);
                        history.push_back(StatusEvent {
                            sequence: next_sequence,
                            kind,
                            detail,
                        });
                        while history.len() > EVENT_HISTORY_CAP {
                            history.pop_front();
                        }
                        let events: Vec<StatusEvent> = history.iter().cloned().collect();
                        if qt_thread.queue(move |m| apply_events(m, &events)).is_err() {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }
}

/// Maps a raw Core notification to (kind, detail) for the events this
/// component surfaces, or `None` for everything else (including
/// `media.indexing`/`media.scraping`, which `MediaStatus` already owns).
/// `detail` is a single plain-text argument — QML supplies the rest of
/// the sentence via `qsTr()`.
fn classify(notification: &Notification) -> Option<(String, String)> {
    match notification.method.as_str() {
        "tokens.added" => {
            let detail = string_field(&notification.params, "text")
                .filter(|s| !s.is_empty())
                .or_else(|| string_field(&notification.params, "uid"))
                .unwrap_or_default();
            Some(("token_scanned".to_owned(), detail))
        }
        "playtime.limit.warning" => {
            let detail = string_field(&notification.params, "remaining").unwrap_or_default();
            Some(("playtime_warning".to_owned(), detail))
        }
        "inbox.added" => {
            let detail = string_field(&notification.params, "title").unwrap_or_default();
            Some(("inbox_message".to_owned(), detail))
        }
        _ => None,
    }
}

fn string_field(params: &Value, key: &str) -> Option<String> {
    params.get(key)?.as_str().map(str::to_owned)
}

fn apply_events(mut model: Pin<&mut ffi::StatusEvents>, events: &[StatusEvent]) {
    let Some(last) = events.last() else {
        return;
    };
    let mut sequences = QStringList::default();
    let mut kinds = QStringList::default();
    let mut details = QStringList::default();
    for event in events {
        sequences.append(QString::from(event.sequence.to_string().as_str()));
        kinds.append(QString::from(event.kind.as_str()));
        details.append(QString::from(event.detail.as_str()));
    }
    model.as_mut().set_kind(QString::from(last.kind.as_str()));
    model
        .as_mut()
        .set_detail(QString::from(last.detail.as_str()));
    model.as_mut().set_event_kinds(kinds);
    model.as_mut().set_event_details(details);
    model.as_mut().set_event_sequences(sequences);
    // Set sequence last: its scalar NOTIFY edge is the batch-ready signal
    // (mirrors `action_error.rs::apply_events`).
    model.as_mut().set_sequence(last.sequence);
}

#[cfg(test)]
mod tests {
    use super::classify;
    use serde_json::json;
    use zaparoo_core::client::Notification;

    #[test]
    fn classify_tokens_added_prefers_text_over_uid() {
        let (kind, detail) = classify(&Notification {
            method: "tokens.added".into(),
            params: json!({"type": "nfc", "uid": "04a1b2", "text": "Super Mario World"}),
        })
        .expect("tokens.added should classify");
        assert_eq!(kind, "token_scanned");
        assert_eq!(detail, "Super Mario World");
    }

    #[test]
    fn classify_tokens_added_falls_back_to_uid_when_text_empty() {
        let (kind, detail) = classify(&Notification {
            method: "tokens.added".into(),
            params: json!({"type": "nfc", "uid": "04a1b2", "text": ""}),
        })
        .expect("tokens.added should classify");
        assert_eq!(kind, "token_scanned");
        assert_eq!(detail, "04a1b2");
    }

    #[test]
    fn classify_playtime_warning_reports_remaining() {
        let (kind, detail) = classify(&Notification {
            method: "playtime.limit.warning".into(),
            params: json!({"interval": "5m", "remaining": "4m58s"}),
        })
        .expect("playtime.limit.warning should classify");
        assert_eq!(kind, "playtime_warning");
        assert_eq!(detail, "4m58s");
    }

    #[test]
    fn classify_inbox_added_reports_title() {
        let (kind, detail) = classify(&Notification {
            method: "inbox.added".into(),
            params: json!({"id": 1, "title": "Update available", "severity": 0}),
        })
        .expect("inbox.added should classify");
        assert_eq!(kind, "inbox_message");
        assert_eq!(detail, "Update available");
    }

    #[test]
    fn classify_ignores_unrelated_methods() {
        assert!(classify(&Notification {
            method: "media.indexing".into(),
            params: json!({}),
        })
        .is_none());
    }
}
