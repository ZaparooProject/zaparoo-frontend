// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Menu fork native video: DDR contract v2 presenter. The normative
// consumer, and the contract this implements, is the Menu fork's
// `rtl/native_video_reader.sv`.
//
// Slint renders into our own cached RAM buffer and the copy converts
// RGBA -> BGRX (the "linuxfb byte order" the contract expects; the core
// swaps bytes in RTL) on the way into the slot. Only the rows that changed
// since a slot was last filled are copied. Native geometry belongs to this
// DDR writer, not fb0: Main may independently reassert its framebuffer
// during startup or keep it at an HDMI resolution.
//
// A cached page slide goes out the same way: each step is composed into a
// second RAM frame Slint never renders into, and its damage is copied into
// the next slot and published exactly like a rendered frame.
//
// The window is outside the kernel's RAM, so `/dev/mem` can only map it as
// uncached device memory, where a full PAL frame costs about 27 ms to
// write. With Main's native lease the `zaparoo_scanout` module maps the
// frame slots write-combined instead (about 1 ms) and paces on the raster's
// own vertical sync. `/dev/mem` and a sleep remain the fallback.

use super::scanout::{self, Damage, Mapping};
use super::transition::{CachedSlide, BACKGROUND_RGBA};
use super::Presenter;
use slint::platform::software_renderer::{PremultipliedRgbaColor, SoftwareRenderer};
use std::fs::{File, OpenOptions};
use std::mem::size_of;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{fence, AtomicI32, Ordering};
use std::time::{Duration, Instant};

const NATIVE_VIDEO_BASE: i64 = 0x3A00_0000;
const REGION_SIZE: usize = 0x0030_0000;
const WORD0_OFFSET: usize = 0x0;
const WORD1_OFFSET: usize = 0x4;
const BUFFER0_OFFSET: usize = 0x1000;
const BUFFER1_OFFSET: usize = 0x0018_0000;
const BYTES_PER_PIXEL: usize = 4;
/// Consecutive failed sync waits before pacing goes back to the sleep.
const VBLANK_MISS_LIMIT: u8 = 3;
/// Everything: clamped to the window when it is copied.
const FULL: Damage = Damage {
    x0: 0,
    y0: 0,
    x1: u32::MAX,
    y1: u32::MAX,
};

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
// v2-extended contract; the RTL clamps to this same common window
// (native_video_timing.sv). Asymmetric physics the user
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

/// Explicit native rasters supported by the DDR reader.
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
// The module maps the control page and the slots after it separately.
const _: () = assert!(BUFFER0_OFFSET == 0x1000);
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

/// Monotonic cadence for a software-paced writer, independent of scanout.
/// Late work skips to the next boundary instead of replaying missed ticks.
struct FrameDeadline {
    period: Duration,
    next: Instant,
}

impl FrameDeadline {
    fn new(start: Instant, period: Duration) -> Self {
        Self {
            period,
            next: start + period,
        }
    }

    fn remaining(&mut self, now: Instant) -> Duration {
        if now > self.next {
            // Native periods are at most 20 ms, so the remainder fits u64.
            let remainder = now.duration_since(self.next).as_nanos() % self.period.as_nanos();
            let phase = Duration::from_nanos(u64::try_from(remainder).unwrap_or(0));
            self.next = now + (self.period.saturating_sub(phase));
        }
        let wait = self.next.saturating_duration_since(now);
        self.next += self.period;
        wait
    }
}

/// The whole window through `/dev/mem`, unmapped on drop.
struct DevMem {
    base: *mut u8,
}

impl Drop for DevMem {
    fn drop(&mut self) {
        // SAFETY: unmapping the region mapped in `open_dev_mem` with the
        // same base pointer and length.
        unsafe {
            libc::munmap(self.base.cast(), REGION_SIZE);
        }
    }
}

/// What holds the window mapped. Fields drop in declaration order:
/// mappings, the module's device, then Main's lease.
enum Window {
    DevMem(#[allow(dead_code, reason = "held for its Drop")] DevMem),
    Module {
        _control: Mapping,
        _pixels: Mapping,
        device: File,
        _lease: super::lease::Lease,
        /// The module's sync wait is still answering.
        vblank: bool,
        vblank_misses: u8,
    },
}

pub struct DdrPresenter {
    /// The two control words: always uncached, so a publish is never held
    /// behind the pixels it announces.
    control: *mut u8,
    /// Slot 0's first pixel; slot 1 follows at the contract's distance.
    pixels: *mut u8,
    window: Window,
    mode: NativeVideoMode,
    frame: u32,
    active: usize,
    buffer: Vec<PremultipliedRgbaColor>,
    /// Cached page motion and the presentation-only frame it shows, the
    /// same size as `buffer`.
    slide: CachedSlide<PremultipliedRgbaColor>,
    /// This presenter owns the screen's cached page slides. False for the
    /// second head of a dual-head pair, which the other head's presenter
    /// paces.
    cached_transitions_available: bool,
    /// Content inset (right/down spill past the timing window). The
    /// Slint window shrinks by this much and the content lands at
    /// (`inset_h`, `inset_v`) in the raster; the vacated strip stays
    /// black from the open()-time slot clear.
    inset_h: usize,
    inset_v: usize,
    h_offset: i32,
    v_offset: i32,
    pace_in_present: bool,
    pacing: FrameDeadline,
    /// Slots whose vacated inset strips must be cleared before their
    /// next write. Clearing lazily avoids touching the slot currently
    /// being scanned out.
    clear_slots: u8,
    /// Damage accumulated per slot since that slot was last filled.
    stale: [Damage; 2],
    last_copy: Duration,
}

// SAFETY: the raw DDR pointer is only dereferenced from the render
// thread that owns the presenter; the type is moved, never shared.
unsafe impl Send for DdrPresenter {}

impl DdrPresenter {
    /// Select a supported native raster, map its independent DDR window,
    /// and arm the control words. fb0 geometry never controls this writer.
    pub fn open(
        width: u32,
        height: u32,
        h_offset: i32,
        v_offset: i32,
        pace_in_present: bool,
        native_scanout: bool,
        allow_cached_transitions: bool,
    ) -> Result<Self, slint::PlatformError> {
        let (h_offset, v_offset) = zaparoo_core::config::clamp_crt_offsets(h_offset, v_offset);
        set_requested_offsets(h_offset, v_offset);
        let mode = mode_for_geometry(width, height).ok_or_else(|| {
            slint::PlatformError::Other(format!(
                "{width}x{height} does not match a v2 native-video mode"
            ))
        })?;
        let module = if native_scanout {
            open_module_window()
                .inspect_err(|e| {
                    tracing::warn!("native scanout unavailable, mapping through /dev/mem: {e}");
                })
                .ok()
        } else {
            None
        };
        let (control, pixels, window) = match module {
            Some(mapped) => mapped,
            None => open_dev_mem()?,
        };

        // Compose the user offsets: raster timing first, content
        // inset for the remainder. With the symmetric user bounds the
        // spill is only ever rightward/downward (positive), and small
        // enough that the window never shrinks meaningfully.
        let (timing_h, inset_h) = split_offset(h_offset, TIMING_H_MIN, TIMING_H_MAX);
        let (timing_v, inset_v) = split_offset(v_offset, TIMING_V_MIN, TIMING_V_MAX);
        let inset_h = usize::try_from(inset_h).unwrap_or(0);
        let inset_v = usize::try_from(inset_v).unwrap_or(0);

        let mut presenter = Self {
            control,
            pixels,
            window,
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
            slide: CachedSlide::new(
                (mode.width as usize - inset_h) * (mode.height as usize - inset_v),
                BACKGROUND_RGBA,
            ),
            cached_transitions_available: allow_cached_transitions,
            inset_h,
            inset_v,
            h_offset,
            v_offset,
            pace_in_present,
            pacing: FrameDeadline::new(
                Instant::now(),
                Duration::from_micros(if mode.mode == 2 { 20_000 } else { 16_667 }),
            ),
            clear_slots: 0,
            // Both slots were just zeroed; neither holds a frame.
            stale: [FULL; 2],
            last_copy: Duration::ZERO,
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
            write_combined = matches!(presenter.window, Window::Module { .. }),
            cached_transitions = allow_cached_transitions,
            "native video DDR presenter armed"
        );
        if allow_cached_transitions {
            super::transition::set_available(true);
        }
        Ok(presenter)
    }

    #[allow(
        clippy::cast_ptr_alignment,
        reason = "base is a page-aligned mmap and both control word offsets are 4-byte aligned"
    )]
    fn write_word(&mut self, offset: usize, value: u32) {
        // SAFETY: offset is one of the two in-bounds control word
        // offsets within the mapped control page; volatile so the store
        // is not elided or reordered by the compiler.
        unsafe {
            self.control.add(offset).cast::<u32>().write_volatile(value);
        }
    }

    /// A slot's first pixel. Both mappings put slot 0 at `pixels`.
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "mmap is page-aligned and compile-time assertions prove aligned slot offsets"
    )]
    fn slot_ptr(&self, slot: usize) -> *mut u32 {
        let offset = if slot == 0 {
            0
        } else {
            BUFFER1_OFFSET - BUFFER0_OFFSET
        };
        // SAFETY: the compile-time asserts prove both slots fit inside
        // the mapped region even in the largest mode; the mapping
        // lives as long as self.
        unsafe { self.pixels.add(offset).cast() }
    }

    fn clear_slot(&mut self, slot: usize) {
        let words = self.mode.frame_bytes() / size_of::<u32>();
        // MiSTer's DDR window is device memory: unaligned halfword stores
        // fault even though normal ARM RAM permits them. musl's optimized
        // memset intentionally uses such stores, so slice.fill(0) cannot
        // clear this mapping. Volatile aligned words avoid both memset
        // lowering and writes being elided across the MMIO boundary.
        let ptr = self.slot_ptr(slot);
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
        if self.wait_native_vblank() {
            return;
        }
        // Render/copy time belongs inside the frame period. The FPGA still
        // latches independently; this changes no buffers, fences, or ABI.
        let remaining = self.pacing.remaining(Instant::now());
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
    }

    fn frame_period(&self) -> Duration {
        self.pacing.period
    }

    fn reports_period(&self) -> bool {
        true
    }

    fn last_copy(&self) -> Duration {
        self.last_copy
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
            // A page slide in flight was composed for the old window.
            if self.cached_transitions_available {
                self.slide.resize(self.buffer.len());
            }
            // A shifted window leaves different strips untouched. Mark
            // both slots for clearing before their next write; never clear
            // the slot the FPGA may currently be scanning.
            self.clear_slots = 0b11;
            self.stale = [FULL; 2];
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

    fn render_and_present(&mut self, renderer: &SoftwareRenderer) -> Duration {
        let busy_start = Instant::now();
        let win_w = self.mode.width as usize - self.inset_h;
        let win_h = self.mode.height as usize - self.inset_v;

        // An interrupted page slide may have left either slot on a partial
        // page. Republish the complete canonical frame, even when Slint
        // itself only dirtied a cursor or a small modal.
        let cancelled = self.cached_transitions_available && super::transition::take_cancelled();
        if cancelled {
            self.slide.abandon();
        }
        // A request arrives before destination properties render. Preserve
        // outgoing pixels first, then let Slint build its canonical endpoint.
        if self.cached_transitions_available {
            self.slide.begin(&self.buffer, win_w, win_h);
        }
        // Render to cached RAM, then convert-copy what changed into the
        // slot. The window lands at (inset_h, inset_v) in the raster; the
        // strip it vacates was blacked at open() and is never touched.
        let region = renderer.render(self.buffer.as_mut_slice(), win_w);
        let origin = region.bounding_box_origin();
        let size = region.bounding_box_size();
        let slint_damage = Damage {
            x0: origin.x.max(0) as u32,
            y0: origin.y.max(0) as u32,
            x1: origin.x.max(0) as u32 + size.width,
            y1: origin.y.max(0) as u32 + size.height,
        };
        let (damage, slide_frame, finished) = self
            .slide
            .compose(&self.buffer, win_w, win_h)
            .map_or((slint_damage, false, false), |(damage, done)| {
                (damage, true, done)
            });
        self.publish(if cancelled { FULL } else { damage }, slide_frame);
        if finished {
            self.slide.finish();
        }

        let busy = busy_start.elapsed();
        if self.pace_in_present {
            self.wait_vsync();
        }
        busy
    }

    fn present_cached_transition(&mut self) -> Option<Duration> {
        if !self.cached_transitions_available {
            return None;
        }
        if super::transition::take_cancelled() {
            self.slide.abandon();
            let start = Instant::now();
            self.publish(FULL, false);
            let busy = start.elapsed();
            if self.pace_in_present {
                self.wait_vsync();
            }
            return Some(busy);
        }
        if !self.slide.is_active() {
            return None;
        }
        let start = Instant::now();
        let win_w = self.mode.width as usize - self.inset_h;
        let win_h = self.mode.height as usize - self.inset_v;
        let (damage, finished) = self.slide.compose(&self.buffer, win_w, win_h)?;
        self.publish(damage, true);
        if finished {
            self.slide.finish();
        }
        let busy = start.elapsed();
        if self.pace_in_present {
            self.wait_vsync();
        }
        Some(busy)
    }
}

impl DdrPresenter {
    /// Brings the next slot up to date and shows it: `damage` plus whatever
    /// that slot missed, taken from the cached slide's frame while one is
    /// showing and from the canonical frame otherwise.
    fn publish(&mut self, damage: Damage, slide_frame: bool) {
        let win_w = self.mode.width as usize - self.inset_h;
        let win_h = self.mode.height as usize - self.inset_v;
        let copy_start = Instant::now();
        let slot = self.active;
        let slot_bit = 1_u8 << slot;
        if self.clear_slots & slot_bit != 0 {
            self.clear_slot(slot);
            self.clear_slots &= !slot_bit;
            self.stale[slot] = FULL;
        }
        let to_copy = take_stale(&mut self.stale, slot, damage);
        let source = if slide_frame {
            self.slide.frame()
        } else {
            &self.buffer
        };
        // SAFETY: both frames hold win_w * win_h pixels and the slot holds
        // the whole raster, which contains the window at its inset.
        unsafe {
            copy_rows(
                source,
                (win_w, win_h),
                self.slot_ptr(slot),
                self.mode.width as usize,
                (self.inset_h, self.inset_v),
                to_copy,
            );
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
        self.last_copy = copy_start.elapsed();
    }

    /// Waits on the raster's own vertical sync where the module offers it.
    /// False means the caller still has to pace this turn itself.
    fn wait_native_vblank(&mut self) -> bool {
        let Window::Module {
            device,
            vblank,
            vblank_misses,
            ..
        } = &mut self.window
        else {
            return false;
        };
        if !*vblank {
            return false;
        }
        let Err(error) = scanout::wait_native_vblank(device) else {
            *vblank_misses = 0;
            return true;
        };
        if error.kind() == std::io::ErrorKind::Interrupted {
            return false;
        }
        *vblank_misses += 1;
        if *vblank_misses >= VBLANK_MISS_LIMIT {
            *vblank = false;
            tracing::warn!(%error, "native vertical sync wait failed; pacing by sleep");
        }
        // A timeout already spent more than a frame period waiting.
        error.raw_os_error() == Some(libc::ETIMEDOUT)
    }
}

/// The module's mappings of the window, under Main's native lease.
fn open_module_window() -> Result<(*mut u8, *mut u8, Window), slint::PlatformError> {
    let lease = super::lease::acquire_native()
        .map_err(|e| slint::PlatformError::Other(format!("native scanout lease: {e}")))?;
    let (device, layout) = scanout::open_device()?;
    let control = Mapping::new(
        &device,
        layout.native_control_offset_bytes,
        layout.native_control_bytes as usize,
        "native control page",
    )?;
    let pixels = Mapping::new(
        &device,
        layout.native_pixels_offset_bytes,
        layout.native_pixels_bytes as usize,
        "native frame slots",
    )?;
    Ok((
        control.ptr(),
        pixels.ptr(),
        Window::Module {
            _control: control,
            _pixels: pixels,
            device,
            _lease: lease,
            vblank: true,
            vblank_misses: 0,
        },
    ))
}

/// The whole window as uncached device memory.
fn open_dev_mem() -> Result<(*mut u8, *mut u8, Window), slint::PlatformError> {
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
    // SAFETY: the slots start one control page into the mapped region.
    let pixels = unsafe { base.add(BUFFER0_OFFSET) };
    Ok((base, pixels, Window::DevMem(DevMem { base })))
}

/// What a slot has to be brought up to date with before it is shown: this
/// frame's damage plus everything drawn since the slot was last filled. The
/// other slot now owes this frame's damage too.
fn take_stale(stale: &mut [Damage; 2], slot: usize, damage: Damage) -> Damage {
    let to_copy = stale[slot].union(damage);
    stale[slot] = Damage::EMPTY;
    stale[1 - slot] = stale[1 - slot].union(damage);
    to_copy
}

/// Converts and stores the damaged part of the window into a slot, one
/// aligned word per pixel. The window is uncached device memory on the
/// fallback mapping, where a narrower or unaligned store faults or costs a
/// bus cycle each, so the stores are volatile to keep them whole.
///
/// # Safety
/// `dst` must be valid for `raster_w` words on every row the window covers
/// once offset by `inset`.
unsafe fn copy_rows(
    src: &[PremultipliedRgbaColor],
    (win_w, win_h): (usize, usize),
    dst: *mut u32,
    raster_w: usize,
    (inset_h, inset_v): (usize, usize),
    damage: Damage,
) {
    let x1 = (damage.x1 as usize).min(win_w);
    let y1 = (damage.y1 as usize).min(win_h);
    let (x0, y0) = (damage.x0 as usize, damage.y0 as usize);
    for y in y0..y1 {
        let row = &src[y * win_w..(y + 1) * win_w];
        let dst_row = (y + inset_v) * raster_w + inset_h;
        for (x, px) in row.iter().enumerate().take(x1).skip(x0) {
            let word = u32::from(px.blue) | u32::from(px.green) << 8 | u32::from(px.red) << 16;
            // SAFETY: (x, y) is inside the window, which the caller
            // guarantees the slot holds at this inset.
            unsafe { dst.add(dst_row + x).write_volatile(word) };
        }
    }
}

impl Drop for DdrPresenter {
    fn drop(&mut self) {
        if self.cached_transitions_available {
            super::transition::set_available(false);
        }
        // word0 = 0 is the "writer stopped" signal (the core reverts
        // to its noise pattern within one frame); word1 zeroed after
        // for tidiness, same order as the C++ cleanup path.
        self.write_word(WORD0_OFFSET, 0);
        self.write_word(WORD1_OFFSET, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(red: u8, green: u8, blue: u8) -> PremultipliedRgbaColor {
        PremultipliedRgbaColor {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    #[test]
    fn only_the_damaged_window_pixels_are_stored_as_bgrx_at_the_inset() {
        // A 4x3 window inset by (1, 2) inside a 6x5 raster.
        let (win_w, win_h, raster_w) = (4_usize, 3_usize, 6_usize);
        let src: Vec<_> = (0..win_w * win_h)
            .map(|i| px(0x10 + i as u8, 0x40 + i as u8, 0x80 + i as u8))
            .collect();
        let mut slot = vec![0xDEAD_BEEF_u32; raster_w * 5];
        let damage = Damage {
            x0: 1,
            y0: 1,
            x1: 3,
            y1: 3,
        };
        // SAFETY: the slot holds the 6x5 raster the inset window fits in.
        unsafe {
            copy_rows(
                &src,
                (win_w, win_h),
                slot.as_mut_ptr(),
                raster_w,
                (1, 2),
                damage,
            );
        }
        for y in 0..5 {
            for x in 0..raster_w {
                let inside = (2..4).contains(&x) && (3..5).contains(&y);
                let got = slot[y * raster_w + x];
                if inside {
                    let i = ((y - 2) * win_w + (x - 1)) as u32;
                    assert_eq!(got, (0x80 + i) | (0x40 + i) << 8 | (0x10 + i) << 16);
                    assert_eq!(got.to_le_bytes()[3], 0);
                } else {
                    assert_eq!(got, 0xDEAD_BEEF);
                }
            }
        }
    }

    #[test]
    fn full_damage_is_clamped_to_the_window() {
        let src = vec![px(1, 2, 3); 6];
        let mut slot = vec![0_u32; 12];
        // SAFETY: a 3x2 window at the origin of a 4x3 raster.
        unsafe { copy_rows(&src, (3, 2), slot.as_mut_ptr(), 4, (0, 0), FULL) };
        let stored = slot.iter().filter(|w| **w == 0x0001_0203).count();
        assert_eq!(stored, 6);
        assert_eq!(slot[3], 0);
        assert!(slot[8..].iter().all(|w| *w == 0));
    }

    #[test]
    fn each_slot_catches_up_on_the_frame_it_missed() {
        let a = Damage {
            x0: 0,
            y0: 0,
            x1: 4,
            y1: 4,
        };
        let b = Damage {
            x0: 10,
            y0: 10,
            x1: 12,
            y1: 12,
        };
        let mut stale = [Damage::EMPTY; 2];
        // Frame 1 goes to slot 1; slot 0 did not get it.
        assert_eq!(take_stale(&mut stale, 1, a), a);
        // Frame 2 goes to slot 0 and has to carry frame 1 as well.
        assert_eq!(take_stale(&mut stale, 0, b), a.union(b));
        // Frame 3 with nothing new still owes slot 1 frame 2.
        assert_eq!(take_stale(&mut stale, 1, Damage::EMPTY), b);
        assert!(take_stale(&mut stale, 0, Damage::EMPTY).is_empty());
    }

    #[test]
    fn crt_deadline_subtracts_work_for_ntsc_and_pal() {
        for micros in [16_667, 20_000] {
            let period = Duration::from_micros(micros);
            let start = Instant::now();
            let work = Duration::from_millis(5);
            let mut pacing = FrameDeadline::new(start, period);
            assert_eq!(pacing.remaining(start + work), period.saturating_sub(work));
            assert_eq!(
                pacing.remaining(start + period + work),
                period.saturating_sub(work)
            );
            assert_eq!(pacing.next, start + period * 3);
        }
    }

    #[test]
    fn missed_crt_deadlines_skip_without_catch_up_bursts() {
        let period = Duration::from_micros(16_667);
        let start = Instant::now();
        let mut pacing = FrameDeadline::new(start, period);
        let work = Duration::from_millis(5);
        assert_eq!(
            pacing.remaining(start + period * 1000 + work),
            period.saturating_sub(work)
        );
        assert_eq!(pacing.next, start + period * 1002);
        assert_eq!(
            pacing.remaining(start + period * 1001 + work),
            period.saturating_sub(work)
        );
        assert_eq!(pacing.remaining(start + period * 1003), Duration::ZERO);
        assert_eq!(pacing.next, start + period * 1004);
    }

    #[test]
    fn sleep_overshoot_does_not_accumulate_into_the_cadence() {
        let period = Duration::from_millis(20);
        let start = Instant::now();
        let mut pacing = FrameDeadline::new(start, period);
        let work = Duration::from_millis(4);
        let overshoot = Duration::from_micros(73);
        let mut now = start;
        for tick in 1..=1000 {
            now += work;
            now += pacing.remaining(now) + overshoot;
            assert_eq!(now, start + period * tick + overshoot);
        }
    }

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
