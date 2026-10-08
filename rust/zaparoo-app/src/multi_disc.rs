// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A folder of one game's discs: Core lists it as one row that launches a
//! single disc, and the row's menu offers the rest. The Core calls stay in
//! the shell.

/// Browse cap for a game's disc folder.
pub const MAX_DISCS: u32 = 100;

/// How one disc reads in the list of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// Core's disc number; the view words it.
    Disc(String),
    /// Core tagged no disc, so the file's own name tells it apart: the
    /// discs of one game all carry the game's title.
    File(String),
}

/// The label for one entry of a disc folder, from its `(type, value)` tags.
pub fn label<'a>(
    tags: impl IntoIterator<Item = (&'a str, &'a str)>,
    path: &str,
    name: &str,
) -> Label {
    tags.into_iter()
        .find(|(tag_type, value)| *tag_type == "disc" && !value.trim().is_empty())
        .map_or_else(
            || Label::File(crate::media_list::file_stem_or_name(path, name)),
            |(_, value)| Label::Disc(value.trim().to_string()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_is_named_by_its_number_and_otherwise_by_its_file() {
        assert_eq!(
            label(
                [("region", "us"), ("disc", " 2 ")],
                "/g/Chrono Cross/Chrono Cross (Disc 2).cue",
                "Chrono Cross"
            ),
            Label::Disc("2".into())
        );
        assert_eq!(
            label(
                [("region", "us"), ("disc", "")],
                "/g/Chrono Cross/Chrono Cross (Disc 2).bin",
                "Chrono Cross"
            ),
            Label::File("Chrono Cross (Disc 2)".into())
        );
    }
}
