// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use zaparoo_core::media_types::TagInfo;

pub fn tag_display_value(tag: &TagInfo) -> String {
    let label = tag.label.trim();
    if label.is_empty() {
        tag.tag.trim().to_string()
    } else {
        label.to_string()
    }
}

/// Maximum number of variant tokens surfaced per item. Core orders tags by
/// display priority and only emits types that actually differ across siblings,
/// so the leading few differentiate; this is a defensive cap so a pathological
/// tag set can't overflow a tile caption or list row.
const MAX_DISAMBIGUATING_TAGS: usize = 4;

/// Character cap on a free-text fallback token (edition/unknown types that have
/// no canonical short form). Curated tags are already short; this only guards
/// against a pathological value blowing out the inline caption. Hard cut with no
/// ellipsis marker so the token stays renderable in the `MiSTer` bitmap font,
/// which can't be relied on to carry a `…` glyph.
const MAX_FALLBACK_TOKEN_CHARS: usize = 14;

/// Format Core's `disambiguatingTags` into compact inline tokens, preserving the
/// server's display-priority order and capping the count. Tokens are short and
/// lowercase (Core normalizes values to lowercase; uppercasing is purely
/// stylistic and costs readability), e.g. `us`, `d2`, `r1`, `2p`, `'96`, `hack`.
/// Free-text values are de-dashed into words (`atari-lightgun` -> `atari
/// lightgun`) so the sibling-diff below can split them on whitespace. Tags with
/// an empty value are skipped.
pub fn disambiguating_tag_labels(tags: &[TagInfo]) -> Vec<String> {
    tags.iter()
        .filter_map(format_disambiguating_tag)
        .filter(|token| !token.is_empty())
        .take(MAX_DISAMBIGUATING_TAGS)
        .collect()
}

fn format_disambiguating_tag(tag: &TagInfo) -> Option<String> {
    let value = tag.tag.trim();
    if value.is_empty() {
        return None;
    }
    let token = match tag.tag_type.trim().to_ascii_lowercase().as_str() {
        // Region/language can be multi-valued (e.g. `us,eu`); keep each part as
        // its lowercase code, joined with `/`. `world` -> `w` (MiSTer Arcade's
        // own shorthand) and a couple of long names get shortened.
        "region" | "lang" => value
            .split(',')
            .map(|part| compact_region(part.trim()))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/"),
        // Compact alphanumeric forms: d2 / r1 / 2p. The rev value may itself be
        // spelled "revision-a"/"v1" in arcade sets; strip that prefix first.
        "disc" => format!("d{value}"),
        "rev" => format!("r{}", strip_rev_prefix(value)),
        "players" => format!("{value}p"),
        // Normalized to `YYYY-MM-DD`; the two-digit year differentiates siblings
        // in the least space (`'96`).
        "builddate" => format_year_token(value),
        "edition" => compact_edition(value),
        // Free-text / flag types: lowercase readable value with dashes turned
        // into word breaks, hard-capped so it can't run away.
        _ => truncate_token(&dedash(&tag_display_value(tag))),
    };
    Some(token.trim().to_string())
}

/// Short code for a region/language value. Most canonical values are already
/// two-letter codes; only a few need shortening.
fn compact_region(value: &str) -> String {
    match value {
        "world" => "w".to_string(),
        "scandinavia" => "scan".to_string(),
        other => other.to_string(),
    }
}

/// Strip a spelled-out revision/version prefix so `revision-a` -> `a`, `v1` ->
/// `1`. Returns the original value when no prefix matches or stripping would
/// empty it (so a bare `a`/`1`/`1-2` passes through unchanged).
fn strip_rev_prefix(value: &str) -> &str {
    for prefix in [
        "revision-",
        "version-",
        "revision",
        "version",
        "rev-",
        "ver-",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            if !rest.is_empty() {
                return rest;
            }
        }
    }
    value
}

/// Short forms for the common edition values; everything else falls back to the
/// de-dashed, capped value.
fn compact_edition(value: &str) -> String {
    match value {
        "directors-cut" => "dc".to_string(),
        "collectors" => "ce".to_string(),
        "limited" => "le".to_string(),
        "special" => "se".to_string(),
        "deluxe" => "dlx".to_string(),
        "ultimate" => "ult".to_string(),
        "anniversary" => "anniv".to_string(),
        "remaster" | "remastered" => "remas".to_string(),
        other => truncate_token(&dedash(other)),
    }
}

/// `YYYY-MM-DD` (or `YYYY/MM/DD`) -> `'YY`. Falls back to the raw leading
/// segment for non-standard dates so they still differentiate.
fn format_year_token(value: &str) -> String {
    let year = value.split(['-', '/']).next().unwrap_or(value).trim();
    if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) {
        format!("'{}", &year[2..])
    } else {
        year.to_string()
    }
}

/// Replace dashes with spaces and collapse runs of whitespace, so a normalized
/// value like `atari-lightgun` reads (and word-splits) as `atari lightgun`.
fn dedash(value: &str) -> String {
    value
        .split(['-', ' '])
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hard-cap a free-text token to `MAX_FALLBACK_TOKEN_CHARS` on a char boundary.
fn truncate_token(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() <= MAX_FALLBACK_TOKEN_CHARS {
        return value.to_string();
    }
    value.chars().take(MAX_FALLBACK_TOKEN_CHARS).collect()
}

/// Per-row disambiguation display strings with sibling-aware common-affix
/// trimming. `rows` is each entry's `(display_name, compact tokens)` in display
/// order. Runs of equal name form a sibling group; within a group, the word-run
/// shared by ALL members at the leading and trailing ends is trimmed from every
/// member (never to empty), so variants differ by what actually distinguishes
/// them: `atari joystick`/`atari lightgun` -> `joystick`/`lightgun`. Returns the
/// trimmed, space-joined token string per row (untouched for singleton groups).
pub fn sibling_disambiguation_displays(rows: &[(String, Vec<String>)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(rows.len());
    let mut i = 0;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() && rows[j].0 == rows[i].0 {
            j += 1;
        }
        let members: Vec<Vec<String>> = rows[i..j]
            .iter()
            .map(|(_, toks)| split_words(toks))
            .collect();
        let (lead, trail) = common_affix(&members);
        for member in &members {
            out.push(join_trimmed(member, lead, trail));
        }
        i = j;
    }
    out
}

/// Flatten a row's tokens into whitespace-separated words. Compact structured
/// tokens (`r1-2`, `us`) stay atomic; only free-text values were de-dashed into
/// multiple words at format time, so this splits on spaces alone.
fn split_words(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|t| t.split(' '))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Longest leading and trailing word-runs shared by ALL members, capped so no
/// member is trimmed to empty. `(0, 0)` for groups smaller than two.
fn common_affix(members: &[Vec<String>]) -> (usize, usize) {
    if members.len() < 2 {
        return (0, 0);
    }
    let min_len = members.iter().map(Vec::len).min().unwrap_or(0);
    if min_len == 0 {
        return (0, 0);
    }
    let mut lead = 0;
    while lead < min_len && members[1..].iter().all(|m| m[lead] == members[0][lead]) {
        lead += 1;
    }
    let mut trail = 0;
    while trail < min_len
        && members[1..]
            .iter()
            .all(|m| m[m.len() - 1 - trail] == members[0][members[0].len() - 1 - trail])
    {
        trail += 1;
    }
    // Keep at least one word in every member: lead + trail <= min_len - 1.
    let allowed = min_len - 1;
    if lead + trail > allowed {
        let trail = trail.min(allowed);
        return (lead.min(allowed - trail), trail);
    }
    (lead, trail)
}

fn join_trimmed(words: &[String], lead: usize, trail: usize) -> String {
    let end = words.len() - trail;
    words[lead..end].join(" ")
}

#[cfg(test)]
mod tests {
    use super::{disambiguating_tag_labels, sibling_disambiguation_displays, TagInfo};

    fn tag(value: &str, tag_type: &str) -> TagInfo {
        TagInfo {
            tag: value.into(),
            tag_type: tag_type.into(),
            label: String::new(),
        }
    }

    #[test]
    fn formats_common_types_into_short_lowercase_tokens() {
        assert_eq!(
            disambiguating_tag_labels(&[
                tag("us", "region"),
                tag("ja", "lang"),
                tag("2", "disc"),
                tag("1", "rev"),
            ]),
            vec!["us", "ja", "d2", "r1"]
        );
        assert_eq!(
            disambiguating_tag_labels(&[
                tag("2", "players"),
                tag("1996-10-04", "builddate"),
                tag("hack", "unlicensed"),
            ]),
            vec!["2p", "'96", "hack"]
        );
    }

    #[test]
    fn region_world_uses_mister_shorthand() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("world", "region")]),
            vec!["w"]
        );
    }

    #[test]
    fn rev_strips_spelled_out_prefix() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("revision-a", "rev")]),
            vec!["ra"]
        );
        assert_eq!(
            disambiguating_tag_labels(&[tag("1-2", "rev")]),
            vec!["r1-2"]
        );
    }

    #[test]
    fn region_multi_value_joins_with_slash() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("us,eu", "region")]),
            vec!["us/eu"]
        );
    }

    #[test]
    fn free_text_is_dedashed_and_capped() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("atari-lightgun", "unknown")]),
            vec!["atari lightgun"]
        );
        // de-dashed to "homebrew translation" (20 chars) -> hard cut to 14.
        assert_eq!(
            disambiguating_tag_labels(&[tag("homebrew-translation", "unknown")]),
            vec!["homebrew trans"]
        );
    }

    #[test]
    fn caps_token_count() {
        let tags = vec![
            tag("us", "region"),
            tag("1", "disc"),
            tag("1", "rev"),
            tag("2", "players"),
            tag("2000", "year"),
            tag("hack", "unlicensed"),
        ];
        assert_eq!(disambiguating_tag_labels(&tags).len(), 4);
    }

    #[test]
    fn sibling_diff_strips_common_leading_word() {
        let rows = vec![
            ("Crossbow".into(), vec!["atari joystick".into()]),
            ("Crossbow".into(), vec!["atari lightgun".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["joystick", "lightgun"]
        );
    }

    #[test]
    fn sibling_diff_keeps_each_member_nonempty() {
        let rows = vec![
            ("Arkanoid".into(), vec!["unl lives slow".into()]),
            ("Arkanoid".into(), vec!["unl lives".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["lives slow", "lives"]
        );
    }

    #[test]
    fn sibling_diff_drops_shared_trailing_token() {
        let rows = vec![
            ("Game".into(), vec!["us".into(), "r1".into()]),
            ("Game".into(), vec!["eu".into(), "r1".into()]),
        ];
        assert_eq!(sibling_disambiguation_displays(&rows), vec!["us", "eu"]);
    }

    #[test]
    fn sibling_diff_singleton_is_unchanged() {
        let rows = vec![("Solo".into(), vec!["us".into(), "r1".into()])];
        assert_eq!(sibling_disambiguation_displays(&rows), vec!["us r1"]);
    }

    #[test]
    fn sibling_diff_distinct_names_not_grouped() {
        let rows = vec![
            ("A".into(), vec!["atari joystick".into()]),
            ("B".into(), vec!["atari lightgun".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["atari joystick", "atari lightgun"]
        );
    }
}
