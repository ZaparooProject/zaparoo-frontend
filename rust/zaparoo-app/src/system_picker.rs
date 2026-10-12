// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The system picker's list: every system under its manufacturer, with the
//! ones used lately repeated in a short section at the top. A long catalog
//! is then a few section jumps deep rather than a hundred rows.

/// A system the picker can offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct System {
    pub id: String,
    pub name: String,
    /// Empty when Core names none.
    pub manufacturer: String,
    /// Games indexed for it, when Core counted them.
    pub games: Option<u32>,
}

/// A section of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Section {
    Recent,
    Manufacturer(String),
    /// Systems with no manufacturer.
    Other,
}

/// One row, top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(Section),
    System(System),
}

/// Systems repeated under Recent.
pub const RECENT_LIMIT: usize = 3;

/// A catalog this small is quicker to read whole than to split up.
pub const SECTION_THRESHOLD: usize = 8;

fn sort_key(text: &str) -> String {
    text.to_lowercase()
}

/// The picker's rows. `recent` is system ids, most recent first; ids the
/// catalog does not hold are passed over. A small catalog comes back as a
/// plain alphabetical list with no sections.
pub fn rows(systems: &[System], recent: &[String]) -> Vec<Row> {
    let mut sorted: Vec<&System> = systems.iter().collect();
    sorted.sort_by_key(|s| sort_key(&s.name));
    if sorted.len() <= SECTION_THRESHOLD {
        return sorted.into_iter().cloned().map(Row::System).collect();
    }

    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let recents: Vec<&System> = recent
        .iter()
        .filter_map(|id| {
            if seen.contains(&id.as_str()) {
                return None;
            }
            seen.push(id.as_str());
            sorted.iter().copied().find(|s| &s.id == id)
        })
        .take(RECENT_LIMIT)
        .collect();
    if !recents.is_empty() {
        out.push(Row::Header(Section::Recent));
        out.extend(recents.into_iter().cloned().map(Row::System));
    }

    // One section per manufacturer however Core cased it, named by the
    // spelling most of its systems carry.
    let spelled = |maker: &str| {
        sorted
            .iter()
            .filter(|s| s.manufacturer.trim() == maker)
            .count()
    };
    let mut makers: Vec<&str> = sorted
        .iter()
        .map(|s| s.manufacturer.trim())
        .filter(|m| !m.is_empty())
        .collect();
    makers.sort_by_key(|m| (sort_key(m), std::cmp::Reverse(spelled(m))));
    makers.dedup_by_key(|m| sort_key(m));
    for maker in makers {
        out.push(Row::Header(Section::Manufacturer(maker.to_string())));
        out.extend(
            sorted
                .iter()
                .filter(|s| sort_key(s.manufacturer.trim()) == sort_key(maker))
                .map(|s| Row::System((*s).clone())),
        );
    }
    let unnamed: Vec<Row> = sorted
        .iter()
        .filter(|s| s.manufacturer.trim().is_empty())
        .map(|s| Row::System((*s).clone()))
        .collect();
    if !unnamed.is_empty() {
        out.push(Row::Header(Section::Other));
        out.extend(unnamed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(id: &str, name: &str, manufacturer: &str) -> System {
        System {
            id: id.into(),
            name: name.into(),
            manufacturer: manufacturer.into(),
            games: Some(3),
        }
    }

    fn catalog() -> Vec<System> {
        vec![
            system("SNES", "Super Nintendo", "Nintendo"),
            system("Genesis", "Genesis", "Sega"),
            system("NES", "NES", "nintendo"),
            system("PSX", "PlayStation", "Sony"),
            system("DOS", "DOS", ""),
            system("Saturn", "Saturn", "Sega"),
            system("N64", "Nintendo 64", "Nintendo"),
            system("GameGear", "Game Gear", "Sega"),
            system("Arcade", "Arcade", " "),
        ]
    }

    fn outline(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                Row::Header(Section::Recent) => "# Recent".to_string(),
                Row::Header(Section::Manufacturer(name)) => format!("# {name}"),
                Row::Header(Section::Other) => "# Other".to_string(),
                Row::System(s) => s.id.clone(),
            })
            .collect()
    }

    #[test]
    fn systems_group_under_their_manufacturer_with_unnamed_ones_last() {
        assert_eq!(
            outline(&rows(&catalog(), &[])),
            [
                "# Nintendo",
                "NES",
                "N64",
                "SNES",
                "# Sega",
                "GameGear",
                "Genesis",
                "Saturn",
                "# Sony",
                "PSX",
                "# Other",
                "Arcade",
                "DOS",
            ]
        );
    }

    #[test]
    fn recent_systems_repeat_at_the_top_capped_and_deduplicated() {
        let recent: Vec<String> = ["Saturn", "Gone", "NES", "Saturn", "PSX", "DOS"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let out = outline(&rows(&catalog(), &recent));
        assert_eq!(out[..4], ["# Recent", "Saturn", "NES", "PSX"]);
        assert_eq!(out[4], "# Nintendo");
        // Each still sits under its manufacturer as well.
        assert_eq!(out.iter().filter(|r| *r == "Saturn").count(), 2);
    }

    #[test]
    fn a_small_catalog_is_one_plain_alphabetical_list() {
        let few: Vec<System> = catalog().into_iter().take(SECTION_THRESHOLD).collect();
        let out = outline(&rows(&few, &["NES".to_string()]));
        assert_eq!(out.len(), SECTION_THRESHOLD);
        assert!(out.iter().all(|r| !r.starts_with('#')));
        assert_eq!(out[0], "DOS");
    }
}
