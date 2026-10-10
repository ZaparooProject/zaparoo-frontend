// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// MiSTer target: custom `slint::platform` with the software renderer,
// an animation clock stepped one refresh period per presented frame, raw
// evdev keyboard input (MiSTer forwards controller buttons as keyboard
// keys), and a `/dev/fb0` wait-vsync + dirty-copy presenter.
//
// Written against Slint's public platform API and the Linux fbdev /
// evdev UAPI. Three presenters share the `Presenter` seam: fb0 for
// stock-MiSTer HDMI, the DDR contract v2 writer for the Menu fork's
// native CRT path, and the vblank-latch scanout path.

mod ddr;
mod fb0;
mod fb_mapping;
mod input;
mod latch;
pub mod lease;
mod platform;
mod scanout;
mod service;
mod transition;
mod tty;
mod uio;
pub mod video_mode;

pub use platform::{install_platform, set_orientation, ResolutionPolicy};
pub use service::ensure_core_running;
pub use tty::TtyGuard;

const BROWSE_TRANSITION_FRAMES: u32 = 15;

pub fn cancel_page_transition() {
    transition::cancel();
}

#[cfg(test)]
pub(crate) fn with_cached_page_transitions(test: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            transition::set_available(false);
        }
    }
    let _reset = Reset;
    transition::set_available(true);
    test();
}

/// Asks the presenter that owns the screen for a cached slide of a browse
/// grid's band: the scene's full width, `grid_height` tall from `grid_y`.
/// False when no presenter can take it; the caller keeps its own fallback.
pub fn request_browse_page_transition(
    app: &crate::App,
    grid_y: f32,
    grid_height: f32,
    direction: i32,
) -> bool {
    use slint::ComponentHandle;
    let sizing = app.global::<crate::Sizing>();
    let scene = (
        sizing.get_screen_width().round().max(0.0) as u32,
        sizing.get_screen_height().round().max(0.0) as u32,
    );
    let Some(geometry) = crate::sizing::mister_browse_grid_transition_geometry(
        scene.0,
        scene.1,
        grid_y.round().max(0.0) as u32,
        grid_height.round().max(0.0) as u32,
    ) else {
        return false;
    };
    let window = app.window().size();
    request_page_transition(
        geometry,
        direction,
        app.global::<crate::Shell>().get_orientation(),
        scene,
        (window.width, window.height),
    )
}

/// `geometry` and `direction` are in the scene's own coordinates. The scene
/// sits centered in the window (a CRT scene is inset from it), and the
/// presenters render that window turned by `orientation`, so the band and
/// the way it travels are mapped into their frame here.
pub fn request_page_transition(
    geometry: crate::sizing::BrowseGridTransitionGeometry,
    direction: i32,
    orientation: crate::Orientation,
    scene: (u32, u32),
    window: (u32, u32),
) -> bool {
    use crate::frame_transition::{to_frame, Direction, Rotation, Spec, Started};
    let rotation = match orientation {
        crate::Orientation::Horizontal => Rotation::None,
        crate::Orientation::Cw => Rotation::Cw,
        crate::Orientation::Ccw => Rotation::Ccw,
    };
    let Some(rect) = window_rect(geometry, scene, window) else {
        return false;
    };
    let Some((rect, direction)) = to_frame(
        rect,
        if direction > 0 {
            Direction::Up
        } else {
            Direction::Down
        },
        rotation,
        (window.0 as usize, window.1 as usize),
    ) else {
        return false;
    };
    transition::request(Spec {
        rect,
        gap: geometry.gap as usize,
        direction,
        total_frames: BROWSE_TRANSITION_FRAMES,
        // Counted on the clock Slint's own motion is sampled on.
        started: platform::timeline().map_or_else(
            || Started::Wall(std::time::Instant::now()),
            Started::Timeline,
        ),
    })
}

/// A scene region in window pixels. None when the scene cannot sit centered
/// on whole pixels inside the window.
fn window_rect(
    geometry: crate::sizing::BrowseGridTransitionGeometry,
    scene: (u32, u32),
    window: (u32, u32),
) -> Option<crate::frame_transition::Rect> {
    let inset = |outer: u32, inner: u32| {
        let spare = outer.checked_sub(inner)?;
        spare.is_multiple_of(2).then_some(spare / 2)
    };
    Some(crate::frame_transition::Rect {
        x: (inset(window.0, scene.0)? + geometry.x) as usize,
        y: (inset(window.1, scene.1)? + geometry.y) as usize,
        width: geometry.width as usize,
        height: geometry.height as usize,
    })
}

/// The pending request, for a test to play the presenter's part.
#[cfg(test)]
pub(crate) fn take_page_transition_request() -> Option<crate::frame_transition::Spec> {
    transition::take_request()
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

/// Where the rendered frame goes. fb0 is the stock-MiSTer HDMI path,
/// the DDR presenter drives the Menu fork's native CRT contract, and
/// the vblank-latch presenter drives the fork's scanout slots.
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
    /// One refresh of the output this presenter paces to: the budget a
    /// frame's work is profiled against.
    fn frame_period(&self) -> std::time::Duration {
        std::time::Duration::from_micros(16_667)
    }
    /// True when `frame_period` is the output's real rate. Otherwise the
    /// platform measures the rate from the waits for vertical blank.
    fn reports_period(&self) -> bool {
        false
    }
    /// How much of the last frame's busy time went on copying it out to
    /// the display's memory. Zero where the presenter does not time it.
    fn last_copy(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_transition::Rect;
    use crate::sizing::BrowseGridTransitionGeometry;

    #[test]
    fn a_scene_band_lands_where_the_scene_sits_in_the_window() {
        let band = BrowseGridTransitionGeometry {
            x: 0,
            y: 40,
            width: 316,
            height: 150,
            gap: 0,
        };
        // HDMI: the scene is the window.
        assert_eq!(
            window_rect(band, (316, 216), (316, 216)),
            Some(Rect {
                x: 0,
                y: 40,
                width: 316,
                height: 150
            })
        );
        // CRT: a 352x240 window around a scene inset 18 and 12 a side.
        assert_eq!(
            window_rect(band, (316, 216), (352, 240)),
            Some(Rect {
                x: 18,
                y: 52,
                width: 316,
                height: 150
            })
        );
        // A scene larger than its window, or off the pixel grid, is refused.
        assert_eq!(window_rect(band, (316, 216), (300, 240)), None);
        assert_eq!(window_rect(band, (316, 216), (353, 240)), None);
    }
}
