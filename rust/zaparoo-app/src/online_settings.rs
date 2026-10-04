// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Online settings page's own rules: the four consent toggles read as
//! one tri-state switch, and what turning that switch on or off means for
//! each of them. Core's own feature flags and Warp's own gating of cloud
//! backup stay exactly as the reference TUI (`pkg/ui/tui/online.go`)
//! defines them; this is a Rust port of that page's pure logic, not a new
//! design.

/// The four per-feature consent settings Core tracks separately, read
/// together as one tri-state row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent consent settings Core tracks separately"
)]
pub struct OnlineFeatures {
    pub remote_control: bool,
    pub play_history: bool,
    pub library: bool,
    pub cloud_backup: bool,
}

/// What the "All online features" row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriState {
    Off,
    Mixed,
    On,
}

impl OnlineFeatures {
    /// Cloud backup only counts toward the tri-state when Warp is
    /// confirmed active: an account without it reads as all-on once every
    /// other feature is, the same as `allOnlineFeaturesUpdate` never turns
    /// cloud backup on for one.
    pub fn tri_state(self, warp_available: bool) -> TriState {
        let mut on = usize::from(self.remote_control)
            + usize::from(self.play_history)
            + usize::from(self.library);
        let mut total = 3;
        if warp_available {
            total += 1;
            on += usize::from(self.cloud_backup);
        }
        match on {
            0 => TriState::Off,
            n if n == total => TriState::On,
            _ => TriState::Mixed,
        }
    }
}

/// What turning the switch on or off changes: `None` for a feature this
/// press leaves untouched. Turning off always covers all four. Turning on
/// includes cloud backup only when Warp is confirmed active, since a
/// scheduled backup without it would just fail; otherwise cloud backup is
/// left exactly as it was, for the user to turn on once Warp resolves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OnlineFeatureChanges {
    pub remote_control: Option<bool>,
    pub play_history: Option<bool>,
    pub library: Option<bool>,
    pub cloud_backup: Option<bool>,
}

/// The new feature state and the changes needed to reach it.
pub fn all_features_update(
    current: OnlineFeatures,
    on: bool,
    warp_available: bool,
) -> (OnlineFeatures, OnlineFeatureChanges) {
    let mut next = current;
    next.remote_control = on;
    next.play_history = on;
    next.library = on;
    let mut changes = OnlineFeatureChanges {
        remote_control: Some(on),
        play_history: Some(on),
        library: Some(on),
        cloud_backup: None,
    };
    if !on || warp_available {
        next.cloud_backup = on;
        changes.cloud_backup = Some(on);
    }
    (next, changes)
}

/// Undo a change Core refused. Each feature the request set goes back to
/// what it was `before`, but only while it still shows the value that
/// request put there: a later change the user made in the meantime is
/// theirs, and an overlapping request that already succeeded is Core's.
pub fn roll_back(
    current: OnlineFeatures,
    before: OnlineFeatures,
    changes: OnlineFeatureChanges,
) -> OnlineFeatures {
    let mut next = current;
    if changes.remote_control == Some(current.remote_control) {
        next.remote_control = before.remote_control;
    }
    if changes.play_history == Some(current.play_history) {
        next.play_history = before.play_history;
    }
    if changes.library == Some(current.library) {
        next.library = before.library;
    }
    if changes.cloud_backup == Some(current.cloud_backup) {
        next.cloud_backup = before.cloud_backup;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::{all_features_update, roll_back, OnlineFeatureChanges, OnlineFeatures, TriState};

    fn features(on: [bool; 4]) -> OnlineFeatures {
        OnlineFeatures {
            remote_control: on[0],
            play_history: on[1],
            library: on[2],
            cloud_backup: on[3],
        }
    }

    #[test]
    fn every_feature_off_reads_off_regardless_of_warp() {
        assert_eq!(
            features([false, false, false, false]).tri_state(true),
            TriState::Off
        );
        assert_eq!(
            features([false, false, false, false]).tri_state(false),
            TriState::Off
        );
    }

    #[test]
    fn three_features_on_without_warp_reads_fully_on() {
        assert_eq!(
            features([true, true, true, false]).tri_state(false),
            TriState::On,
            "an account with no Warp has nothing left to turn on"
        );
    }

    #[test]
    fn three_features_on_with_warp_reads_mixed_until_cloud_backup_joins() {
        assert_eq!(
            features([true, true, true, false]).tri_state(true),
            TriState::Mixed
        );
        assert_eq!(
            features([true, true, true, true]).tri_state(true),
            TriState::On
        );
    }

    #[test]
    fn one_feature_on_reads_mixed() {
        assert_eq!(
            features([true, false, false, false]).tri_state(true),
            TriState::Mixed
        );
    }

    #[test]
    fn a_refused_change_goes_back_to_what_each_feature_was() {
        // Remote control was already on when "all on" was pressed. A
        // refusal must leave it on, not flip it to the opposite of what
        // the request asked for.
        let before = features([true, false, false, false]);
        let (optimistic, changes) = all_features_update(before, true, true);
        assert_eq!(roll_back(optimistic, before, changes), before);
    }

    #[test]
    fn a_roll_back_leaves_a_later_change_alone() {
        let before = features([false, false, false, false]);
        let (optimistic, changes) = all_features_update(before, true, true);
        // The user turned library sync back off while the request ran.
        let mut current = optimistic;
        current.library = false;
        assert_eq!(
            roll_back(current, before, changes),
            features([false, false, false, false]),
            "the three still showing this request's value go back"
        );
        // And a field this request never touched is not touched now.
        let (_, without_backup) = all_features_update(before, true, false);
        let mut current = features([true, true, true, true]);
        current.remote_control = false;
        assert_eq!(
            roll_back(current, before, without_backup),
            features([false, false, false, true])
        );
    }

    #[test]
    fn turning_off_always_clears_all_four() {
        let (next, changes) = all_features_update(features([true, true, true, true]), false, true);
        assert_eq!(next, features([false, false, false, false]));
        assert_eq!(
            changes,
            OnlineFeatureChanges {
                remote_control: Some(false),
                play_history: Some(false),
                library: Some(false),
                cloud_backup: Some(false),
            }
        );
    }

    #[test]
    fn turning_on_with_warp_available_includes_cloud_backup() {
        let (next, changes) = all_features_update(OnlineFeatures::default(), true, true);
        assert_eq!(next, features([true, true, true, true]));
        assert_eq!(changes.cloud_backup, Some(true));
    }

    #[test]
    fn turning_on_without_confirmed_warp_leaves_cloud_backup_untouched() {
        let (next, changes) = all_features_update(OnlineFeatures::default(), true, false);
        assert_eq!(
            next,
            features([true, true, true, false]),
            "cloud backup is left as it was, not assumed off"
        );
        assert_eq!(changes.cloud_backup, None);
    }
}
