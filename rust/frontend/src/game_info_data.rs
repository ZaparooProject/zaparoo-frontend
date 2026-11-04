//! Metadata and carousel ordering for the Game Info modal.

use crate::tag_utils::tag_display_value;
use std::collections::BTreeSet;
use zaparoo_core::media_types::{MediaMeta, TagInfo, CORE_SERVEABLE_IMAGE_TYPES};

const ORDERED_TAG_TYPES: &[&str] = &[
    "system",
    "platform",
    "year",
    "release_date",
    "genre",
    "players",
    "play_mode",
    "cooperative",
    "developer",
    "publisher",
    "rating",
];

pub fn normalize_type(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

pub fn description(meta: &MediaMeta) -> String {
    meta.title
        .properties
        .get("property:description")
        .or_else(|| meta.properties.get("property:description"))
        .map(|property| property.text.trim().to_string())
        .unwrap_or_default()
}

pub fn rows(meta: &MediaMeta, path: &str) -> Vec<(String, String)> {
    let tags = if meta.title.tags.is_empty() {
        &meta.tags
    } else {
        &meta.title.tags
    };
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for wanted in ORDERED_TAG_TYPES {
        for tag in tags
            .iter()
            .filter(|tag| normalize_type(&tag.tag_type) == *wanted)
        {
            let value = tag_display_value(tag);
            if !value.is_empty() {
                seen.insert((*wanted).to_string());
                rows.push(((*wanted).to_string(), value));
            }
        }
    }
    for tag in tags {
        let key = normalize_type(&tag.tag_type);
        let value = tag_display_value(tag);
        if !key.is_empty() && !ORDERED_TAG_TYPES.contains(&key.as_str()) && !value.is_empty() {
            seen.insert(key.clone());
            rows.push((key, value));
        }
    }
    if !seen.contains("system") && !meta.title.system.name.trim().is_empty() {
        rows.insert(
            0,
            ("system".into(), meta.title.system.name.trim().to_string()),
        );
        seen.insert("system".into());
    }
    // Tags win over properties; title properties win over ROM properties.
    for properties in [&meta.title.properties, &meta.properties] {
        let mut keys: Vec<_> = properties.keys().collect();
        keys.sort();
        for key in keys {
            let Some(name) = key.strip_prefix("property:").map(str::trim) else {
                continue;
            };
            if name.is_empty() || name.starts_with("image") {
                continue;
            }
            let name = normalize_type(name);
            // The manual opens from the panel; its path means nothing here.
            if name == "description" || name == "manual" || !seen.insert(name.clone()) {
                continue;
            }
            if let Some(property) = properties.get(key) {
                let value = property.text.trim();
                if !value.is_empty() {
                    rows.push((name, value.to_string()));
                }
            }
        }
    }
    let file = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem).trim();
    if !stem.is_empty() {
        rows.push(("filename".into(), stem.to_string()));
    }
    for (_, value) in &mut rows {
        *value = value.replace(['\t', '\r', '\n'], " ");
    }
    rows
}

/// One row of the details table: a section heading, a tag type's values,
/// or a plain fact (system, path, launcher) with its `text`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoRow {
    pub header: bool,
    /// The tag type, or `system`, `tags`, `path`, `launcher`; for a
    /// heading, `details` or `technical`.
    pub kind: String,
    pub values: Vec<zaparoo_app::game_details::Value>,
    pub text: String,
}

impl InfoRow {
    fn header(kind: &str) -> Self {
        Self {
            header: true,
            kind: kind.into(),
            ..Self::default()
        }
    }

    fn text(kind: &str, text: &str) -> Self {
        Self {
            kind: kind.into(),
            text: text.trim().into(),
            ..Self::default()
        }
    }

    fn values(kind: &str, values: Vec<zaparoo_app::game_details::Value>) -> Self {
        Self {
            kind: kind.into(),
            values,
            ..Self::default()
        }
    }

    /// A on the row searches by one of its values.
    pub fn searchable(&self) -> bool {
        !self.header && !self.values.is_empty()
    }
}

/// A game's tags for the details: the title's, then the media's, each
/// shown once.
pub fn details(meta: &MediaMeta) -> zaparoo_app::game_details::Details {
    let title = core_tags(&meta.title.tags);
    let media = core_tags(&meta.tags);
    zaparoo_app::game_details::organize(&[&title, &media])
}

fn core_tags(list: &[TagInfo]) -> Vec<zaparoo_app::game_details::Tag<'_>> {
    list.iter()
        .map(|tag| zaparoo_app::game_details::Tag {
            tag_type: &tag.tag_type,
            tag: &tag.tag,
            label: &tag.label,
        })
        .collect()
}

/// The details table, as the Zaparoo App lays it out: the system and the
/// facts under Details, the leftover tags on one row, then the technical
/// facts with the path and launcher.
pub fn info_rows(
    meta: &MediaMeta,
    details: &zaparoo_app::game_details::Details,
    path: &str,
) -> Vec<InfoRow> {
    let mut rows = vec![InfoRow::header("details")];
    let system = meta.title.system.name.trim();
    if !system.is_empty() {
        rows.push(InfoRow::text("system", system));
    }
    rows.extend(
        details
            .facts
            .iter()
            .map(|fact| InfoRow::values(fact.kind, fact.values.clone())),
    );
    if !details.tags.is_empty() {
        rows.push(InfoRow::values("tags", details.tags.clone()));
    }
    let path = if meta.path.trim().is_empty() {
        path
    } else {
        meta.path.as_str()
    };
    let launcher = meta.launcher_override.as_deref().unwrap_or_default();
    if !details.technical.is_empty() || !path.trim().is_empty() || !launcher.trim().is_empty() {
        rows.push(InfoRow::header("technical"));
        rows.extend(
            details
                .technical
                .iter()
                .map(|fact| InfoRow::values(fact.kind, fact.values.clone())),
        );
        if !path.trim().is_empty() {
            rows.push(InfoRow::text("path", path));
        }
        if !launcher.trim().is_empty() {
            rows.push(InfoRow::text("launcher", launcher));
        }
    }
    rows
}

/// A value's name in the reader's language. The view holds the strings;
/// Rust joins a row's values, which a view cannot.
pub fn name_text(app: &crate::App, name: &zaparoo_app::game_details::Name) -> String {
    use slint::ComponentHandle;
    app.global::<crate::TagNames>()
        .invoke_name(
            name.key.as_str().into(),
            name.arg.as_str().into(),
            name.text.as_str().into(),
        )
        .to_string()
}

/// A row's values as one line. Leftover tags keep their type in front.
pub fn joined(app: &crate::App, row: &InfoRow) -> String {
    if row.values.is_empty() {
        return row.text.clone();
    }
    row.values
        .iter()
        .map(|value| value_text(app, value, row.kind == "tags"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One value as the values page lists it.
pub fn value_text(
    app: &crate::App,
    value: &zaparoo_app::game_details::Value,
    typed: bool,
) -> String {
    let name = name_text(app, &value.name);
    if typed {
        format!("{}:{name}", value.kind)
    } else {
        name
    }
}

pub fn image_types(meta: &MediaMeta) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for kind in meta
        .available_image_types
        .iter()
        .chain(&meta.title.available_image_types)
    {
        if !kind.trim().is_empty() && seen.insert(kind.clone()) {
            ordered.push(kind.clone());
        }
    }
    if ordered.is_empty() {
        for key in meta.title.properties.keys().chain(meta.properties.keys()) {
            if let Some(suffix) = key.strip_prefix("property:image") {
                let kind = if suffix.is_empty() {
                    "image"
                } else {
                    suffix.trim_start_matches('-')
                };
                if !kind.is_empty() {
                    seen.insert(kind.to_string());
                }
            }
        }
        if seen.remove("image") {
            ordered.push("image".into());
        }
        ordered.extend(seen);
    }
    ordered
        .into_iter()
        .filter(|kind| CORE_SERVEABLE_IMAGE_TYPES.contains(&kind.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::media_types::{MediaMetaProperty, TagInfo};

    fn property(text: &str) -> MediaMetaProperty {
        MediaMetaProperty {
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn title_metadata_wins_and_rows_keep_qt_order_and_filename() {
        let mut meta = MediaMeta::default();
        meta.title.system.name = "Super Nintendo".into();
        meta.title.properties.insert(
            "property:description".into(),
            property(" Title description "),
        );
        meta.properties
            .insert("property:description".into(), property("ROM description"));
        meta.title
            .properties
            .insert("property:rating".into(), property("Excellent\nreview"));
        meta.properties
            .insert("property:rating".into(), property("ROM rating"));
        meta.title
            .properties
            .insert("property:image-boxart".into(), property("not a row"));
        meta.title.tags = vec![
            TagInfo {
                tag_type: "Genre".into(),
                tag: "RPG".into(),
                ..Default::default()
            },
            TagInfo {
                tag_type: "Release Date".into(),
                tag: "1994".into(),
                ..Default::default()
            },
        ];
        assert_eq!(description(&meta), "Title description");
        assert_eq!(
            rows(&meta, "Games/Test.sfc"),
            vec![
                ("system".into(), "Super Nintendo".into()),
                ("release_date".into(), "1994".into()),
                ("genre".into(), "RPG".into()),
                ("rating".into(), "Excellent review".into()),
                ("filename".into(), "Test".into()),
            ]
        );
    }

    #[test]
    fn carousel_preserves_core_order_deduplicates_and_filters() {
        let mut meta = MediaMeta {
            available_image_types: vec!["screenshot".into(), "boxart".into(), "unsupported".into()],
            ..Default::default()
        };
        meta.title.available_image_types = vec!["boxart".into(), "image".into()];
        assert_eq!(image_types(&meta), ["screenshot", "boxart", "image"]);
        meta.available_image_types.clear();
        meta.title.available_image_types.clear();
        meta.properties
            .insert("property:image-screenshot".into(), property(""));
        meta.properties
            .insert("property:image".into(), property(""));
        assert_eq!(image_types(&meta), ["image", "screenshot"]);
    }
}
