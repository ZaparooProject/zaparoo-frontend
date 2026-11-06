// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Single-threaded handoff between router page commits and the presenter
//! that owns the screen, and the cached page slide and cover fades every
//! presenter runs from it.

use super::scanout::Damage;
use crate::frame_transition::{Active, Blend, CoverFades, Rect, Spec, Started, BLEND_ONE};
use slint::platform::software_renderer::{
    PhysicalRegion, PremultipliedRgbaColor, RepaintBufferType, Rgb565Pixel, SoftwareRenderer,
    TargetPixel, TargetPixelBuffer,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};

static AVAILABLE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Spec>> = Mutex::new(None);
static FADES_AVAILABLE: AtomicBool = AtomicBool::new(false);
static FADE_REQUESTS: Mutex<Vec<(Rect, std::time::Instant)>> = Mutex::new(Vec::new());

/// A fade request no render has taken by now belongs to a frame that never
/// came; starting it would fade whatever the next frame shows there.
const FADE_REQUEST_TTL: std::time::Duration = std::time::Duration::from_millis(100);

pub(super) fn set_available(available: bool) {
    AVAILABLE.store(available, Ordering::SeqCst);
    if !available {
        *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = None;
        BUSY.store(false, Ordering::SeqCst);
        CANCELLED.store(false, Ordering::SeqCst);
    }
}

pub(super) fn request(spec: Spec) -> bool {
    if !AVAILABLE.load(Ordering::SeqCst)
        || BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return false;
    }
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(spec);
    true
}

pub(super) fn take_request() -> Option<Spec> {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

pub(super) fn finish() {
    BUSY.store(false, Ordering::SeqCst);
}

pub(super) fn cancel() {
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = None;
    CANCELLED.store(true, Ordering::SeqCst);
    BUSY.store(false, Ordering::SeqCst);
}

pub(super) fn take_cancelled() -> bool {
    CANCELLED.swap(false, Ordering::SeqCst)
}

pub(super) fn set_fades_available(available: bool) {
    FADES_AVAILABLE.store(available, Ordering::SeqCst);
    if !available {
        take_fade_requests();
    }
}

/// Asks for the covers that just landed in `rects`, in the presenter's
/// frame, to fade in. False when no presenter runs cover fades or a page
/// slide owns the frame.
pub(super) fn request_fades(rects: &[Rect]) -> bool {
    if !FADES_AVAILABLE.load(Ordering::SeqCst) || BUSY.load(Ordering::SeqCst) {
        return false;
    }
    let now = std::time::Instant::now();
    FADE_REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(rects.iter().map(|&rect| (rect, now)));
    true
}

pub(super) fn take_fade_requests() -> Vec<Rect> {
    std::mem::take(&mut *FADE_REQUESTS.lock().unwrap_or_else(PoisonError::into_inner))
        .into_iter()
        .filter(|(_, at)| at.elapsed() < FADE_REQUEST_TTL)
        .map(|(rect, _)| rect)
        .collect()
}

/// The default scheme's `bg-deep` (`#050608`), shown in the inter-page gap
/// until the frontend publishes the live ground (`set_ground`).
pub(super) const BACKGROUND_RGBA: PremultipliedRgbaColor = PremultipliedRgbaColor {
    red: 0x05,
    green: 0x06,
    blue: 0x08,
    alpha: 0xff,
};

/// What page slides compose over: the scheme's ground, and the prepared
/// background image for each output size that has one. A slide moves only
/// the pixels that differ from it, so the background stays still.
struct Backgrounds {
    generation: u64,
    ground: [u8; 3],
    images: Vec<(usize, usize, Published)>,
    /// Where each output's glass card sits, in its pixels.
    cutouts: Vec<(usize, usize, Cutout)>,
}

/// A glass card's place on an output: its rectangle and corner radius.
/// Inside it the frame is filled from the card-darkness bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cutout {
    pub rect: zaparoo_app::background::Rect,
    pub radius: u32,
}

/// A prepared background as the frontend hands it over: the RGB8 bitmap
/// the view would draw, and, for an RGB565 presenter, the same pixels
/// already converted on the background worker so the render thread never
/// has to.
#[derive(Clone)]
pub(crate) struct Published {
    pub rgb: Arc<Vec<u8>>,
    pub rgb565: Option<Arc<Vec<Rgb565Pixel>>>,
    /// The same image at the glass card's darkness, when cards are glass:
    /// the presenter shows it inside the card's cutout, so the darker area
    /// moves with the card in the same frame.
    pub card: Option<Box<Published>>,
}

/// RGB565 pixels exactly as the latch presenter's render would draw the
/// opaque RGB8 bitmap.
pub(crate) fn to_rgb565(rgb: &[u8]) -> Vec<Rgb565Pixel> {
    rgb.chunks_exact(3)
        .map(|px| <Rgb565Pixel as TargetPixel>::from_rgb(px[0], px[1], px[2]))
        .collect()
}

static BACKGROUNDS: Mutex<Backgrounds> = Mutex::new(Backgrounds {
    generation: 0,
    ground: [0x05, 0x06, 0x08],
    images: Vec::new(),
    cutouts: Vec::new(),
});

fn backgrounds() -> std::sync::MutexGuard<'static, Backgrounds> {
    BACKGROUNDS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The scheme's `bg-deep`, for the gap between pages.
pub(crate) fn set_ground(rgb: [u8; 3]) {
    let mut state = backgrounds();
    if state.ground != rgb {
        state.ground = rgb;
        state.generation += 1;
    }
}

/// The background image an output of `width x height` shows, as the RGB8
/// bitmap Slint draws 1:1, or none.
pub(crate) fn set_background(width: usize, height: usize, image: Option<Published>) {
    let mut state = backgrounds();
    state.images.retain(|(w, h, _)| (*w, *h) != (width, height));
    if let Some(image) = image {
        state.images.push((width, height, image));
    }
    state.generation += 1;
}

/// Where an output of `width x height` shows its glass card, or none.
/// The next render fills from the merged bitmap.
pub(crate) fn set_cutout(width: usize, height: usize, cutout: Option<Cutout>) {
    let mut state = backgrounds();
    let current = state
        .cutouts
        .iter()
        .find(|(w, h, _)| (*w, *h) == (width, height))
        .map(|(_, _, c)| *c);
    if current == cutout {
        return;
    }
    state
        .cutouts
        .retain(|(w, h, _)| (*w, *h) != (width, height));
    if let Some(cutout) = cutout {
        state.cutouts.push((width, height, cutout));
    }
    state.generation += 1;
}

/// A presenter's frame buffer that fills Slint's dirty region from the
/// background bitmap. Slint calls `fill_background` before it draws the
/// items, so they blend onto the real background pixels; the fill itself
/// is a row copy where an `Image` item would cost a per-pixel gather.
pub(super) struct BackdropBuffer<'a, T> {
    pub data: &'a mut [T],
    pub stride: usize,
    /// The bitmap in the same layout as `data`, when one is shown.
    pub background: Option<&'a [T]>,
}

impl<T: TargetPixel> TargetPixelBuffer for BackdropBuffer<'_, T> {
    type TargetPixel = T;

    fn line_slice(&mut self, line_number: usize) -> &mut [T] {
        let start = line_number * self.stride;
        &mut self.data[start..start + self.stride]
    }

    fn num_lines(&self) -> usize {
        self.data.len() / self.stride
    }

    fn fill_background(&mut self, _brush: &slint::Brush, region: &PhysicalRegion) -> bool {
        let Some(background) = self.background else {
            return false;
        };
        // Each rectangle of the region on its own: Slint redraws items only
        // inside them, so filling their bounding box would leave bare
        // background in the gaps between two rectangles.
        let lines = self.num_lines().min(background.len() / self.stride.max(1));
        for (origin, size) in region.iter() {
            let x0 = (origin.x.max(0) as usize).min(self.stride);
            let x1 = (x0 + size.width as usize).min(self.stride);
            let y0 = (origin.y.max(0) as usize).min(lines);
            let y1 = (y0 + size.height as usize).min(lines);
            for y in y0..y1 {
                let row = y * self.stride;
                self.data[row + x0..row + x1].copy_from_slice(&background[row + x0..row + x1]);
            }
        }
        true
    }
}

/// Render a frame into `data` (`stride` pixels a line, the window `height`
/// lines tall), filling the dirty
/// region from the presenter's background bitmap. A bitmap that changed
/// since the last render repaints the whole frame once.
pub(super) fn render_with_backdrop<T: PresenterPixel + TargetPixel>(
    renderer: &SoftwareRenderer,
    slide: &mut CachedSlide<T>,
    data: &mut [T],
    stride: usize,
    height: usize,
) -> PhysicalRegion {
    let (image, changed) = slide.backdrop(stride, height);
    if changed {
        renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
    }
    let region = renderer.render_into_buffer(&mut BackdropBuffer {
        data,
        stride,
        background: image.as_deref().map(Vec::as_slice),
    });
    if changed {
        renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
    }
    region
}

/// Presenter pixels made the way Slint's software renderer draws an opaque
/// RGB8 image, so a background pixel in the canonical frame compares equal
/// to its converted copy.
pub(super) trait PresenterPixel: Blend + Default {
    fn from_rgb8(r: u8, g: u8, b: u8) -> Self;

    /// The bitmap already in this pixel type, when the worker made it.
    fn prepared(_image: &Published) -> Option<Arc<Vec<Self>>> {
        None
    }
}

impl PresenterPixel for Rgb565Pixel {
    fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        <Self as TargetPixel>::from_rgb(r, g, b)
    }

    fn prepared(image: &Published) -> Option<Arc<Vec<Self>>> {
        image.rgb565.clone()
    }
}

impl PresenterPixel for PremultipliedRgbaColor {
    fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        <Self as TargetPixel>::from_rgb(r, g, b)
    }
}

/// A published bitmap in a presenter's pixels, `w x h` laid into a frame
/// `width` wide; columns past `w` take the ground. Unpadded frames take
/// the worker's conversion as is.
fn convert<T: PresenterPixel>(
    image: &Published,
    w: usize,
    h: usize,
    width: usize,
    ground: T,
) -> Arc<Vec<T>> {
    if w == width {
        if let Some(ready) = T::prepared(image).filter(|p| p.len() == w * h) {
            return ready;
        }
    }
    let mut pixels = vec![ground; width * h];
    for y in 0..h {
        let src = &image.rgb[y * w * 3..(y + 1) * w * 3];
        for (dst, px) in pixels[y * width..y * width + w]
            .iter_mut()
            .zip(src.chunks_exact(3))
        {
            *dst = T::from_rgb8(px[0], px[1], px[2]);
        }
    }
    Arc::new(pixels)
}

/// The plain bitmap with the card-darkness bitmap inside the card's
/// rounded outline: a row copy per card row, once per change.
fn merge_card<T: Copy>(
    plain: &[T],
    card: &[T],
    width: usize,
    height: usize,
    cutout: Cutout,
) -> Vec<T> {
    let mut merged = plain.to_vec();
    let rect = cutout.rect;
    let rows = (rect.y as usize).min(height)..((rect.y + rect.height) as usize).min(height);
    for y in rows {
        let inset =
            zaparoo_app::background::corner_inset(&rect, cutout.radius, y as u32 - rect.y) as usize;
        let x0 = (rect.x as usize + inset).min(width);
        let x1 = ((rect.x + rect.width) as usize)
            .saturating_sub(inset)
            .min(width);
        if x1 > x0 {
            let row = y * width;
            merged[row + x0..row + x1].copy_from_slice(&card[row + x0..row + x1]);
        }
    }
    merged
}

/// One presenter's converted copy of the background for its frame.
struct Converted<T> {
    generation: u64,
    width: usize,
    height: usize,
    ground: T,
    image: Option<Arc<Vec<T>>>,
}

/// One presenter's cached page slide. Slint renders the outgoing and the
/// incoming page once each into the presenter's canonical frame; between
/// them the presenter shows this second frame, which only ever holds the
/// two pages composed at the current step.
pub(super) struct CachedSlide<T> {
    /// Presentation-only buffer for cached page motion. Slint never
    /// renders into it, so its destination cache remains coherent.
    frame: Vec<T>,
    active: Option<Active<T>>,
    /// Shown in the gap between the two pages.
    background: T,
    converted: Option<Converted<T>>,
    /// The background generation the last render was filled from, so a
    /// new bitmap repaints the whole frame once.
    filled: u64,
}

impl<T: PresenterPixel> CachedSlide<T> {
    pub(super) fn new(pixels: usize, background: T) -> Self {
        Self {
            frame: vec![T::default(); pixels],
            active: None,
            background,
            converted: None,
            filled: u64::MAX,
        }
    }

    /// The canonical frame changed size. Motion in flight was composed for
    /// the old one, so it is dropped along with any request waiting to
    /// start, and the next ordinary frame republishes everything.
    pub(super) fn resize(&mut self, pixels: usize) {
        if self.active.take().is_some() || take_request().is_some() {
            cancel();
        }
        self.frame = vec![T::default(); pixels];
    }

    pub(super) const fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn frame(&self) -> &[T] {
        &self.frame
    }

    /// Forgets motion in flight without publishing anything.
    pub(super) fn abandon(&mut self) {
        self.active = None;
    }

    /// Starts a requested slide. A request arrives before the destination
    /// properties render, so the caller runs this first to preserve the
    /// outgoing pixels, then lets Slint build its canonical endpoint.
    pub(super) fn begin(&mut self, canonical: &[T], width: usize, height: usize) {
        let Some(spec) = take_request() else {
            return;
        };
        let frame_len = width * height;
        self.frame[..frame_len].copy_from_slice(&canonical[..frame_len]);
        let (ground, image) = self.background_for(width, height);
        match Active::new(&canonical[..frame_len], width, height, spec, ground) {
            Ok(mut transition) => {
                if let Some(image) = image {
                    let matched = transition.set_background(image);
                    tracing::debug!(
                        matched_pct = (matched * 100.0) as u32,
                        "cached page transition over a background image"
                    );
                }
                tracing::info!(
                    x = spec.rect.x,
                    y = spec.rect.y,
                    width = spec.rect.width,
                    height = spec.rect.height,
                    direction = ?spec.direction,
                    frames = spec.total_frames,
                    "cached page transition started"
                );
                self.active = Some(transition);
            }
            Err(error) => {
                finish();
                tracing::warn!(?error, "cached page transition rejected");
            }
        }
    }

    /// Composes the current step into the presentation frame. Returns what
    /// it changed and whether that was the last step, or None when no slide
    /// is active. The last step is the whole canonical frame.
    pub(super) fn compose(
        &mut self,
        canonical: &[T],
        width: usize,
        height: usize,
    ) -> Option<(Damage, bool)> {
        let active = self.active.as_mut()?;
        let frame_len = width * height;
        // The destination is final once Slint has rendered it.
        active.ensure_destination_spans(&canonical[..frame_len]);
        let result = active.compose_now(
            &canonical[..frame_len],
            &mut self.frame[..frame_len],
            super::platform::timeline().unwrap_or_default(),
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
                self.frame[..frame_len].copy_from_slice(&canonical[..frame_len]);
                Some((
                    Damage {
                        x0: 0,
                        y0: 0,
                        x1: width as u32,
                        y1: height as u32,
                    },
                    true,
                ))
            }
        }
    }

    /// The gap color and the background image for a `width x height`
    /// frame, converted once per published change. An image is used only
    /// when it is this frame's size; a rotated or resized frame slides as
    /// before, over the ground.
    /// The background bitmap a `width x height` frame is filled from, and
    /// whether it changed since the last render, which then has to repaint
    /// the whole frame: Slint only redraws what it knows is dirty.
    pub(super) fn backdrop(&mut self, width: usize, height: usize) -> (Option<Arc<Vec<T>>>, bool) {
        let (_, image) = self.background_for(width, height);
        let generation = self.converted.as_ref().map_or(0, |c| c.generation);
        let changed = self.filled != generation;
        self.filled = generation;
        (image, changed)
    }

    fn background_for(&mut self, width: usize, height: usize) -> (T, Option<Arc<Vec<T>>>) {
        let state = backgrounds();
        let fresh = self.converted.as_ref().is_some_and(|c| {
            c.generation == state.generation && c.width == width && c.height == height
        });
        if !fresh {
            let [r, g, b] = state.ground;
            let ground = T::from_rgb8(r, g, b);
            let cutout = state
                .cutouts
                .iter()
                .find(|(w, h, _)| (*w, *h) == (width, height))
                .map(|(_, _, c)| *c);
            let image = state
                .images
                .iter()
                .find(|(w, h, image)| *w <= width && *h == height && image.rgb.len() == w * h * 3)
                .map(|(w, h, image)| {
                    let plain = convert(image, *w, *h, width, ground);
                    let card = image
                        .card
                        .as_deref()
                        .filter(|card| card.rgb.len() == w * h * 3);
                    match (card, cutout) {
                        (Some(card), Some(cutout)) => {
                            let card = convert(card, *w, *h, width, ground);
                            Arc::new(merge_card(&plain, &card, width, height, cutout))
                        }
                        _ => plain,
                    }
                });
            self.converted = Some(Converted {
                generation: state.generation,
                width,
                height,
                ground,
                image,
            });
        }
        drop(state);
        self.converted
            .as_ref()
            .map_or((self.background, None), |c| (c.ground, c.image.clone()))
    }

    /// The last step has been shown.
    pub(super) fn finish(&mut self) {
        self.active = None;
        finish();
    }
}

impl Blend for Rgb565Pixel {
    fn same(self, other: Self) -> bool {
        self == other
    }

    fn same_run(left: &[Self], right: &[Self]) -> bool {
        left == right
    }

    fn mix(old: Self, new: Self, weight: u32) -> Self {
        // Green moves to the high half so each channel has room for the
        // product: one multiply mixes all three.
        const SPREAD: u32 = 0x07E0_F81F;
        let spread = |pixel: Self| (u32::from(pixel.0) | u32::from(pixel.0) << 16) & SPREAD;
        let mixed =
            ((spread(old) * (BLEND_ONE - weight) + spread(new) * weight) / BLEND_ONE) & SPREAD;
        Self((mixed | mixed >> 16) as u16)
    }
}

impl Blend for PremultipliedRgbaColor {
    fn same(self, other: Self) -> bool {
        (self.red, self.green, self.blue, self.alpha)
            == (other.red, other.green, other.blue, other.alpha)
    }

    fn mix(old: Self, new: Self, weight: u32) -> Self {
        let channel = |old: u8, new: u8| {
            ((u32::from(old) * (BLEND_ONE - weight) + u32::from(new) * weight) / BLEND_ONE) as u8
        };
        Self {
            red: channel(old.red, new.red),
            green: channel(old.green, new.green),
            blue: channel(old.blue, new.blue),
            alpha: channel(old.alpha, new.alpha),
        }
    }
}

/// One presenter's cover fades. Unlike a page slide they never take the
/// frame over: Slint goes on rendering everything else, and the presenter
/// reads each row it publishes through `row`.
pub(super) struct PresenterFades<T> {
    fades: CoverFades<T>,
    available: bool,
}

impl<T: Blend> PresenterFades<T> {
    /// `available` is whether this presenter owns the screen the drivers
    /// lay their grids out on. It answers fade requests for as long as it
    /// lives.
    pub(super) fn new(available: bool) -> Self {
        if available {
            set_fades_available(true);
        }
        Self {
            fades: CoverFades::new(),
            available,
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.fades.is_active()
    }

    /// Drops every fade, for a caller about to republish the whole frame.
    pub(super) fn clear(&mut self) {
        self.fades.clear();
    }

    /// Before Slint renders: keeps what is under each cover about to land.
    /// A page slide owns the frame while it runs, so `sliding` ends the
    /// fades rather than starting more.
    pub(super) fn before_render(
        &mut self,
        canonical: &[T],
        width: usize,
        height: usize,
        sliding: bool,
    ) {
        if !self.available {
            return;
        }
        let requests = take_fade_requests();
        if sliding {
            self.fades.clear();
        } else if !requests.is_empty() {
            let started = super::platform::timeline().map_or_else(
                || Started::Wall(std::time::Instant::now()),
                Started::Timeline,
            );
            self.fades
                .begin(canonical, width, height, &requests, started);
        }
    }

    /// After Slint renders `region` into `canonical`.
    pub(super) fn rendered(&mut self, canonical: &[T], region: &PhysicalRegion) {
        if !self.fades.is_active() {
            return;
        }
        let damage: Vec<Rect> = region
            .iter()
            .map(|(origin, size)| Rect {
                x: origin.x.max(0) as usize,
                y: origin.y.max(0) as usize,
                width: size.width as usize,
                height: size.height as usize,
            })
            .collect();
        self.fades.rendered(canonical, &damage);
    }

    /// Advances every fade to this frame and returns the regions the
    /// display has to be brought up to date with.
    pub(super) fn step(&mut self) -> &[Rect] {
        self.fades
            .step(super::platform::timeline().unwrap_or_default())
    }

    /// See `CoverFades::mixed_segments`.
    pub(super) fn mixed_segments(
        &mut self,
        y: usize,
        x0: usize,
        x1: usize,
        row: &[T],
        out: impl FnMut(usize, &[T]),
    ) {
        self.fades.mixed_segments(y, x0, x1, row, out);
    }

    /// See `CoverFades::row`.
    pub(super) fn row<'a>(&'a mut self, y: usize, x0: usize, canonical: &'a [T]) -> &'a [T] {
        self.fades.row(y, x0, canonical)
    }
}

impl<T> Drop for PresenterFades<T> {
    fn drop(&mut self) {
        if self.available {
            set_fades_available(false);
        }
    }
}

/// `rects` as one box, for the presenters that bring a slot up to date by
/// box.
pub(super) fn bounds(rects: &[Rect]) -> Damage {
    rects.iter().fold(Damage::EMPTY, |damage, rect| {
        damage.union(Damage {
            x0: rect.x as u32,
            y0: rect.y as u32,
            x1: (rect.x + rect.width) as u32,
            y1: (rect.y + rect.height) as u32,
        })
    })
}

#[cfg(test)]
impl PresenterPixel for u8 {
    fn from_rgb8(r: u8, _: u8, _: u8) -> Self {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_transition::Direction;
    use std::time::{Duration, Instant};

    #[test]
    fn rgb565_mixes_each_channel_on_its_own() {
        // All red against all green and blue.
        let old = Rgb565Pixel(0xF800);
        let new = Rgb565Pixel(0x07FF);
        assert_eq!(Rgb565Pixel::mix(old, new, 0), old);
        assert_eq!(Rgb565Pixel::mix(old, new, BLEND_ONE), new);
        let half = Rgb565Pixel::mix(old, new, BLEND_ONE / 2).0;
        assert_eq!((half >> 11, (half >> 5) & 0x3f, half & 0x1f), (15, 31, 15));
        let white = Rgb565Pixel(0xffff);
        for weight in 0..=BLEND_ONE {
            assert_eq!(
                Rgb565Pixel::mix(white, white, weight),
                white,
                "no channel bleeds"
            );
        }
    }

    #[test]
    fn cancellation_discards_obsolete_request_and_allows_immediate_retarget() {
        set_available(true);
        let spec = Spec {
            rect: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            gap: 1,
            direction: Direction::Up,
            total_frames: 14,
            started: Started::Wall(Instant::now()),
        };
        assert!(request(spec));
        assert!(!request(spec), "never queue a second animation");
        cancel();
        assert_eq!(take_request(), None);
        assert!(request(spec), "new input does not wait for an old deadline");
        assert!(take_cancelled());
        assert!(!take_cancelled());
        assert_eq!(take_request(), Some(spec));
        finish();
        set_available(false);
    }

    fn band(started: Started) -> Spec {
        Spec {
            rect: Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            gap: 0,
            direction: Direction::Up,
            total_frames: 4,
            started,
        }
    }

    #[test]
    fn a_slide_presents_its_own_frame_and_only_the_band_changes() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let mut canonical: Vec<u8> = (0..16).collect();
        // Nothing was requested, so nothing begins.
        slide.begin(&canonical, 4, 4);
        assert!(!slide.is_active());
        assert_eq!(slide.compose(&canonical, 4, 4), None);

        assert!(request(band(Started::Wall(Instant::now()))));
        slide.begin(&canonical, 4, 4);
        assert!(slide.is_active());
        assert_eq!(take_request(), None, "the request was consumed");
        // Slint renders the destination, and dirties more than the band.
        for pixel in &mut canonical {
            *pixel += 100;
        }
        assert_eq!(
            slide.compose(&canonical, 4, 4),
            Some((
                Damage {
                    x0: 1,
                    y0: 1,
                    x1: 3,
                    y1: 3
                },
                false
            ))
        );
        // Outside the band the presented frame is still the outgoing page.
        for index in [0, 3, 4, 7, 8, 11, 12, 15] {
            assert_eq!(slide.frame()[index], index as u8);
        }
        assert!(
            !request(band(Started::Wall(Instant::now()))),
            "one slide at a time"
        );
        slide.abandon();
        assert!(!slide.is_active());
        set_available(false);
    }

    #[test]
    fn the_last_step_is_the_whole_canonical_frame_and_frees_the_channel() {
        set_available(true);
        let started = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let mut canonical: Vec<u8> = (0..16).collect();
        assert!(request(band(Started::Wall(started))));
        slide.begin(&canonical, 4, 4);
        for pixel in &mut canonical {
            *pixel += 100;
        }
        assert_eq!(
            slide.compose(&canonical, 4, 4),
            Some((
                Damage {
                    x0: 0,
                    y0: 0,
                    x1: 4,
                    y1: 4
                },
                true
            ))
        );
        assert_eq!(slide.frame(), canonical.as_slice());
        slide.finish();
        assert!(!slide.is_active());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }

    #[test]
    fn a_resize_drops_motion_in_flight_and_asks_for_a_full_republish() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let canonical: Vec<u8> = (0..16).collect();
        // Nothing in flight: a resize is only a resize.
        slide.resize(12);
        assert_eq!(slide.frame().len(), 12);
        assert!(!take_cancelled());

        slide.resize(16);
        assert!(request(band(Started::Wall(Instant::now()))));
        slide.begin(&canonical, 4, 4);
        slide.resize(9);
        assert!(!slide.is_active());
        assert_eq!(slide.frame().len(), 9);
        assert!(take_cancelled(), "the next frame republishes everything");
        // The band was solved for the old frame: it must not start in the
        // new one, and the channel is free for the next request.
        assert!(request(band(Started::Wall(Instant::now()))));
        slide.resize(16);
        assert_eq!(take_request(), None);
        assert!(take_cancelled());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }

    #[test]
    fn a_band_that_does_not_fit_the_frame_is_rejected_and_frees_the_channel() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let canonical: Vec<u8> = (0..16).collect();
        assert!(request(band(Started::Wall(Instant::now()))));
        // A 2x2 frame cannot hold a band that ends at column and row 3.
        slide.begin(&canonical, 2, 2);
        assert!(!slide.is_active());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }

    #[test]
    fn the_card_fills_its_rounded_outline_from_the_card_bitmap() {
        let (width, height) = (12, 12);
        let plain = vec![1_u8; width * height];
        let card = vec![2_u8; width * height];
        let cutout = Cutout {
            rect: zaparoo_app::background::Rect {
                x: 2,
                y: 2,
                width: 8,
                height: 8,
            },
            radius: 3,
        };
        let merged = merge_card(&plain, &card, width, height, cutout);
        let at = |x: usize, y: usize| merged[y * width + x];
        // Outside the card, and the card's very corners, stay plain.
        assert_eq!(at(0, 0), 1);
        assert_eq!(at(11, 6), 1);
        assert_eq!(at(2, 2), 1);
        assert_eq!(at(9, 9), 1);
        // The card's middle and the middle of each edge are the card's.
        assert_eq!(at(6, 6), 2);
        assert_eq!(at(6, 2), 2);
        assert_eq!(at(2, 6), 2);
        assert_eq!(at(9, 6), 2);
    }
}
