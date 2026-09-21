// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Optional host-owned scan for installed launchers. Framework identifiers
//! and document URIs stay in the embedding host; UI receives only bounded counts.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Ready,
    Opening,
    Detected,
    Cancelled,
    Denied,
    Failed,
    Unavailable,
}

#[derive(Debug, Default)]
pub struct Model {
    pub state: State,
    pub detected: u16,
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

    pub fn update(&mut self, revision: u64, state: State, detected: u16) -> bool {
        if revision <= self.revision {
            return false;
        }
        self.revision = revision;
        self.state = state;
        self.detected = detected;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_rejects_overlap_and_stale_snapshots() {
        let mut model = Model::default();
        assert!(model.begin());
        assert!(!model.begin());
        assert!(model.update(2, State::Detected, 7));
        assert!(!model.update(1, State::Denied, 0));
        assert!(!model.update(2, State::Failed, 0));
        assert_eq!(model.detected, 7);
        assert!(model.begin());
        assert!(model.update(3, State::Cancelled, 7));
        assert_eq!(model.detected, 7);
    }
}
