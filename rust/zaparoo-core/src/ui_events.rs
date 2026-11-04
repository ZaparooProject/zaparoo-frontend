// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Core's transient UI events (`ui`, `ui.changed`, `ui.respond`): notices,
// loaders, pickers and confirmations that any connected client may show.
// On MiSTer, while Main reports the frontend active, Core shows none of
// them itself, so this frontend is the only renderer.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiEventKind {
    #[default]
    Notice,
    Loader,
    Picker,
    Confirm,
    /// A kind this build does not know; never shown.
    #[serde(other)]
    Unknown,
}

/// The local game a picker choice launches, when it is one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct UiChoiceMedia {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct UiChoice {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub media: Option<UiChoiceMedia>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: UiEventKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub choices: Vec<UiChoice>,
    #[serde(default)]
    pub selected_choice_id: String,
    #[serde(default)]
    pub dismissible: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiResolution {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub outcome: String,
}

/// The `ui` result and the `ui.changed` params: the whole current state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct UiState {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub events: Vec<UiEvent>,
    #[serde(default)]
    pub resolved: Vec<UiResolution>,
}

impl UiState {
    /// The event to show. Core keeps at most one today.
    #[must_use]
    pub fn current(&self) -> Option<&UiEvent> {
        self.events
            .iter()
            .find(|event| event.kind != UiEventKind::Unknown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiAction {
    Dismiss,
    Select,
    Confirm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiRespondParams {
    pub id: String,
    pub action: UiAction,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub choice_id: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_picker_with_game_choices() {
        let state: UiState = serde_json::from_value(json!({
            "revision": 12,
            "events": [{"id": "e1", "kind": "picker", "title": "Couch co-op",
                "choices": [{"id": "c1", "label": "Mario",
                             "media": {"system": "SNES", "path": "/m.sfc", "name": "Mario"}},
                            {"id": "c2", "label": "Script"}],
                "selectedChoiceId": "c1", "dismissible": true,
                "createdAt": "2026-07-16T12:00:00Z"}],
            "resolved": []
        }))
        .unwrap();
        let event = state.current().unwrap();
        assert_eq!(event.kind, UiEventKind::Picker);
        assert_eq!(event.choices[0].media.as_ref().unwrap().system, "SNES");
        assert!(event.choices[1].media.is_none());
    }

    #[test]
    fn unknown_kinds_are_never_current() {
        let state: UiState = serde_json::from_value(json!({
            "revision": 1, "events": [{"id": "e", "kind": "survey", "dismissible": false}],
            "resolved": []
        }))
        .unwrap();
        assert!(state.current().is_none());
    }

    #[test]
    fn respond_omits_choice_unless_selecting() {
        let dismiss = UiRespondParams {
            id: "e".into(),
            action: UiAction::Dismiss,
            choice_id: String::new(),
        };
        assert_eq!(
            serde_json::to_value(dismiss).unwrap(),
            json!({"id": "e", "action": "dismiss"})
        );
    }
}
