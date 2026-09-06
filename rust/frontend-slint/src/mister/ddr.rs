// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Menu fork native video: DDR contract v2 presenter. Rust port of
// `src/app/native_video_writer.cpp` (our own code); the normative
// consumer is the Menu fork's `rtl/native_video_reader.sv` and the
// contract is documented in the fork's
// `docs/native-video-frontend-brief.md`.
//
// Differences from the C++ writer: Qt rendered into fb0 and the
// writer bulk-copied fb0 -> DDR; here Slint renders into our own
// cached RAM buffer and the copy converts RGBA -> BGRX (the "linuxfb
// byte order" the contract expects; the core swaps bytes in RTL) on
// the way into the uncached slot. fb0 is still opened read-only for
// the geometry validation, which doubles as the mode selector and the
// self-disable path against an old host.

use super::Presenter;
use slint::platform::software_renderer::{PremultipliedRgbaColor, SoftwareRenderer};
use std::fs::OpenOptions;
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{fence, AtomicI32, Ordering};

const NATIVE_VIDEO_BASE: i64 = 0x3A00_0000;
const REGION_SIZE: usize = 0x0030_0000;
const WORD0_OFFSET: usize = 0x0;
const WORD1_OFFSET: usize = 0x4;
const BUFFER0_OFFSET: usize = 0x1000;
const BUFFER1_OFFSET: usize = 0x0018_0000;
const BYTES_PER_PIXEL: usize = 4;

static REQUESTED_H_OFFSET: AtomicI32 = AtomicI32::new(0);
static REQUESTED_V_OFFSET: AtomicI32 = AtomicI32::new(0);

pub fn set_requested_offsets(h_offset: i32, v_offset: i32) {
    let (h, v) = zaparoo_core::config::clamp_crt_offsets(h_offset, v_offset);
    REQUESTED_H_OFFSET.store(h, Ordering::SeqCst);
    REQUESTED_V_OFFSET.store(v, Ordering::SeqCst);
}

fn requested_offsets() -> (i32, i32) {
    (
        REQUESTED_H_OFFSET.load(Ordering::SeqCst),
        REQUESTED_V_OFFSET.load(Ordering::SeqCst),
    )
}

// Raster timing trim window guaranteed across every mode by the
// v2-extended contract; the RTL additionally clamps per mode
// (native_video_timing.sv header table). Asymmetric physics the user
// never sees: the user-facing offset is one symmetric range
// (zaparoo-core CRT_*_OFFSET_*), composed as
//   timing = clamp(user, TIMING_*), inset = user - timing
// where the inset shifts the rendered content inside the raster
// (window shrinks by the inset, vacated strip stays black). Timing
// movement is preferred - it moves the whole raster and costs no
// active pixels - so the inset only absorbs the right/down overflow
// past the standard front porches.
pub const TIMING_H_MIN: i32 = -31;
pub const TIMING_H_MAX: i32 = 9;
pub const TIMING_V_MIN: i32 = -14;
pub const TIMING_V_MAX: i32 = 2;

/// Split a user offset into (timing trim, content inset).
pub fn split_offset(user: i32, timing_min: i32, timing_max: i32) -> (i32, i32) {
    let timing = user.clamp(timing_min, timing_max);
    (timing, user - timing)
}

#[derive(Debug, Clone, Copy)]
struct NativeVideoMode {
    mode: u32,
    width: u32,
    height: u32,
}

impl NativeVideoMode {
    const fn stride(self) -> usize {
        self.width as usize * BYTES_PER_PIXEL
    }
    const fn frame_bytes(self) -> usize {
        self.stride() * self.height as usize
    }
}

/// fb0 geometry doubles as the mode selector: the host configures fb0
/// to one of exactly these shapes before frames flow.
const MODES: [NativeVideoMode; 3] = [
    NativeVideoMode {
        mode: 0,
        width: 352,
        height: 240,
    }, // NTSC 60p
    NativeVideoMode {
        mode: 1,
        width: 720,
        height: 480,
    }, // 480i60, rendered progressive (core extracts fields)
    NativeVideoMode {
        mode: 2,
        width: 352,
        height: 288,
    }, // PAL 50p
];

const _: () = assert!(MODES[0].frame_bytes() == 0x52800);
const _: () = assert!(MODES[1].frame_bytes() == 0x0015_1800);
const _: () = assert!(MODES[2].frame_bytes() == 0x63000);
const _: () = assert!(BUFFER0_OFFSET + MODES[1].frame_bytes() <= BUFFER1_OFFSET);
const _: () = assert!(BUFFER1_OFFSET + MODES[1].frame_bytes() <= REGION_SIZE);
const _: () = assert!(BUFFER0_OFFSET.is_multiple_of(size_of::<u32>()));
const _: () = assert!(BUFFER1_OFFSET.is_multiple_of(size_of::<u32>()));
const _: () = assert!(MODES[0].frame_bytes().is_multiple_of(size_of::<u32>()));
const _: () = assert!(MODES[1].frame_bytes().is_multiple_of(size_of::<u32>()));
const _: () = assert!(MODES[2].frame_bytes().is_multiple_of(size_of::<u32>()));

/// Pack the v2-extended control block's word1: `[31:16]` magic 0x5A51,
/// `[15:8]` `h_offset` as signed int8 (+ = right), `[7:2]` `v_offset`
/// as a signed 6-bit field (+ = down), `[1:0]` mode. The wider trim
/// window replaced v2's 4-bit `v_offset`; a pre-extension menu core
/// treats the unknown magic as "no writer" and shows its noise
/// pattern, which is the intended loud failure for a version skew.
pub fn pack_word1(h_offset: i32, v_offset: i32, mode: u32) -> u32 {
    let h = (h_offset as i8) as u8;
    let v = ((v_offset as i8) as u8) & 0x3F;
    (0x5A51_u32 << 16) | (u32::from(h) << 8) | (u32::from(v) << 2) | (mode & 0x3)
}

fn mode_for_geometry(width: u32, height: u32) -> Option<NativeVideoMode> {
    MODES
        .iter()
        .copied()
        .find(|m| m.width == width && m.height == height)
}

pub struct DdrPresenter {
    base: *mut u8,
    mode: NativeVideoMode,
    frame: u32,
    active: usize,
    buffer: Vec<PremultipliedRgbaColor>,
    /// Content inset (right/down spill past the timing window). The
    /// Slint window shrinks by this much and the content lands at
    /// (`inset_h`, `inset_v`) in the raster; the vacated strip stays
    /// black from the open()-time slot clear.
    inset_h: usize,
    inset_v: usize,
    h_offset: i32,
    v_offset: i32,
    pace_in_present: bool,
    /// Slots whose vacated inset strips must be cleared before their
    /// next write. Clearing lazily avoids touching the slot currently
    /// being scanned out.
    clear_slots: u8,
}

// SAFETY: the raw DDR pointer is only dereferenced from the render
// thread that owns the presenter; the type is moved, never shared.
unsafe impl Send for DdrPresenter {}

impl DdrPresenter {
    /// Select an explicit native raster, optionally validate matching
    /// fb0 geometry for direct-video/single-output mode, map the DDR
    /// window, and arm the control words. Dual-head mode deliberately
    /// leaves fb0 at HDMI resolution and skips that coupling.
    pub fn open(
        width: u32,
        height: u32,
        h_offset: i32,
        v_offset: i32,
        validate_fb: bool,
        pace_in_present: bool,
    ) -> Result<Self, slint::PlatformError> {
        let (h_offset, v_offset) = zaparoo_core::config::clamp_crt_offsets(h_offset, v_offset);
        set_requested_offsets(h_offset, v_offset);
        let mode = mode_for_geometry(width, height).ok_or_else(|| {
            slint::PlatformError::Other(format!(
                "{width}x{height} does not match a v2 native-video mode"
            ))
        })?;
        if validate_fb {
            let fb = OpenOptions::new()
                .read(true)
                .open("/dev/fb0")
                .map_err(|e| slint::PlatformError::Other(format!("open /dev/fb0: {e}")))?;
            let (var, fix) = super::fb0::query_fb(fb.as_raw_fd())?;
            if var.xres != width
                || var.yres != height
                || var.bits_per_pixel != 32
                || fix.line_length as usize != mode.stride()
                || var.xoffset != 0
                || var.yoffset != 0
            {
                return Err(slint::PlatformError::Other(format!(
                    "fb0 mode {}x{} {}bpp stride={} offset=({},{}) violates the v2 \
                     single-output precondition for {width}x{height}; native writer disabled",
                    var.xres,
                    var.yres,
                    var.bits_per_pixel,
                    fix.line_length,
                    var.xoffset,
                    var.yoffset
                )));
            }
        }

        let mem = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/mem")
            .map_err(|e| slint::PlatformError::Other(format!("open /dev/mem: {e}")))?;
        // SAFETY: mapping the Menu fork's documented DDR control
        // window; the fd is valid and the length/offset are the
        // contract constants.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                REGION_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                mem.as_raw_fd(),
                NATIVE_VIDEO_BASE as libc::off_t,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(slint::PlatformError::Other(
                "mmap native video DDR window failed".into(),
            ));
        }
        let base: *mut u8 = base.cast();

        // Compose the user offsets: raster timing first, content
        // inset for the remainder. With the symmetric user bounds the
        // spill is only ever rightward/downward (positive), and small
        // enough that the window never shrinks meaningfully.
        let (timing_h, inset_h) = split_offset(h_offset, TIMING_H_MIN, TIMING_H_MAX);
        let (timing_v, inset_v) = split_offset(v_offset, TIMING_V_MIN, TIMING_V_MAX);
        let inset_h = usize::try_from(inset_h).unwrap_or(0);
        let inset_v = usize::try_from(inset_v).unwrap_or(0);

        let mut presenter = Self {
            base,
            mode,
            frame: 0,
            // word0's slot bit starts at 0, so the FPGA scans slot 0
            // first; the first write goes to slot 1 so the initial
            // copy never races the scanout.
            active: 1,
            buffer: vec![
                PremultipliedRgbaColor::default();
                (mode.width as usize - inset_h) * (mode.height as usize - inset_v)
            ],
            inset_h,
            inset_v,
            h_offset,
            v_offset,
            pace_in_present,
            clear_slots: 0,
        };

        // Zero both slots (ghost-clear), then word1 BEFORE word0: the
        // core reads both words in one atomic 64-bit beat per vblank.
        // word0 stays 0 ("writer stopped") until the first frame.
        presenter.clear_slot(0);
        presenter.clear_slot(1);
        presenter.write_word(WORD1_OFFSET, pack_word1(timing_h, timing_v, mode.mode));
        presenter.write_word(WORD0_OFFSET, 0);

        tracing::info!(
            width = mode.width,
            height = mode.height,
            mode = mode.mode,
            h_offset,
            v_offset,
            timing_h,
            timing_v,
            inset_h,
            inset_v,
            "native video DDR presenter armed"
        );
        Ok(presenter)
    }

    #[allow(
        clippy::cast_ptr_alignment,
        reason = "base is a page-aligned mmap and both control word offsets are 4-byte aligned"
    )]
    fn write_word(&mut self, offset: usize, value: u32) {
        // SAFETY: offset is one of the two in-bounds control word
        // offsets within the mapped region; volatile so the store is
        // not elided or reordered by the compiler.
        unsafe {
            self.base.add(offset).cast::<u32>().write_volatile(value);
        }
    }

    const fn slot_offset(slot: usize) -> usize {
        if slot == 0 {
            BUFFER0_OFFSET
        } else {
            BUFFER1_OFFSET
        }
    }

    fn slot_slice(&mut self, slot: usize) -> &mut [u8] {
        let offset = Self::slot_offset(slot);
        // SAFETY: the compile-time asserts prove both slots fit inside
        // the mapped region even in the largest mode; the mapping
        // lives as long as self.
        unsafe { std::slice::from_raw_parts_mut(self.base.add(offset), self.mode.frame_bytes()) }
    }

    #[allow(
        clippy::cast_ptr_alignment,
        reason = "mmap is page-aligned and compile-time assertions prove aligned slot offsets"
    )]
    fn clear_slot(&mut self, slot: usize) {
        let offset = Self::slot_offset(slot);
        let words = self.mode.frame_bytes() / size_of::<u32>();
        // MiSTer's DDR window is device memory: unaligned halfword stores
        // fault even though normal ARM RAM permits them. musl's optimized
        // memset intentionally uses such stores, so slice.fill(0) cannot
        // clear this mapping. Volatile aligned words avoid both memset
        // lowering and writes being elided across the MMIO boundary.
        // SAFETY: mmap is page-aligned and offset alignment is asserted.
        let ptr = unsafe { self.base.add(offset).cast::<u32>() };
        for i in 0..words {
            // SAFETY: slot offsets and frame sizes are 4-byte aligned by
            // compile-time assertions, and every word stays in its slot.
            unsafe { ptr.add(i).write_volatile(0) };
        }
    }
}

impl Presenter for DdrPresenter {
    fn size(&self) -> (u32, u32) {
        // The Slint window is the raster minus the content inset; the
        // percentage-driven layouts adapt to the slightly smaller
        // canvas and the vacated strip stays black.
        (
            self.mode.width - self.inset_h as u32,
            self.mode.height - self.inset_v as u32,
        )
    }

    fn wait_vsync(&mut self) {
        // The core latches at its own field boundary; the writer only
        // needs pacing. Mode 2 (PAL) is 50 Hz; modes 0/1 are 60 Hz.
        let period_us = if self.mode.mode == 2 { 20_000 } else { 16_667 };
        std::thread::sleep(std::time::Duration::from_micros(period_us));
    }

    fn sync_controls(&mut self) -> Option<(u32, u32)> {
        let (h_offset, v_offset) = requested_offsets();
        if (h_offset, v_offset) == (self.h_offset, self.v_offset) {
            return None;
        }
        let (timing_h, inset_h) = split_offset(h_offset, TIMING_H_MIN, TIMING_H_MAX);
        let (timing_v, inset_v) = split_offset(v_offset, TIMING_V_MIN, TIMING_V_MAX);
        let inset_h = usize::try_from(inset_h).unwrap_or(0);
        let inset_v = usize::try_from(inset_v).unwrap_or(0);
        let size_changed = (inset_h, inset_v) != (self.inset_h, self.inset_v);

        self.h_offset = h_offset;
        self.v_offset = v_offset;
        if size_changed {
            self.inset_h = inset_h;
            self.inset_v = inset_v;
            self.buffer = vec![
                PremultipliedRgbaColor::default();
                (self.mode.width as usize - inset_h)
                    * (self.mode.height as usize - inset_v)
            ];
            // A shifted window leaves different strips untouched. Mark
            // both slots for clearing before their next write; never clear
            // the slot the FPGA may currently be scanning.
            self.clear_slots = 0b11;
        }
        // Next frame's seq_cst publish orders this control word ahead
        // of word0, so timing and pixels latch together at vblank.
        self.write_word(WORD1_OFFSET, pack_word1(timing_h, timing_v, self.mode.mode));
        tracing::info!(
            h_offset,
            v_offset,
            timing_h,
            timing_v,
            inset_h,
            inset_v,
            "native video offsets updated"
        );
        size_changed.then_some((
            self.mode.width - self.inset_h as u32,
            self.mode.height - self.inset_v as u32,
        ))
    }

    fn render_and_present(&mut self, renderer: &SoftwareRenderer) -> std::time::Duration {
        let busy_start = std::time::Instant::now();
        let win_w = self.mode.width as usize - self.inset_h;
        let win_h = self.mode.height as usize - self.inset_v;
        // Render to cached RAM, then one full-frame convert-copy into
        // the uncached slot (cached reads + sequential uncached writes
        // burst well on Cortex-A9; the frame is only ~330 KB). The
        // window lands at (inset_h, inset_v) in the raster; the strip
        // it vacates was blacked at open() and is never touched.
        renderer.render(self.buffer.as_mut_slice(), win_w);

        // Copy without holding two mutable borrows of self.
        let raster_w = self.mode.width as usize;
        let (inset_h, inset_v) = (self.inset_h, self.inset_v);
        let src_ptr = self.buffer.as_ptr();
        let next_slot = self.active;
        let active_bit = 1_u8 << next_slot;
        if self.clear_slots & active_bit != 0 {
            self.clear_slot(next_slot);
            self.clear_slots &= !active_bit;
        }
        let dst = self.slot_slice_active();
        for y in 0..win_h {
            let dst_row = ((y + inset_v) * raster_w + inset_h) * 4;
            for x in 0..win_w {
                // SAFETY: y*win_w+x < win_w*win_h == buffer length,
                // established at construction from the same window
                // dimensions.
                let px = unsafe { *src_ptr.add(y * win_w + x) };
                let out = [px.blue, px.green, px.red, 0];
                dst[dst_row + x * 4..dst_row + x * 4 + 4].copy_from_slice(&out);
            }
        }

        // The fence orders the pixel stores ahead of the word0
        // publish, mirroring the C++ writer's seq_cst fence.
        fence(Ordering::SeqCst);
        self.frame = self.frame.wrapping_add(1) & 0x3FFF_FFFF;
        if self.frame == 0 {
            self.frame = 1;
        }
        let word0 = (self.frame << 2) | self.active as u32;
        self.write_word(WORD0_OFFSET, word0);
        self.active ^= 1;
        let busy = busy_start.elapsed();
        if self.pace_in_present {
            self.wait_vsync();
        }
        busy
    }
}

impl DdrPresenter {
    fn slot_slice_active(&mut self) -> &mut [u8] {
        let slot = self.active;
        self.slot_slice(slot)
    }
}

impl Drop for DdrPresenter {
    fn drop(&mut self) {
        // word0 = 0 is the "writer stopped" signal (the core reverts
        // to its noise pattern within one frame); word1 zeroed after
        // for tidiness, same order as the C++ cleanup path.
        self.write_word(WORD0_OFFSET, 0);
        self.write_word(WORD1_OFFSET, 0);
        // SAFETY: unmapping the region mapped in open() with the same
        // base pointer and length.
        unsafe {
            libc::munmap(self.base.cast(), REGION_SIZE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // v2-extended layout goldens (magic 0x5A51, 6-bit v_offset at
    // [7:2], 2-bit mode). Field placement cross-checked against the
    // reader RTL's parse (`$signed(ctrl_word1[7:2])`, `[1:0]` mode).
    #[test]
    fn word1_packing_matches_the_v2e_contract() {
        assert_eq!(pack_word1(0, 0, 0), 0x5A51_0000);
        assert_eq!(pack_word1(1, 1, 1), 0x5A51_0105);
        assert_eq!(pack_word1(8, 2, 2), 0x5A51_080A);
        assert_eq!(pack_word1(-1, -1, 0), 0x5A51_FFFC);
        // The new range actually encodes: -31 px / -14 lines.
        assert_eq!(pack_word1(-31, -14, 0), 0x5A51_E1C8);
        // Sign bit of the 6-bit field lands at bit 7.
        assert_eq!(pack_word1(0, -14, 0) & 0xFF, 0b1100_1000);
    }

    // The full symmetric user range decomposes into timing + inset
    // with timing preferred and only right/down ever spilling.
    #[test]
    fn offsets_split_into_timing_plus_inset() {
        assert_eq!(split_offset(0, TIMING_H_MIN, TIMING_H_MAX), (0, 0));
        assert_eq!(split_offset(-16, TIMING_H_MIN, TIMING_H_MAX), (-16, 0));
        assert_eq!(split_offset(9, TIMING_H_MIN, TIMING_H_MAX), (9, 0));
        assert_eq!(split_offset(16, TIMING_H_MIN, TIMING_H_MAX), (9, 7));
        assert_eq!(split_offset(-10, TIMING_V_MIN, TIMING_V_MAX), (-10, 0));
        assert_eq!(split_offset(2, TIMING_V_MIN, TIMING_V_MAX), (2, 0));
        assert_eq!(split_offset(10, TIMING_V_MIN, TIMING_V_MAX), (2, 8));
    }

    #[test]
    fn geometry_selects_the_documented_modes() {
        assert_eq!(mode_for_geometry(352, 240).map(|m| m.mode), Some(0));
        assert_eq!(mode_for_geometry(720, 480).map(|m| m.mode), Some(1));
        assert_eq!(mode_for_geometry(352, 288).map(|m| m.mode), Some(2));
        assert!(mode_for_geometry(320, 240).is_none());
    }
}
