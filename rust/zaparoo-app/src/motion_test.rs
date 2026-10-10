// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The `ZAPAROO_MOTION` test switch. The software-rendered build runs every
//! motion effect the GPU build does; this names the ones to turn back off
//! for one run, so each can be measured on a device against the cut it
//! replaced. Unset or empty leaves everything on.

/// The effects one run turns off. Each accessor is true when its effect was
/// named, and the driver then keeps the behavior that effect replaced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "five independent effects, each switched by its own name"
)]
pub struct Disabled {
    zoom: bool,
    cover_fade: bool,
    rail: bool,
    slides: bool,
    clock: bool,
}

impl Disabled {
    /// Reads a comma-separated list of effect names. Names are trimmed and
    /// matched exactly; the ones that match nothing come back so the caller
    /// can report them once.
    pub fn parse(value: &str) -> (Self, Vec<String>) {
        let mut disabled = Self::default();
        let mut unknown = Vec::new();
        for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match name {
                "zoom" => disabled.zoom = true,
                "cover-fade" => disabled.cover_fade = true,
                "rail" => disabled.rail = true,
                "slides" => disabled.slides = true,
                "clock" => disabled.clock = true,
                other => unknown.push(other.to_string()),
            }
        }
        (disabled, unknown)
    }

    /// The focused tile's growth.
    pub const fn zoom(self) -> bool {
        self.zoom
    }

    /// The fade of cover art that lands while its tile is on screen.
    pub const fn cover_fade(self) -> bool {
        self.cover_fade
    }

    /// The fast-scroll rail's fade and its highlight's glide.
    pub const fn rail(self) -> bool {
        self.rail
    }

    /// Cached page slides on the presenters that did not have one.
    pub const fn slides(self) -> bool {
        self.slides
    }

    /// The fixed-step animation clock.
    pub const fn clock(self) -> bool {
        self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_empty_leaves_every_effect_on() {
        for value in ["", " ", ",", " , ,"] {
            let (disabled, unknown) = Disabled::parse(value);
            assert_eq!(disabled, Disabled::default(), "{value:?}");
            assert!(unknown.is_empty(), "{value:?}");
        }
        let on = Disabled::default();
        assert!(!on.zoom() && !on.cover_fade() && !on.rail());
        assert!(!on.slides() && !on.clock());
    }

    #[test]
    fn each_name_turns_off_only_its_own_effect() {
        let flags = |d: Disabled| [d.zoom(), d.cover_fade(), d.rail(), d.slides(), d.clock()];
        for (index, name) in ["zoom", "cover-fade", "rail", "slides", "clock"]
            .into_iter()
            .enumerate()
        {
            let (disabled, unknown) = Disabled::parse(name);
            let mut expected = [false; 5];
            expected[index] = true;
            assert_eq!(flags(disabled), expected, "{name}");
            assert!(unknown.is_empty(), "{name}");
        }
    }

    #[test]
    fn lists_combine_and_tolerate_spaces_and_repeats() {
        let (disabled, unknown) = Disabled::parse(" zoom , clock,zoom,,slides ");
        assert!(disabled.zoom() && disabled.clock() && disabled.slides());
        assert!(!disabled.cover_fade() && !disabled.rail());
        assert!(unknown.is_empty());
    }

    #[test]
    fn unknown_names_are_ignored_and_reported() {
        let (disabled, unknown) = Disabled::parse("zoom,Zoom,cover_fade,all");
        assert!(disabled.zoom());
        assert!(!disabled.cover_fade());
        assert_eq!(unknown, ["Zoom", "cover_fade", "all"]);
    }
}
