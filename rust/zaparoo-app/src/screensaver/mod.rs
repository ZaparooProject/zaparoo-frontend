// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The screensaver: which style plays, what the Bouncing style bounces,
//! and each style's motion as a pure, seeded step function.
//!
//! Every style draws on solid black with art the app already has: the
//! Zaparoo logo, the embedded system logos and the covers Core scraped.
//! The adapter owns the timers and the images; the rules here decide where
//! things are and what they show. Positions are in *height units*: the
//! scene is `aspect` wide and 1 tall, so a square stays square on any
//! output.

pub mod bounce;
pub mod contour;
pub mod flyby;
pub mod hops;
pub mod life;
pub mod marquee;
pub mod maze;
pub mod paddle;
pub mod pipes;
pub mod polygons;
pub mod spin;
pub mod stars;
pub mod wall;

/// A screensaver style, in the order the picker offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Style {
    #[default]
    Bouncing,
    SpinningLogo,
    SystemLogos,
    CoverWall,
    RecentGames,
    PaddleClock,
    Starfield,
    Flyby,
    Polygons,
    Pipes,
    Life,
    Maze,
    Random,
}

/// Every style token, in picker order.
pub const STYLES: &[&str] = &[
    "bouncing",
    "spinning-logo",
    "system-logos",
    "cover-wall",
    "recent-games",
    "paddle-clock",
    "starfield",
    "flyby",
    "polygons",
    "pipes",
    "life",
    "maze",
    "random",
];

/// The persisted default style.
pub const DEFAULT_STYLE: &str = "bouncing";

impl Style {
    /// Every style but Random, which only ever resolves to one of these.
    pub const PLAYABLE: [Self; 12] = [
        Self::Bouncing,
        Self::SpinningLogo,
        Self::SystemLogos,
        Self::CoverWall,
        Self::RecentGames,
        Self::PaddleClock,
        Self::Starfield,
        Self::Flyby,
        Self::Polygons,
        Self::Pipes,
        Self::Life,
        Self::Maze,
    ];

    /// A persisted token; anything unknown is Bouncing.
    pub fn parse(token: &str) -> Self {
        match token.trim() {
            "spinning-logo" => Self::SpinningLogo,
            "system-logos" => Self::SystemLogos,
            "cover-wall" => Self::CoverWall,
            "recent-games" => Self::RecentGames,
            "paddle-clock" => Self::PaddleClock,
            "starfield" => Self::Starfield,
            "flyby" => Self::Flyby,
            "polygons" => Self::Polygons,
            "pipes" => Self::Pipes,
            "life" => Self::Life,
            "maze" => Self::Maze,
            "random" => Self::Random,
            _ => Self::Bouncing,
        }
    }

    /// The persisted token for this style.
    pub fn token(self) -> &'static str {
        match self {
            Self::Bouncing => "bouncing",
            Self::SpinningLogo => "spinning-logo",
            Self::SystemLogos => "system-logos",
            Self::CoverWall => "cover-wall",
            Self::RecentGames => "recent-games",
            Self::PaddleClock => "paddle-clock",
            Self::Starfield => "starfield",
            Self::Flyby => "flyby",
            Self::Polygons => "polygons",
            Self::Pipes => "pipes",
            Self::Life => "life",
            Self::Maze => "maze",
            Self::Random => "random",
        }
    }

    /// Whether the style draws into a pixel buffer the adapter renders
    /// every frame, rather than moving a few Slint items.
    pub fn renders_frames(self) -> bool {
        matches!(self, Self::Life | Self::Maze | Self::Polygons)
    }

    /// Whether the style repaints most of the screen every step, so the
    /// renderer should draw it at a reduced resolution and let the
    /// display scale it up: the picture styles and the starfield and
    /// flyby, whose movers spread over the whole screen.
    pub fn low_resolution(self) -> bool {
        self.renders_frames() || matches!(self, Self::Starfield | Self::Flyby)
    }
}

/// What the Bouncing style bounces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Bouncing {
    /// The header logo: the Zaparoo logo, or the user's custom image.
    #[default]
    Logo,
    Clock,
    /// A recently played game's cover with its name and system, a new
    /// game at each wall hit.
    RecentGame,
}

/// Every Bouncing token, in picker order.
pub const BOUNCINGS: &[&str] = &["logo", "clock", "recent-game"];

/// The persisted default. An empty saved value also reads as this; the
/// migration below tells the two apart.
pub const DEFAULT_BOUNCING: &str = "logo";

impl Bouncing {
    /// A persisted token; anything unknown (empty included) is Logo.
    pub fn parse(token: &str) -> Self {
        match token.trim() {
            "clock" => Self::Clock,
            "recent-game" => Self::RecentGame,
            _ => Self::Logo,
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::Clock => "clock",
            Self::RecentGame => "recent-game",
        }
    }
}

/// The Bouncing choice after the header logo changes to `header`: hiding
/// the logo turns a bouncing logo into the clock. Nothing else couples
/// them, so every other change keeps the choice.
pub fn after_header_logo(bouncing: Bouncing, header: crate::branding::HeaderLogo) -> Bouncing {
    if header == crate::branding::HeaderLogo::None && bouncing == Bouncing::Logo {
        Bouncing::Clock
    } else {
        bouncing
    }
}

/// The Bouncing token to save on first load, if any: an install that had
/// already hidden its header logo saw a bouncing clock before this setting
/// existed, so an unset value becomes Clock there. `None` leaves the saved
/// value as it is.
pub fn migrate(saved_bouncing: &str, header_logo: &str) -> Option<&'static str> {
    if saved_bouncing.trim().is_empty()
        && crate::branding::HeaderLogo::parse(header_logo) == crate::branding::HeaderLogo::None
    {
        Some(Bouncing::Clock.token())
    } else {
        None
    }
}

/// The covers the Cover wall wants before it plays; with fewer it falls
/// back to the bouncing logo.
pub const WALL_MIN_COVERS: usize = 8;

/// What the art pool holds when the saver arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Art {
    /// Covers decoded and ready to draw.
    pub covers: usize,
    /// Recently played games with a name to show.
    pub recents: usize,
    /// Systems with games, whose logos can show.
    pub systems: usize,
}

/// What actually plays: a style that is never Random, and the Bouncing
/// content when that style is Bouncing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub style: Style,
    pub bouncing: Bouncing,
}

/// Whether `style` (with `bouncing` for the Bouncing style) can play with
/// `art` as it is, rather than falling back.
pub fn playable(style: Style, bouncing: Bouncing, art: Art) -> bool {
    match style {
        Style::CoverWall => art.covers >= WALL_MIN_COVERS,
        Style::Bouncing => {
            bouncing != Bouncing::RecentGame || (art.covers >= 1 && art.recents >= 1)
        }
        Style::Random => false,
        _ => true,
    }
}

/// The style that plays for the saved `style` and `bouncing`, given the
/// art on hand and the style that played last (Random never repeats it).
/// A style that cannot play falls back to the bouncing logo.
pub fn resolve(
    style: Style,
    bouncing: Bouncing,
    art: Art,
    last: Option<Style>,
    rng: &mut Rng,
) -> Resolved {
    let style = if style == Style::Random {
        let choices: Vec<Style> = Style::PLAYABLE
            .iter()
            .copied()
            .filter(|s| Some(*s) != last && playable(*s, bouncing, art))
            .collect();
        if choices.is_empty() {
            Style::Bouncing
        } else {
            choices[rng.below(choices.len())]
        }
    } else {
        style
    };
    if playable(style, bouncing, art) {
        Resolved { style, bouncing }
    } else {
        Resolved {
            style: Style::Bouncing,
            bouncing: Bouncing::Logo,
        }
    }
}

/// A small seeded generator (splitmix64): the styles need cheap,
/// repeatable randomness, not quality.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`; 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// A value in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A value in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// Fisher-Yates in place.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// What a sprite draws. Indexes point into the adapter's lists: the
/// system logos and covers it gathered for this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The Zaparoo logo (or the user's custom image where a style says so).
    Logo,
    /// The Zaparoo app icon (`resources/images/logo/icon-256.png`).
    Mark,
    Clock,
    System(usize),
    Cover(usize),
    /// A flat rectangle in palette slot `n`.
    Block(u8),
    /// A disc in palette slot `n`.
    Round(u8),
}

/// One thing on screen, in height units (see the module docs): its
/// top-left corner, its size, and how opaque it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sprite {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub opacity: f32,
    pub look: Look,
}

impl Sprite {
    pub fn new(x: f32, y: f32, w: f32, h: f32, look: Look) -> Self {
        Self {
            x,
            y,
            w,
            h,
            opacity: 1.0,
            look,
        }
    }
}

/// The colors a style picks from, in palette slots: the accent family,
/// all readable on black.
pub const SLOTS: u8 = 6;

/// An RGB color for palette slot `n` (mod `SLOTS`): fixed hues the styles
/// share, picked to read on black on every output, CRT included.
pub fn slot_rgb(n: u8) -> (u8, u8, u8) {
    match n % SLOTS {
        0 => (0xF5, 0xB0, 0x41), // amber
        1 => (0xE0, 0x3E, 0xC8), // magenta
        2 => (0x3A, 0x8B, 0xF5), // blue
        3 => (0x3E, 0xD0, 0x8C), // green
        4 => (0xF2, 0x5C, 0x54), // red
        _ => (0x9B, 0x6B, 0xF2), // violet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branding::HeaderLogo;

    #[test]
    fn tokens_round_trip_and_unknown_reads_default() {
        for token in STYLES {
            assert_eq!(Style::parse(token).token(), *token);
        }
        for token in BOUNCINGS {
            assert_eq!(Bouncing::parse(token).token(), *token);
        }
        assert_eq!(Style::parse("toasters"), Style::Bouncing);
        assert_eq!(Bouncing::parse(""), Bouncing::Logo);
        assert_eq!(Bouncing::parse("nope"), Bouncing::Logo);
        assert_eq!(STYLES.len(), Style::PLAYABLE.len() + 1);
    }

    #[test]
    fn full_screen_styles_render_at_low_resolution() {
        let low: Vec<Style> = Style::PLAYABLE
            .iter()
            .copied()
            .filter(|s| s.low_resolution())
            .collect();
        assert_eq!(
            low,
            [
                Style::Starfield,
                Style::Flyby,
                Style::Polygons,
                Style::Life,
                Style::Maze
            ]
        );
        assert!(Style::PLAYABLE
            .iter()
            .filter(|s| s.renders_frames())
            .all(|s| s.low_resolution()));
    }

    #[test]
    fn hiding_the_logo_turns_only_a_bouncing_logo_into_the_clock() {
        assert_eq!(
            after_header_logo(Bouncing::Logo, HeaderLogo::None),
            Bouncing::Clock
        );
        assert_eq!(
            after_header_logo(Bouncing::RecentGame, HeaderLogo::None),
            Bouncing::RecentGame
        );
        assert_eq!(
            after_header_logo(Bouncing::Logo, HeaderLogo::Custom),
            Bouncing::Logo
        );
        assert_eq!(
            after_header_logo(Bouncing::Clock, HeaderLogo::Zaparoo),
            Bouncing::Clock
        );
    }

    #[test]
    fn migration_only_sets_clock_for_an_unset_value_with_no_logo() {
        assert_eq!(migrate("", "none"), Some("clock"));
        assert_eq!(migrate("  ", "none"), Some("clock"));
        assert_eq!(migrate("logo", "none"), None);
        assert_eq!(migrate("", "zaparoo"), None);
        assert_eq!(migrate("", "custom"), None);
    }

    #[test]
    fn styles_short_of_covers_fall_back_to_the_bouncing_logo() {
        let mut rng = Rng::new(1);
        let none = Art::default();
        let r = resolve(Style::CoverWall, Bouncing::Clock, none, None, &mut rng);
        assert_eq!((r.style, r.bouncing), (Style::Bouncing, Bouncing::Logo));
        let r = resolve(Style::Bouncing, Bouncing::RecentGame, none, None, &mut rng);
        assert_eq!((r.style, r.bouncing), (Style::Bouncing, Bouncing::Logo));
        let art = Art {
            covers: 8,
            recents: 3,
            systems: 4,
        };
        let r = resolve(Style::CoverWall, Bouncing::Clock, art, None, &mut rng);
        assert_eq!(r.style, Style::CoverWall);
        let r = resolve(Style::Maze, Bouncing::Logo, none, None, &mut rng);
        assert_eq!(r.style, Style::Maze, "covers on its walls are optional");
    }

    #[test]
    fn random_never_repeats_and_skips_styles_that_would_fall_back() {
        let mut rng = Rng::new(7);
        let art = Art {
            covers: 2,
            recents: 1,
            systems: 3,
        };
        let mut last = None;
        for _ in 0..500 {
            let r = resolve(Style::Random, Bouncing::Clock, art, last, &mut rng);
            assert_ne!(Some(r.style), last);
            assert_ne!(r.style, Style::Random);
            assert_ne!(r.style, Style::CoverWall, "two covers cannot fill a wall");
            if r.style == Style::Bouncing {
                assert_eq!(r.bouncing, Bouncing::Clock, "Random keeps the saved choice");
            }
            last = Some(r.style);
        }
    }

    #[test]
    fn random_reaches_every_playable_style() {
        let mut rng = Rng::new(3);
        let art = Art {
            covers: 20,
            recents: 5,
            systems: 9,
        };
        let mut seen = std::collections::HashSet::new();
        let mut last = None;
        for _ in 0..2000 {
            let r = resolve(Style::Random, Bouncing::Logo, art, last, &mut rng);
            seen.insert(r.style);
            last = Some(r.style);
        }
        assert_eq!(seen.len(), Style::PLAYABLE.len());
    }

    #[test]
    fn rng_is_repeatable_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
            let u = a.unit();
            assert!((0.0..1.0).contains(&u));
            assert!(a.below(5) < 5);
            b.unit();
            b.below(5);
        }
        assert_eq!(a.below(0), 0);
    }
}
