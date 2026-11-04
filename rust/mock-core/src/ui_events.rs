// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Core's UI events (`ui`, `ui.respond`, `ui.changed`): one active event at
// a time, a revision that grows on every change, and the resolution of the
// event that left. Mirrors `pkg/ui/events` in Core: opening an event
// supersedes the one showing, `ui.respond` validates the id, the action
// and the choice, and an event with an expiry times out.
//
// `run` opens them from ZapScript: `**playlist.open:deck://<id>` opens a
// picker over the deck's items (a select launches the item), and the
// mock-only `**mock.confirm`, `**mock.notice[:text]` and `**mock.loader`
// open the other kinds so a renderer can be tried against each.

use serde_json::{json, Value};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::decks;
use crate::media_state::{self, Notifier};

/// How long an event with an expiry stays open before it times out.
pub const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the mock loader runs before it completes.
pub const LOADER_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Notice,
    Loader,
    Picker,
    Confirm,
}

impl Kind {
    fn wire(self) -> &'static str {
        match self {
            Kind::Notice => "notice",
            Kind::Loader => "loader",
            Kind::Picker => "picker",
            Kind::Confirm => "confirm",
        }
    }
}

#[derive(Clone, Debug)]
struct Choice {
    id: String,
    label: String,
    media: Option<Value>,
    /// The script a select launches.
    launch: Option<String>,
}

#[derive(Clone, Debug)]
struct Event {
    id: String,
    kind: Kind,
    title: String,
    message: String,
    choices: Vec<Choice>,
    selected_choice_id: Option<String>,
    dismissible: bool,
    created_at: SystemTime,
    /// When the event times out, as a deadline and as the wall time the
    /// client is told.
    expires: Option<(Instant, SystemTime)>,
}

impl Event {
    fn json(&self) -> Value {
        let mut event = json!({
            "id": self.id,
            "kind": self.kind.wire(),
            "createdAt": rfc3339(self.created_at),
            "dismissible": self.dismissible,
        });
        if let Some((_, wall)) = self.expires {
            event["expiresAt"] = json!(rfc3339(wall));
        }
        if !self.title.is_empty() {
            event["title"] = json!(self.title);
        }
        if !self.message.is_empty() {
            event["message"] = json!(self.message);
        }
        if let Some(selected) = &self.selected_choice_id {
            event["selectedChoiceId"] = json!(selected);
        }
        if !self.choices.is_empty() {
            event["choices"] = Value::Array(
                self.choices
                    .iter()
                    .map(|choice| {
                        let mut out = json!({ "id": choice.id, "label": choice.label });
                        if let Some(media) = &choice.media {
                            out["media"] = media.clone();
                        }
                        out
                    })
                    .collect(),
            );
        }
        event
    }
}

#[derive(Debug, Default)]
struct UiStore {
    revision: u64,
    active: Option<Event>,
    next_id: u64,
}

impl UiStore {
    fn state(&self, resolved: Vec<Value>) -> Value {
        let events: Vec<Value> = self.active.iter().map(Event::json).collect();
        json!({
            "revision": self.revision,
            "events": events,
            "resolved": Value::Array(resolved),
        })
    }
}

/// Runs `f` on the UI store. The server shares one store across every
/// connection; unit tests get one per test thread.
fn with_store<R>(f: impl FnOnce(&mut UiStore) -> R) -> R {
    #[cfg(not(test))]
    {
        use std::sync::{Mutex, OnceLock, PoisonError};
        static STORE: OnceLock<Mutex<UiStore>> = OnceLock::new();
        let mut guard = STORE
            .get_or_init(|| Mutex::new(UiStore::default()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }
    #[cfg(test)]
    {
        thread_local! {
            static STORE: std::cell::RefCell<UiStore> =
                std::cell::RefCell::new(UiStore::default());
        }
        STORE.with_borrow_mut(f)
    }
}

fn resolution(id: &str, outcome: &str, choice_id: Option<&str>) -> Value {
    let mut resolved = json!({ "id": id, "outcome": outcome });
    if let Some(choice_id) = choice_id {
        resolved["choiceId"] = json!(choice_id);
    }
    resolved
}

/// `ui`: the current event, if any. `resolved` is always empty here.
pub fn state() -> Value {
    with_store(|store| store.state(Vec::new()))
}

/// What a new event shows, before it gets an id and times.
struct Request {
    kind: Kind,
    title: String,
    message: String,
    /// `(label, media, launch)` per choice, with its own id.
    choices: Vec<(String, String, Option<Value>, Option<String>)>,
    selected: usize,
    dismissible: bool,
    expires: bool,
}

/// Opens an event, superseding the one showing, and announces it. Returns
/// the new event's id.
fn open(request: Request, notifier: &Notifier) -> String {
    let now = Instant::now();
    let wall = SystemTime::now();
    let (id, state) = with_store(|store| {
        store.next_id += 1;
        let id = format!("mock-ui-{}", store.next_id);
        let choices: Vec<Choice> = request
            .choices
            .into_iter()
            .map(|(choice_id, label, media, launch)| Choice {
                id: choice_id,
                label,
                media,
                launch,
            })
            .collect();
        let selected_choice_id = choices
            .get(request.selected)
            .map(|choice| choice.id.clone());
        let event = Event {
            id: id.clone(),
            kind: request.kind,
            title: request.title,
            message: request.message,
            choices,
            selected_choice_id,
            dismissible: request.dismissible,
            created_at: wall,
            expires: request
                .expires
                .then(|| (now + EVENT_TIMEOUT, wall + EVENT_TIMEOUT)),
        };
        let resolved: Vec<Value> = store
            .active
            .take()
            .map(|old| resolution(&old.id, "superseded", None))
            .into_iter()
            .collect();
        store.active = Some(event);
        store.revision += 1;
        (id, store.state(resolved))
    });
    notifier.send("ui.changed", &state);

    if request.expires {
        let notifier = notifier.clone();
        let timed = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(EVENT_TIMEOUT).await;
            time_out(&timed, &notifier);
        });
    }
    if request.kind == Kind::Loader {
        let notifier = notifier.clone();
        let loading = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(LOADER_DURATION).await;
            complete_loader(&loading, &notifier);
        });
    }
    id
}

/// Ends the active event when it is still `id`, announcing `outcome`.
fn resolve(id: &str, outcome: &str, notifier: &Notifier) -> bool {
    let state = with_store(|store| {
        if store.active.as_ref().is_none_or(|event| event.id != id) {
            return None;
        }
        store.active = None;
        store.revision += 1;
        Some(store.state(vec![resolution(id, outcome, None)]))
    });
    match state {
        Some(state) => {
            notifier.send("ui.changed", &state);
            true
        }
        None => false,
    }
}

/// The expiry timer: times the event out when it is still showing.
pub fn time_out(id: &str, notifier: &Notifier) -> bool {
    resolve(id, "timed_out", notifier)
}

/// The mock loader's work finishing.
pub fn complete_loader(id: &str, notifier: &Notifier) -> bool {
    resolve(id, "completed", notifier)
}

/// Handles a script `run` passes along when it opens a UI event;
/// None for any other script.
pub fn run_script(text: &str, notifier: &Notifier) -> Option<Result<Value, String>> {
    let text = text.trim();
    if let Some(deck_id) = text.strip_prefix("**playlist.open:deck://") {
        return Some(open_deck_picker(deck_id.trim(), notifier));
    }
    let request = if text == "**mock.confirm" {
        Request {
            kind: Kind::Confirm,
            title: "Mock confirm".into(),
            message: "Launch the next game?".into(),
            choices: Vec::new(),
            selected: 0,
            dismissible: true,
            expires: true,
        }
    } else if text == "**mock.notice" || text.starts_with("**mock.notice:") {
        let message = text
            .strip_prefix("**mock.notice:")
            .map(str::trim)
            .filter(|message| !message.is_empty())
            .unwrap_or("Mock notice from Core");
        Request {
            kind: Kind::Notice,
            title: String::new(),
            message: message.into(),
            choices: Vec::new(),
            selected: 0,
            dismissible: true,
            expires: true,
        }
    } else if text == "**mock.loader" {
        Request {
            kind: Kind::Loader,
            title: "Mock loader".into(),
            message: "Loading".into(),
            choices: Vec::new(),
            selected: 0,
            dismissible: false,
            expires: false,
        }
    } else {
        return None;
    };
    open(request, notifier);
    Some(Ok(Value::Null))
}

fn open_deck_picker(deck_id: &str, notifier: &Notifier) -> Result<Value, String> {
    let (title, entries) = decks::picker_source(deck_id).ok_or("deck not found")?;
    if entries.is_empty() {
        return Err("playlist is empty".into());
    }
    let choices = entries
        .into_iter()
        .map(|entry| {
            let media = entry.media.map(
                |(system, path, name)| json!({ "system": system, "path": path, "name": name }),
            );
            (entry.id.to_string(), entry.label, media, entry.zapscript)
        })
        .collect();
    open(
        Request {
            kind: Kind::Picker,
            title,
            message: String::new(),
            choices,
            selected: 0,
            dismissible: true,
            expires: true,
        },
        notifier,
    );
    Ok(Value::Null)
}

/// `ui.respond`.
pub fn respond(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    respond_at(params, notifier, Instant::now())
}

/// `ui.respond` as of `now`, so tests can answer after the expiry.
fn respond_at(params: &Value, notifier: &Notifier, now: Instant) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("invalid params: id is required")?;
    let action = params.get("action").and_then(Value::as_str).unwrap_or("");
    if !matches!(action, "dismiss" | "select" | "confirm") {
        return Err("invalid params: action must be one of dismiss, select, confirm".into());
    }
    let choice_id = params.get("choiceId").and_then(Value::as_str).unwrap_or("");

    let failed = |reason: &str| format!("UI response failed: {reason}");
    let outcome = with_store(|store| -> Result<(Value, Option<String>), String> {
        let Some(event) = store.active.as_ref() else {
            return Err(failed("no active UI event"));
        };
        if event.id != id {
            return Err(failed("UI event is not active"));
        }
        if event.expires.is_some_and(|(deadline, _)| now >= deadline) {
            store.active = None;
            store.revision += 1;
            return Ok((store.state(vec![resolution(id, "timed_out", None)]), None));
        }
        let (outcome, chosen, launch) = match action {
            "dismiss" if !event.dismissible => {
                return Err(failed("UI event is not dismissible"));
            }
            "dismiss" => ("dismissed", None, None),
            "confirm" if event.kind != Kind::Confirm => {
                return Err(failed("invalid UI response action"));
            }
            "confirm" => ("confirmed", None, None),
            _ if event.kind != Kind::Picker => {
                return Err(failed("invalid UI response action"));
            }
            _ if choice_id.is_empty() => return Err(failed("choice ID is required")),
            _ => {
                let choice = event
                    .choices
                    .iter()
                    .find(|choice| choice.id == choice_id)
                    .ok_or_else(|| failed("UI choice was not found"))?;
                ("selected", Some(choice_id), choice.launch.clone())
            }
        };
        store.active = None;
        store.revision += 1;
        Ok((store.state(vec![resolution(id, outcome, chosen)]), launch))
    })?;

    let (state, launch) = outcome;
    notifier.send("ui.changed", &state);
    let expired = state["resolved"][0]["outcome"] == "timed_out";
    if expired {
        return Err(failed("UI event has expired"));
    }
    if let Some(zap_script) = launch {
        tracing::info!(%zap_script, "picker launch");
        media_state::start_launch(&zap_script, notifier);
    }
    Ok(Value::Null)
}

/// RFC 3339 UTC time with whole seconds, as Go formats `time.Time`.
fn rfc3339(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail fast on malformed output")]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_formats_known_instants() {
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        let leap = UNIX_EPOCH + Duration::from_hours(264_384);
        assert_eq!(rfc3339(leap), "2000-02-29T00:00:00Z");
        let later = UNIX_EPOCH + Duration::from_secs(1_793_000_000 + 3_661);
        assert_eq!(rfc3339(later), "2026-10-26T08:34:21Z");
    }

    #[tokio::test]
    async fn an_answer_after_the_expiry_times_the_event_out() {
        let (notifier, mut rx) = Notifier::channel();
        run_script("**mock.confirm", &notifier)
            .expect("handled")
            .expect("opened");
        let opened = rx.try_recv().expect("ui.changed");
        let id = opened["params"]["events"][0]["id"]
            .as_str()
            .expect("id")
            .to_string();
        assert!(opened["params"]["events"][0]["expiresAt"].is_string());

        let later = Instant::now() + EVENT_TIMEOUT + Duration::from_secs(1);
        let err = respond_at(&json!({ "id": id, "action": "confirm" }), &notifier, later)
            .expect_err("expired");
        assert_eq!(err, "UI response failed: UI event has expired");
        let changed = rx.try_recv().expect("ui.changed");
        assert_eq!(changed["params"]["resolved"][0]["outcome"], "timed_out");
        assert!(changed["params"]["events"]
            .as_array()
            .expect("events")
            .is_empty());
        assert!(state()["events"].as_array().expect("events").is_empty());
    }
}
