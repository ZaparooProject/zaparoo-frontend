// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Runtime: what device is the frontend binary running on?
//
// Independent of `platform` (which describes the Zaparoo Core server).
// See docs/architecture.md for the gating rules.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Mister,
    Desktop,
}

impl Runtime {
    pub fn is_mister(self) -> bool {
        matches!(self, Self::Mister)
    }

    pub fn is_desktop(self) -> bool {
        matches!(self, Self::Desktop)
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mister => "mister",
            Self::Desktop => "desktop",
        })
    }
}

pub fn current() -> Runtime {
    static CACHED: OnceLock<Runtime> = OnceLock::new();
    *CACHED.get_or_init(detect)
}

fn detect() -> Runtime {
    if std::path::Path::new("/media/fat").exists() {
        Runtime::Mister
    } else {
        Runtime::Desktop
    }
}

#[cfg(test)]
mod tests {
    use super::{current, Runtime};

    #[test]
    fn current_is_stable_across_calls() {
        let a = current();
        let b = current();
        assert_eq!(a, b);
    }

    #[test]
    fn helpers_are_mutually_exclusive() {
        for r in [Runtime::Mister, Runtime::Desktop] {
            assert_ne!(r.is_mister(), r.is_desktop());
        }
    }

    #[test]
    fn display_matches_expected_tokens() {
        assert_eq!(Runtime::Mister.to_string(), "mister");
        assert_eq!(Runtime::Desktop.to_string(), "desktop");
    }
}
