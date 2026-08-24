// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::runtime;
use std::path::PathBuf;

pub fn config_file_path() -> PathBuf {
    if runtime::current().is_mister() {
        PathBuf::from("/media/fat/zaparoo/frontend.toml")
    } else {
        dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zaparoo")
            .join("frontend.toml")
    }
}

pub fn log_file_path() -> PathBuf {
    if runtime::current().is_mister() {
        PathBuf::from("/tmp/zaparoo/frontend.log")
    } else {
        dirs_next::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zaparoo")
            .join("logs")
            .join("frontend.log")
    }
}

/// Path to the raw stderr capture file. The frontend dup2's its own
/// `STDERR_FILENO` onto this file early in startup so that the chained
/// default panic hook, libc `abort()` diagnostics, glibc backtraces, and
/// any kernel signal-default output land in a durable location instead
/// of `/dev/null` (which is where the `MiSTer` wrapper sends stderr).
pub fn stderr_log_path() -> PathBuf {
    if runtime::current().is_mister() {
        PathBuf::from("/tmp/zaparoo/frontend.stderr.log")
    } else {
        dirs_next::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zaparoo")
            .join("logs")
            .join("frontend.stderr.log")
    }
}

/// Root directory scanned at startup for user-supplied customization
/// assets. Holds `systems/` and `hub/` subfolders of override images named
/// by id. Returned even when it does not exist on disk — the scan treats a
/// missing directory as "no overrides" so the feature works with zero
/// config. `[custom] dir` in `frontend.toml` overrides this default.
pub fn custom_dir() -> PathBuf {
    if runtime::current().is_mister() {
        PathBuf::from("/media/fat/zaparoo/custom")
    } else {
        let mut path = config_file_path();
        path.set_file_name("custom");
        path
    }
}

/// Frontend-owned cache directory — currently just the Hub/Resume
/// cold-boot cover manifest (a path list, not image bytes; see
/// `media_image_cache.rs`'s "Memory only — never disk" module doc, which
/// stays true of the bytes themselves). On `MiSTer` this is a sibling of
/// Core's own `cache/thumbs/` inside Core's existing cache root
/// (`config.CacheDir` in the Core repo) — confirmed safe from Core's own
/// cleanup, which only ever touches `cache/thumbs/*` and a handful of
/// named `.gob`/`.json` files, and already excluded from Core's
/// backup/sync policy (`cache/` is regenerable, never collected). Returned
/// even when it does not exist on disk — callers create it on first
/// write, same convention as `custom_dir`.
pub fn cache_dir() -> PathBuf {
    if runtime::current().is_mister() {
        PathBuf::from("/media/fat/zaparoo/cache/frontend")
    } else {
        dirs_next::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zaparoo")
            .join("frontend")
    }
}

pub fn state_file_path() -> PathBuf {
    // ZAPAROO_STATE_FILE lets tests (and ad-hoc runs) redirect state
    // persistence away from the real user path. Checked first so the
    // override applies on every platform.
    if let Ok(custom) = std::env::var("ZAPAROO_STATE_FILE") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    if runtime::current().is_mister() {
        PathBuf::from("/tmp/zaparoo/state.toml")
    } else {
        let mut path = config_file_path();
        path.set_file_name("state.toml");
        path
    }
}

/// Path to `Main_MiSTer`'s alt-launcher controller-input report (see
/// `crate::controller_report`) -- a fixed `/tmp` location on every runtime,
/// since only a colocated `Main_MiSTer` ever writes it. Not gated on
/// `runtime::current().is_mister()` here; `controller_report::spawn_watcher`
/// does that gating itself (and also honors this same override to force the
/// watcher on off-`MiSTer`). `ZAPAROO_INPUT_REPORT_FILE` lets tests and
/// desktop dev runs point at a fixture, mirroring `ZAPAROO_STATE_FILE`
/// above.
pub fn launcher_input_report_path() -> PathBuf {
    if let Ok(custom) = std::env::var("ZAPAROO_INPUT_REPORT_FILE") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    PathBuf::from("/tmp/zaparoo_launcher_input.json")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        cache_dir, config_file_path, custom_dir, launcher_input_report_path, log_file_path,
        state_file_path, stderr_log_path,
    };
    use crate::runtime;

    #[test]
    fn paths_end_with_expected_filenames() {
        let cfg = config_file_path();
        assert_eq!(
            cfg.file_name().and_then(|n| n.to_str()),
            Some("frontend.toml")
        );

        let log = log_file_path();
        assert_eq!(
            log.file_name().and_then(|n| n.to_str()),
            Some("frontend.log")
        );

        let stderr_log = stderr_log_path();
        assert_eq!(
            stderr_log.file_name().and_then(|n| n.to_str()),
            Some("frontend.stderr.log")
        );

        let state = state_file_path();
        assert_eq!(
            state.file_name().and_then(|n| n.to_str()),
            Some("state.toml")
        );
    }

    #[test]
    fn runtime_matches_configured_paths() {
        // When runtime is Desktop, paths route through dirs_next (per-user dirs)
        // rather than the fixed MiSTer locations. Asserts the branches stay in sync.
        if runtime::current().is_mister() {
            assert_eq!(
                config_file_path().to_str(),
                Some("/media/fat/zaparoo/frontend.toml")
            );
            assert_eq!(log_file_path().to_str(), Some("/tmp/zaparoo/frontend.log"));
            assert_eq!(
                stderr_log_path().to_str(),
                Some("/tmp/zaparoo/frontend.stderr.log")
            );
            assert_eq!(state_file_path().to_str(), Some("/tmp/zaparoo/state.toml"));
        } else {
            let cfg = config_file_path();
            assert!(
                cfg.ends_with("zaparoo/frontend.toml"),
                "config path did not end with zaparoo/frontend.toml: {cfg:?}"
            );
            let log = log_file_path();
            assert!(
                log.ends_with("zaparoo/logs/frontend.log"),
                "log path did not end with zaparoo/logs/frontend.log: {log:?}"
            );
            let stderr_log = stderr_log_path();
            assert!(
                stderr_log.ends_with("zaparoo/logs/frontend.stderr.log"),
                "stderr log path did not end with zaparoo/logs/frontend.stderr.log: {stderr_log:?}"
            );
            let state = state_file_path();
            assert!(
                state.ends_with("zaparoo/state.toml"),
                "state path did not end with zaparoo/state.toml: {state:?}"
            );
        }
    }

    #[test]
    fn custom_dir_resolves_per_runtime() {
        let dir = custom_dir();
        if runtime::current().is_mister() {
            assert_eq!(dir.to_str(), Some("/media/fat/zaparoo/custom"));
        } else {
            assert!(
                dir.ends_with("zaparoo/custom"),
                "custom dir did not end with zaparoo/custom: {dir:?}"
            );
            // Sibling of frontend.toml, like state.toml.
            assert_eq!(dir.parent(), config_file_path().parent());
        }
    }

    #[test]
    fn cache_dir_resolves_per_runtime() {
        let dir = cache_dir();
        if runtime::current().is_mister() {
            assert_eq!(dir.to_str(), Some("/media/fat/zaparoo/cache/frontend"));
        } else {
            assert!(
                dir.ends_with("zaparoo/frontend"),
                "cache dir did not end with zaparoo/frontend: {dir:?}"
            );
        }
    }

    #[test]
    fn state_file_sits_next_to_config_file_on_desktop() {
        if runtime::current().is_mister() {
            return;
        }
        let cfg = config_file_path();
        let state = state_file_path();
        assert_eq!(
            cfg.parent(),
            state.parent(),
            "state.toml must be a sibling of frontend.toml: cfg={cfg:?} state={state:?}"
        );
    }

    #[test]
    fn launcher_input_report_path_is_fixed_tmp_location() {
        // No test sets ZAPAROO_INPUT_REPORT_FILE, so the default applies on
        // every platform -- it is a MiSTer-only artifact at a fixed path.
        if std::env::var("ZAPAROO_INPUT_REPORT_FILE").is_ok() {
            return;
        }
        assert_eq!(
            launcher_input_report_path().to_str(),
            Some("/tmp/zaparoo_launcher_input.json")
        );
    }
}
