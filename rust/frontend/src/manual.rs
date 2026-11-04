//! A game's manual, read as a page of the Game Info panel. Core streams the
//! PDF through `media.asset`; a worker thread owns the parsed document and
//! renders one page at a time at the size the panel shows it. The file and
//! its pages live in process memory only, and only while the page is open.

use crate::router::{lock, Ctx};
use crate::{App, GameInfoView, ManualFit, ManualState};
use slint::ComponentHandle;
use std::collections::VecDeque;
use std::sync::mpsc;
use zaparoo_app::manual::{self as rules, Chunk, Document, Download, Fit, Reader, Step};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{MediaAssetParams, MEDIA_ASSET_MAX_CHUNK};

use zaparoo_app::manual::PAGE_CACHE_BYTES;

/// Which game's documents Game Info is showing.
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub media_id: Option<i64>,
    pub system: String,
    pub path: String,
}

#[derive(Debug, Default)]
pub struct ManualModel {
    ticket: u64,
    source: Option<Source>,
    documents: Vec<Document>,
    open: bool,
    reader: Option<Reader>,
    /// Each page's size in PDF units, as the worker read it.
    page_sizes: Vec<(f32, f32)>,
    worker: Option<mpsc::Sender<Job>>,
    /// The whole-page box in pixels, as the view last reported it, and the
    /// room the arrows take from each side of it at fit width.
    page_box: (u32, u32),
    arrow_room: u32,
    pages: VecDeque<Rendered>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wanted {
    page: usize,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct Rendered {
    wanted: Wanted,
    buffer: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
}

#[derive(Debug)]
struct Job {
    ticket: u64,
    wanted: Wanted,
    /// A page turn may want it next; rendered after anything shown.
    prefetch: bool,
}

impl ManualModel {
    /// A new game in Game Info: forget the last one's documents and close
    /// any manual, ending its worker.
    pub fn reset(&mut self, source: Option<Source>, documents: Vec<Document>) {
        self.close();
        self.source = source;
        self.documents = documents;
    }

    /// The reader's whole box and the arrows' room, which the image viewer
    /// shares.
    pub fn page_box(&self) -> (u32, u32, u32) {
        (self.page_box.0, self.page_box.1, self.arrow_room)
    }

    pub fn has_documents(&self) -> bool {
        !self.documents.is_empty()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn close(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.open = false;
        self.reader = None;
        self.page_sizes.clear();
        // Dropping the sender ends the worker once its current page is done.
        self.worker = None;
        self.pages.clear();
    }

    fn wanted(&self, page: usize) -> Option<Wanted> {
        let reader = self.reader?;
        let (page_w, page_h) = *self.page_sizes.get(page)?;
        let (box_w, box_h) = match reader.fit {
            Fit::Page => self.page_box,
            Fit::Width => (
                self.page_box.0.saturating_sub(2 * self.arrow_room),
                self.page_box.1.saturating_sub(2 * self.arrow_room),
            ),
        };
        let (width, height) = rules::render_size(page_w, page_h, box_w, box_h, reader.fit)?;
        Some(Wanted {
            page,
            width,
            height,
        })
    }

    fn cached(&self, wanted: Wanted) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
        self.pages
            .iter()
            .find(|rendered| rendered.wanted == wanted)
            .map(|rendered| rendered.buffer.clone())
    }

    fn remember(&mut self, wanted: Wanted, buffer: slint::SharedPixelBuffer<slint::Rgba8Pixel>) {
        self.pages.retain(|rendered| rendered.wanted != wanted);
        self.pages.push_back(Rendered { wanted, buffer });
        let bytes = |pages: &VecDeque<Rendered>| {
            pages
                .iter()
                .map(|rendered| rendered.buffer.as_bytes().len())
                .sum::<usize>()
        };
        while self.pages.len() > 1 && bytes(&self.pages) > PAGE_CACHE_BYTES {
            self.pages.pop_front();
        }
    }
}

/// The page box: pages are rendered to it, not scaled down from a bigger one.
pub fn bind(ctx: &std::sync::Arc<Ctx>, app: &App) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.global::<GameInfoView>()
        .on_manual_box(move |width, height, room| {
            // A turn later, as `art-box` is: the view reports while it lays out.
            let ctx = ctx.clone();
            let weak = weak.clone();
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                let Some(app) = weak.upgrade() else { return };
                let page_box = (
                    u32::try_from(width).unwrap_or(0),
                    u32::try_from(height).unwrap_or(0),
                );
                let room = u32::try_from(room).unwrap_or(0);
                let open = {
                    let mut shared = lock(&ctx.shared);
                    let model = &mut shared.game_info.manual;
                    if model.page_box == page_box && model.arrow_room == room {
                        return;
                    }
                    model.page_box = page_box;
                    model.arrow_room = room;
                    model.open
                };
                if open {
                    show_page(&ctx, &app);
                }
                crate::game_info::viewer_box_changed(&ctx, &app);
            });
        });
}

/// Game Info's metadata arrived: say whether the game has a manual.
pub fn set_documents(ctx: &Ctx, app: &App, source: Source, documents: Vec<Document>) {
    let has = !documents.is_empty();
    lock(&ctx.shared)
        .game_info
        .manual
        .reset(Some(source), documents);
    app.global::<GameInfoView>().set_has_manual(has);
    publish_closed(app);
}

/// Game Info closed or moved to another game.
pub fn clear(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).game_info.manual.reset(None, Vec::new());
    let view = app.global::<GameInfoView>();
    view.set_has_manual(false);
    publish_closed(app);
}

fn publish_closed(app: &App) {
    let view = app.global::<GameInfoView>();
    view.set_manual_state(ManualState::Closed);
    view.set_manual_image(slint::Image::default());
    view.set_manual_has_image(false);
    view.set_manual_page(0);
    view.set_manual_pages(0);
    view.set_manual_fit(ManualFit::Page);
    view.set_manual_scroll(0.0);
}

/// Opens the game's document. Only manuals exist, so it opens directly.
pub fn open(ctx: &Ctx, app: &App) {
    let (ticket, source, document) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info.manual;
        let (Some(source), Some(&document)) = (model.source.clone(), model.documents.first())
        else {
            return;
        };
        model.close();
        model.open = true;
        (model.ticket, source, document)
    };
    publish_closed(app);
    app.global::<GameInfoView>()
        .set_manual_state(ManualState::Loading);

    let ctx_task = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = fetch(&ctx_task, &source, document).await;
        let ctx = ctx_task;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(bytes) => start_worker(&ctx, &app, ticket, bytes),
            Err(state) => fail(&ctx, &app, ticket, state),
        });
    });
}

async fn fetch(ctx: &Ctx, source: &Source, document: Document) -> Result<Vec<u8>, ManualState> {
    use base64::Engine as _;
    let mut download = Download::default();
    let mut offset = 0;
    let mut etag = None;
    loop {
        let params = MediaAssetParams {
            media_id: source.media_id,
            system: if source.media_id.is_some() {
                String::new()
            } else {
                source.system.clone()
            },
            path: if source.media_id.is_some() {
                String::new()
            } else {
                source.path.clone()
            },
            asset_type: document.asset_type().into(),
            offset: (offset > 0).then_some(offset),
            length: Some(MEDIA_ASSET_MAX_CHUNK),
            etag: etag.clone(),
        };
        let chunk = ctx
            .store
            .client()
            .media_asset(params)
            .await
            .map_err(|error| {
                tracing::warn!(path = %source.path, "manual fetch failed: {}", error.message);
                ManualState::Failed
            })?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(chunk.data.as_bytes())
            .map_err(|_| ManualState::Failed)?;
        match download.accept(Chunk {
            size: chunk.size,
            etag: &chunk.etag,
            offset: chunk.offset,
            data: &data,
            next_offset: chunk.next_offset,
            complete: chunk.complete,
        }) {
            Step::Next {
                offset: next,
                etag: tag,
            } => {
                offset = next;
                etag = Some(tag);
            }
            Step::Done => return Ok(download.into_bytes()),
            Step::TooLarge => return Err(ManualState::TooLarge),
            Step::Invalid => {
                tracing::warn!(path = %source.path, "manual chunks did not line up");
                return Err(ManualState::Failed);
            }
        }
    }
}

fn fail(ctx: &Ctx, app: &App, ticket: u64, state: ManualState) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info.manual;
        if model.ticket != ticket || !model.open {
            return;
        }
        model.worker = None;
    }
    app.global::<GameInfoView>().set_manual_state(state);
}

/// Hands the file to a worker of its own. The parsed document never leaves
/// that thread; pages come back as pixel buffers.
fn start_worker(ctx: &Ctx, app: &App, ticket: u64, bytes: Vec<u8>) {
    let (tx, rx) = mpsc::channel::<Job>();
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info.manual;
        if model.ticket != ticket || !model.open {
            return;
        }
        model.worker = Some(tx);
    }
    let worker_ctx = ctx.clone();
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("manual".into())
        .spawn(move || run_worker(bytes, ticket, &rx, &weak, &worker_ctx));
    if spawned.is_err() {
        fail(ctx, app, ticket, ManualState::Failed);
    }
}

fn run_worker(
    bytes: Vec<u8>,
    ticket: u64,
    rx: &mpsc::Receiver<Job>,
    weak: &slint::Weak<App>,
    ctx: &Ctx,
) {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;

    let Ok(pdf) = Pdf::new(bytes) else {
        tracing::warn!("manual is not a PDF hayro can read");
        post_failure(ctx, weak, ticket);
        return;
    };
    let sizes: Vec<(f32, f32)> = pdf
        .pages()
        .iter()
        .map(hayro::hayro_syntax::page::Page::render_dimensions)
        .collect();
    if sizes.is_empty() {
        post_failure(ctx, weak, ticket);
        return;
    }
    {
        let ctx = ctx.clone();
        let _ = weak.upgrade_in_event_loop(move |app| loaded(&ctx, &app, ticket, sizes));
    }
    let cache = hayro::RenderCache::new();
    let interpreter = InterpreterSettings::default();
    let render = hayro::RenderSettings::default();
    while let Ok(first) = rx.recv() {
        // Only the newest request of each kind matters: pages flipped past
        // while one rendered are never drawn.
        let mut show = None;
        let mut prefetch = None;
        for job in std::iter::once(first).chain(rx.try_iter()) {
            if job.prefetch {
                prefetch = Some(job);
            } else {
                show = Some(job);
            }
        }
        for job in show.into_iter().chain(prefetch) {
            let Some(page) = pdf.pages().get(job.wanted.page) else {
                continue;
            };
            let (page_w, page_h) = page.render_dimensions();
            let settings = hayro::PixmapSettings {
                x_scale: job.wanted.width as f32 / page_w,
                y_scale: job.wanted.height as f32 / page_h,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            };
            let started = std::time::Instant::now();
            let pixmap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                hayro::render(page, &cache, &interpreter, &render, &settings)
            }));
            let Ok(pixmap) = pixmap else {
                tracing::warn!(page = job.wanted.page, "manual page failed to render");
                post_failure(ctx, weak, ticket);
                return;
            };
            tracing::debug!(
                page = job.wanted.page,
                width = pixmap.width(),
                height = pixmap.height(),
                ms = started.elapsed().as_millis() as u64,
                "manual page rendered"
            );
            // A white background leaves every pixel opaque, so the
            // premultiplied bytes are the straight ones.
            let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                pixmap.data_as_u8_slice(),
                u32::from(pixmap.width()),
                u32::from(pixmap.height()),
            );
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                landed(&ctx, &app, job.ticket, job.wanted, buffer);
            });
        }
    }
}

fn post_failure(ctx: &Ctx, weak: &slint::Weak<App>, ticket: u64) {
    let ctx = ctx.clone();
    let _ = weak.upgrade_in_event_loop(move |app| fail(&ctx, &app, ticket, ManualState::Failed));
}

fn loaded(ctx: &Ctx, app: &App, ticket: u64, sizes: Vec<(f32, f32)>) {
    let pages = sizes.len();
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info.manual;
        if model.ticket != ticket || !model.open {
            return;
        }
        model.page_sizes = sizes;
        model.reader = Some(Reader::new(pages));
    }
    let view = app.global::<GameInfoView>();
    view.set_manual_pages(i32::try_from(pages).unwrap_or(i32::MAX));
    view.set_manual_page(0);
    view.set_manual_state(ManualState::Ready);
    show_page(ctx, app);
}

fn landed(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    requested: Wanted,
    buffer: slint::SharedPixelBuffer<slint::Rgba8Pixel>,
) {
    let current = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info.manual;
        if model.ticket != ticket || !model.open {
            return;
        }
        // Keyed by what was asked for, so the lookup that asked finds it
        // even when the renderer rounded the canvas.
        model.remember(requested, buffer.clone());
        model
            .reader
            .and_then(|reader| model.wanted(reader.page))
            .is_some_and(|wanted| wanted == requested)
    };
    if current {
        let view = app.global::<GameInfoView>();
        view.set_manual_image(slint::Image::from_rgba8(buffer));
        view.set_manual_has_image(true);
        prefetch(ctx);
    }
}

/// Shows the reader's page from the cache, or asks the worker for it.
fn show_page(ctx: &Ctx, app: &App) {
    let (reader, wanted, cached) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info.manual;
        let Some(reader) = model.reader else { return };
        let wanted = model.wanted(reader.page);
        let cached = wanted.and_then(|wanted| model.cached(wanted));
        (reader, wanted, cached)
    };
    let view = app.global::<GameInfoView>();
    view.set_manual_page(i32::try_from(reader.page).unwrap_or(0));
    view.set_manual_fit(match reader.fit {
        Fit::Width => ManualFit::Width,
        Fit::Page => ManualFit::Page,
    });
    if let Some(buffer) = cached {
        view.set_manual_image(slint::Image::from_rgba8(buffer));
        view.set_manual_has_image(true);
        prefetch(ctx);
        return;
    }
    view.set_manual_has_image(false);
    view.set_manual_image(slint::Image::default());
    if let Some(wanted) = wanted {
        send(ctx, wanted, false);
    }
}

/// Renders the next page ahead of a turn to it.
fn prefetch(ctx: &Ctx) {
    let next = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info.manual;
        model
            .reader
            .filter(|reader| reader.page + 1 < reader.pages)
            .and_then(|reader| model.wanted(reader.page + 1))
            .filter(|wanted| model.cached(*wanted).is_none())
    };
    if let Some(wanted) = next {
        send(ctx, wanted, true);
    }
}

fn send(ctx: &Ctx, wanted: Wanted, prefetch: bool) {
    let shared = lock(&ctx.shared);
    let model = &shared.game_info.manual;
    if let Some(worker) = &model.worker {
        let _ = worker.send(Job {
            ticket: model.ticket,
            wanted,
            prefetch,
        });
    }
}

/// Input while the manual is open. Back returns to the details.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::CANCEL => {
            lock(&ctx.shared).game_info.manual.close();
            publish_closed(app);
        }
        actions::LEFT | actions::RIGHT | actions::PAGE_PREV | actions::PAGE_NEXT => {
            let delta = if action == actions::LEFT || action == actions::PAGE_PREV {
                -1
            } else {
                1
            };
            let moved = lock(&ctx.shared)
                .game_info
                .manual
                .reader
                .as_mut()
                .is_some_and(|reader| reader.turn(delta));
            if moved {
                app.global::<GameInfoView>().set_manual_scroll(0.0);
                show_page(ctx, app);
            }
        }
        actions::ACCEPT => {
            let toggled = lock(&ctx.shared)
                .game_info
                .manual
                .reader
                .as_mut()
                .map(Reader::toggle_fit)
                .is_some();
            if toggled {
                app.global::<GameInfoView>().set_manual_scroll(0.0);
                show_page(ctx, app);
            }
        }
        actions::UP => app.invoke_game_info_scroll(crate::ScrollAction::Up),
        actions::DOWN => app.invoke_game_info_scroll(crate::ScrollAction::Down),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests should fail fast")]
    use super::*;

    fn buffer(side: u32) -> slint::SharedPixelBuffer<slint::Rgba8Pixel> {
        slint::SharedPixelBuffer::new(side, side)
    }

    #[test]
    fn closing_retires_the_ticket_and_the_worker() {
        let mut model = ManualModel::default();
        model.reset(Some(Source::default()), vec![Document::Manual]);
        assert!(model.has_documents());
        let (tx, _rx) = mpsc::channel();
        model.open = true;
        model.worker = Some(tx);
        let ticket = model.ticket;
        model.reset(None, Vec::new());
        assert_ne!(model.ticket, ticket);
        assert!(model.worker.is_none());
        assert!(!model.is_open());
        assert!(!model.has_documents());
    }

    #[test]
    fn the_page_cache_stays_under_its_cap_but_keeps_the_newest() {
        let mut model = ManualModel::default();
        // 2048² RGBA is 16 MiB: two of them overflow the 24 MiB cap.
        for page in 0..3 {
            model.remember(
                Wanted {
                    page,
                    width: 2048,
                    height: 2048,
                },
                buffer(2048),
            );
        }
        assert_eq!(model.pages.len(), 1);
        assert_eq!(model.pages[0].wanted.page, 2);
    }

    #[test]
    fn wanted_follows_the_reader_and_the_box() {
        let mut model = ManualModel {
            page_sizes: vec![(612.0, 792.0), (792.0, 612.0)],
            page_box: (1000, 500),
            ..ManualModel::default()
        };
        assert_eq!(model.wanted(0), None);
        model.reader = Some(Reader::new(2));
        // A manual opens on the whole page.
        assert_eq!(
            model.wanted(0),
            Some(Wanted {
                page: 0,
                width: 386,
                height: 500
            })
        );
        if let Some(reader) = model.reader.as_mut() {
            reader.toggle_fit();
        }
        assert_eq!(
            model.wanted(0),
            Some(Wanted {
                page: 0,
                width: 1000,
                height: 1294
            })
        );
        assert_eq!(
            model.wanted(1),
            Some(Wanted {
                page: 1,
                width: 1000,
                height: 773
            })
        );
        // Fit width gives the arrows their room on each side.
        model.arrow_room = 50;
        assert_eq!(
            model.wanted(1),
            Some(Wanted {
                page: 1,
                width: 900,
                height: 695
            })
        );
    }

    #[test]
    fn the_mock_manual_renders() {
        let response = mock_core_pdf();
        let pdf = hayro::hayro_syntax::Pdf::new(response).expect("valid PDF");
        assert_eq!(pdf.pages().len(), 3);
        let page = &pdf.pages()[0];
        let pixmap = hayro::render(
            page,
            &hayro::RenderCache::new(),
            &hayro::hayro_interpret::InterpreterSettings::default(),
            &hayro::RenderSettings::default(),
            &hayro::PixmapSettings {
                x_scale: 0.5,
                y_scale: 0.5,
                bg_color: hayro::vello_cpu::color::palette::css::WHITE,
            },
        );
        assert_eq!((pixmap.width(), pixmap.height()), (306, 396));
        // Some text was drawn: not every pixel is the white page.
        assert!(pixmap.data_as_u8_slice().iter().any(|&b| b < 128));
    }

    /// The PDF the mock Core serves, built the same way.
    fn mock_core_pdf() -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let text = "BT /F1 28 Tf 72 700 Td (Manual) Tj ET\n";
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [4 0 R 6 0 R 8 0 R] /Count 3 >>".into(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ];
        for page in 0..3 {
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", 5 + page * 2));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{text}endstream",
                text.len()
            ));
        }
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}
