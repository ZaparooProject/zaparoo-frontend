// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Vblank-latch presenter: guaranteed tear-free page flips on the HDMI
// scaler path. Renders RGB565 into cached RAM, copies the stale
// region into one of two hidden write-combined slots provided by the
// `mister-magik-scanout-slots` kernel module, then posts the slot's
// physical address through the menu core's latch protocol (io_uio
// command 0x57); the FPGA latches the new base at vblank, so scanout
// never observes a partial frame.
//
// Engaged only when Main spawned us with --latch (it gates its own
// FPGA writes for our lifetime - see the Main fork's uio lease). Both
// the module and the latch-capable menu RBF are probed at open();
// any missing piece falls back to the fb0 presenter, loudly.
//
// Written against the module's UAPI header and latch-protocol.json.

use super::fb0::{query_fb, FBIO_WAITFORVSYNC};
use super::uio::Uio;
use super::Presenter;
use crate::frame_transition::Active as ActiveFrameTransition;
use crate::latch_protocol::{
    self, parse_caps, parse_receipt, SetCommand, CMD_CAPS, CMD_RECEIPT, CMD_SET,
    DISPOSITION_ACCEPTED, LIMIT_MAX_HEIGHT, LIMIT_MAX_STRIDE_BYTES, LIMIT_MAX_WIDTH,
};
use slint::platform::software_renderer::{Rgb565Pixel, SoftwareRenderer};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

const DEVICE_PATH: &str = "/dev/mister-magik-scanout-slots";
const SLOT_COUNT: usize = 2;
const ABI_VERSION: u32 = 3;
const MODE_ENABLE: u16 = 0x8000;
/// Scaler filter enable (`route_flt`, the stock fbuf's `FB_FLT` bit):
/// without it the ascal upscales nearest-neighbor and any non-integer
/// scale renders jagged. Set whenever the render size differs from
/// the output raster.
const MODE_FILTER: u16 = 0x4000;
const MODE_FMT_RGB565: u16 = 0x0014;
/// Theme.bg (`#0f0f23`) encoded as RGB565 for the inter-page gap.
const TRANSITION_BACKGROUND: Rgb565Pixel = Rgb565Pixel(0x0864);

// UAPI struct from mister_magik_scanout_slots_uapi.h (ABI v3).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SlotsLayout {
    abi_version: u32,
    slot_count: u32,
    max_width: u32,
    max_height: u32,
    max_stride_bytes: u32,
    slot_capacity_bytes: u32,
    map_bytes: u32,
    flags: u32,
    slots: [[u32; 2]; SLOT_COUNT], // physical_address, mmap_offset_bytes
    reserved: [u32; 4],
}

// _IOR('M', 0x01, struct mister_magik_scanout_slots_layout):
// direction READ (2) << 30 | size << 16 | 'M' << 8 | nr.
const GET_LAYOUT: libc::Ioctl =
    (2 << 30) | ((size_of::<SlotsLayout>() as libc::Ioctl) << 16) | (0x4D << 8) | 0x01;

/// Damage bounding box in buffer pixels, inclusive-exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Damage {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Damage {
    const EMPTY: Self = Self {
        x0: u32::MAX,
        y0: u32::MAX,
        x1: 0,
        y1: 0,
    };
    fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }
    fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

pub struct LatchPresenter {
    uio: Uio,
    /// Keeps the module device open; the mappings live independently.
    _slots_file: File,
    /// One write-combined mapping per slot (see `open_slots`).
    maps: [*mut u8; SLOT_COUNT],
    map_bytes: usize,
    slot_phys: [u32; SLOT_COUNT],
    /// Canonical Slint render target (RGB565, `width` px stride), sized
    /// for the largest geometry; low-res renders use a prefix slice.
    frame: Vec<Rgb565Pixel>,
    /// Presentation-only buffer for cached page motion. Slint never
    /// renders into it, so its destination cache remains coherent.
    transition_frame: Vec<Rgb565Pixel>,
    transition: Option<ActiveFrameTransition<Rgb565Pixel>>,
    cached_transitions_available: bool,
    width: u32,
    height: u32,
    /// Output raster the scaler stretches the posted frame onto.
    out_width: u32,
    out_height: u32,
    /// Dynamic resolution pair: motion res (exact half of output,
    /// integer 2x nearest upscale) and sharpest settled geometry that
    /// fits latch slots. None when output is small enough to stay at
    /// one geometry.
    res_modes: Option<((u32, u32), (u32, u32))>,
    /// Damage accumulated per slot since that slot was last filled.
    stale: [Damage; SLOT_COUNT],
    next_slot: usize,
    sequence: u16,
    /// /dev/fb0 kept open for `FBIO_WAITFORVSYNC` pacing; the menu core
    /// keeps generating video timing regardless of which base the
    /// scaler scans.
    fb0: File,
    vsync_supported: bool,
    post_failures: u64,
    posts: u64,
}

// SAFETY: the mapping is only touched from the render thread that
// owns the presenter; the type is moved, never shared.
unsafe impl Send for LatchPresenter {}

/// Kernel-module half of `open()`: device node, layout ioctl, and one
/// mapping PER SLOT. The module's mmap handler accepts only exact
/// `map_bytes`-sized shared mappings whose file offset selects the
/// slot (slot0 at offset 0, slot1 at its `mmap_offset_bytes`); a
/// single combined mapping does not exist.
fn open_slots() -> Result<(File, SlotsLayout, [*mut u8; SLOT_COUNT]), slint::PlatformError> {
    let err = slint::PlatformError::Other;
    let slots_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(DEVICE_PATH)
        .map_err(|e| err(format!("open {DEVICE_PATH}: {e} (module not loaded?)")))?;
    let mut layout = SlotsLayout::default();
    // SAFETY: GET_LAYOUT fills the layout struct; fd is valid.
    let rc = unsafe { libc::ioctl(slots_file.as_raw_fd(), GET_LAYOUT, &raw mut layout) };
    if rc != 0 {
        return Err(err(format!(
            "scanout-slots GET_LAYOUT failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    if layout.abi_version != ABI_VERSION || layout.slot_count as usize != SLOT_COUNT {
        return Err(err(format!(
            "scanout-slots ABI mismatch: version {} slots {}",
            layout.abi_version, layout.slot_count
        )));
    }
    let mut maps = [std::ptr::null_mut::<u8>(); SLOT_COUNT];
    for (i, map) in maps.iter_mut().enumerate() {
        // SAFETY: mapping one slot exactly as the module's handler
        // requires; write-combine attributes applied module-side.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                layout.map_bytes as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                slots_file.as_raw_fd(),
                libc::off_t::from(layout.slots[i][1]),
            )
        };
        if p == libc::MAP_FAILED {
            return Err(err(format!(
                "mmap scanout slot {i}: {}",
                std::io::Error::last_os_error()
            )));
        }
        *map = p.cast();
    }
    Ok((slots_file, layout, maps))
}

/// Render at the largest clean fraction of the output that fits the
/// latch limits: native when it fits, else 2/3 (1080p -> 1280x720,
/// a gentler 1.5x upscale than the original half-res), else 1/2,
/// else 1/3. Fractions must divide the output exactly so the raster
/// stays integer.
fn geometry_fits(caps: &latch_protocol::Caps, width: u32, height: u32) -> bool {
    let max_w = u32::from(caps.max_width.min(LIMIT_MAX_WIDTH));
    let max_h = u32::from(caps.max_height.min(LIMIT_MAX_HEIGHT));
    let max_stride = u32::from(caps.max_stride_bytes.min(LIMIT_MAX_STRIDE_BYTES));
    width <= max_w && height <= max_h && width * 2 <= max_stride
}

fn choose_geometry(
    caps: &latch_protocol::Caps,
    out_w: u32,
    out_h: u32,
) -> Result<(u32, u32), slint::PlatformError> {
    for (num, den) in [(1_u32, 1_u32), (2, 3), (1, 2), (1, 3)] {
        if (out_w * num).is_multiple_of(den) && (out_h * num).is_multiple_of(den) {
            let (w, h) = (out_w * num / den, out_h * num / den);
            if geometry_fits(caps, w, h) {
                return Ok((w, h));
            }
        }
    }
    Err(slint::PlatformError::Other(format!(
        "no latch-compatible geometry for {out_w}x{out_h} output"
    )))
}

fn dynamic_resolution_pair(
    settled: (u32, u32),
    output: (u32, u32),
) -> Option<((u32, u32), (u32, u32))> {
    let motion = (output.0 / 2, output.1 / 2);
    (output.0.is_multiple_of(2)
        && output.1.is_multiple_of(2)
        && output.0 > 1300
        && motion.0 <= settled.0
        && motion.1 <= settled.1
        && motion != settled)
        .then_some((motion, settled))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionPolicy {
    Adaptive,
    Fixed(u32, u32),
}

fn select_geometry(
    caps: &latch_protocol::Caps,
    output: (u32, u32),
    policy: ResolutionPolicy,
) -> Result<(u32, u32), slint::PlatformError> {
    let sharpest = choose_geometry(caps, output.0, output.1)?;
    match policy {
        ResolutionPolicy::Adaptive => Ok(sharpest),
        ResolutionPolicy::Fixed(width, height)
            if geometry_fits(caps, width, height)
                && width <= output.0
                && height <= output.1
                && u64::from(width) * u64::from(output.1)
                    == u64::from(height) * u64::from(output.0) =>
        {
            Ok((width, height))
        }
        ResolutionPolicy::Fixed(width, height) => Err(slint::PlatformError::Other(format!(
            "fixed latch geometry {width}x{height} is incompatible with {}x{} output",
            output.0, output.1
        ))),
    }
}

impl LatchPresenter {
    #[allow(
        clippy::too_many_lines,
        reason = "initialization follows the module, RTL capability, geometry, and buffer validation sequence"
    )]
    pub fn open(
        resolution_policy: ResolutionPolicy,
        allow_cached_transitions: bool,
    ) -> Result<Self, slint::PlatformError> {
        let err = slint::PlatformError::Other;

        // 1. Kernel module: hidden slot layout + per-slot mappings.
        let (slots_file, layout, maps) = open_slots()?;
        let map_bytes = layout.map_bytes as usize;

        // 2. Menu core: latch caps probe. A stock menu RBF answers
        // garbage that fails the CRC and we fall back to fb0. Retried
        // for a few seconds: we start probing right after exec, but
        // Main's uio abstention only engages at finalize_spawn (tty
        // ready), and until then its poll traffic interleaves with
        // ours and resets the bridge framing mid-transaction.
        let mut uio = Uio::open().map_err(|e| err(format!("uio open: {e}")))?;
        let caps = {
            let mut last_err = String::new();
            let mut found = None;
            for attempt in 0..14 {
                std::thread::sleep(Duration::from_millis(250));
                let mut caps_words = [0_u16; 6];
                match uio.transact(CMD_CAPS, &mut caps_words) {
                    Err(e) => last_err = format!("latch caps probe: {e}"),
                    Ok(()) => match parse_caps(&caps_words) {
                        Ok(caps) => {
                            if attempt > 0 {
                                tracing::info!(attempt, "latch caps answered after retry");
                            }
                            found = Some(caps);
                            break;
                        }
                        Err(e) => last_err = format!("no latch in loaded menu core (caps {e:?})"),
                    },
                }
            }
            found.ok_or_else(|| err(last_err))?
        };
        if caps.version != latch_protocol::PROTOCOL_VERSION {
            return Err(err(format!(
                "latch protocol version {} (need {})",
                caps.version,
                latch_protocol::PROTOCOL_VERSION
            )));
        }

        // 3. Output geometry from the fb the menu core scans today.
        let fb0 = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/fb0")
            .map_err(|e| err(format!("open /dev/fb0: {e}")))?;
        let (var, _fix) = query_fb(fb0.as_raw_fd())?;
        let (width, height) = select_geometry(&caps, (var.xres, var.yres), resolution_policy)?;
        let frame_bytes = width * 2 * height;
        if frame_bytes > layout.slot_capacity_bytes {
            return Err(err(format!(
                "frame {frame_bytes} B exceeds slot capacity {} B",
                layout.slot_capacity_bytes
            )));
        }
        let _ = super::OUTPUT_SIZE.set((var.xres, var.yres));

        // Adaptive mode uses an exact half-output motion raster and
        // returns to the sharpest supported settled geometry.
        let res_modes = match resolution_policy {
            ResolutionPolicy::Adaptive => {
                dynamic_resolution_pair((width, height), (var.xres, var.yres))
            }
            ResolutionPolicy::Fixed(_, _) => None,
        };
        let settled = (width, height);
        let (width, height) = res_modes.map_or(settled, |(low, _)| low);
        let cached_transitions_available = allow_cached_transitions && res_modes.is_none();

        tracing::info!(
            width,
            height,
            output_w = var.xres,
            output_h = var.yres,
            drs = res_modes.is_some(),
            cached_transitions = cached_transitions_available,
            slot0 = format!("{:#010x}", layout.slots[0][0]),
            slot1 = format!("{:#010x}", layout.slots[1][0]),
            caps_flags = caps.flags,
            "latch presenter ready (vblank-latched flips)"
        );

        let frame_capacity = settled.0 * settled.1;
        super::transition::set_available(cached_transitions_available);
        Ok(Self {
            uio,
            _slots_file: slots_file,
            maps,
            map_bytes,
            slot_phys: [layout.slots[0][0], layout.slots[1][0]],
            frame: vec![Rgb565Pixel::default(); frame_capacity as usize],
            transition_frame: vec![Rgb565Pixel::default(); frame_capacity as usize],
            transition: None,
            cached_transitions_available,
            width,
            height,
            out_width: var.xres,
            out_height: var.yres,
            res_modes,
            // Both slots start unwritten: first two frames copy fully.
            stale: [
                Damage {
                    x0: 0,
                    y0: 0,
                    x1: width,
                    y1: height,
                },
                Damage {
                    x0: 0,
                    y0: 0,
                    x1: width,
                    y1: height,
                },
            ],
            next_slot: 0,
            sequence: 0,
            fb0,
            vsync_supported: true,
            post_failures: 0,
            posts: 0,
        })
    }

    #[allow(
        clippy::cast_ptr_alignment,
        reason = "slot offsets are page-aligned and every row offset is a multiple of 2 bytes, the Rgb565Pixel alignment"
    )]
    fn copy_to_slot(&mut self, slot: usize, damage: Damage, transition_frame: bool) {
        if damage.is_empty() {
            return;
        }
        let x0 = damage.x0.min(self.width) as usize;
        let x1 = damage.x1.min(self.width) as usize;
        let y0 = damage.y0.min(self.height) as usize;
        let y1 = damage.y1.min(self.height) as usize;
        let width = self.width as usize;
        let row_px = x1 - x0;
        if row_px == 0 {
            return;
        }
        let source = if transition_frame {
            &self.transition_frame
        } else {
            &self.frame
        };
        for y in y0..y1 {
            let src = &source[y * width + x0..y * width + x1];
            // SAFETY: the slot mapping is map_bytes >= slot capacity
            // >= width*height*2; x/y are clamped above.
            unsafe {
                let dst = self.maps[slot]
                    .add((y * width + x0) * 2)
                    .cast::<Rgb565Pixel>();
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst, row_px);
            }
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

    fn begin_cached_transition(&mut self) {
        let Some(spec) = super::transition::take_request() else {
            return;
        };
        let width = self.width as usize;
        let height = self.height as usize;
        let frame_len = width * height;
        self.transition_frame[..frame_len].copy_from_slice(&self.frame[..frame_len]);
        match ActiveFrameTransition::new(
            &self.frame[..frame_len],
            width,
            height,
            spec,
            TRANSITION_BACKGROUND,
        ) {
            Ok(transition) => {
                tracing::info!(
                    x = spec.rect.x,
                    y = spec.rect.y,
                    width = spec.rect.width,
                    height = spec.rect.height,
                    direction = ?spec.direction,
                    frames = spec.total_frames,
                    "cached page transition started"
                );
                self.transition = Some(transition);
            }
            Err(error) => {
                super::transition::finish();
                tracing::warn!(?error, "cached page transition rejected");
            }
        }
    }

    fn compose_cached_transition(&mut self) -> Option<(Damage, bool)> {
        let width = self.width as usize;
        let height = self.height as usize;
        let frame_len = width * height;
        let result = self.transition.as_mut()?.compose_next(
            &self.frame[..frame_len],
            &mut self.transition_frame[..frame_len],
        );
        match result {
            Ok(step) => Some((
                Damage {
                    x0: step.damage.x as u32,
                    y0: step.damage.y as u32,
                    x1: (step.damage.x + step.damage.width) as u32,
                    y1: (step.damage.y + step.damage.height) as u32,
                },
                step.finished,
            )),
            Err(error) => {
                tracing::warn!(?error, "cached page transition composition failed");
                self.transition_frame[..frame_len].copy_from_slice(&self.frame[..frame_len]);
                Some((self.full_damage(), true))
            }
        }
    }

    fn publish(&mut self, damage: Damage, transition_frame: bool) {
        let slot = self.next_slot;
        // The slot last showed the frame from two presents ago: copy
        // everything that changed since then, not just this frame's
        // damage.
        let to_copy = self.stale[slot].union(damage);
        self.copy_to_slot(slot, to_copy, transition_frame);
        self.stale[slot] = Damage::EMPTY;
        self.stale[1 - slot] = self.stale[1 - slot].union(damage);

        // Order the slot stores ahead of the uio post: the FPGA may
        // start scanning the slot the moment the flip lands.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        self.post(slot);
        self.next_slot = 1 - slot;
    }

    fn finish_cached_transition(&mut self) {
        self.transition = None;
        super::transition::finish();
    }

    fn post(&mut self, slot: usize) {
        self.sequence = self.sequence.wrapping_add(1);
        // Destination is the OUTPUT window the scaler stretches the
        // frame onto (like the stock fbuf's HMIN/HMAX/VMIN/VMAX), not
        // source coordinates: full raster = fullscreen upscale. The
        // scaler filter only helps NON-integer scales; an exact
        // integer ratio wants nearest so every logical pixel stays a
        // crisp NxN block (the DRS motion res is an exact half).
        let scaling = self.width != self.out_width || self.height != self.out_height;
        let integer_scale = self.out_width.is_multiple_of(self.width)
            && self.out_height.is_multiple_of(self.height);
        let filter = scaling && !integer_scale;
        let cmd = SetCommand {
            mode: MODE_ENABLE | MODE_FMT_RGB565 | if filter { MODE_FILTER } else { 0 },
            base: self.slot_phys[slot],
            width: self.width as u16,
            height: self.height as u16,
            destination_left: 0,
            destination_right: (self.out_width - 1) as u16,
            destination_top: 0,
            destination_bottom: (self.out_height - 1) as u16,
            stride: (self.width * 2) as u16,
            sequence: self.sequence,
        };
        let mut words = cmd.words();
        let posted = self.uio.transact(CMD_SET, &mut words);
        self.posts += 1;

        let outcome = posted.map(|()| {
            let mut receipt_words = [0_u16; 11];
            self.uio
                .transact(CMD_RECEIPT, &mut receipt_words)
                .map(|()| parse_receipt(&receipt_words))
        });
        let accepted = matches!(
            &outcome,
            Ok(Ok(Ok(r))) if r.disposition == DISPOSITION_ACCEPTED
        );
        if accepted {
            return;
        }

        self.post_failures += 1;
        // First failure and every 300th (5 s at 60 Hz): enough to
        // diagnose without turning the log into the bottleneck.
        if self.post_failures == 1 || self.post_failures.is_multiple_of(300) {
            match outcome {
                Ok(Ok(Ok(r))) => tracing::warn!(
                    disposition = r.disposition,
                    reason = latch_protocol::reject_reason_name(r.reject_reason),
                    failures = self.post_failures,
                    posts = self.posts,
                    "latch post rejected"
                ),
                Ok(Ok(Err(e))) => tracing::warn!(
                    error = ?e,
                    failures = self.post_failures,
                    "latch receipt unreadable"
                ),
                Ok(Err(e)) | Err(e) => tracing::warn!(
                    error = %e,
                    failures = self.post_failures,
                    "latch post transport error"
                ),
            }
        }
    }
}

impl Presenter for LatchPresenter {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn window_scale_factor(&self) -> f32 {
        self.res_modes
            .map_or(1.0, |(_, high)| self.width as f32 / high.0 as f32)
    }

    fn supports_res_modes(&self) -> bool {
        self.res_modes.is_some()
    }

    fn set_res_mode(&mut self, high: bool) {
        let Some((low, high_res)) = self.res_modes else {
            return;
        };
        let (w, h) = if high { high_res } else { low };
        if (w, h) == (self.width, self.height) {
            return;
        }
        self.width = w;
        self.height = h;
        // Slot contents are the other geometry's pixels now: both
        // need a full rewrite before their next post.
        let full = Damage {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        };
        self.stale = [full, full];
    }

    fn wait_vsync(&mut self) {
        if self.vsync_supported {
            let mut arg: u32 = 0;
            // SAFETY: FBIO_WAITFORVSYNC reads a u32 argument; blocks
            // until the next vertical blank.
            let rc = unsafe { libc::ioctl(self.fb0.as_raw_fd(), FBIO_WAITFORVSYNC, &raw mut arg) };
            if rc == 0 {
                return;
            }
            self.vsync_supported = false;
            tracing::warn!("FBIO_WAITFORVSYNC unsupported; falling back to sleep pacing");
        }
        std::thread::sleep(Duration::from_micros(16_667));
    }

    fn render_and_present(&mut self, renderer: &SoftwareRenderer) -> Duration {
        let start = Instant::now();
        let width = self.width as usize;
        let height = self.height as usize;
        let frame_len = width * height;

        // A request arrives before destination properties render. Preserve
        // outgoing pixels first, then let Slint build its canonical endpoint.
        self.begin_cached_transition();
        let region = renderer.render(&mut self.frame[..frame_len], width);

        let origin = region.bounding_box_origin();
        let size = region.bounding_box_size();
        let slint_damage = Damage {
            x0: origin.x.max(0) as u32,
            y0: origin.y.max(0) as u32,
            x1: origin.x.max(0) as u32 + size.width,
            y1: origin.y.max(0) as u32 + size.height,
        };
        let (damage, transition_frame, finished) = self
            .compose_cached_transition()
            .map_or((slint_damage, false, false), |(damage, done)| {
                (damage, true, done)
            });
        self.publish(damage, transition_frame);
        if finished {
            self.finish_cached_transition();
        }

        let busy = start.elapsed();
        self.wait_vsync();
        busy
    }

    fn present_cached_transition(&mut self) -> Option<Duration> {
        self.transition.as_ref()?;
        let start = Instant::now();
        let (damage, finished) = self.compose_cached_transition()?;
        self.publish(damage, true);
        if finished {
            self.finish_cached_transition();
        }
        let busy = start.elapsed();
        self.wait_vsync();
        Some(busy)
    }
}

impl Drop for LatchPresenter {
    fn drop(&mut self) {
        if self.cached_transitions_available {
            super::transition::set_available(false);
        }
        // All-zero set = route disable (the RTL accepts the disable
        // form only when every field is zero); scanout returns to the
        // stock framebuffer path before Main resumes.
        let mut words = SetCommand {
            mode: 0,
            base: 0,
            width: 0,
            height: 0,
            destination_left: 0,
            destination_right: 0,
            destination_top: 0,
            destination_bottom: 0,
            stride: 0,
            sequence: 0,
        }
        .words();
        if let Err(e) = self.uio.transact(CMD_SET, &mut words) {
            tracing::warn!(error = %e, "latch disable post failed on shutdown");
        }
        for map in self.maps {
            // SAFETY: each map came from mmap(map_bytes) in open_slots().
            unsafe { libc::munmap(map.cast(), self.map_bytes) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dynamic_resolution_pair;

    #[test]
    fn enables_motion_pair_when_1080p_settles_at_720p() {
        assert_eq!(
            dynamic_resolution_pair((1280, 720), (1920, 1080)),
            Some(((960, 540), (1280, 720)))
        );
    }

    #[test]
    fn skips_pair_for_small_or_identical_geometry() {
        assert_eq!(dynamic_resolution_pair((1280, 720), (1280, 720)), None);
        assert_eq!(dynamic_resolution_pair((1280, 720), (2560, 1440)), None);
    }
}
