// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Credits screen's reading of `rust/frontend/LICENSE.txt`, the one
// license and notices file `scripts/generate-notices.sh` writes and the
// release bundles ship. The binary embeds that file, so what the screen
// shows is the bundled text, never a second copy that could drift.
//
// The layout: a part opens with its title between two lines of 70 `#`,
// an entry with its title between two lines of 70 `-`. An entry whose
// body starts with `Used by:` is a crate license, and every crate it
// lists is a component; any other entry is one component under its own
// title. The generator refuses source text that would read as a rule.

use std::borrow::Cow;
use std::ops::Range;

const PART_RULE: &str = "######################################################################";
const ENTRY_RULE: &str = "----------------------------------------------------------------------";
const USED_BY: &str = "Used by:";
const USED_BY_ITEM: &str = "  - ";

/// The part of the file a component comes from, which orders the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The frontend's own license and the project's artwork.
    Project,
    /// Fonts, artwork and data from elsewhere.
    Assets,
    /// Third-party software: notices and crate licenses.
    Software,
}

#[derive(Debug, Clone)]
struct Entry {
    title: Range<usize>,
    crates: Vec<Range<usize>>,
    body: Range<usize>,
}

/// One row of the Credits list.
#[derive(Debug, Clone)]
pub struct Component {
    pub part: Part,
    name: Range<usize>,
    /// The entries holding its text: one for a notice, one per license a
    /// crate is used under.
    entries: Vec<usize>,
    /// A crate, read under the licenses its entries are titled with.
    is_crate: bool,
}

/// One block of a component's text on the reading page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<'a> {
    /// The license a crate's following text is under.
    Heading(&'a str),
    /// A run of lines between blank lines, reflowed (see [`reflow`]).
    Paragraph(Cow<'a, str>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The text does not open with the project part.
    NoParts,
    /// A part or entry rule was not followed by a title and its closing rule.
    BrokenRule { line: usize },
    /// An entry came before any part.
    EntryOutsidePart { line: usize },
    /// A part title the screen does not know.
    UnknownPart { line: usize },
}

/// The parsed file: the text itself and the components that index it.
#[derive(Debug, Clone)]
pub struct Credits {
    text: String,
    entries: Vec<Entry>,
    components: Vec<Component>,
}

fn part_for(title: &str) -> Option<Part> {
    match title {
        "Zaparoo Frontend" => Some(Part::Project),
        "Fonts, artwork and data" => Some(Part::Assets),
        "Software" => Some(Part::Software),
        _ => None,
    }
}

/// The text's lines with their byte ranges, the newline left out.
fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(start..at);
            start = at + 1;
        }
    }
    if start < text.len() {
        lines.push(start..text.len());
    }
    lines
}

/// Trim blank lines off both ends of `lines[from..to]`, as a byte range.
fn body_range(text: &str, lines: &[Range<usize>], from: usize, to: usize) -> Range<usize> {
    let blank = |i: usize| text[lines[i].clone()].trim().is_empty();
    let mut first = from;
    while first < to && blank(first) {
        first += 1;
    }
    let mut last = to;
    while last > first && blank(last - 1) {
        last -= 1;
    }
    if first == last {
        return 0..0;
    }
    lines[first].start..lines[last - 1].end
}

/// The list's rows: each notice in file order, then every crate named in a
/// "Used by" list, once, sorted by name, holding each license entry it is
/// listed under.
fn components_of(text: &str, entries: &[Entry], entry_parts: &[Part]) -> Vec<Component> {
    let mut components: Vec<Component> = entries
        .iter()
        .zip(entry_parts)
        .enumerate()
        .filter(|(_, (entry, _))| entry.crates.is_empty())
        .map(|(index, (entry, &part))| Component {
            part,
            name: entry.title.clone(),
            entries: vec![index],
            is_crate: false,
        })
        .collect();
    let mut crates: Vec<Component> = Vec::new();
    let mut by_name: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        for name in &entry.crates {
            match by_name.entry(&text[name.clone()]) {
                std::collections::hash_map::Entry::Occupied(at) => {
                    crates[*at.get()].entries.push(index);
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(crates.len());
                    crates.push(Component {
                        part: Part::Software,
                        name: name.clone(),
                        entries: vec![index],
                        is_crate: true,
                    });
                }
            }
        }
    }
    crates.sort_by(|a, b| {
        let (a, b) = (&text[a.name.clone()], &text[b.name.clone()]);
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    components.extend(crates);
    components
}

impl Credits {
    /// Read the file's parts, entries and components. Components keep the
    /// file's order for notices; crates follow, sorted by name.
    pub fn parse(text: String) -> Result<Self, ParseError> {
        let lines = line_ranges(&text);
        let line = |i: usize| &text[lines[i].clone()];
        // (part, entry title line, first body line)
        let mut heads: Vec<(Part, usize, usize)> = Vec::new();
        let mut part = None;
        let mut i = 0;
        while i < lines.len() {
            let rule = line(i);
            if rule == PART_RULE || rule == ENTRY_RULE {
                if i + 2 >= lines.len() || line(i + 2) != rule {
                    return Err(ParseError::BrokenRule { line: i + 1 });
                }
                if rule == PART_RULE {
                    part =
                        Some(part_for(line(i + 1)).ok_or(ParseError::UnknownPart { line: i + 2 })?);
                    heads.push((part.unwrap_or(Part::Project), usize::MAX, i));
                } else {
                    let Some(part) = part else {
                        return Err(ParseError::EntryOutsidePart { line: i + 1 });
                    };
                    heads.push((part, i + 1, i + 3));
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        if part.is_none() {
            return Err(ParseError::NoParts);
        }
        // A part head (title line MAX) only bounds the entry before it.
        let mut entries = Vec::new();
        let mut entry_parts = Vec::new();
        for (n, &(part, title_line, body_from)) in heads.iter().enumerate() {
            if title_line == usize::MAX {
                continue;
            }
            let body_to = heads
                .get(n + 1)
                .map_or(lines.len(), |&(_, next_title, next_from)| {
                    if next_title == usize::MAX {
                        next_from
                    } else {
                        next_title - 1
                    }
                });
            let mut from = body_from;
            while from < body_to && line(from).trim().is_empty() {
                from += 1;
            }
            let mut crates = Vec::new();
            if from < body_to && line(from) == USED_BY {
                from += 1;
                while from < body_to {
                    let item = line(from);
                    let Some(name) = item.strip_prefix(USED_BY_ITEM) else {
                        break;
                    };
                    let start = lines[from].start + USED_BY_ITEM.len();
                    crates.push(start..start + name.len());
                    from += 1;
                }
            }
            entries.push(Entry {
                title: lines[title_line].clone(),
                body: body_range(&text, &lines, from, body_to),
                crates,
            });
            entry_parts.push(part);
        }
        let components = components_of(&text, &entries, &entry_parts);
        Ok(Self {
            text,
            entries,
            components,
        })
    }

    pub fn components(&self) -> &[Component] {
        &self.components
    }

    pub fn name(&self, component: &Component) -> &str {
        &self.text[component.name.clone()]
    }

    /// The licenses a crate is used under, in file order; empty for a
    /// notice, whose name says what it is.
    pub fn licenses(&self, component: &Component) -> Vec<&str> {
        if !component.is_crate {
            return Vec::new();
        }
        component
            .entries
            .iter()
            .map(|&i| &self.text[self.entries[i].title.clone()])
            .collect()
    }

    /// The component at `name`, for a restore.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.components.iter().position(|c| self.name(c) == name)
    }

    /// The reading page's blocks: the text paragraph by paragraph, a
    /// crate's under the name of each license it is used under.
    pub fn blocks(&self, component: &Component) -> Vec<Block<'_>> {
        let mut blocks = Vec::new();
        for &index in &component.entries {
            let entry = &self.entries[index];
            if component.is_crate {
                blocks.push(Block::Heading(&self.text[entry.title.clone()]));
            }
            blocks.extend(
                paragraphs(&self.text[entry.body.clone()]).map(|p| Block::Paragraph(reflow(p))),
            );
        }
        blocks
    }
}

/// The runs of lines between blank lines, trailing spaces dropped.
fn paragraphs(body: &str) -> impl Iterator<Item = &str> {
    let mut rest = body;
    std::iter::from_fn(move || {
        let start = rest.find(|c: char| !c.is_whitespace())?;
        // Back up to the start of the line, so leading indentation stays.
        let start = rest[..start].rfind('\n').map_or(0, |at| at + 1);
        rest = &rest[start..];
        let mut end = 0;
        let mut found_blank = None;
        for line in rest.split_inclusive('\n') {
            if line.trim().is_empty() {
                found_blank = Some(end);
                break;
            }
            end += line.len();
        }
        let end = found_blank.unwrap_or(rest.len());
        let paragraph = rest[..end].trim_end();
        rest = &rest[end..];
        Some(paragraph)
    })
}

/// A paragraph as it should wrap on a screen narrower than the file:
/// the indentation every line shares comes off, and a line break the
/// file's own wrapping made becomes a space. A break is the file's
/// wrapping when the next word would not have fit on the line, measured
/// against the paragraph's widest line before its last, and that width is
/// a text column's ([`WRAP_MIN`]). A break before an indented line or one
/// that opens with the same word stays, so a list of copyright lines or
/// a table keeps its shape.
const WRAP_MIN: usize = 50;

fn first_word(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

fn reflow(paragraph: &str) -> Cow<'_, str> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let common = paragraph.lines().map(indent).min().unwrap_or(0);
    let lines: Vec<&str> = paragraph.lines().map(|line| &line[common..]).collect();
    if common == 0 && lines.len() < 2 {
        return Cow::Borrowed(paragraph);
    }
    let width = lines[..lines.len().saturating_sub(1)]
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::with_capacity(paragraph.len());
    for (n, line) in lines.iter().enumerate() {
        if n > 0 {
            let previous = lines[n - 1];
            let next_word = first_word(line);
            let soft = width >= WRAP_MIN
                && indent(line) == 0
                && !next_word.is_empty()
                && next_word != first_word(previous)
                && previous.chars().count() + 1 + next_word.chars().count() > width;
            out.push(if soft { ' ' } else { '\n' });
        }
        out.push_str(line);
    }
    Cow::Owned(out)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests fail fast on a file that does not parse"
)]
mod tests {
    use super::*;

    fn sample() -> String {
        let part = |title: &str| format!("{PART_RULE}\n{title}\n{PART_RULE}\n\n");
        let entry =
            |title: &str, body: &str| format!("{ENTRY_RULE}\n{title}\n{ENTRY_RULE}\n\n{body}\n\n");
        let mut text = String::from("Header\n======\n\nIntro.\n\n");
        text += &part("Zaparoo Frontend");
        text += &entry("Zaparoo Frontend", "License one.\nLine two.\n\n  Indented.");
        text += &part("Fonts, artwork and data");
        text += &entry("Noto Sans", "Font notice.");
        text += &part("Software");
        text += &entry("zaparoo-update", "Update notice.");
        text += &entry("MIT License", "Used by:\n  - serde\n  - Zeta\n\nMIT text.");
        text += &entry(
            "Apache License 2.0",
            "Used by:\n  - serde\n  - alpha\n\nApache text.",
        );
        text
    }

    #[test]
    fn notices_keep_file_order_and_crates_follow_sorted() {
        let credits = Credits::parse(sample()).expect("parse");
        let names: Vec<&str> = credits
            .components()
            .iter()
            .map(|c| credits.name(c))
            .collect();
        assert_eq!(
            names,
            [
                "Zaparoo Frontend",
                "Noto Sans",
                "zaparoo-update",
                "alpha",
                "serde",
                "Zeta"
            ]
        );
        let parts: Vec<Part> = credits.components().iter().map(|c| c.part).collect();
        assert_eq!(parts[..3], [Part::Project, Part::Assets, Part::Software]);
    }

    #[test]
    fn every_crate_license_is_headed_and_a_crate_under_two_reads_both() {
        let credits = Credits::parse(sample()).expect("parse");
        let serde = &credits.components()[credits.position("serde").expect("serde")];
        assert_eq!(
            credits.licenses(serde),
            ["MIT License", "Apache License 2.0"]
        );
        assert_eq!(
            credits.blocks(serde),
            [
                Block::Heading("MIT License"),
                Block::Paragraph("MIT text.".into()),
                Block::Heading("Apache License 2.0"),
                Block::Paragraph("Apache text.".into()),
            ]
        );
        let alpha = &credits.components()[credits.position("alpha").expect("alpha")];
        assert_eq!(
            credits.blocks(alpha),
            [
                Block::Heading("Apache License 2.0"),
                Block::Paragraph("Apache text.".into())
            ]
        );
    }

    #[test]
    fn a_notice_reads_as_paragraphs_with_its_indentation() {
        let credits = Credits::parse(sample()).expect("parse");
        let own = &credits.components()[0];
        assert!(credits.licenses(own).is_empty());
        assert_eq!(
            credits.blocks(own),
            [
                Block::Paragraph("License one.\nLine two.".into()),
                Block::Paragraph("Indented.".into()),
            ]
        );
    }

    #[test]
    fn wrapped_lines_join_and_short_or_indented_ones_keep_their_break() {
        let wrapped = "      Permission is hereby granted, free of charge, to any person\n      obtaining a copy of this software.";
        assert_eq!(
            reflow(wrapped),
            "Permission is hereby granted, free of charge, to any person obtaining a copy of this software."
        );
        let list = "Copyright (c) 2014 Someone with a long enough name to be wide\nCopyright (c) 2015 Someone Else";
        assert_eq!(reflow(list), list);
        let nested = "1. Definitions, which are long enough here to be a text column\n   and wrap on to an indented line.";
        assert_eq!(reflow(nested), nested);
        assert!(matches!(reflow("One line."), Cow::Borrowed("One line.")));
    }

    #[test]
    fn a_broken_rule_or_an_unknown_part_is_refused() {
        let broken = format!("{PART_RULE}\nZaparoo Frontend\n\n");
        assert!(matches!(
            Credits::parse(broken),
            Err(ParseError::BrokenRule { .. })
        ));
        let unknown = format!("{PART_RULE}\nOther\n{PART_RULE}\n");
        assert!(matches!(
            Credits::parse(unknown),
            Err(ParseError::UnknownPart { .. })
        ));
        assert_eq!(
            Credits::parse("no parts".into()).err(),
            Some(ParseError::NoParts)
        );
    }

    /// The committed file the binary embeds parses, with every part and
    /// the crates the frontend is built from.
    #[test]
    fn the_shipped_license_file_parses() {
        let text = include_str!("../../frontend/LICENSE.txt").to_string();
        let credits = Credits::parse(text).expect("LICENSE.txt parses");
        let components = credits.components();
        assert_eq!(credits.name(&components[0]), "Zaparoo Frontend");
        assert!(components.iter().any(|c| c.part == Part::Assets));
        for name in ["tokio", "serde", "image"] {
            let at = credits.position(name).expect(name);
            assert!(!credits.licenses(&components[at]).is_empty(), "{name}");
            assert!(!credits.blocks(&components[at]).is_empty(), "{name}");
        }
        assert!(components.iter().all(|c| !credits.blocks(c).is_empty()));
    }
}
