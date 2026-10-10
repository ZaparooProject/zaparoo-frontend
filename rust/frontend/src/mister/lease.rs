// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Private Main/child slot handshake. v2 never grants direct FPGA access:
//! Main serializes UIO transactions with its own OSD and video traffic. On
//! the native CRT path Main grants the module's native video window instead,
//! with no bus proxy behind it.
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const REQUEST: &[u8] = b"ZAPAROO-SCANOUT-2";
const GRANTED: &[u8] = b"ZAPAROO-SCANOUT-2 PROXY";
const GRANTED_NATIVE: &[u8] = b"ZAPAROO-SCANOUT-2 NATIVE";
static OFFER: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static MANAGED: AtomicBool = AtomicBool::new(false);
static RASTER_QUERY: AtomicBool = AtomicBool::new(false);

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
    RASTER_QUERY.store(
        std::env::var("ZAPAROO_SCANOUT_RASTER").is_ok_and(|value| value == "1"),
        Ordering::SeqCst,
    );
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

/// Main answers the proxy's raster query. An older Main ignores the packet,
/// so it is only sent where Main advertised it.
pub fn raster_query() -> bool {
    RASTER_QUERY.load(Ordering::SeqCst)
}

/// Retain until route disable, mappings and slot descriptor have been released.
/// EOF then returns bus ownership to Main, including on process death.
pub struct Lease {
    socket: File,
}

impl Lease {
    pub fn proxy_socket(&self) -> io::Result<File> {
        self.socket.try_clone()
    }
}

pub fn acquire() -> io::Result<Lease> {
    handshake(offer()?, 5000, GRANTED)
}

/// The native CRT path's lease: Main has loaded the verified module and
/// lets this process map its native video window. Nothing is proxied.
pub fn acquire_native() -> io::Result<Lease> {
    handshake(offer()?, 5000, GRANTED_NATIVE)
}

fn offer() -> io::Result<File> {
    OFFER
        .get()
        .and_then(|slot| slot.lock().ok()?.take())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Main did not offer a scanout lease",
            )
        })
}

fn handshake(file: File, timeout_ms: i32, granted: &[u8]) -> io::Result<Lease> {
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
    if count != granted.len() as isize || &reply[..granted.len()] != granted {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Main declined scanout ownership",
        ));
    }
    Ok(Lease { socket: file })
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
            b"ZAPAROO-SCANOUT-1 OK",
            b"ZAPAROO-SCANOUT-2 NO",
            b"OK",
            b"ZAPAROO-SCANOUT-2 PROXY extra",
            GRANTED_NATIVE,
        ] {
            for wanted in [GRANTED, GRANTED_NATIVE] {
                let (mut parent, child) = pair()?;
                parent.write_all(reply)?;
                let result = handshake(child, 100, wanted);
                let mut request = [0; 64];
                let count = parent.read(&mut request)?;
                assert_eq!(&request[..count], REQUEST);
                // A proxy grant is not a native one, nor the reverse.
                assert_eq!(result.is_ok(), reply == wanted);
                drop(result);
                assert_eq!(parent.read(&mut request)?, 0);
            }
        }
        Ok(())
    }

    #[test]
    fn no_acknowledgment_never_grants_bus_access() -> io::Result<()> {
        let (_parent, child) = pair()?;
        assert!(
            matches!(handshake(child, 0, GRANTED), Err(e) if e.kind() == io::ErrorKind::TimedOut)
        );
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
