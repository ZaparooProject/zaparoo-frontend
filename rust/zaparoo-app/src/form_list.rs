// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Cursor rules for a form list: rows under section headers, where a header
//! takes no focus and a sideways press jumps a section at a time.

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Something to pick.
    Option,
    /// A section's name; never focused.
    Header,
    /// An action set apart at the foot of the list.
    Action,
}

impl Role {
    fn takes_focus(self) -> bool {
        self != Role::Header
    }
}

/// The first row that takes focus, or 0 when none does.
pub fn first(roles: &[Role]) -> usize {
    roles.iter().position(|r| r.takes_focus()).unwrap_or(0)
}

/// One row up or down, wrapping at the ends and passing over headers.
pub fn step(roles: &[Role], index: usize, forward: bool) -> usize {
    let len = roles.len();
    if len == 0 {
        return 0;
    }
    let mut next = index.min(len - 1);
    for _ in 0..len {
        next = if forward {
            (next + 1) % len
        } else {
            (next + len - 1) % len
        };
        if roles[next].takes_focus() {
            return next;
        }
    }
    index.min(len - 1)
}

/// Where each section starts: the first row, and the first focusable row
/// under every header. Rows before the first header are a section too.
fn section_starts(roles: &[Role]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut open = true;
    for (index, role) in roles.iter().enumerate() {
        if *role == Role::Header {
            open = true;
        } else if open {
            starts.push(index);
            open = false;
        }
    }
    starts
}

/// Jump a section. Forward lands on the next section's first row and wraps
/// to the first; back lands on the start of this section, or of the one
/// before when already there, and wraps to the last.
pub fn jump(roles: &[Role], index: usize, forward: bool) -> usize {
    let starts = section_starts(roles);
    let (Some(&first), Some(&last)) = (starts.first(), starts.last()) else {
        return index;
    };
    if forward {
        starts.iter().copied().find(|&s| s > index).unwrap_or(first)
    } else {
        starts
            .iter()
            .copied()
            .rev()
            .find(|&s| s < index)
            .unwrap_or(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::{Action, Header, Option as Opt};

    #[test]
    fn stepping_wraps_and_passes_over_headers() {
        let roles = [Opt, Header, Opt, Opt, Header, Opt, Action];
        assert_eq!(first(&roles), 0);
        assert_eq!(first(&[Header, Opt]), 1);
        assert_eq!(step(&roles, 0, true), 2);
        assert_eq!(step(&roles, 3, true), 5);
        assert_eq!(step(&roles, 5, true), 6);
        assert_eq!(step(&roles, 6, true), 0);
        assert_eq!(step(&roles, 0, false), 6);
        assert_eq!(step(&roles, 5, false), 3);
        assert_eq!(step(&roles, 2, false), 0);
        // Nothing to land on, or nothing at all, leaves the cursor alone.
        assert_eq!(step(&[Header, Header], 1, true), 1);
        assert_eq!(step(&[], 4, true), 0);
        assert_eq!(step(&[Opt], 9, false), 0);
    }

    #[test]
    fn jumping_lands_on_section_starts_and_wraps() {
        //            0    1       2    3    4       5    6
        let roles = [Opt, Header, Opt, Opt, Header, Opt, Opt];
        assert_eq!(jump(&roles, 0, true), 2);
        assert_eq!(jump(&roles, 2, true), 5);
        assert_eq!(jump(&roles, 3, true), 5);
        assert_eq!(jump(&roles, 6, true), 0);
        // Back goes to the start of this section first, then the one before.
        assert_eq!(jump(&roles, 6, false), 5);
        assert_eq!(jump(&roles, 5, false), 2);
        assert_eq!(jump(&roles, 2, false), 0);
        assert_eq!(jump(&roles, 0, false), 5);
        // A list with no headers is one section.
        assert_eq!(jump(&[Opt, Opt, Opt], 2, true), 0);
        assert_eq!(jump(&[Opt, Opt, Opt], 2, false), 0);
        assert_eq!(jump(&[], 3, true), 3);
    }
}
