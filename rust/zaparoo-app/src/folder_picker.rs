// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Host-owned folder permission flow. No paths or storage-provider details enter UI rules.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Ready,
    Opening,
    Saved,
    Cancelled,
    Denied,
    Failed,
    Revoked,
}

#[derive(Debug, Default)]
pub struct Model {
    pub state: State,
    pub saved: u32,
    revision: u64,
}

impl Model {
    pub fn begin(&mut self) -> bool {
        if self.state == State::Opening {
            return false;
        }
        self.state = State::Opening;
        true
    }

    /// Host snapshots are ordered within one window generation.
    pub fn update(&mut self, revision: u64, state: State, saved: u32) -> bool {
        if revision <= self.revision {
            return false;
        }
        self.revision = revision;
        self.state = if state == State::Ready && saved < self.saved {
            State::Revoked
        } else {
            state
        };
        self.saved = saved;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoked_grants_become_an_actionable_state() {
        let mut model = Model::default();
        assert!(model.update(1, State::Ready, 2));
        assert!(model.update(2, State::Ready, 1));
        assert_eq!(model.state, State::Revoked);
        assert_eq!(model.saved, 1);
        assert!(model.update(3, State::Ready, 1));
        assert_eq!(model.state, State::Ready);
    }

    #[test]
    fn overlapping_requests_and_stale_snapshots_are_rejected() {
        let mut model = Model::default();
        assert!(model.begin());
        assert!(!model.begin());
        assert!(model.update(2, State::Saved, 1));
        assert!(!model.update(1, State::Ready, 0));
        assert!(!model.update(2, State::Failed, 0));
        assert_eq!(model.state, State::Saved);
        assert_eq!(model.saved, 1);
        assert!(model.begin());
        assert!(model.update(3, State::Cancelled, 1));
        assert_eq!(model.saved, 1);
        assert!(model.begin());
        assert!(model.update(4, State::Denied, 0));
        assert_eq!(model.saved, 0);
    }
}
