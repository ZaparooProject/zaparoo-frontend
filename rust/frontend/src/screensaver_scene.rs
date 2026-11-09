// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! One screensaver run's picture: the style's simulation from
//! `zaparoo_app::screensaver`, stepped and projected onto the `Saver`
//! global. It holds no timers, no Core client and no caches, only the art
//! it was handed, so the runtime driver (`screensaver.rs`) and the
//! snapshot tool share it.

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Image, Model, ModelRc, Rgb8Pixel, SharedPixelBuffer, VecModel};
use zaparoo_app::screensaver::contour::{self, Mask};
use zaparoo_app::screensaver::flyby::Flyby;
use zaparoo_app::screensaver::hops::Hops;
use zaparoo_app::screensaver::life::Life;
use zaparoo_app::screensaver::marquee::{self, Marquee, Pass};
use zaparoo_app::screensaver::maze::{self, Maze, Texture};
use zaparoo_app::screensaver::paddle::{self, Paddle};
use zaparoo_app::screensaver::polygons::Polygons;
use zaparoo_app::screensaver::snakes::Snakes;
use zaparoo_app::screensaver::spin::Spin;
use zaparoo_app::screensaver::stars::Stars;
use zaparoo_app::screensaver::wall::{Grid, Tile, Wall};
use zaparoo_app::screensaver::{self as rules, Look, Resolved, Rng, Sprite, Style};

use crate::{Saver, SaverLook, SaverSprite};

/// The tallest any style draws a logo or cover, as a share of the
/// screen's height: the rasters are made at this size once per run.
pub(crate) const FACE_HEIGHT: f32 = 0.3;
/// Covers the Maze hangs, and the size it samples them at.
const MAZE_TEXTURE: u32 = 64;
/// The Maze's picture height on HDMI; the CRT draws at its own lines.
const MAZE_LINES: usize = 180;
/// The Maze's grid of cells.
const MAZE_CELLS: (usize, usize) = (8, 6);
/// Life's board height on HDMI; the CRT uses four-pixel cells.
const LIFE_ROWS: usize = 90;
/// The Polygons picture's height on HDMI.
const POLYGON_LINES: usize = 270;
/// Points around the bolt the Polygons morph gathers into.
const MARK_POINTS: usize = 40;

/// A logo or cover, with its width over its height.
#[derive(Clone)]
pub(crate) struct Face {
    pub image: Image,
    pub aspect: f32,
}

impl Face {
    pub(crate) fn of(image: Image) -> Self {
        let size = image.size();
        let aspect = if size.height == 0 {
            1.0
        } else {
            size.width as f32 / size.height as f32
        };
        Self { image, aspect }
    }
}

/// A played game's cover: Bouncing: Recent game's card.
#[derive(Clone)]
pub(crate) struct Card {
    pub cover: Image,
}

/// A recently played game for the Recent games line: its system's logo
/// (when one is ready) and its title.
#[derive(Clone)]
pub(crate) struct Line {
    pub logo: Image,
    pub title: String,
}

/// Everything a run can draw besides the Zaparoo logo and icon.
#[derive(Clone, Default)]
pub(crate) struct Art {
    pub systems: Vec<Face>,
    pub covers: Vec<Face>,
    pub cards: Vec<Card>,
    pub lines: Vec<Line>,
}

impl Art {
    pub(crate) fn counts(&self) -> rules::Art {
        rules::Art {
            covers: self.covers.len(),
            recents: self.cards.len(),
            systems: self.systems.len(),
        }
    }
}

/// The scene in physical pixels, and whether it is a low-resolution
/// (240p-class) output.
#[derive(Debug, Clone, Copy)]
pub(crate) struct View {
    pub width: f32,
    pub height: f32,
    pub low_res: bool,
}

impl View {
    pub(crate) fn of(app: &crate::App) -> Self {
        let sizing = app.global::<crate::Sizing>();
        Self {
            width: sizing.get_screen_width().max(1.0),
            height: sizing.get_screen_height().max(1.0),
            low_res: sizing.get_pixel_grid(),
        }
    }

    pub(crate) fn aspect(self) -> f32 {
        self.width / self.height
    }

    /// The pixel height every logo and cover raster is made at.
    /// The side of a Cover wall tile, the largest box a cover is painted in.
    pub(crate) fn cover_px(self) -> u32 {
        let grid = Grid::for_scene(self.aspect());
        (self.height * grid.tile_h).round().max(8.0) as u32
    }

    pub(crate) fn face_px(self) -> u32 {
        (self.height * FACE_HEIGHT).round().max(8.0) as u32
    }
}

enum Sim {
    /// Bouncing: the view walks it.
    Still,
    Spin(Spin),
    Hops(Hops),
    Wall(Wall),
    Marquee {
        marquee: Marquee,
        elapsed: f32,
    },
    Paddle(Paddle),
    Stars(Stars),
    Flyby(Flyby),
    Polygons {
        polygons: Polygons,
        /// The picture's size in pixels.
        size: (usize, usize),
    },
    Snakes(Snakes),
    Life(Life),
    Maze {
        maze: Box<Maze>,
        size: (usize, usize),
        textures: Vec<Texture>,
        mark: Texture,
        rats: Vec<Texture>,
        scratch: maze::Scratch,
    },
}

/// One run of one style.
pub(crate) struct Scene {
    style: Style,
    sim: Sim,
    art: Art,
    logo: Face,
    mark: Image,
    view: View,
    sprites: Rc<VecModel<SaverSprite>>,
    /// Block(0) draws white rather than palette slot 0 (stars, paddles).
    white: bool,
    card: usize,
    twelve_hour: bool,
}

impl Scene {
    /// Build `resolved` with `art` and publish its first frame.
    pub(crate) fn new(
        app: &crate::App,
        resolved: Resolved,
        art: Art,
        view: View,
        seed: u64,
        time: (u8, u8),
        twelve_hour: bool,
    ) -> Self {
        let face_px = view.face_px();
        let logo_source = crate::brand::logo_rgba();
        let logo = logo_source
            .as_ref()
            .and_then(|s| crate::brand::raster_at_height(s, face_px))
            .map_or_else(
                || Face {
                    image: Image::default(),
                    aspect: 3.0,
                },
                Face::of,
            );
        let mark_px = (view.height * paddle::BALL * 2.0).round().max(8.0) as u32;
        let mark = crate::brand::icon_rgba()
            .and_then(|s| crate::brand::raster_at_height(&s, mark_px))
            .unwrap_or_default();
        let aspect = view.aspect();
        let style = resolved.style;
        let sim = match style {
            Style::Bouncing | Style::Random => Sim::Still,
            Style::SpinningLogo => {
                Sim::Spin(Spin::new(aspect, logo.aspect, art.systems.len(), seed))
            }
            Style::SystemLogos => Sim::Hops(Hops::new(art.systems.len(), seed)),
            Style::CoverWall => {
                Sim::Wall(Wall::new(Grid::for_scene(aspect), art.covers.len(), seed))
            }
            Style::RecentGames => Sim::Marquee {
                marquee: Marquee::new(art.lines.len(), seed),
                elapsed: 0.0,
            },
            Style::PaddleClock => Sim::Paddle(Paddle::new(aspect, time.0, time.1, seed)),
            Style::Starfield => Sim::Stars(Stars::new(
                if view.low_res { 60 } else { 80 },
                art.systems.len(),
                seed,
            )),
            Style::Flyby => Sim::Flyby(Flyby::new(art.systems.len(), art.covers.len(), seed)),
            Style::Polygons => Sim::Polygons {
                polygons: Polygons::new(
                    aspect,
                    logo_source
                        .as_ref()
                        .map_or_else(fallback_mark, bolt_outline),
                    seed,
                ),
                size: frame_size(view, POLYGON_LINES),
            },
            Style::Snakes => Sim::Snakes(Snakes::new(aspect, view.low_res, seed)),
            Style::Life => {
                let (w, h) = life_board(view);
                let mask = logo_source.as_ref().map(|s| life_mask(s, w, h));
                Sim::Life(Life::new(w, h, mask, seed))
            }
            Style::Maze => maze_sim(&art, view, seed),
        };
        let mut scene = Self {
            style,
            sim,
            art,
            logo,
            mark,
            view,
            sprites: Rc::new(VecModel::default()),
            white: matches!(style, Style::Starfield | Style::PaddleClock),
            card: 0,
            twelve_hour,
        };
        let saver = app.global::<Saver>();
        saver.set_style(style.into());
        saver.set_bouncing(resolved.bouncing.into());
        saver.set_aspect(aspect);
        saver.set_fade(style == Style::SystemLogos);
        saver.set_frame(Image::default());
        saver.set_sprites(ModelRc::from(scene.sprites.clone()));
        scene.show_card(app);
        scene.first_frame(app, time);
        scene
    }

    /// How often the style steps; `None` when the view animates itself.
    pub(crate) fn interval(&self) -> Option<Duration> {
        let ms = match self.style {
            Style::Bouncing | Style::Random => return None,
            Style::SpinningLogo | Style::RecentGames | Style::PaddleClock => 33,
            Style::Starfield | Style::Flyby | Style::Polygons | Style::Maze => 50,
            Style::SystemLogos => 100,
            Style::Snakes | Style::Life => 83,
            Style::CoverWall => 250,
        };
        Some(Duration::from_millis(ms))
    }

    /// The style repaints most of the screen: the renderer should run at
    /// its motion resolution, halved again where the display can scale,
    /// while it plays.
    pub(crate) fn heavy(&self) -> bool {
        self.style.low_resolution()
    }

    /// Bouncing: Recent game hit an edge: the next game's card.
    pub(crate) fn bounced(&mut self, app: &crate::App) {
        if self.art.cards.len() > 1 {
            self.card = (self.card + 1) % self.art.cards.len();
            self.show_card(app);
        }
    }

    fn show_card(&self, app: &crate::App) {
        let saver = app.global::<Saver>();
        if let Some(card) = self.art.cards.get(self.card) {
            saver.set_card_image(card.cover.clone());
        } else {
            saver.set_card_image(Image::default());
        }
    }

    fn first_frame(&mut self, app: &crate::App, time: (u8, u8)) {
        match &mut self.sim {
            Sim::Wall(wall) => {
                let rows: Vec<Sprite> = (0..wall.tiles().len())
                    .map(|i| wall_sprite(wall, &self.art, i))
                    .collect();
                let rows: Vec<SaverSprite> = rows.iter().map(|s| self.row(s)).collect();
                self.sprites.set_vec(rows);
            }
            Sim::Marquee { marquee, .. } => {
                let pass = marquee.next_pass();
                self.show_pass(app, pass);
            }
            Sim::Paddle(p) => {
                let (score, shift) = (p.score(), p.shift());
                self.show_score(app, score, shift);
            }
            Sim::Snakes(snakes) => {
                let rows: Vec<SaverSprite> = snakes.sprites().iter().map(|s| self.row(s)).collect();
                self.sprites.set_vec(rows);
            }
            _ => {}
        }
        self.tick(app, 0.0, time);
    }

    /// Advance by `dt` seconds with the wall clock at `time` (hour, minute).
    #[allow(
        clippy::too_many_lines,
        reason = "one arm per style; each is short and splitting them scatters the borrows"
    )]
    pub(crate) fn tick(&mut self, app: &crate::App, dt: f32, time: (u8, u8)) {
        let aspect = self.view.aspect();
        let art = &self.art;
        let logo_aspect = self.logo.aspect;
        let faces = |look: Look| face_aspect(art, logo_aspect, look);
        let sprites: Option<Vec<Sprite>> = match &mut self.sim {
            Sim::Still => None,
            Sim::Spin(spin) => {
                spin.step(dt, aspect, faces);
                Some(vec![spin.sprite()])
            }
            Sim::Hops(hops) => {
                let changed = hops.step(dt, aspect, faces);
                (changed || self.sprites.row_count() == 0).then(|| hops.sprites().to_vec())
            }
            Sim::Wall(wall) => {
                let changed = wall.step(dt);
                let rows: Vec<(usize, Sprite)> = changed
                    .into_iter()
                    .map(|i| (i, wall_sprite(wall, &self.art, i)))
                    .collect();
                for (i, s) in rows {
                    let row = self.row(&s);
                    if i < self.sprites.row_count() {
                        self.sprites.set_row_data(i, row);
                    }
                }
                None
            }
            Sim::Marquee { marquee, elapsed } => {
                *elapsed += dt;
                if *elapsed >= marquee::CROSS_SECONDS {
                    *elapsed = 0.0;
                    let pass = marquee.next_pass();
                    self.show_pass(app, pass);
                }
                if let Sim::Marquee { elapsed, .. } = &self.sim {
                    app.global::<Saver>()
                        .set_pass_progress((*elapsed / marquee::CROSS_SECONDS).clamp(0.0, 1.0));
                }
                None
            }
            Sim::Paddle(p) => {
                if p.step(dt, time.0, time.1) {
                    let (score, shift) = (p.score(), p.shift());
                    self.show_score(app, score, shift);
                }
                let Sim::Paddle(p) = &self.sim else {
                    return;
                };
                Some(p.sprites().to_vec())
            }
            Sim::Stars(stars) => {
                stars.step(dt, aspect, faces);
                Some(stars.sprites(aspect))
            }
            Sim::Flyby(flyby) => {
                flyby.step(dt, aspect, faces);
                Some(flyby.sprites())
            }
            Sim::Polygons { polygons, size } => {
                polygons.step(dt, aspect);
                let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(size.0 as u32, size.1 as u32);
                zaparoo_app::screensaver::polygons::render(
                    &polygons.outlines(aspect),
                    aspect,
                    buffer.make_mut_bytes(),
                    size.0,
                    size.1,
                );
                app.global::<Saver>().set_frame(Image::from_rgb8(buffer));
                None
            }
            Sim::Snakes(snakes) => {
                let changes: Vec<(usize, SaverSprite)> = snakes
                    .step(dt)
                    .iter()
                    .map(|(i, s)| (*i, self.row(s)))
                    .collect();
                for (i, row) in changes {
                    self.sprites.set_row_data(i, row);
                }
                None
            }
            Sim::Life(life) => {
                if life.step(dt, time) || dt == 0.0 {
                    let mut buffer =
                        SharedPixelBuffer::<Rgb8Pixel>::new(life.width as u32, life.height as u32);
                    life.render(buffer.make_mut_bytes());
                    app.global::<Saver>().set_frame(Image::from_rgb8(buffer));
                }
                None
            }
            Sim::Maze {
                maze,
                size,
                textures,
                mark,
                rats,
                scratch,
            } => {
                maze.step(dt);
                let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(size.0 as u32, size.1 as u32);
                maze.render(
                    buffer.make_mut_bytes(),
                    *size,
                    textures,
                    mark,
                    rats,
                    scratch,
                );
                app.global::<Saver>().set_frame(Image::from_rgb8(buffer));
                None
            }
        };
        if let Some(sprites) = sprites {
            let rows: Vec<SaverSprite> = sprites.iter().map(|s| self.row(s)).collect();
            publish(&self.sprites, rows);
        }
    }

    fn show_pass(&self, app: &crate::App, pass: Pass) {
        let saver = app.global::<Saver>();
        saver.set_pass_y(pass.y);
        saver.set_pass_progress(0.0);
        if let Some(line) = pass.entry.and_then(|i| self.art.lines.get(i)) {
            saver.set_pass_clock(false);
            saver.set_pass_logo(line.logo.clone());
            saver.set_pass_title(line.title.as_str().into());
        } else {
            saver.set_pass_clock(true);
            saver.set_pass_logo(Image::default());
            saver.set_pass_title("".into());
        }
    }

    fn show_score(&self, app: &crate::App, (hour, minute): (u8, u8), shift: (f32, f32)) {
        let saver = app.global::<Saver>();
        let hour = if self.twelve_hour {
            match hour % 12 {
                0 => 12,
                h => h,
            }
        } else {
            hour
        };
        saver.set_score_left(hour.to_string().into());
        saver.set_score_right(format!("{minute:02}").into());
        saver.set_shift_x(shift.0);
        saver.set_shift_y(shift.1);
    }

    /// A sprite as the view draws it.
    fn row(&self, s: &Sprite) -> SaverSprite {
        let (look, image, color) = match s.look {
            Look::Logo => (
                SaverLook::Image,
                self.logo.image.clone(),
                slint::Color::default(),
            ),
            Look::Mark => (SaverLook::Image, self.mark.clone(), slint::Color::default()),
            Look::System(i) => (
                SaverLook::Image,
                self.art
                    .systems
                    .get(i)
                    .map(|f| f.image.clone())
                    .unwrap_or_default(),
                slint::Color::default(),
            ),
            Look::Cover(i) => (
                SaverLook::Image,
                self.art
                    .covers
                    .get(i)
                    .map(|f| f.image.clone())
                    .unwrap_or_default(),
                slint::Color::default(),
            ),
            Look::Clock => (SaverLook::Clock, Image::default(), slint::Color::default()),
            Look::Block(n) => (SaverLook::Block, Image::default(), self.slot(n)),
        };
        SaverSprite {
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
            opacity: s.opacity,
            look,
            image,
            color,
        }
    }

    fn slot(&self, n: u8) -> slint::Color {
        if self.white && n == 0 {
            return slint::Color::from_rgb_u8(0xFF, 0xFF, 0xFF);
        }
        let (r, g, b) = rules::slot_rgb(n);
        slint::Color::from_rgb_u8(r, g, b)
    }
}

/// Empty every `Saver` model and picture, so nothing a run held stays
/// alive after it ends.
pub(crate) fn clear(app: &crate::App) {
    let saver = app.global::<Saver>();
    saver.set_sprites(ModelRc::default());
    saver.set_frame(Image::default());
    saver.set_card_image(Image::default());
    saver.set_pass_logo(Image::default());
}

/// Replace a model's rows, touching only the rows that changed.
fn publish<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (i, row) in rows.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&row) {
            model.set_row_data(i, row);
        }
    }
}

fn face_aspect(art: &Art, logo_aspect: f32, look: Look) -> f32 {
    match look {
        Look::Logo => logo_aspect,
        Look::System(i) => art.systems.get(i).map_or(2.0, |f| f.aspect),
        Look::Cover(i) => art.covers.get(i).map_or(0.75, |f| f.aspect),
        Look::Clock => 2.6,
        Look::Mark | Look::Block(_) => 1.0,
    }
}

/// Tile `i` of the wall: a cover fitted inside its square, keeping its
/// shape, or the clock across the whole tile.
fn wall_sprite(wall: &Wall, art: &Art, tile: usize) -> Sprite {
    match wall.tiles().get(tile) {
        Some(Tile::Cover(cover)) => {
            let aspect = art.covers.get(*cover).map_or(0.75, |f| f.aspect);
            let (left, top, width, height) = wall.grid.fit(tile, aspect);
            Sprite::new(left, top, width, height, Look::Cover(*cover))
        }
        Some(Tile::Clock) | None => {
            let (left, top) = wall.grid.origin(tile);
            Sprite::new(left, top, wall.grid.tile_w, wall.grid.tile_h, Look::Clock)
        }
    }
}

/// The bolt at the logo's left, outlined, resampled and normalized.
fn bolt_outline(logo: &image::RgbaImage) -> (Vec<(f32, f32)>, f32) {
    let (w, h) = (logo.width() as usize, logo.height() as usize);
    let alpha: Vec<u8> = logo.pixels().map(|p| p.0[3]).collect();
    let mask = Mask {
        alpha: &alpha,
        width: w,
        height: h,
    };
    let Some((x0, y0, bw, bh)) = contour::mark_bounds(mask) else {
        return fallback_mark();
    };
    let mut cropped = Vec::with_capacity(bw * bh);
    for y in y0..y0 + bh {
        cropped.extend_from_slice(&alpha[y * w + x0..y * w + x0 + bw]);
    }
    let points = contour::trace(Mask {
        alpha: &cropped,
        width: bw,
        height: bh,
    });
    if points.len() < 3 {
        return fallback_mark();
    }
    contour::normalize(&contour::resample(&points, MARK_POINTS))
}

fn fallback_mark() -> (Vec<(f32, f32)>, f32) {
    let square = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    contour::normalize(&contour::resample(&square, MARK_POINTS))
}

/// The Maze's simulation: the maze, the covers framed on its walls, the
/// Zaparoo mark spinning at its junctions and the system logos running
/// about.
fn maze_sim(art: &Art, view: View, seed: u64) -> Sim {
    let textures: Vec<Texture> = art
        .covers
        .iter()
        .take(maze::MAX_COVERS)
        .filter_map(|f| texture(&f.image))
        .collect();
    let mark = crate::brand::icon_rgba().map_or_else(
        || Texture {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        },
        |s| texture_from(&s),
    );
    let rats: Vec<Texture> = art
        .systems
        .iter()
        .take(maze::MAX_RATS)
        .filter_map(|f| logo_texture(&f.image))
        .collect();
    Sim::Maze {
        maze: Box::new(Maze::new(MAZE_CELLS, textures.len(), rats.len(), seed)),
        size: frame_size(view, MAZE_LINES),
        textures,
        mark,
        rats,
        scratch: maze::Scratch::default(),
    }
}

fn life_board(view: View) -> (usize, usize) {
    if view.low_res {
        (
            (view.width / 4.0).floor().max(8.0) as usize,
            (view.height / 4.0).floor().max(8.0) as usize,
        )
    } else {
        (
            (LIFE_ROWS as f32 * view.aspect()).round() as usize,
            LIFE_ROWS,
        )
    }
}

/// The logo as live cells, at most three fifths of the board each way.
fn life_mask(logo: &image::RgbaImage, board_w: usize, board_h: usize) -> (Vec<bool>, usize, usize) {
    let aspect = logo.width() as f32 / logo.height().max(1) as f32;
    let mut w = (board_w as f32 * 0.6).floor().max(1.0);
    let mut h = (w / aspect).round().max(1.0);
    if h > board_h as f32 * 0.6 {
        h = (board_h as f32 * 0.6).floor().max(1.0);
        w = (h * aspect).round().max(1.0);
    }
    let (w, h) = (w as u32, h as u32);
    let small = image::imageops::resize(logo, w, h, image::imageops::FilterType::Triangle);
    let mask = small.pixels().map(|p| p.0[3] >= 128).collect();
    (mask, w as usize, h as usize)
}

/// A rendered style's picture size: `lines` tall on the scene's shape,
/// or the scene's own pixels in a 240p scene.
fn frame_size(view: View, lines: usize) -> (usize, usize) {
    if view.low_res {
        (
            view.width.round().clamp(8.0, 400.0) as usize,
            view.height.round().clamp(8.0, 300.0) as usize,
        )
    } else {
        ((lines as f32 * view.aspect()).round() as usize, lines)
    }
}

fn texture(image: &Image) -> Option<Texture> {
    let buffer = image.to_rgba8()?;
    let source =
        image::RgbaImage::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())?;
    Some(texture_from(&source))
}

/// A logo as a texture `MAZE_TEXTURE` tall, keeping its shape.
fn logo_texture(image: &Image) -> Option<Texture> {
    let buffer = image.to_rgba8()?;
    let (w, h) = (buffer.width().max(1), buffer.height().max(1));
    let source = image::RgbaImage::from_raw(w, h, buffer.as_bytes().to_vec())?;
    let height = MAZE_TEXTURE;
    let width =
        (u64::from(w) * u64::from(height) / u64::from(h)).clamp(1, u64::from(height) * 3) as u32;
    let small = image::imageops::resize(
        &source,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    Some(Texture {
        width: width as usize,
        height: height as usize,
        rgba: small.into_raw(),
    })
}

fn texture_from(source: &image::RgbaImage) -> Texture {
    let small = image::imageops::resize(
        source,
        MAZE_TEXTURE,
        MAZE_TEXTURE,
        image::imageops::FilterType::Triangle,
    );
    Texture {
        width: MAZE_TEXTURE as usize,
        height: MAZE_TEXTURE as usize,
        rgba: small.into_raw(),
    }
}

/// A fresh seed from the clock: every run differs.
pub(crate) fn seed_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

/// A generator for `resolve`, seeded from the clock.
pub(crate) fn rng_now() -> Rng {
    Rng::new(seed_now())
}
