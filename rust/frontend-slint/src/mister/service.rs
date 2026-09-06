// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The two things the frontend owes the rest of the MiSTer install at
// startup, ported from `mister_runtime.rs`: the kernel-backed resource
// lease Core reads to know a frontend is on screen, and the
// fire-and-forget service start so a fresh boot has a Core to talk to.

use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::OnceLock;

/// Held for the process lifetime: dropping the file drops the lock,
/// which is exactly the signal Core watches for.
static RESOURCE_LEASE: OnceLock<std::fs::File> = OnceLock::new();

const LEASE_PATH: &str = "/tmp/zaparoo/frontend.active.lock";

/// Take the frontend resource lease. Core responds to it dynamically,
/// so launch order and service restarts do not decide CPU or IRQ
/// topology; this only has to be held while a frontend is up.
fn acquire_resource_lease() {
    if RESOURCE_LEASE.get().is_some() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all("/tmp/zaparoo") {
        tracing::warn!("failed to create resource lease directory: {e}");
        return;
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(LEASE_PATH)
    {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("failed to open frontend resource lease: {e}");
            return;
        }
    };
    // SAFETY: the file stays open in RESOURCE_LEASE for the process
    // lifetime, so the lock outlives this call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        tracing::warn!(
            "failed to acquire frontend resource lease: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    if RESOURCE_LEASE.set(file).is_ok() {
        tracing::info!("acquired MiSTer frontend resource lease");
    }
}

fn start_command() -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/taskset");
    command.args([
        "-c",
        "0-1",
        "/media/fat/Scripts/zaparoo.sh",
        "-service",
        "start",
    ]);
    command
}

/// Take the lease and ask the service wrapper to start Core, pinned to
/// the first two cores. Fire and forget: a Core that is already running
/// makes this a no-op, and a failure only means the frontend waits for
/// a connection like any other.
pub fn ensure_core_running() {
    acquire_resource_lease();
    tracing::info!("spawning core service wrapper with CPU affinity 0-1");
    if let Err(e) = start_command().spawn() {
        tracing::warn!("failed to start zaparoo.sh with taskset: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_start_is_pinned_to_the_first_two_cores() {
        let command = start_command();
        assert_eq!(command.get_program(), "/usr/bin/taskset");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                "-c",
                "0-1",
                "/media/fat/Scripts/zaparoo.sh",
                "-service",
                "start"
            ]
        );
    }
}
