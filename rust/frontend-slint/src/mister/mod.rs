// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// MiSTer target: custom `slint::platform` with the software renderer,
// a fixed-60 animation clock, raw evdev keyboard input (MiSTer
// forwards controller buttons as keyboard keys), and a `/dev/fb0`
// wait-vsync + dirty-copy presenter.
//
// Written against Slint's public platform API and the Linux fbdev /
// evdev UAPI. This is the Phase 2 presenter; the DDR contract v2
// writer (CRT) and the vblank-latch scanout path (HDMI) plug into the
// same `Presenter` seam in later phases.

mod ddr;
mod fb0;
mod fb_mapping;
mod input;
mod latch;
pub mod lease;
mod platform;
mod service;
mod transition;
mod tty;
mod uio;
pub mod video_mode;

pub use platform::{install_platform, set_orientation, ResolutionPolicy};
pub use service::ensure_core_running;

const BROWSE_TRANSITION_FRAMES: u32 = 15;
const ROUTE_TRANSITION_FRAMES: u32 = 10;

pub fn request_page_transition(
    geometry: crate::sizing::BrowseGridTransitionGeometry,
    direction: i32,
) -> bool {
    let direction = if direction > 0 {
        crate::frame_transition::Direction::Up
    } else {
        crate::frame_transition::Direction::Down
    };
    transition::request(crate::frame_transition::Spec {
        rect: crate::frame_transition::Rect {
            x: geometry.x as usize,
            y: geometry.y as usize,
            width: geometry.width as usize,
            height: geometry.height as usize,
        },
        gap: geometry.gap as usize,
        direction,
        total_frames: BROWSE_TRANSITION_FRAMES,
    })
}

pub fn request_route_transition(
    geometry: crate::sizing::RouteTransitionGeometry,
    direction: i32,
) -> bool {
    let direction = if direction > 0 {
        crate::frame_transition::Direction::Left
    } else {
        crate::frame_transition::Direction::Right
    };
    transition::request(crate::frame_transition::Spec {
        rect: crate::frame_transition::Rect {
            x: geometry.x as usize,
            y: geometry.y as usize,
            width: geometry.width as usize,
            height: geometry.height as usize,
        },
        gap: 0,
        direction,
        total_frames: ROUTE_TRANSITION_FRAMES,
    })
}

/// Direct-video CRT bypasses vmode: Main's command loop is unavailable
/// while the alt launcher owns analog video. Dual-head CRT travels via DDR.
pub fn prepare_crt_mode((width, height): (u32, u32)) {
    const FB_MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
    let mode = format!("8888 1 {width} {height} {}", width * 4);
    match std::fs::read_to_string(FB_MODE_PATH) {
        Ok(current) if current.trim() == mode => {}
        Ok(_) => {
            if let Err(e) = std::fs::write(FB_MODE_PATH, format!("{mode}\n")) {
                tracing::warn!("could not set CRT fb mode via {FB_MODE_PATH}: {e}");
            }
        }
        Err(e) => tracing::warn!("could not inspect {FB_MODE_PATH}: {e}"),
    }
}

pub fn set_crt_offsets(h_offset: i32, v_offset: i32) {
    ddr::set_requested_offsets(h_offset, v_offset);
}

/// Physical output raster, set by the presenter that owns the screen.
/// Under dynamic resolution the Slint window size is NOT the output
/// size; grid-shape solving and cover tiers must use this instead so
/// they stay stable across res switches. None on desktop builds.
pub static OUTPUT_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();

/// Where the rendered frame goes. fb0 is the stock-MiSTer HDMI path;
/// the DDR presenter drives the Menu fork's native CRT contract; the
/// vblank-latch scanout path (Phase 4) joins them later.
pub trait Presenter {
    fn size(&self) -> (u32, u32);
    /// Block until the pacing boundary (vsync or a frame-period sleep).
    fn wait_vsync(&mut self);
    /// Render the dirty scene through `renderer` and publish a frame.
    /// Returns the busy time (render + copy + publish, excluding any
    /// vsync wait) for the frame profiler.
    fn render_and_present(
        &mut self,
        renderer: &slint::platform::software_renderer::SoftwareRenderer,
    ) -> std::time::Duration;
    /// Publish one cached transition frame without asking Slint to
    /// traverse the component tree. None means no transition is active.
    fn present_cached_transition(&mut self) -> Option<std::time::Duration> {
        None
    }
    /// Scale physical render pixels into stable logical UI space.
    /// DRS presenters vary this with their backing geometry so a
    /// fidelity switch never causes responsive layout reflow.
    fn window_scale_factor(&self) -> f32 {
        1.0
    }
    /// Dynamic resolution scaling: true when the presenter offers a
    /// motion (low) / idle-sharp (high) geometry pair. The platform
    /// loop owns the switching policy.
    fn supports_res_modes(&self) -> bool {
        false
    }
    /// Switch between the low/high geometry. `size()` reflects the
    /// change; the caller resizes the Slint window to match.
    fn set_res_mode(&mut self, _high: bool) {}
    /// Apply pending presenter controls before timers/rendering. A size
    /// return asks the platform to resize the Slint scene immediately.
    fn sync_controls(&mut self) -> Option<(u32, u32)> {
        None
    }
}
