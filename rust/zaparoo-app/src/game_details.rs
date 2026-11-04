// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Game details: which of a game's tags show, how they group, and what
//! each value is called. Ported from the Zaparoo App's details modal
//! (`libraryMedia.ts`, `libraryTagLabels.ts`) so both read the same.
//!
//! Core sends a label only for free-text values; canonical vocabulary
//! values arrive bare, so the closed vocabularies are named here. A name is
//! a key the view translates (`Name::key`), or text shown as it is.

/// One tag as Core sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag<'a> {
    pub tag_type: &'a str,
    pub tag: &'a str,
    pub label: &'a str,
}

/// What a tag value is called. `key` empty: `text` as it is. Otherwise
/// `key` names a string the view translates, `arg` filling its slot:
/// - `v:<vocabulary>.<entry>`: a vocabulary name, or a counting template
///   such as `v:input.joystickWays` ("8-way joystick").
/// - `numbered`: a numbered build of the stage `text` names, a `v:` key
///   ("Beta 2").
/// - `rev`: "Revision {}".
/// - `players`: a player count, `arg` the number.
/// - `players-of`: "{} players" around text that is not a count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Name {
    pub key: String,
    pub arg: String,
    pub text: String,
}

impl Name {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    fn key(key: impl Into<String>, arg: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            arg: arg.into(),
            text: String::new(),
        }
    }
}

/// One value of a fact: what it is called, and the `type:tag` a search for
/// it asks Core for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub kind: String,
    pub name: Name,
    pub search: String,
}

/// A labelled row: every value of one tag type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub kind: &'static str,
    pub values: Vec<Value>,
}

/// A game's tags, sorted for the details view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    /// The facts most people read, in reading order.
    pub facts: Vec<Fact>,
    /// Facts about the dump or the hardware, shown last.
    pub technical: Vec<Fact>,
    /// Every other tag, on one line as `type:name`.
    pub tags: Vec<Value>,
}

/// Every tag type with a row of its own, the most-read first.
pub const FACT_TYPES: &[&str] = &[
    "year",
    "players",
    "developer",
    "publisher",
    "genre",
    "gamegenre",
    "rating",
    "gamefamily",
    "series",
    "arcadeboard",
    "region",
    "lang",
    "edition",
    "rev",
    "disc",
    "disctotal",
    "release",
    "rerelease",
    "builddate",
    "input",
    "perspective",
    "art",
    "category",
    "based",
    "platform",
    "port",
    "media",
    "video",
    "save",
    "addon",
    "embedded",
    "cabinet",
    "protection",
    "mameparent",
    "compatibility",
    "accessibility",
    "distribution",
    "copyright",
    "credit",
    "dump",
    "alt",
    "patch",
    "unfinished",
    "unlicensed",
    "reboxed",
    "multigame",
    "set",
    "supplement",
    "season",
    "episode",
    "volume",
    "issue",
    "track",
    "extension",
];

/// Reference facts about the dump or the hardware.
pub const TECHNICAL_TYPES: &[&str] = &[
    "builddate",
    "media",
    "video",
    "embedded",
    "cabinet",
    "protection",
    "mameparent",
    "compatibility",
    "distribution",
    "copyright",
    "dump",
    "alt",
    "reboxed",
    "set",
    "extension",
];

/// The user's own marks on a game: shown by the UI's own controls, never
/// as tags.
const PREFERENCES: &[&str] = &["favorite", "liked", "disliked", "playlater"];

/// Tags the details never show: preferences, deck membership, and what a
/// scraper recorded about its own run.
pub fn hidden(tag: &Tag<'_>) -> bool {
    let kind = tag.tag_type.trim().to_ascii_lowercase();
    let value = tag.tag.trim().to_ascii_lowercase();
    (kind == "user" && (PREFERENCES.contains(&value.as_str()) || value.starts_with("deck:")))
        || kind.starts_with("scraper.")
}

/// Sorts `groups` of tags (title tags first, then the media's) for the
/// details view. A tag in more than one group shows once.
pub fn organize(groups: &[&[Tag<'_>]]) -> Details {
    let mut seen = std::collections::HashSet::new();
    let visible: Vec<&Tag<'_>> = groups
        .iter()
        .flat_map(|group| group.iter())
        .filter(|tag| seen.insert((tag.tag_type, tag.tag)))
        .filter(|tag| !hidden(tag))
        .collect();
    let kind_of = |tag: &Tag<'_>| tag.tag_type.trim().to_ascii_lowercase();
    let mut details = Details::default();
    for kind in FACT_TYPES {
        let values: Vec<Value> = visible
            .iter()
            .filter(|tag| kind_of(tag) == *kind)
            .filter(|tag| {
                let shown = if tag.label.trim().is_empty() {
                    tag.tag
                } else {
                    tag.label
                };
                !shown.trim().is_empty()
            })
            .flat_map(|tag| values(tag))
            .collect();
        if values.is_empty() {
            continue;
        }
        let fact = Fact { kind, values };
        if TECHNICAL_TYPES.contains(kind) {
            details.technical.push(fact);
        } else {
            details.facts.push(fact);
        }
    }
    details.tags = visible
        .iter()
        .filter(|tag| !FACT_TYPES.contains(&kind_of(tag).as_str()))
        .filter(|tag| !tag.tag.trim().is_empty())
        .flat_map(|tag| values(tag))
        .collect();
    details
}

/// The values one tag reads as. A comma-separated list Core sent as one
/// tag is named part by part when any part is known; each part searches
/// for the whole tag.
pub fn values(tag: &Tag<'_>) -> Vec<Value> {
    let kind = tag.tag_type.trim().to_ascii_lowercase();
    let raw = tag.tag.trim();
    let search = format!("{}:{}", tag.tag_type.trim(), raw);
    let value = |name: Name| Value {
        kind: kind.clone(),
        name,
        search: search.clone(),
    };
    let label = tag.label.trim();
    if !label.is_empty() && label != raw {
        return vec![value(Name::text(label))];
    }
    if raw.contains(',') {
        let parts: Vec<&str> = raw
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        let names: Vec<Name> = parts.iter().map(|part| single(&kind, part)).collect();
        if names
            .iter()
            .zip(&parts)
            .any(|(name, part)| !name.key.is_empty() || name.text != *part)
        {
            return names.into_iter().map(value).collect();
        }
        return vec![value(Name::text(raw))];
    }
    vec![value(single(&kind, raw))]
}

const REGION_KEYS: &[(&str, &str)] = &[
    ("world", "world"),
    ("ww", "world"),
    ("us", "us"),
    ("usa", "us"),
    ("eu", "eu"),
    ("europe", "eu"),
    ("jp", "jp"),
    ("japan", "jp"),
    ("au", "au"),
    ("kr", "kr"),
    ("cn", "cn"),
    ("tw", "tw"),
    ("hk", "hk"),
    ("br", "br"),
    ("ca", "ca"),
    ("uk", "uk"),
    ("gb", "uk"),
    ("fr", "fr"),
    ("de", "de"),
    ("it", "it"),
    ("es", "es"),
    ("scandinavia", "scandinavia"),
    ("asia", "asia"),
];

const LANGUAGE_KEYS: &[(&str, &str)] = &[
    ("en", "en"),
    ("eng", "en"),
    ("fr", "fr"),
    ("fre", "fr"),
    ("fra", "fr"),
    ("de", "de"),
    ("ger", "de"),
    ("deu", "de"),
    ("es", "es"),
    ("spa", "es"),
    ("it", "it"),
    ("ita", "it"),
    ("ja", "ja"),
    ("jpn", "ja"),
    ("ko", "ko"),
    ("kor", "ko"),
    ("zh", "zh"),
    ("chi", "zh"),
    ("zho", "zh"),
    ("pt", "pt"),
    ("por", "pt"),
    ("nl", "nl"),
    ("dut", "nl"),
    ("nld", "nl"),
    ("sv", "sv"),
    ("no", "no"),
    ("da", "da"),
    ("fi", "fi"),
    ("pl", "pl"),
    ("pol", "pl"),
    ("ru", "ru"),
    ("rus", "ru"),
    ("ar", "ar"),
    ("ara", "ar"),
    ("en-us", "en-US"),
    ("en-gb", "en-GB"),
];

const EDITION_KEYS: &[(&str, &str)] = &[
    ("directors-cut", "directorsCut"),
    ("collectors", "collectors"),
    ("limited", "limited"),
    ("special", "special"),
    ("deluxe", "deluxe"),
    ("ultimate", "ultimate"),
    ("anniversary", "anniversary"),
    ("remaster", "remastered"),
    ("remastered", "remastered"),
    ("remake", "remake"),
    ("remix", "remix"),
    ("complete", "complete"),
    ("goty", "goty"),
];

/// Closed vocabularies whose entries the view names: the Core value with
/// `:` replaced by `_`.
const VOCABULARIES: &[(&str, &[&str])] = &[
    (
        "genre",
        &[
            "action",
            "action_platformer",
            "action_maze",
            "action_blockbreaker",
            "action_runandgun",
            "action_hackandslash",
            "action_metroidvania",
            "action_roguelite",
            "adventure",
            "adventure_pointandclick",
            "adventure_visualnovel",
            "adventure_survivalhorror",
            "adventure_text",
            "board",
            "board_cards",
            "board_hanafuda",
            "board_chess",
            "board_shougi",
            "board_go",
            "board_mahjong",
            "board_reversi",
            "board_othello",
            "board_backgammon",
            "board_party",
            "board_jankenpon",
            "brawler",
            "fighting",
            "fighting_melee",
            "minigames",
            "parlor",
            "parlor_pinball",
            "parlor_jackpot",
            "parlor_pachinko",
            "parlor_darts",
            "parlor_bowling",
            "parlor_billiards",
            "parlor_mogurataiji",
            "parlor_kiddieride",
            "parlor_mechanical",
            "quiz",
            "racing",
            "racing_combat",
            "racing_driving",
            "rpg",
            "rpg_a",
            "rpg_j",
            "rpg_s",
            "rpg_dungeoncrawler",
            "rpg_mmo",
            "rhythm",
            "rhythm_karaoke",
            "rhythm_dance",
            "shmup",
            "shmup_h",
            "shmup_v",
            "shmup_i",
            "shmup_danmaku",
            "shooting",
            "shooting_gallery",
            "shooting_rail",
            "shooting_fps",
            "shooting_tps",
            "puzzle",
            "puzzle_drop",
            "puzzle_mind",
            "sim",
            "sim_strategy",
            "sim_cardgame",
            "sim_flight",
            "sim_train",
            "sim_date",
            "sim_otome",
            "sim_life",
            "sim_farm",
            "sim_pet",
            "sim_fishing",
            "sim_god",
            "sim_derby",
            "sim_building",
            "sim_cooking",
            "sports",
            "sports_soccer",
            "sports_basketball",
            "sports_baseball",
            "sports_volleyball",
            "sports_rugby",
            "sports_football",
            "sports_dodgeball",
            "sports_hockey",
            "sports_skiing",
            "sports_skateboarding",
            "sports_snowboarding",
            "sports_tennis",
            "sports_pingpong",
            "sports_paddle",
            "sports_squash",
            "sports_badminton",
            "sports_flyingdisc",
            "sports_cycling",
            "sports_formula1",
            "sports_rally",
            "sports_nascar",
            "sports_motogp",
            "sports_motocross",
            "sports_karting",
            "sports_jetski",
            "sports_golf",
            "sports_cricket",
            "sports_boxing",
            "sports_kickboxing",
            "sports_wrestling",
            "sports_sumo",
            "sports_karate",
            "sports_judo",
            "sports_kendo",
            "sports_taekwondo",
            "sports_mma",
            "sports_decathlon",
            "sports_running",
            "sports_archery",
            "sports_swimming",
            "sports_rowing",
            "sports_kayak",
            "sports_surf",
            "notagame",
            "notagame_educational",
            "notagame_drawing",
            "notagame_popcorn",
            "notagame_purikura",
            "notagame_redemption",
            "notagame_media",
            "notagame_magazine",
            "notagame_application",
            "notagame_test",
            "notagame_sdk",
            "notagame_slideshow",
            "notagame_sound",
            "sim_management",
            "sports_roller",
        ],
    ),
    ("players", &["vs", "coop", "alt", "simultaneous", "mmo"]),
    (
        "input",
        &[
            "joystick",
            "joystick_2h",
            "joystick_2v",
            "joystick_double",
            "joystick_rotary",
            "stick",
            "stick_twin",
            "trackball",
            "paddle",
            "spinner",
            "wheel",
            "dial",
            "lightgun",
            "optical",
            "positional",
            "buttons",
            "buttons_pneumatic",
            "pedals",
            "puncher",
            "motion",
            "keyboard",
            "keyboard_mahjong",
            "touchscreen",
            "touchscreen_resistive",
            "touchscreen_capacitive",
            "accelerometer",
            "ddrpad",
            "hrm",
        ],
    ),
    ("save", &["backup", "password"]),
    (
        "media",
        &["disc", "disk", "file", "part", "side", "tape", "cart"],
    ),
    (
        "unfinished",
        &[
            "alpha",
            "beta",
            "proto",
            "demo",
            "demo_auto",
            "demo_kiosk",
            "demo_playable",
            "demo_rolling",
            "demo_slideshow",
            "sample",
            "debug",
            "competition",
            "preview",
            "prerelease",
            "final",
            "wip",
        ],
    ),
    (
        "unlicensed",
        &[
            "unlicensed",
            "bootleg",
            "hack",
            "clone",
            "pirate",
            "translation",
            "translation_old",
            "aftermarket",
        ],
    ),
    (
        "release",
        &[
            "homebrew",
            "unreleased",
            "public-domain",
            "reissue",
            "classics",
            "promo",
            "not-for-resale",
            "kiosk",
        ],
    ),
    (
        "dump",
        &[
            "cracked",
            "fixed",
            "hacked",
            "modified",
            "pirated",
            "trained",
            "translated",
            "overdump",
            "underdump",
            "virus",
            "bad",
            "alternate",
            "verified",
            "pending",
            "checksum-bad",
            "checksum-unknown",
            "hacked_intro",
            "hacked_intro-removed",
            "no-boot",
        ],
    ),
    (
        "copyright",
        &["cw", "cw-r", "fw", "gw", "gw-r", "lw", "pd", "sw", "sw-r"],
    ),
    (
        "perspective",
        &[
            "firstperson",
            "thirdperson",
            "topdown",
            "isometric",
            "fixedcamera",
            "sidescroll_horizontal",
            "sidescroll_vertical",
        ],
    ),
    (
        "art",
        &["pixelart", "celshaded", "vector", "digitized", "handdrawn"],
    ),
    (
        "accessibility",
        &[
            "visual_colorblindmode",
            "visual_highcontrast",
            "visual_textsizeadjust",
            "audio_subtitles",
            "audio_monoaudio",
            "audio_visualcues",
            "input_remappablecontrols",
            "input_onebuttonmode",
        ],
    ),
    ("cabinet", &["upright", "cocktail", "cabaret", "sitdown"]),
    ("protection", &["no-protection", "encrypted", "decrypted"]),
    ("supplement", &["update", "expansion", "theme", "avatar"]),
    ("based", &["manganime", "movie"]),
    ("multigame", &["compilation", "menu"]),
    ("alt", &["alt"]),
];

/// Numbered members of a vocabulary, shown through a counting template.
const COUNT_TEMPLATES: &[(&str, &[(&str, &str)])] = &[
    (
        "input",
        &[
            ("joystick", "joystickWays"),
            ("buttons", "buttonsCount"),
            ("pedals", "pedalsCount"),
            ("positional", "positionalCount"),
            ("puncher", "puncherCount"),
        ],
    ),
    ("multigame", &[("vol", "volume")]),
];

/// Codes and proper names that read the same in every language.
const FIXED_LABELS: &[(&str, &[(&str, &str)])] = &[
    (
        "video",
        &[
            ("ntsc", "NTSC"),
            ("pal", "PAL"),
            ("pal-60", "PAL 60"),
            ("ntsc-pal", "NTSC/PAL"),
            ("pal-ntsc", "PAL/NTSC"),
            ("cga", "CGA"),
            ("ega", "EGA"),
            ("hgc", "HGC"),
            ("mcga", "MCGA"),
            ("mda", "MDA"),
            ("svga", "SVGA"),
            ("vga", "VGA"),
            ("xga", "XGA"),
            ("15khz", "15 kHz"),
            ("31khz", "31 kHz"),
        ],
    ),
    (
        "media",
        &[
            ("n64dd", "64DD"),
            ("fds", "Famicom Disk System"),
            ("ereader", "e-Reader"),
            ("multiboot", "Multiboot"),
            ("lnx", "LNX"),
        ],
    ),
    ("art", &[("2d", "2D"), ("3d", "3D")]),
    ("supplement", &[("dlc", "DLC")]),
    ("dump", &[("bios", "BIOS"), ("hacked:ffe", "FFE")]),
    (
        "protection",
        &[
            ("fd1094", "FD1094"),
            ("fd1089", "FD1089"),
            ("mc-8123", "MC-8123"),
        ],
    ),
    (
        "based",
        &[
            ("disney", "Disney"),
            ("dnd", "Dungeons & Dragons"),
            ("jurassicpark", "Jurassic Park"),
            ("looneytunes", "Looney Tunes"),
            ("marvel", "Marvel"),
            ("simpsons", "The Simpsons"),
            ("smurfs", "The Smurfs"),
            ("starwars", "Star Wars"),
            ("tmnt", "Teenage Mutant Ninja Turtles"),
            ("cartoonnetwork", "Cartoon Network"),
            ("hannabarbera", "Hanna-Barbera"),
            ("nickelodeon", "Nickelodeon"),
        ],
    ),
    (
        "distribution",
        &[
            ("virtual-console", "Virtual Console"),
            ("wiiware", "WiiWare"),
            ("xblig", "Xbox Live Indie Games"),
            ("dsiware", "DSiWare"),
            ("gamecube", "GameCube"),
            ("switch-online", "Nintendo Switch Online"),
            ("disk-writer", "Disk Writer"),
            ("steam", "Steam"),
            ("sega-channel", "Sega Channel"),
            ("genesis-mini", "Genesis Mini"),
            ("sega-ages", "Sega Ages"),
            ("sega-smash-pack", "Sega Smash Pack"),
            ("wii", "Wii"),
            ("club-nintendo", "Club Nintendo"),
            ("gba-e-reader", "GBA e-Reader"),
        ],
    ),
    (
        "patch",
        &[
            ("fastrom", "FastROM"),
            ("slowrom", "SlowROM"),
            ("msu1", "MSU-1"),
            ("sa1", "SA-1"),
            ("sram", "SRAM"),
            ("no-sram", "No SRAM"),
            ("qol", "QoL"),
        ],
    ),
];

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn single(kind: &str, raw: &str) -> Name {
    let value = raw.to_ascii_lowercase();
    let mapped = match kind {
        "region" => lookup(REGION_KEYS, &value),
        "lang" => lookup(LANGUAGE_KEYS, &value),
        "edition" => lookup(EDITION_KEYS, &value),
        _ => None,
    };
    if let Some(entry) = mapped {
        return Name::key(format!("v:{kind}.{entry}"), "");
    }
    if kind == "rev" {
        let revision = [
            "revision-",
            "version-",
            "revision",
            "version",
            "rev-",
            "ver-",
        ]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(&value);
        if !revision.is_empty() && revision.bytes().all(|b| b.is_ascii_digit()) {
            return Name::key("rev", revision);
        }
    }
    let name = vocabulary(kind, &value).unwrap_or_else(|| Name::text(raw));
    if kind == "players" {
        return players(name, raw);
    }
    name
}

/// Player counts read "1 player" and "4 players"; a named mode reads as
/// its name; anything else, such as a range, reads "1-4 players".
fn players(name: Name, raw: &str) -> Name {
    if !name.key.is_empty() {
        return name;
    }
    if raw.parse::<u32>().is_ok() {
        return Name::key("players", raw);
    }
    Name::key("players-of", raw)
}

fn vocabulary(kind: &str, value: &str) -> Option<Name> {
    if let Some(fixed) = lookup(FIXED_LABELS, kind) {
        if let Some(text) = lookup(fixed, value) {
            return Some(Name::text(text));
        }
    }
    let name = if kind == "gamegenre" { "genre" } else { kind };
    let entries = lookup(VOCABULARIES, name)?;
    let key = value.replace(':', "_");
    if entries.contains(&key.as_str()) {
        return Some(Name::key(format!("v:{name}.{key}"), ""));
    }
    if let Some((family, count)) = value.split_once(':') {
        if !family.is_empty()
            && family.bytes().all(|b| b.is_ascii_lowercase())
            && !count.is_empty()
            && count.bytes().all(|b| b.is_ascii_digit())
        {
            if let Some(template) = lookup(COUNT_TEMPLATES, name).and_then(|t| lookup(t, family)) {
                return Some(Name::key(format!("v:{name}.{template}"), count));
            }
            // beta:2, proto:3 and demo:1 are numbered builds of a named stage.
            if name == "unfinished" && entries.contains(&family) {
                return Some(Name {
                    key: "numbered".into(),
                    arg: count.into(),
                    text: format!("v:{name}.{family}"),
                });
            }
        }
    }
    if name == "media" {
        if let Some(side) = value.strip_prefix("side-") {
            if side.len() == 1 && side.bytes().all(|b| b.is_ascii_lowercase()) {
                return Some(Name::key("v:media.sideLetter", side.to_ascii_uppercase()));
            }
        }
    }
    None
}

/// What focus can rest on in the details, top to bottom: the art, the
/// action row, the description, and each row that searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Image,
    Manual,
    Description,
    /// A row of the table, by its index there.
    Row(usize),
}

/// The details' stops in reading order. `searchable` says, for each table
/// row, whether A on it searches.
pub fn stops(image: bool, manual: bool, description: bool, searchable: &[bool]) -> Vec<Stop> {
    let mut stops = Vec::new();
    if image {
        stops.push(Stop::Image);
    }
    if manual {
        stops.push(Stop::Manual);
    }
    if description {
        stops.push(Stop::Description);
    }
    stops.extend(
        searchable
            .iter()
            .enumerate()
            .filter(|(_, searchable)| **searchable)
            .map(|(row, _)| Stop::Row(row)),
    );
    stops
}

/// Focus `delta` stops on from `focus`, stopping at either end.
pub fn step(focus: usize, delta: i32, stops: usize) -> usize {
    let last = stops.saturating_sub(1) as i64;
    (focus as i64 + i64::from(delta)).clamp(0, last) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag<'a>(tag_type: &'a str, value: &'a str) -> Tag<'a> {
        Tag {
            tag_type,
            tag: value,
            label: "",
        }
    }

    fn names(values: &[Value]) -> Vec<(String, String, String)> {
        values
            .iter()
            .map(|v| (v.name.key.clone(), v.name.arg.clone(), v.name.text.clone()))
            .collect()
    }

    #[test]
    fn stops_run_top_to_bottom_over_what_acts() {
        assert_eq!(
            stops(true, true, true, &[false, true, false, true]),
            [
                Stop::Image,
                Stop::Manual,
                Stop::Description,
                Stop::Row(1),
                Stop::Row(3)
            ]
        );
        assert_eq!(stops(false, false, false, &[false]), []);
        assert_eq!(step(0, -1, 4), 0);
        assert_eq!(step(2, 1, 4), 3);
        assert_eq!(step(3, 1, 4), 3);
        assert_eq!(step(0, 1, 0), 0);
    }

    #[test]
    fn values_of_one_type_share_a_row_in_reading_order() {
        let title = [
            tag("genre", "action:platformer"),
            tag("genre", "puzzle"),
            tag("year", "1994"),
        ];
        let media = [tag("players", "2"), tag("genre", "puzzle")];
        let details = organize(&[&title, &media]);
        let kinds: Vec<_> = details.facts.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, ["year", "players", "genre"]);
        let genre = &details.facts[2];
        assert_eq!(genre.values.len(), 2, "the duplicate shows once");
        assert_eq!(
            names(&genre.values),
            [
                (
                    "v:genre.action_platformer".into(),
                    String::new(),
                    String::new()
                ),
                ("v:genre.puzzle".into(), String::new(), String::new()),
            ]
        );
        assert_eq!(genre.values[0].search, "genre:action:platformer");
    }

    #[test]
    fn internal_tags_never_show_and_unknown_ones_share_one_line() {
        let tags = [
            tag("scraper.gamelist.xml", "matched"),
            tag("user", "favorite"),
            tag("user", "deck:abc"),
            tag("user", "hidden"),
            tag("custom", "thing"),
            tag("dump", "verified"),
        ];
        let details = organize(&[&tags]);
        assert!(details.facts.is_empty());
        assert_eq!(details.technical.len(), 1);
        assert_eq!(details.technical[0].kind, "dump");
        let leftovers: Vec<_> = details
            .tags
            .iter()
            .map(|v| (v.kind.as_str(), v.name.text.as_str()))
            .collect();
        assert_eq!(leftovers, [("user", "hidden"), ("custom", "thing")]);
    }

    #[test]
    fn a_core_label_wins_and_free_text_shows_as_sent() {
        let labelled = Tag {
            tag_type: "developer",
            tag: "capcom",
            label: "Capcom",
        };
        assert_eq!(values(&labelled)[0].name, Name::text("Capcom"));
        assert_eq!(
            values(&tag("developer", "Konami"))[0].name,
            Name::text("Konami")
        );
        assert_eq!(
            values(&tag("genre", "action:foo"))[0].name,
            Name::text("action:foo")
        );
    }

    #[test]
    fn codes_counts_and_revisions_are_named() {
        let name = |kind: &str, value: &str| values(&tag(kind, value))[0].name.clone();
        assert_eq!(name("region", "USA"), Name::key("v:region.us", ""));
        assert_eq!(name("lang", "jpn"), Name::key("v:lang.ja", ""));
        assert_eq!(
            name("edition", "remaster"),
            Name::key("v:edition.remastered", "")
        );
        assert_eq!(name("rev", "revision-2"), Name::key("rev", "2"));
        assert_eq!(name("rev", "a"), Name::text("a"));
        assert_eq!(name("video", "pal-60"), Name::text("PAL 60"));
        assert_eq!(
            name("input", "joystick:8"),
            Name::key("v:input.joystickWays", "8")
        );
        assert_eq!(
            name("unfinished", "beta:2"),
            Name {
                key: "numbered".into(),
                arg: "2".into(),
                text: "v:unfinished.beta".into()
            }
        );
        assert_eq!(
            name("media", "side-a"),
            Name::key("v:media.sideLetter", "A")
        );
        assert_eq!(name("gamegenre", "rpg:j"), Name::key("v:genre.rpg_j", ""));
        assert_eq!(name("players", "2"), Name::key("players", "2"));
        assert_eq!(name("players", "coop"), Name::key("v:players.coop", ""));
        assert_eq!(name("players", "1-4"), Name::key("players-of", "1-4"));
    }

    #[test]
    fn a_comma_list_is_named_part_by_part_when_any_part_is_known() {
        let parts = values(&tag("region", "us,eu"));
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].name, Name::key("v:region.eu", ""));
        assert!(parts.iter().all(|v| v.search == "region:us,eu"));
        let raw = values(&tag("developer", "Foo, Bar"));
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].name, Name::text("Foo, Bar"));
    }
}
