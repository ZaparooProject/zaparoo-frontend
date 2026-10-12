// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `/dev/fb0` presenter: render to cached RAM, wait for vsync, copy
// only the dirty rows into the framebuffer. Zero-copy (rendering
// straight into the fb mapping) is deliberately not attempted: the fb
// aperture is uncached/write-combined, and rendering into it means
// every blend reads uncached memory. Render-to-cached + copy is the
// right shape on Cortex-A9.
//
// A cached page slide goes out the same way: each step is composed into a
// second RAM frame Slint never renders into, and only the rows the step
// changed are copied.
//
// Written against the Linux fbdev UAPI (linux/fb.h).

use super::scanout::Damage;
use super::transition::{CachedSlide, BACKGROUND_RGBA};
use super::Presenter;
use slint::platform::software_renderer::{PremultipliedRgbaColor, SoftwareRenderer};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

// libc::Ioctl is c_ulong on glibc but c_int on musl; all three request
// values fit in i32, so the alias keeps both targets building.
const FBIOGET_VSCREENINFO: libc::Ioctl = 0x4600;
const FBIOGET_FSCREENINFO: libc::Ioctl = 0x4602;
pub(super) const FBIO_WAITFORVSYNC: libc::Ioctl = 0x4004_4620;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub(super) struct FbBitfield {
    pub offset: u32,
    pub length: u32,
    pub msb_right: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub(super) struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red: FbBitfield,
    pub green: FbBitfield,
    pub blue: FbBitfield,
    pub transp: FbBitfield,
    pub nonstd: u32,
    pub activate: u32,
    pub height: u32,
    pub width: u32,
    pub accel_flags: u32,
    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct FbFixScreeninfo {
    pub id: [u8; 16],
    pub smem_start: libc::c_ulong,
    pub smem_len: u32,
    pub fb_type: u32,
    pub type_aux: u32,
    pub visual: u32,
    pub xpanstep: u16,
    pub ypanstep: u16,
    pub ywrapstep: u16,
    pub line_length: u32,
    pub mmio_start: libc::c_ulong,
    pub mmio_len: u32,
    pub accel: u32,
    pub capabilities: u16,
    pub reserved: [u16; 2],
}

/// Query fb0 geometry. Shared with the DDR presenter, whose mode
/// selection and old-host self-disable both key off fb0's shape.
pub(super) fn query_fb(
    fd: std::os::fd::RawFd,
) -> Result<(FbVarScreeninfo, FbFixScreeninfo), slint::PlatformError> {
    let mut var = FbVarScreeninfo::default();
    // SAFETY: FBIOGET_VSCREENINFO writes a fb_var_screeninfo into the
    // pointed-to struct; ours matches the UAPI layout and outlives
    // the call.
    let rc = unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO, &mut var) };
    if rc != 0 {
        return Err(slint::PlatformError::Other(
            "FBIOGET_VSCREENINFO failed".into(),
        ));
    }
    let mut fix = FbFixScreeninfo::default();
    // SAFETY: same contract as above for fb_fix_screeninfo.
    let rc = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO, &mut fix) };
    if rc != 0 {
        return Err(slint::PlatformError::Other(
            "FBIOGET_FSCREENINFO failed".into(),
        ));
    }
    Ok((var, fix))
}

impl Default for FbFixScreeninfo {
    fn default() -> Self {
        // SAFETY: FbFixScreeninfo is a plain-old-data ioctl output
        // struct; the all-zeroes bit pattern is a valid value for
        // every field.
        unsafe { std::mem::zeroed() }
    }
}

pub struct Fb0Presenter {
    file: File,
    /// Native fbdev mapping, or the same physical aperture via /dev/mem.
    mapping: super::fb_mapping::FbMapping,
    line_length: usize,
    width: u32,
    height: u32,
    /// Byte offset of red within a 32-bit pixel (16 = xRGB/BGRx
    /// memory order, 0 = xBGR).
    red_offset: u32,
    /// Cached RAM render target.
    buffer: Vec<PremultipliedRgbaColor>,
    /// Cached page motion and the presentation-only frame it shows. The
    /// frame is empty where this presenter does not run page slides.
    slide: CachedSlide<PremultipliedRgbaColor>,
    cached_transitions_available: bool,
    vsync_supported: bool,
    last_copy: std::time::Duration,
}

// SAFETY: the raw fb pointer is only dereferenced from the render
// thread that owns the presenter; the type is moved, never shared.
unsafe impl Send for Fb0Presenter {}

impl Fb0Presenter {
    pub fn open(allow_cached_transitions: bool) -> Result<Self, slint::PlatformError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/fb0")
            .map_err(|e| slint::PlatformError::Other(format!("open /dev/fb0: {e}")))?;
        let fd = file.as_raw_fd();

        let (var, fix) = query_fb(fd)?;
        if var.bits_per_pixel != 32 {
            return Err(slint::PlatformError::Other(format!(
                "unsupported fb depth {} (need 32bpp)",
                var.bits_per_pixel
            )));
        }

        if !fix.line_length.is_multiple_of(4) {
            return Err(slint::PlatformError::Other(
                "framebuffer stride must be aligned to 32-bit pixels".into(),
            ));
        }
        let mut mapping = super::fb_mapping::FbMapping::open(&file, &fix)
            .map_err(|error| slint::PlatformError::Other(format!("map framebuffer: {error}")))?;
        // Wipe whatever the console left behind: our per-frame blits
        // only cover rows the scene dirties, so stale text outside the
        // UI would otherwise persist for the whole session.
        mapping.clear();

        let width = var.xres;
        let height = var.yres;
        let _ = super::OUTPUT_SIZE.set((width, height));
        tracing::info!(
            width,
            height,
            line_length = fix.line_length,
            red_offset = var.red.offset,
            cached_transitions = allow_cached_transitions,
            "fb0 presenter ready"
        );
        let pixels = (width as usize) * (height as usize);
        if allow_cached_transitions {
            super::transition::set_available(true);
        }
        Ok(Self {
            file,
            mapping,
            line_length: fix.line_length as usize,
            width,
            height,
            red_offset: var.red.offset,
            buffer: vec![PremultipliedRgbaColor::default(); pixels],
            slide: CachedSlide::new(
                if allow_cached_transitions { pixels } else { 0 },
                BACKGROUND_RGBA,
            ),
            cached_transitions_available: allow_cached_transitions,
            vsync_supported: true,
            last_copy: std::time::Duration::ZERO,
        })
    }

    /// Convert one dirty span from premultiplied RGBA to the fb's
    /// 32-bit layout and store it. The span comes from the cached slide's
    /// frame while one is showing and from the render target otherwise.
    fn copy_row(&mut self, slide_frame: bool, row: usize, x0: usize, w: usize) {
        let stride = self.width as usize;
        let source = if slide_frame {
            self.slide.frame()
        } else {
            &self.buffer
        };
        let src = &source[row * stride + x0..row * stride + x0 + w];
        let byte_off = row * self.line_length + x0 * 4;
        if byte_off + w * 4 > self.mapping.len() {
            return;
        }
        // xRGB memory order (red at bit 16) means bytes B,G,R,X on
        // little-endian; xBGR means bytes R,G,B,X.
        let rgb_order_matches = self.red_offset == 0;
        for (i, px) in src.iter().enumerate() {
            let out = if rgb_order_matches {
                [px.red, px.green, px.blue, 0]
            } else {
                [px.blue, px.green, px.red, 0]
            };
            self.mapping
                .write_word(byte_off + i * 4, u32::from_ne_bytes(out));
        }
    }
}

impl Fb0Presenter {
    /// Copies a damage box, clamped to the screen, into the fb.
    fn copy_damage(&mut self, slide_frame: bool, damage: Damage) {
        if damage.is_empty() {
            return;
        }
        let x0 = damage.x0.min(self.width) as usize;
        let x1 = damage.x1.min(self.width) as usize;
        let y0 = damage.y0.min(self.height) as usize;
        let y1 = damage.y1.min(self.height) as usize;
        for row in y0..y1 {
            self.copy_row(slide_frame, row, x0, x1 - x0);
        }
    }

    fn full_damage(&self) -> Damage {
        Damage {
            x0: 0,
            y0: 0,
            x1: self.width,
            y1: self.height,
        }
    }
}

impl Presenter for Fb0Presenter {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn wait_vsync(&mut self) {
        if self.vsync_supported {
            let mut arg: u32 = 0;
            // SAFETY: FBIO_WAITFORVSYNC reads a u32 argument; blocks
            // until the next vertical blank.
            let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), FBIO_WAITFORVSYNC, &mut arg) };
            if rc == 0 {
                return;
            }
            tracing::warn!("FBIO_WAITFORVSYNC unsupported; falling back to sleep pacing");
            self.vsync_supported = false;
        }
        std::thread::sleep(std::time::Duration::from_micros(16_667));
    }

    /// Render the dirty part of the scene into the cached buffer,
    /// wait for vblank, then copy the dirty rows into the fb.
    fn render_and_present(&mut self, renderer: &SoftwareRenderer) -> std::time::Duration {
        let stride = self.width as usize;
        let height = self.height as usize;
        let render_start = std::time::Instant::now();
        // An interrupted page slide may have left the fb on a partial page.
        // Copy the complete canonical frame, even when Slint itself only
        // dirtied a cursor or a small modal.
        let cancelled = self.cached_transitions_available && super::transition::take_cancelled();
        if cancelled {
            self.slide.abandon();
        }
        // A request arrives before destination properties render. Preserve
        // outgoing pixels first, then let Slint build its canonical endpoint.
        if self.cached_transitions_available {
            self.slide.begin(&self.buffer, stride, height);
        }
        let region = renderer.render(self.buffer.as_mut_slice(), stride);
        let slide_step = self.slide.compose(&self.buffer, stride, height);
        let render_time = render_start.elapsed();
        self.wait_vsync();
        let copy_start = std::time::Instant::now();
        if cancelled {
            self.copy_damage(slide_step.is_some(), self.full_damage());
        } else if let Some((damage, _)) = slide_step {
            self.copy_damage(true, damage);
        } else {
            for (origin, size) in region.iter() {
                let x0 = origin.x.max(0) as usize;
                let y0 = origin.y.max(0) as usize;
                let w = (size.width as usize).min(stride.saturating_sub(x0));
                let h = (size.height as usize).min(height.saturating_sub(y0));
                for row in y0..y0 + h {
                    self.copy_row(false, row, x0, w);
                }
            }
        }
        if matches!(slide_step, Some((_, true))) {
            self.slide.finish();
        }
        self.last_copy = copy_start.elapsed();
        render_time + self.last_copy
    }

    fn present_cached_transition(&mut self) -> Option<std::time::Duration> {
        if !self.cached_transitions_available {
            return None;
        }
        if super::transition::take_cancelled() {
            self.slide.abandon();
            self.wait_vsync();
            let copy_start = std::time::Instant::now();
            self.copy_damage(false, self.full_damage());
            self.last_copy = copy_start.elapsed();
            return Some(self.last_copy);
        }
        if !self.slide.is_active() {
            return None;
        }
        let compose_start = std::time::Instant::now();
        let (damage, finished) =
            self.slide
                .compose(&self.buffer, self.width as usize, self.height as usize)?;
        let compose_time = compose_start.elapsed();
        self.wait_vsync();
        let copy_start = std::time::Instant::now();
        self.copy_damage(true, damage);
        if finished {
            self.slide.finish();
        }
        self.last_copy = copy_start.elapsed();
        Some(compose_time + self.last_copy)
    }

    fn last_copy(&self) -> std::time::Duration {
        self.last_copy
    }
}

impl Drop for Fb0Presenter {
    fn drop(&mut self) {
        if self.cached_transitions_available {
            super::transition::set_available(false);
        }
    }
}
