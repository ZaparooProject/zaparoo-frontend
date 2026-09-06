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
}

/// Kernel keycode -> Slint key text. Mirrors the subset in
/// `zaparoo_core::input_actions::qt_key_code`.
fn key_text(code: u16) -> Option<SharedString> {
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

impl InputReader {
    pub fn open() -> Self {
        let mut devices = Vec::new();
        for n in 0..32 {
            let path = format!("/dev/input/event{n}");
            let Ok(file) = File::open(&path) else {
                continue;
            };
            // SAFETY: setting O_NONBLOCK on a file descriptor we own;
            // no memory is passed to the kernel.
            let rc = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
            if rc == 0 {
                devices.push(file);
            }
        }
        if devices.is_empty() {
            tracing::warn!("no /dev/input/event* devices opened; input will be dead");
        } else {
            tracing::info!(count = devices.len(), "evdev devices opened");
        }
        Self {
            devices,
            buf: vec![0u8; EVENT_SIZE * 64],
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
                    window.dispatch_event(event);
                    any = true;
                }
            }
        }
        any
    }
}
