// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::config::Config;
use crate::platform_paths::log_file_path;
use std::{
    ffi::c_void,
    io::{self, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::Path,
    sync::Arc,
};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const LOG_TERMINAL_FD_ENV: &str = "ZAPAROO_LOG_TERMINAL_FD";

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

pub fn debug_logging_enabled(config: &Config) -> bool {
    config.debug_logging || std::env::var("ZAPAROO_DEBUG").is_ok_and(|v| debug_env_is_truthy(&v))
}

pub fn install_at(config: &Config, log_path: &Path) -> LoggerGuard {
    let debug = debug_logging_enabled(config);
    let terminal_fd = terminal_log_fd()
        .and_then(|fd| duplicate_terminal_log_fd(fd).ok())
        .map(Arc::new);

    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or(Path::new(".")),
        log_path.file_name().unwrap_or_default(),
    );
    let (non_blocking_file, file_guard) = tracing_appender::non_blocking(file_appender);

    let stderr_layer = fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_timer(fmt::time::UtcTime::rfc_3339());

    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .json()
        .with_timer(fmt::time::UtcTime::rfc_3339());

    let terminal_layer = terminal_fd.map(|fd| {
        fmt::layer()
            .with_writer(move || TerminalFdWriter {
                fd: Arc::clone(&fd),
            })
            .with_ansi(false)
            .with_target(false)
            .with_timer(fmt::time::UtcTime::rfc_3339())
    });

    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .with(terminal_layer)
        .init();

    LoggerGuard {
        _file_guard: file_guard,
    }
}

fn terminal_log_fd() -> Option<i32> {
    std::env::var(LOG_TERMINAL_FD_ENV)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
}

fn duplicate_terminal_log_fd(fd: i32) -> io::Result<OwnedFd> {
    // SAFETY: `dup` borrows `fd` and returns a new descriptor on success.
    let duplicated = unsafe { dup(fd) };
    if duplicated < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `duplicated` is a fresh descriptor returned by `dup`, so this
    // process owns it and may close it when the logger is dropped.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

#[derive(Clone, Debug)]
struct TerminalFdWriter {
    fd: Arc<OwnedFd>,
}

impl Write for TerminalFdWriter {
    fn write(&mut self, mut buf: &[u8]) -> io::Result<usize> {
        let original_len = buf.len();
        while !buf.is_empty() {
            // SAFETY: `buf` points to live memory for `buf.len()` bytes, and
            // `write` does not retain the pointer after returning.
            let written = unsafe { write(self.fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
            if written < 0 {
                return Err(io::Error::last_os_error());
            }
            if written == 0 {
                break;
            }

            let written = usize::try_from(written)
                .map_err(|_| io::Error::other("terminal log write length overflow"))?;
            buf = &buf[written.min(buf.len())..];
        }
        Ok(original_len - buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

unsafe extern "C" {
    fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
    fn dup(fd: i32) -> i32;
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
