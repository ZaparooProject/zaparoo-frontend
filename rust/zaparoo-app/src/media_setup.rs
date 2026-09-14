// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The media-job setup forms: what the Update media database and Update
//! metadata modals ask before they start, the scope vocabulary they
//! offer, and how a scope resolves to the systems Core is given. Ported
//! from `IndexSetupModal.qml` and `ScrapeSetupModal.qml`.

/// Which job the form starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Index,
    Scrape,
}

/// One form row. The ids double as the label vocabulary keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRow {
    /// Which scraper runs (Scrape only).
    Source,
    /// Which systems the job covers.
    Systems,
    /// Replace metadata that is already there (Scrape only).
    Rescrape,
    /// Start the job.
    Start,
}

impl FormRow {
    pub fn id(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Systems => "systems",
            Self::Rescrape => "rescrape",
            Self::Start => "start",
        }
    }

    /// Which control the row carries, in the settings vocabulary.
    pub fn control(self) -> crate::settings::Control {
        match self {
            Self::Source | Self::Systems => crate::settings::Control::Picker,
            Self::Rescrape => crate::settings::Control::Toggle,
            Self::Start => crate::settings::Control::Action,
        }
    }
}

/// The rows of a form, in order.
pub fn rows(kind: Kind) -> &'static [FormRow] {
    match kind {
        Kind::Index => &[FormRow::Systems, FormRow::Start],
        Kind::Scrape => &[
            FormRow::Source,
            FormRow::Systems,
            FormRow::Rescrape,
            FormRow::Start,
        ],
    }
}

/// The label the confirm button shows for the focused row
/// (`focusedActionLabel`), as a vocabulary key.
pub fn action_label_key(row: Option<FormRow>, on_picker_page: bool) -> &'static str {
    if on_picker_page {
        return "select";
    }
    match row {
        Some(FormRow::Start) => "start",
        Some(FormRow::Rescrape) => "toggle",
        _ => "change",
    }
}

/// The form clamps at its ends rather than wrapping.
pub fn move_index(index: usize, len: usize, delta: i64) -> usize {
    if len == 0 {
        return 0;
    }
    let last = (len - 1) as i64;
    (index as i64 + delta).clamp(0, last) as usize
}

/// A scope the job can run over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every system Core knows.
    All,
    /// Every system in one category.
    Category(String),
    /// One system.
    System(String),
}

/// Parse the scope token the form carries (`"*"`, `"cat:<id>"`, an id).
pub fn parse_scope(value: &str) -> Scope {
    if value.is_empty() || value == "*" {
        Scope::All
    } else if let Some(category) = value.strip_prefix("cat:") {
        Scope::Category(category.to_string())
    } else {
        Scope::System(value.to_string())
    }
}

/// The token for a scope.
pub fn scope_token(scope: &Scope) -> String {
    match scope {
        Scope::All => "*".to_string(),
        Scope::Category(id) => format!("cat:{id}"),
        Scope::System(id) => id.clone(),
    }
}

/// The systems Core is given for a scope; All answers an empty list,
/// which Core reads as "everything".
pub fn resolved_systems(
    scope: &Scope,
    category_systems: &dyn Fn(&str) -> Vec<String>,
) -> Vec<String> {
    match scope {
        Scope::All => Vec::new(),
        Scope::Category(category) => category_systems(category),
        Scope::System(id) => vec![id.clone()],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    All,
    Category,
    System,
}

/// One row of the scope picker: its token, plus how the view should
/// label it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeEntry {
    pub token: String,
    /// The view translates the scope separately from its display name.
    pub kind: ScopeKind,
    /// The category id or the system's display name; empty for All.
    pub name: String,
}

/// The scope list the picker page offers: All systems, then every
/// category that has indexable systems, then every system by name.
pub fn scope_entries(categories: &[String], systems: &[(String, String)]) -> Vec<ScopeEntry> {
    let mut entries = vec![ScopeEntry {
        token: "*".to_string(),
        kind: ScopeKind::All,
        name: String::new(),
    }];
    entries.extend(categories.iter().map(|category| ScopeEntry {
        token: format!("cat:{category}"),
        kind: ScopeKind::Category,
        name: category.clone(),
    }));
    entries.extend(systems.iter().map(|(id, name)| ScopeEntry {
        token: id.clone(),
        kind: ScopeKind::System,
        name: name.clone(),
    }));
    entries
}

/// The label for the scope a form currently holds, as the same
/// (kind, name) pair the picker rows carry.
pub fn scope_label(scope: &Scope, system_name: &dyn Fn(&str) -> String) -> (ScopeKind, String) {
    match scope {
        Scope::All => (ScopeKind::All, String::new()),
        Scope::Category(id) => (ScopeKind::Category, id.clone()),
        Scope::System(id) => (ScopeKind::System, system_name(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_form_has_its_own_rows() {
        assert_eq!(rows(Kind::Index), &[FormRow::Systems, FormRow::Start]);
        assert_eq!(
            rows(Kind::Scrape),
            &[
                FormRow::Source,
                FormRow::Systems,
                FormRow::Rescrape,
                FormRow::Start
            ]
        );
        assert_eq!(FormRow::Systems.control(), crate::settings::Control::Picker);
        assert_eq!(
            FormRow::Rescrape.control(),
            crate::settings::Control::Toggle
        );
        assert_eq!(FormRow::Start.control(), crate::settings::Control::Action);
    }

    #[test]
    fn the_confirm_label_follows_the_focused_row() {
        assert_eq!(action_label_key(Some(FormRow::Start), false), "start");
        assert_eq!(action_label_key(Some(FormRow::Rescrape), false), "toggle");
        assert_eq!(action_label_key(Some(FormRow::Systems), false), "change");
        assert_eq!(action_label_key(Some(FormRow::Source), false), "change");
        // The picker page always confirms with Select.
        assert_eq!(action_label_key(Some(FormRow::Start), true), "select");
        assert_eq!(action_label_key(None, false), "change");
    }

    #[test]
    fn the_form_cursor_clamps_at_both_ends() {
        assert_eq!(move_index(0, 4, -1), 0);
        assert_eq!(move_index(0, 4, 1), 1);
        assert_eq!(move_index(3, 4, 1), 3);
        assert_eq!(move_index(3, 4, -1), 2);
        assert_eq!(move_index(0, 0, 1), 0);
    }

    #[test]
    fn scopes_round_trip_through_their_token() {
        assert_eq!(parse_scope(""), Scope::All);
        assert_eq!(parse_scope("*"), Scope::All);
        assert_eq!(
            parse_scope("cat:Console"),
            Scope::Category("Console".into())
        );
        assert_eq!(parse_scope("NES"), Scope::System("NES".into()));
        assert_eq!(scope_token(&Scope::All), "*");
        assert_eq!(
            scope_token(&Scope::Category("Console".into())),
            "cat:Console"
        );
        assert_eq!(scope_token(&Scope::System("NES".into())), "NES");
    }

    #[test]
    fn a_scope_resolves_to_the_systems_core_is_given() {
        let ids = |category: &str| match category {
            "Console" => vec!["NES".to_string(), "SNES".to_string()],
            _ => Vec::new(),
        };
        assert!(resolved_systems(&Scope::All, &ids).is_empty());
        assert_eq!(
            resolved_systems(&Scope::Category("Console".into()), &ids),
            vec!["NES".to_string(), "SNES".to_string()]
        );
        assert_eq!(
            resolved_systems(&Scope::System("Arcade".into()), &ids),
            vec!["Arcade".to_string()]
        );
    }

    #[test]
    fn the_scope_list_leads_with_everything_then_categories() {
        let entries = scope_entries(
            &["Console".to_string()],
            &[("NES".to_string(), "Nintendo".to_string())],
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].token, "*");
        assert_eq!(entries[0].kind, ScopeKind::All);
        assert_eq!(entries[1].token, "cat:Console");
        assert_eq!(entries[1].kind, ScopeKind::Category);
        assert_eq!(entries[1].name, "Console");
        assert_eq!(entries[2].token, "NES");
        assert_eq!(entries[2].kind, ScopeKind::System);
        assert_eq!(entries[2].name, "Nintendo");
    }

    #[test]
    fn the_form_labels_its_scope_the_way_the_picker_does() {
        let name = |id: &str| format!("{id} name");
        assert_eq!(
            scope_label(&Scope::All, &name),
            (ScopeKind::All, String::new())
        );
        assert_eq!(
            scope_label(&Scope::Category("Handheld".into()), &name),
            (ScopeKind::Category, "Handheld".to_string())
        );
        assert_eq!(
            scope_label(&Scope::System("NES".into()), &name),
            (ScopeKind::System, "NES name".to_string())
        );
    }
}
