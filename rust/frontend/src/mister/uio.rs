// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Main-proxied UIO transactions. The frontend maps scanout pixel slots only;
//! Main remains the sole FPGA bus writer, including while its OSD is visible.
//! Each `SOCK_SEQPACKET` message contains little-endian magic, sequence, command,
//! count, then words. Main executes the entire packet without yielding.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

const MAGIC: u16 = 0x5A52;
const MAX_BYTES: usize = 32;

pub struct Uio {
    socket: File,
    sequence: u16,
    failed: bool,
}

impl Uio {
    pub fn open(lease: &super::lease::Lease) -> io::Result<Self> {
        Ok(Self {
            socket: lease.proxy_socket()?,
            sequence: 0,
            failed: false,
        })
    }

    pub fn transact(&mut self, command: u16, words: &mut [u16]) -> io::Result<()> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "scanout proxy failed",
            ));
        }
        self.sequence = self.sequence.wrapping_add(1);
        let result = exchange(&self.socket, self.sequence, command, words, 1000);
        // Never consume a late reply as the next command's acknowledgment.
        self.failed = result.is_err();
        result
    }
}

fn exchange(
    socket: &File,
    sequence: u16,
    command: u16,
    words: &mut [u16],
    timeout: i32,
) -> io::Result<()> {
    let expected = match command {
        0x57 => 12,
        0x59 => 6,
        0x5B => 11,
        _ => 0,
    };
    if expected == 0 || words.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid scanout transaction",
        ));
    }
    let mut packet = [0_u8; MAX_BYTES];
    let length = 8 + words.len() * 2;
    for (bytes, word) in packet[..length].chunks_exact_mut(2).zip(
        [MAGIC, sequence, command, words.len() as u16]
            .into_iter()
            .chain(words.iter().copied()),
    ) {
        bytes.copy_from_slice(&word.to_le_bytes());
    }
    let fd = socket.as_raw_fd();
    // SAFETY: packet is readable for length bytes and fd is an owned socket.
    if unsafe {
        libc::send(
            fd,
            packet.as_ptr().cast(),
            length,
            libc::MSG_NOSIGNAL | libc::MSG_DONTWAIT,
        )
    } != length as isize
    {
        return Err(io::Error::last_os_error());
    }
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll references one valid writable descriptor entry.
    let ready = unsafe { libc::poll(&raw mut poll, 1, timeout) };
    if ready < 0 {
        return Err(io::Error::last_os_error());
    }
    if ready == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Main scanout proxy timed out",
        ));
    }
    let mut reply = [0_u8; MAX_BYTES];
    // SAFETY: reply is writable for its full length. MSG_TRUNC detects oversized packets.
    let count = unsafe {
        libc::recv(
            fd,
            reply.as_mut_ptr().cast(),
            reply.len(),
            libc::MSG_DONTWAIT | libc::MSG_TRUNC,
        )
    };
    if count != length as isize || reply[..8] != packet[..8] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Main scanout reply",
        ));
    }
    for (word, bytes) in words.iter_mut().zip(reply[8..length].chunks_exact(2)) {
        *word = u16::from_le_bytes([bytes[0], bytes[1]]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;

    fn pair() -> io::Result<(File, File)> {
        let mut fds = [-1; 2];
        // SAFETY: fds has room for the two newly owned socket descriptors.
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
        // SAFETY: successful socketpair transfers both fresh descriptors.
        Ok(unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) })
    }

    #[test]
    fn complete_transactions_round_trip_without_a_hardware_mapping() -> io::Result<()> {
        let (mut parent, child) = pair()?;
        let worker = std::thread::spawn(move || -> io::Result<()> {
            let mut packet = [0; MAX_BYTES];
            let count = parent.read(&mut packet)?;
            assert_eq!(count, 20);
            assert_eq!(&packet[..8], &[0x52, 0x5A, 7, 0, 0x59, 0, 6, 0]);
            packet[8..10].copy_from_slice(&0x1234_u16.to_le_bytes());
            parent.write_all(&packet[..count])
        });
        let mut words = [0; 6];
        exchange(&child, 7, 0x59, &mut words, 1000)?;
        assert_eq!(words[0], 0x1234);
        assert!(matches!(worker.join(), Ok(Ok(()))));
        Ok(())
    }

    #[test]
    fn timeout_bad_sequence_truncation_and_disconnect_never_acknowledge() -> io::Result<()> {
        for kind in 0..4 {
            let (mut parent, child) = pair()?;
            match kind {
                0 => {}
                1 => {
                    parent.write_all(&[0; 20])?;
                }
                2 => {
                    parent.write_all(&[0; 40])?;
                }
                _ => drop(parent),
            }
            assert!(exchange(&child, 1, 0x59, &mut [0; 6], 0).is_err());
        }
        let (_parent, child) = pair()?;
        assert!(exchange(&child, 1, 0x57, &mut [0; 6], 0).is_err());
        assert!(exchange(&child, 1, 0x20, &mut [0; 6], 0).is_err());
        Ok(())
    }
}
