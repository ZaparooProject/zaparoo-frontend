// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use cxx_qt::{Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::{oneshot, Notify};

#[derive(Clone)]
struct ActionErrorEvent {
    sequence: i32,
    kind: String,
    context: String,
}

#[derive(Default)]
pub struct ActionErrorRust {
    sequence: i32,
    kind: QString,
    context: QString,
    event_sequences: QStringList,
    event_kinds: QStringList,
    event_contexts: QStringList,
}

#[derive(Default)]
struct ActionErrorBus {
    queue: Mutex<VecDeque<ActionErrorEvent>>,
    notify: Notify,
}

static NEXT_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static EVENTS: OnceLock<ActionErrorBus> = OnceLock::new();

fn event_bus() -> &'static ActionErrorBus {
    EVENTS.get_or_init(ActionErrorBus::default)
}

/// Publish a user-action failure after its full technical detail has been
/// logged at the call site. QML maps the stable kind to localized, user-safe
/// copy and uses sequence as an event edge even when consecutive kinds match.
pub fn report_action_error(kind: &str, context: impl Into<String>) {
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::SeqCst).wrapping_add(1) as i32;
    let bus = event_bus();
    bus.queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push_back(ActionErrorEvent {
            sequence,
            kind: kind.to_owned(),
            context: context.into(),
        });
    bus.notify.notify_one();
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
        #[qproperty(QString, context)]
        #[qproperty(QStringList, event_sequences)]
        #[qproperty(QStringList, event_kinds)]
        #[qproperty(QStringList, event_contexts)]
        type ActionError = super::ActionErrorRust;
    }

    impl cxx_qt::Threading for ActionError {}
    impl cxx_qt::Initialize for ActionError {}
}

impl Initialize for ffi::ActionError {
    fn initialize(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        crate::models::global_handle().spawn(async move {
            let bus = event_bus();
            loop {
                bus.notify.notified().await;

                // Drain on the Qt callback, not before queueing it. Every report
                // that lands before this callback runs joins the same retained
                // batch, so coalesced wakeups cannot drop an event.
                let (applied_tx, applied_rx) = oneshot::channel();
                if qt_thread
                    .queue(move |model| {
                        let events = bus
                            .queue
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .drain(..)
                            .collect::<Vec<_>>();
                        apply_events(model, &events);
                        let _ = applied_tx.send(());
                    })
                    .is_err()
                {
                    return;
                }
                if applied_rx.await.is_err() {
                    return;
                }
            }
        });
    }
}

fn apply_events(mut model: Pin<&mut ffi::ActionError>, events: &[ActionErrorEvent]) {
    let Some(last) = events.last() else {
        return;
    };
    let mut sequences = QStringList::default();
    let mut kinds = QStringList::default();
    let mut contexts = QStringList::default();
    for event in events {
        sequences.append(QString::from(event.sequence.to_string().as_str()));
        kinds.append(QString::from(event.kind.as_str()));
        contexts.append(QString::from(event.context.as_str()));
    }
    model.as_mut().set_kind(QString::from(last.kind.as_str()));
    model
        .as_mut()
        .set_context(QString::from(last.context.as_str()));
    model.as_mut().set_event_kinds(kinds);
    model.as_mut().set_event_contexts(contexts);
    model.as_mut().set_event_sequences(sequences);
    // Set sequence last: its scalar NOTIFY edge is the batch-ready signal.
    model.as_mut().set_sequence(last.sequence);
}
