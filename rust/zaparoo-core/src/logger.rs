// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::config::Config;
use crate::platform_paths::log_file_path;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// Returned from install(); must be held for the process lifetime to keep the
// file-appender thread alive. Drop causes a flush + shutdown.
#[derive(Debug)]
pub struct LoggerGuard {
    _file_guard: WorkerGuard,
}

pub fn install(config: &Config) -> LoggerGuard {
    let log_path = log_file_path();
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    install_at(config, &log_path)
}

// ZAPAROO_DEBUG is truthy unless explicitly "0" or "false". Set is meant to
// be inclusive: "1", "true", "yes", or anything non-empty flips debug on.
fn debug_env_is_truthy(val: &str) -> bool {
    val != "0" && val != "false"
}

pub fn install_at(config: &Config, log_path: &Path) -> LoggerGuard {
    let debug = config.debug_logging
        || std::env::var("ZAPAROO_DEBUG").is_ok_and(|v| debug_env_is_truthy(&v));

    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or(Path::new(".")),
        log_path.file_name().unwrap_or_default(),
    );
    let (non_blocking_file, file_guard) = tracing_appender::non_blocking(file_appender);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_timer(fmt::time::UtcTime::rfc_3339());

    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .json()
        .with_timer(fmt::time::UtcTime::rfc_3339());

    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .init();

    LoggerGuard {
        _file_guard: file_guard,
    }
}

#[cfg(test)]
mod tests {
    use super::debug_env_is_truthy;

    #[test]
    fn zero_and_false_are_not_truthy() {
        assert!(!debug_env_is_truthy("0"));
        assert!(!debug_env_is_truthy("false"));
    }

    #[test]
    fn everything_else_is_truthy() {
        assert!(debug_env_is_truthy("1"));
        assert!(debug_env_is_truthy("true"));
        assert!(debug_env_is_truthy("yes"));
        assert!(debug_env_is_truthy(""));
        assert!(debug_env_is_truthy("FALSE"));
    }
}
