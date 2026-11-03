// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A game's documents, read inside Game Info. Core streams a manual through
//! `media.asset` in bounded chunks; this module assembles them under a size
//! cap, keeps the reader's page and fit, and sizes a page for the box it is
//! drawn in. The Core calls and the PDF renderer stay in the shell.

/// The largest manual fetched. The whole file sits in memory while it is
/// read, next to Core and the running core in `MiSTer`'s shared RAM.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// The most pixels one rendered page may have. A fit-width page is as tall
/// as its aspect says; a strip-shaped page would otherwise ask for a buffer
/// many screens high.
pub const MAX_PAGE_PIXELS: u64 = 4096 * 4096;

/// A document Core can serve for a game. Only manuals exist today; a later
/// asset type joins here and the reader gains a list page in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Document {
    Manual,
}

impl Document {
    /// The `media.asset` type that fetches it.
    pub fn asset_type(self) -> &'static str {
        match self {
            Self::Manual => "manual",
        }
    }
}

/// The documents a game has, from its `media.meta` property keys (ROM and
/// title properties alike, since Core resolves the asset across both).
pub fn documents<'a>(property_keys: impl IntoIterator<Item = &'a str>) -> Vec<Document> {
    let has_manual = property_keys
        .into_iter()
        .any(|key| key.trim().eq_ignore_ascii_case("property:manual"));
    if has_manual {
        vec![Document::Manual]
    } else {
        Vec::new()
    }
}

/// What the fetch loop does after a chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Ask for the next chunk at `offset` with this `etag`.
    Next { offset: u64, etag: String },
    /// Every byte has arrived.
    Done,
    /// The file is over [`MAX_BYTES`]; nothing more is fetched.
    TooLarge,
    /// The chunk does not continue the file: a different version, a gap, or
    /// more bytes than Core said it had.
    Invalid,
}

/// The bytes of one asset as its chunks arrive.
#[derive(Debug, Default)]
pub struct Download {
    etag: Option<String>,
    size: u64,
    bytes: Vec<u8>,
}

/// One `media.asset` response, decoded.
#[derive(Debug, Clone, Copy)]
pub struct Chunk<'a> {
    pub size: u64,
    pub etag: &'a str,
    pub offset: u64,
    pub data: &'a [u8],
    pub next_offset: Option<u64>,
    pub complete: bool,
}

impl Download {
    /// Folds in one chunk and says what to fetch next.
    pub fn accept(&mut self, chunk: Chunk<'_>) -> Step {
        match &self.etag {
            None => {
                if chunk.offset != 0 {
                    return Step::Invalid;
                }
                if chunk.size > MAX_BYTES {
                    return Step::TooLarge;
                }
                self.etag = Some(chunk.etag.to_string());
                self.size = chunk.size;
                self.bytes = Vec::with_capacity(usize::try_from(chunk.size).unwrap_or(0));
            }
            Some(etag) => {
                if etag != chunk.etag || chunk.size != self.size {
                    return Step::Invalid;
                }
            }
        }
        if chunk.offset != self.bytes.len() as u64 {
            return Step::Invalid;
        }
        let end = chunk.offset + chunk.data.len() as u64;
        if end > self.size {
            return Step::Invalid;
        }
        self.bytes.extend_from_slice(chunk.data);
        if chunk.complete {
            return if end == self.size {
                Step::Done
            } else {
                Step::Invalid
            };
        }
        match chunk.next_offset {
            Some(next) if next == end && !chunk.data.is_empty() => Step::Next {
                offset: next,
                etag: chunk.etag.to_string(),
            },
            _ => Step::Invalid,
        }
    }

    /// The assembled file, once [`Step::Done`] was returned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// How a page sits in the reader's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// As wide as the box: the largest text, scrolled down the page.
    #[default]
    Width,
    /// The whole page inside the box.
    Page,
}

/// The reader's place in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reader {
    pub page: usize,
    pub pages: usize,
    pub fit: Fit,
}

impl Reader {
    pub fn new(pages: usize) -> Self {
        Self {
            page: 0,
            pages,
            fit: Fit::Width,
        }
    }

    /// Turns `delta` pages, stopping at either end. True when it moved.
    pub fn turn(&mut self, delta: i32) -> bool {
        let last = self.pages.saturating_sub(1) as i64;
        let next = (self.page as i64 + i64::from(delta)).clamp(0, last) as usize;
        let moved = next != self.page;
        self.page = next;
        moved
    }

    pub fn toggle_fit(&mut self) {
        self.fit = match self.fit {
            Fit::Width => Fit::Page,
            Fit::Page => Fit::Width,
        };
    }
}

/// The pixel size to render a `page_w`×`page_h` page at for a
/// `box_w`×`box_h` box. `None` for a degenerate page or box.
pub fn render_size(
    page_w: f32,
    page_h: f32,
    box_w: u32,
    box_h: u32,
    fit: Fit,
) -> Option<(u32, u32)> {
    if !(page_w.is_finite() && page_h.is_finite()) || page_w <= 0.0 || page_h <= 0.0 {
        return None;
    }
    if box_w == 0 || box_h == 0 {
        return None;
    }
    let width_scale = f64::from(box_w) / f64::from(page_w);
    let scale = match fit {
        Fit::Width => width_scale,
        Fit::Page => width_scale.min(f64::from(box_h) / f64::from(page_h)),
    };
    let area = f64::from(page_w) * f64::from(page_h) * scale * scale;
    let scale = if area > MAX_PAGE_PIXELS as f64 {
        scale * (MAX_PAGE_PIXELS as f64 / area).sqrt()
    } else {
        scale
    };
    // hayro sizes its canvas in u16.
    let limit = f64::from(u16::MAX);
    let scale = scale
        .min(limit / f64::from(page_w))
        .min(limit / f64::from(page_h));
    let width = (f64::from(page_w) * scale).round().max(1.0) as u32;
    let height = (f64::from(page_h) * scale).round().max(1.0) as u32;
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk<'a>(
        size: u64,
        etag: &'a str,
        offset: u64,
        data: &'a [u8],
        complete: bool,
    ) -> Chunk<'a> {
        Chunk {
            size,
            etag,
            offset,
            data,
            next_offset: (!complete).then_some(offset + data.len() as u64),
            complete,
        }
    }

    #[test]
    fn documents_come_from_the_manual_property() {
        assert_eq!(
            documents(["property:description", "property:manual"]),
            vec![Document::Manual]
        );
        assert!(documents(["property:description"]).is_empty());
        assert_eq!(Document::Manual.asset_type(), "manual");
    }

    #[test]
    fn chunks_assemble_in_order() {
        let mut download = Download::default();
        assert_eq!(
            download.accept(chunk(6, "e", 0, b"abc", false)),
            Step::Next {
                offset: 3,
                etag: "e".into()
            }
        );
        assert_eq!(download.accept(chunk(6, "e", 3, b"def", true)), Step::Done);
        assert_eq!(download.into_bytes(), b"abcdef");
    }

    #[test]
    fn a_single_chunk_file_completes_at_once() {
        let mut download = Download::default();
        assert_eq!(download.accept(chunk(2, "e", 0, b"ab", true)), Step::Done);
    }

    #[test]
    fn an_oversized_file_stops_before_any_bytes() {
        let mut download = Download::default();
        assert_eq!(
            download.accept(chunk(MAX_BYTES + 1, "e", 0, b"ab", false)),
            Step::TooLarge
        );
    }

    #[test]
    fn a_changed_file_gap_or_overrun_is_invalid() {
        let mut download = Download::default();
        download.accept(chunk(6, "e", 0, b"abc", false));
        assert_eq!(
            download.accept(chunk(6, "f", 3, b"def", true)),
            Step::Invalid
        );

        let mut download = Download::default();
        download.accept(chunk(6, "e", 0, b"abc", false));
        assert_eq!(
            download.accept(chunk(6, "e", 4, b"ef", true)),
            Step::Invalid
        );

        let mut download = Download::default();
        assert_eq!(
            download.accept(chunk(2, "e", 0, b"abc", true)),
            Step::Invalid
        );

        let mut download = Download::default();
        assert_eq!(
            download.accept(chunk(6, "e", 0, b"abc", true)),
            Step::Invalid
        );

        let mut download = Download::default();
        assert_eq!(
            download.accept(chunk(6, "e", 3, b"abc", false)),
            Step::Invalid
        );
    }

    #[test]
    fn pages_turn_and_stop_at_the_ends() {
        let mut reader = Reader::new(3);
        assert!(!reader.turn(-1));
        assert!(reader.turn(1));
        assert!(reader.turn(5));
        assert_eq!(reader.page, 2);
        assert!(!reader.turn(1));
        reader.toggle_fit();
        assert_eq!(reader.fit, Fit::Page);
        reader.toggle_fit();
        assert_eq!(reader.fit, Fit::Width);
        assert!(!Reader::new(0).turn(1));
    }

    #[test]
    fn render_size_fits_width_or_page() {
        // A letter page in a 1000x500 box.
        assert_eq!(
            render_size(612.0, 792.0, 1000, 500, Fit::Width),
            Some((1000, 1294))
        );
        assert_eq!(
            render_size(612.0, 792.0, 1000, 500, Fit::Page),
            Some((386, 500))
        );
        assert_eq!(render_size(0.0, 792.0, 1000, 500, Fit::Width), None);
        assert_eq!(render_size(612.0, 792.0, 0, 500, Fit::Width), None);
    }

    #[test]
    fn render_size_caps_a_strip_page() {
        let size = render_size(100.0, 100_000.0, 1000, 500, Fit::Width);
        assert!(matches!(size, Some((w, h))
            if u64::from(w) * u64::from(h) <= MAX_PAGE_PIXELS && u16::try_from(h).is_ok()));
    }
}
