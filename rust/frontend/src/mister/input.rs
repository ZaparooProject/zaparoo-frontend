// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Raw evdev keyboard reader. MiSTer's Main forwards controller
// buttons as keyboard keys (West=Space, North=Tab, East=Enter,
// South=Esc, L/R=PageUp/PageDown), so a keyboard-only reader covers
// pads too. All openable /dev/input/event* nodes are polled
// non-blocking each frame and key events are dispatched into the
// Slint window, which routes them through the same FocusScope ->
// action mapping as the desktop build.

use slint::platform::{Key, WindowEvent};
use slint::SharedString;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;

const EV_KEY: u16 = 1;
/// `_IOW('E', 0xa0, int)`: selects the clock this reader's events are
/// stamped with. Per open file, so Main's own readers are unaffected.
const EVIOCSCLOCKID: libc::Ioctl = 0x4004_45a0;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

const EVENT_SIZE: usize = size_of::<InputEvent>();

pub struct InputReader {
    devices: Vec<File>,
    buf: Vec<u8>,
    /// Every node stamps its events on the monotonic clock, so their times
    /// can be compared with each other.
    event_times: bool,
}

/// Letter and digit keycodes in kernel order, US layout: the top digit
/// row, then the three letter rows. They type into a text field and map
/// to no action elsewhere.
const TEXT_KEYS: [(u16, &str); 4] = [
    (2, "1234567890"),
    (16, "qwertyuiop"),
    (30, "asdfghjkl"),
    (44, "zxcvbnm"),
];

fn text_key(code: u16) -> Option<char> {
    TEXT_KEYS.iter().find_map(|(first, row)| {
        let offset = usize::from(code.checked_sub(*first)?);
        row.chars().nth(offset)
    })
}

/// Kernel keycode -> Slint key text. Mirrors the subset in
/// `zaparoo_core::input_actions::qt_key_code`, plus the keys that type.
fn key_text(code: u16) -> Option<SharedString> {
    if let Some(c) = text_key(code) {
        return Some(SharedString::from(c.to_string()));
    }
    let key = match code {
        105 => Key::LeftArrow,  // KEY_LEFT
        106 => Key::RightArrow, // KEY_RIGHT
        103 => Key::UpArrow,    // KEY_UP
        108 => Key::DownArrow,  // KEY_DOWN
        28 | 96 => Key::Return, // KEY_ENTER, KEY_KPENTER
        1 => Key::Escape,       // KEY_ESC
        14 => Key::Backspace,   // KEY_BACKSPACE
        15 => Key::Tab,         // KEY_TAB
        104 => Key::PageUp,     // KEY_PAGEUP
        109 => Key::PageDown,   // KEY_PAGEDOWN
        57 => Key::Space,       // KEY_SPACE
        _ => return None,
    };
    Some(SharedString::from(key))
}

/// The kernel's event stamp in milliseconds.
fn event_ms(time: libc::timeval) -> u64 {
    let seconds = u64::try_from(time.tv_sec).unwrap_or(0);
    let micros = u64::try_from(time.tv_usec).unwrap_or(0);
    seconds.saturating_mul(1000).saturating_add(micros / 1000)
}

impl InputReader {
    pub fn open() -> Self {
        let mut devices = Vec::new();
        let mut event_times = true;
        for n in 0..32 {
            let path = format!("/dev/input/event{n}");
            let Ok(file) = File::open(&path) else {
                continue;
            };
            // SAFETY: setting O_NONBLOCK on a file descriptor we own;
            // no memory is passed to the kernel.
            let rc = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
            if rc != 0 {
                continue;
            }
            // Event times feed the duplicate guard, so they must not jump
            // when the wall clock is set. A node that refuses stays on the
            // wall clock, and the guard is shared between nodes: one refusal
            // puts every press back on handling time.
            let clock: libc::c_int = libc::CLOCK_MONOTONIC;
            // SAFETY: EVIOCSCLOCKID reads one int from a pointer that is
            // valid for the duration of the call.
            let rc = unsafe { libc::ioctl(file.as_raw_fd(), EVIOCSCLOCKID, &raw const clock) };
            if rc != 0 {
                event_times = false;
            }
            devices.push(file);
        }
        if devices.is_empty() {
            tracing::warn!("no /dev/input/event* devices opened; input will be dead");
        } else {
            tracing::info!(count = devices.len(), event_times, "evdev devices opened");
        }
        Self {
            devices,
            buf: vec![0u8; EVENT_SIZE * 64],
            event_times,
        }
    }

    /// Drain pending events from every device and dispatch key
    /// presses/releases into the window. Returns true when any key
    /// event was dispatched (the DRS policy's motion trigger).
    pub fn poll(&mut self, window: &slint::Window) -> bool {
        let mut any = false;
        for device in &mut self.devices {
            loop {
                let n = match device.read(&mut self.buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                };
                for chunk in self.buf[..n].chunks_exact(EVENT_SIZE) {
                    // SAFETY: InputEvent is repr(C) plain-old-data and
                    // every byte pattern is a valid value; the chunk is
                    // exactly EVENT_SIZE bytes.
                    let ev: InputEvent = unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast()) };
                    if ev.kind != EV_KEY {
                        continue;
                    }
                    let Some(text) = key_text(ev.code) else {
                        continue;
                    };
                    let event = match ev.value {
                        0 => WindowEvent::KeyReleased { text },
                        2 => WindowEvent::KeyPressRepeated { text },
                        _ => WindowEvent::KeyPressed { text },
                    };
                    let time_ms = event_ms(ev.time);
                    // A key that types may be part of a search or a code:
                    // the log says that one was pressed, never which.
                    let code = if text_key(ev.code).is_some() {
                        0
                    } else {
                        ev.code
                    };
                    tracing::debug!(code, value = ev.value, time_ms, "evdev key");
                    // Events are read once per frame, so a slow frame hands
                    // over several at once. The press is timed from when the
                    // button went down, not from when it is handled here.
                    if self.event_times {
                        crate::input::with_event_time(time_ms, || window.dispatch_event(event));
                    } else {
                        window.dispatch_event(event);
                    }
                    any = true;
                }
            }
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use super::{event_ms, key_text, text_key};

    #[test]
    fn event_time_is_milliseconds() {
        let time = libc::timeval {
            tv_sec: 12,
            tv_usec: 345_678,
        };
        assert_eq!(event_ms(time), 12_345);
    }

    #[test]
    fn letters_and_digits_type_and_other_keys_keep_their_meaning() {
        assert_eq!(text_key(2), Some('1'));
        assert_eq!(text_key(11), Some('0'));
        assert_eq!(text_key(16), Some('q'));
        assert_eq!(text_key(25), Some('p'));
        assert_eq!(text_key(30), Some('a'));
        assert_eq!(text_key(38), Some('l'));
        assert_eq!(text_key(44), Some('z'));
        assert_eq!(text_key(50), Some('m'));
        // The keys between the rows (minus, brackets, enter, shift) do not type.
        for code in [1, 12, 13, 14, 15, 26, 28, 29, 39, 42, 51, 57] {
            assert_eq!(text_key(code), None, "{code}");
        }
        assert_eq!(key_text(30).as_deref(), Some("a"));
        assert!(key_text(28).is_some());
        assert_eq!(key_text(51), None);
    }
}
