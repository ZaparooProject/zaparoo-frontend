// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Private Main/child ownership handshake. An inherited offer is not a grant:
//! mode probing runs first; only Main's acknowledgment permits FPGA traffic.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const REQUEST: &[u8] = b"ZAPAROO-SCANOUT-1";
const GRANTED: &[u8] = b"ZAPAROO-SCANOUT-1 OK";
static OFFER: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static MANAGED: AtomicBool = AtomicBool::new(false);

/// Call once during startup, before commands/threads can inherit the descriptor.
pub fn configure() -> bool {
    let Some(fd) = std::env::var("ZAPAROO_SCANOUT_FD")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
    else {
        return false;
    };
    if fd < 3 || !valid_offer(fd) {
        return false;
    }
    // SAFETY: Main transfers this validated inherited descriptor to the child.
    // configure is called once; no other frontend component owns it.
    let file = unsafe { File::from_raw_fd(fd) };
    // SAFETY: valid owned descriptor; prevent inheritance by vmode/Core helpers.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return false;
    }
    if OFFER.set(Mutex::new(Some(file))).is_err() {
        return false;
    }
    MANAGED.store(true, Ordering::SeqCst);
    true
}

fn valid_offer(fd: i32) -> bool {
    let mut kind: libc::c_int = 0;
    let mut len = size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: pointers reference correctly sized writable socket-option values.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&raw mut kind).cast(),
            &raw mut len,
        )
    };
    if rc != 0 || kind != libc::SOCK_SEQPACKET {
        return false;
    }
    let mut peer = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    len = size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: same contract for SO_PEERCRED; getppid/geteuid take no pointers.
    unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut peer).cast(),
            &raw mut len,
        ) == 0
            && peer.pid == libc::getppid()
            && peer.uid == libc::geteuid()
    }
}

pub fn managed() -> bool {
    MANAGED.load(Ordering::SeqCst)
}

/// Retain until route disable, mappings and slot descriptor have been released.
/// EOF then returns bus ownership to Main, including on process death.
pub struct Lease {
    _socket: File,
}

pub fn acquire() -> io::Result<Lease> {
    let file = OFFER
        .get()
        .and_then(|slot| slot.lock().ok()?.take())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Main did not offer a scanout lease",
            )
        })?;
    handshake(file, 5000)
}

fn handshake(file: File, timeout_ms: i32) -> io::Result<Lease> {
    let fd = file.as_raw_fd();
    // SAFETY: request bytes and their length describe a valid readable buffer.
    let sent = unsafe {
        libc::send(
            fd,
            REQUEST.as_ptr().cast(),
            REQUEST.len(),
            libc::MSG_NOSIGNAL,
        )
    };
    if sent != REQUEST.len() as isize {
        return Err(io::Error::last_os_error());
    }
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll points at one valid descriptor entry for the call's duration.
    let ready = unsafe { libc::poll(&raw mut poll, 1, timeout_ms) };
    if ready < 0 {
        return Err(io::Error::last_os_error());
    }
    if ready == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Main scanout grant timed out",
        ));
    }
    let mut reply = [0_u8; 64];
    // SAFETY: reply is writable for the supplied length; socket is still owned.
    let count = unsafe {
        libc::recv(
            fd,
            reply.as_mut_ptr().cast(),
            reply.len(),
            libc::MSG_DONTWAIT,
        )
    };
    if count != GRANTED.len() as isize || &reply[..GRANTED.len()] != GRANTED {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Main declined scanout ownership",
        ));
    }
    Ok(Lease { _socket: file })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> io::Result<(File, File)> {
        let mut fds = [-1; 2];
        // SAFETY: fds holds the two descriptors socketpair writes on success.
        if unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful socketpair transfers two fresh descriptors to us.
        Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
    }

    #[test]
    fn only_exact_grant_transfers_ownership_and_drop_disconnects() -> io::Result<()> {
        use std::io::{Read, Write};
        for reply in [
            GRANTED,
            b"ZAPAROO-SCANOUT-1 NO",
            b"OK",
            b"ZAPAROO-SCANOUT-1 OK extra",
        ] {
            let (mut parent, child) = pair()?;
            parent.write_all(reply)?;
            let result = handshake(child, 100);
            let mut request = [0; 64];
            let count = parent.read(&mut request)?;
            assert_eq!(&request[..count], REQUEST);
            assert_eq!(result.is_ok(), reply == GRANTED);
            drop(result);
            assert_eq!(parent.read(&mut request)?, 0);
        }
        Ok(())
    }

    #[test]
    fn no_acknowledgment_never_grants_bus_access() -> io::Result<()> {
        let (_parent, child) = pair()?;
        assert!(matches!(handshake(child, 0), Err(e) if e.kind() == io::ErrorKind::TimedOut));
        Ok(())
    }

    #[test]
    fn ordinary_files_and_unrelated_socket_peers_are_not_main_offers() -> io::Result<()> {
        let file = File::open("/dev/null")?;
        assert!(!valid_offer(file.as_raw_fd()));
        let (_parent, child) = pair()?;
        // Socketpair was created by this process, not its parent Main.
        assert!(!valid_offer(child.as_raw_fd()));
        Ok(())
    }
}
