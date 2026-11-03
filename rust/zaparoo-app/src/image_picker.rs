// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Host-owned image picker for an embedding host. The host runs its own
//! picker and answers with a readable absolute path; any copying its
//! platform needs to make the file readable is the host's.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Ready,
    Opening,
    Picked(String),
    Cancelled,
    Denied,
    Failed,
}

#[derive(Debug, Default)]
pub struct Model {
    pub state: State,
    revision: u64,
}

impl Model {
    /// Start a request; refused while one is already open.
    pub fn begin(&mut self) -> bool {
        if self.state == State::Opening {
            return false;
        }
        self.state = State::Opening;
        true
    }

    /// A host answer, ordered by revision. Only an open request takes one,
    /// and a pick must be an absolute path.
    pub fn update(&mut self, revision: u64, state: State) -> bool {
        if revision <= self.revision || self.state != State::Opening {
            return false;
        }
        self.revision = revision;
        self.state = match state {
            State::Picked(path) if !crate::customization::is_absolute_icon(&path) => State::Failed,
            State::Opening => return false,
            other => other,
        };
        true
    }

    /// Whether a request is open.
    pub fn pending(&self) -> bool {
        self.state == State::Opening
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_at_a_time_and_stale_answers_are_dropped() {
        let mut model = Model::default();
        assert!(!model.update(1, State::Cancelled), "nothing was asked");
        assert!(model.begin());
        assert!(!model.begin());
        assert!(model.pending());
        assert!(model.update(2, State::Picked("/data/icons/a.png".into())));
        assert_eq!(model.state, State::Picked("/data/icons/a.png".into()));
        assert!(!model.update(3, State::Cancelled), "already answered");
        assert!(model.begin());
        assert!(!model.update(2, State::Denied), "stale revision");
        assert!(model.update(4, State::Denied));
        assert_eq!(model.state, State::Denied);
    }

    #[test]
    fn a_pick_that_is_not_a_path_fails() {
        let mut model = Model::default();
        assert!(model.begin());
        assert!(model.update(1, State::Picked("content://x".into())));
        assert_eq!(model.state, State::Failed);
    }
}
