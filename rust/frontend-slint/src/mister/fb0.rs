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
// Written against the Linux fbdev UAPI (linux/fb.h).

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
    /// mmap of the fb aperture.
    fb_ptr: *mut u8,
    fb_len: usize,
    line_length: usize,
    width: u32,
    height: u32,
    /// Byte offset of red within a 32-bit pixel (16 = xRGB/BGRx
    /// memory order, 0 = xBGR).
    red_offset: u32,
    /// Cached RAM render target.
    buffer: Vec<PremultipliedRgbaColor>,
    vsync_supported: bool,
}

// SAFETY: the raw fb pointer is only dereferenced from the render
// thread that owns the presenter; the type is moved, never shared.
unsafe impl Send for Fb0Presenter {}

impl Fb0Presenter {
    pub fn open() -> Result<Self, slint::PlatformError> {
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

        let fb_len = fix.smem_len as usize;
        // SAFETY: mapping the fb device the kernel advertised with
        // smem_len bytes; MAP_SHARED so stores reach scanout memory.
        let fb_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                fb_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if fb_ptr == libc::MAP_FAILED {
            return Err(slint::PlatformError::Other("mmap /dev/fb0 failed".into()));
        }
        // Wipe whatever the console left behind: our per-frame blits
        // only cover rows the scene dirties, so stale text outside the
        // UI would otherwise persist for the whole session.
        // SAFETY: the mapping is fb_len bytes and writable.
        unsafe { std::ptr::write_bytes(fb_ptr.cast::<u8>(), 0, fb_len) };

        let width = var.xres;
        let height = var.yres;
        let _ = super::OUTPUT_SIZE.set((width, height));
        tracing::info!(
            width,
            height,
            line_length = fix.line_length,
            red_offset = var.red.offset,
            "fb0 presenter ready"
        );
        Ok(Self {
            file,
            fb_ptr: fb_ptr.cast(),
            fb_len,
            line_length: fix.line_length as usize,
            width,
            height,
            red_offset: var.red.offset,
            buffer: vec![PremultipliedRgbaColor::default(); (width as usize) * (height as usize)],
            vsync_supported: true,
        })
    }

    /// Convert one dirty span from premultiplied RGBA to the fb's
    /// 32-bit layout and store it.
    fn copy_row(&mut self, row: usize, x0: usize, w: usize) {
        let stride = self.width as usize;
        let src = &self.buffer[row * stride + x0..row * stride + x0 + w];
        let byte_off = row * self.line_length + x0 * 4;
        if byte_off + w * 4 > self.fb_len {
            return;
        }
        // xRGB memory order (red at bit 16) means bytes B,G,R,X on
        // little-endian; xBGR means bytes R,G,B,X.
        let rgb_order_matches = self.red_offset == 0;
        // SAFETY: byte_off + w*4 is bounds-checked against the
        // mapping length above; the mapping lives as long as self.
        let dst = unsafe { std::slice::from_raw_parts_mut(self.fb_ptr.add(byte_off), w * 4) };
        for (i, px) in src.iter().enumerate() {
            let out = if rgb_order_matches {
                [px.red, px.green, px.blue, 0]
            } else {
                [px.blue, px.green, px.red, 0]
            };
            dst[i * 4..i * 4 + 4].copy_from_slice(&out);
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
        let render_start = std::time::Instant::now();
        let region = renderer.render(self.buffer.as_mut_slice(), stride);
        let render_time = render_start.elapsed();
        self.wait_vsync();
        let copy_start = std::time::Instant::now();
        for (origin, size) in region.iter() {
            let x0 = origin.x.max(0) as usize;
            let y0 = origin.y.max(0) as usize;
            let w = (size.width as usize).min(stride.saturating_sub(x0));
            let h = (size.height as usize).min((self.height as usize).saturating_sub(y0));
            for row in y0..y0 + h {
                self.copy_row(row, x0, w);
            }
        }
        render_time + copy_start.elapsed()
    }
}

impl Drop for Fb0Presenter {
    fn drop(&mut self) {
        // SAFETY: unmapping the region mapped in open(); the pointer
        // and length come from that mmap call.
        unsafe {
            libc::munmap(self.fb_ptr.cast(), self.fb_len);
        }
    }
}
