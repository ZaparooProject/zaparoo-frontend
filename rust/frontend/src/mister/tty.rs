// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Console suppression for the framebuffer presenters. The kernel
// console shares /dev/fb0 with us: without KD_GRAPHICS it keeps
// drawing text (cursor blink, key echo, kernel messages) over the
// pixels we present, and our dirty-region blits only repaint rows the
// scene itself changed. Putting the owning VT into graphics mode stops
// all console rendering; canonical mode and echo are disabled too so a
// fallback to KD_TEXT (or a failed KDSETMODE) never scrolls the log.
//
// Written against the Linux console_ioctl / termios UAPI.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;

const KDSETMODE: libc::Ioctl = 0x4B3A;
const KD_TEXT: libc::c_ulong = 0;
const KD_GRAPHICS: libc::c_ulong = 1;

pub struct TtyGuard {
    file: File,
    saved_termios: Option<libc::termios>,
}

impl TtyGuard {
    /// Take the controlling VT (the wrapper spawns us on tty2) into
    /// graphics mode. Falls back to /dev/tty0 (the active VT) for
    /// manual SSH runs where the controlling terminal is a pty. On
    /// failure the app still runs; the console just fights the
    /// framebuffer like any un-guarded fbdev client.
    pub fn acquire() -> Option<Self> {
        for path in ["/dev/tty", "/dev/tty0"] {
            let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) else {
                continue;
            };
            let fd = file.as_raw_fd();
            // SAFETY: KDSETMODE takes an integer argument and touches
            // no user memory; fd is a valid open tty descriptor.
            if unsafe { libc::ioctl(fd, KDSETMODE, KD_GRAPHICS) } != 0 {
                continue;
            }

            // SAFETY: termios is plain-old-data; the all-zeroes bit
            // pattern is a valid initial value for every field.
            let mut saved: libc::termios = unsafe { std::mem::zeroed() };
            // SAFETY: tcgetattr writes a termios into the pointed-to
            // struct we just zeroed.
            let saved_termios = if unsafe { libc::tcgetattr(fd, &raw mut saved) } == 0 {
                let mut raw_mode = saved;
                raw_mode.c_lflag &= !(libc::ICANON | libc::ECHO);
                // SAFETY: tcsetattr reads the termios we just built.
                unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const raw_mode) };
                Some(saved)
            } else {
                None
            };

            // Belt and braces for the KD_TEXT fallback path: hide the
            // cursor so a blink timer can't repaint under us.
            let _ = file.write_all(b"\x1b[?25l");

            tracing::info!(path, "console suppressed (KD_GRAPHICS)");
            return Some(Self {
                file,
                saved_termios,
            });
        }
        tracing::warn!("no VT accepted KD_GRAPHICS; console may draw over the framebuffer");
        None
    }
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        let fd = self.file.as_raw_fd();
        if let Some(saved) = self.saved_termios {
            // SAFETY: restoring the termios captured in acquire().
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw const saved) };
        }
        // SAFETY: same contract as the KDSETMODE call in acquire().
        unsafe { libc::ioctl(fd, KDSETMODE, KD_TEXT) };
        let _ = self.file.write_all(b"\x1b[?25h");
    }
}
