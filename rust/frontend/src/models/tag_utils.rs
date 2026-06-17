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
/// server's display-priority order and capping the count. Short curated codes
/// render UPPERCASE (`US`, `EU`, `D2`, `R1`, `2P`, `W`, `'96`); dynamic
/// free-text values stay lowercase with their dashes intact (`unl-lives`,
/// `atari-lightgun`). The dashes double as word boundaries for the sibling-diff
/// below, which splits on them. Tags with an empty value are skipped.
pub fn disambiguating_tag_labels(tags: &[TagInfo]) -> Vec<String> {
    tags.iter()
        .filter_map(format_disambiguating_tag)
        .filter(|token| !token.is_empty())
        .take(MAX_DISAMBIGUATING_TAGS)
        .collect()
}

fn format_disambiguating_tag(tag: &TagInfo) -> Option<String> {
    // Normalize the value to trimmed lowercase up front so the type-specific
    // compaction below matches its canonical (lowercase) keys regardless of how
    // Core cased the value: `World` -> `W`, `Revision-A` -> `RA`.
    let value = tag.tag.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    let token = match tag.tag_type.trim().to_ascii_lowercase().as_str() {
        // Region/language can be multi-valued (e.g. `us,eu`); each part is an
        // uppercase code, joined with `/`. `world` -> `W` (MiSTer Arcade's own
        // shorthand) and a couple of long names get shortened.
        "region" | "lang" => value
            .split(',')
            .map(|part| compact_region(part.trim()))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/")
            .to_ascii_uppercase(),
        // Compact alphanumeric forms: D2 / R1 / 2P. The rev value may itself be
        // spelled "revision-a"/"v1" in arcade sets; strip that prefix first.
        "disc" => format!("D{value}").to_ascii_uppercase(),
        "rev" => format!("R{}", strip_rev_prefix(&value)).to_ascii_uppercase(),
        "players" => format!("{value}P").to_ascii_uppercase(),
        // Normalized to `YYYY-MM-DD`; the two-digit year differentiates siblings
        // in the least space (`'96`).
        "builddate" => format_year_token(&value),
        "edition" => compact_edition(&value),
        // Free-text / flag types: lowercase readable value with its dashes kept
        // (they double as word breaks for the sibling-diff), hard-capped so it
        // can't run away.
        _ => truncate_token(&tag_display_value(tag).to_ascii_lowercase()),
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

/// Uppercase short forms for the common edition values; everything else falls
/// back to the lowercase, dash-preserving, capped value.
fn compact_edition(value: &str) -> String {
    match value {
        "directors-cut" => "DC".to_string(),
        "collectors" => "CE".to_string(),
        "limited" => "LE".to_string(),
        "special" => "SE".to_string(),
        "deluxe" => "DLX".to_string(),
        "ultimate" => "ULT".to_string(),
        "anniversary" => "ANNIV".to_string(),
        "remaster" | "remastered" => "REMAS".to_string(),
        other => truncate_token(&other.to_ascii_lowercase()),
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
        let members: Vec<(Vec<String>, Vec<char>)> = rows[i..j]
            .iter()
            .map(|(_, toks)| split_words_with_sep(toks))
            .collect();
        let words_only: Vec<&[String]> = members.iter().map(|(w, _)| w.as_slice()).collect();
        let (lead, trail) = common_affix(&words_only);
        for (words, seps) in &members {
            out.push(join_trimmed(words, seps, lead, trail));
        }
        i = j;
    }
    out
}

/// Split a row's tokens into words plus the separator that sits between each
/// consecutive pair, so the trimmed result can be rejoined with the original
/// delimiter (dashes preserved). Within a free-text token the separator is `-`
/// (`atari-lightgun` -> words `atari`, `lightgun`); between two distinct tokens
/// it is a space (`US`, `R1` -> `US R1`). Structured tokens (`US`, `D2`) have no
/// internal dash and stay atomic. `seps` has one fewer entry than `words`.
fn split_words_with_sep(tokens: &[String]) -> (Vec<String>, Vec<char>) {
    let mut words: Vec<String> = Vec::new();
    let mut seps: Vec<char> = Vec::new();
    for token in tokens {
        for (idx, seg) in token.split('-').filter(|s| !s.is_empty()).enumerate() {
            if !words.is_empty() {
                // First segment of a new token follows a space; later segments
                // of the same token follow the dash they were split on.
                seps.push(if idx == 0 { ' ' } else { '-' });
            }
            words.push(seg.to_string());
        }
    }
    (words, seps)
}

/// Longest leading and trailing word-runs shared by ALL members, capped so no
/// member is trimmed to empty. `(0, 0)` for groups smaller than two.
fn common_affix(members: &[&[String]]) -> (usize, usize) {
    if members.len() < 2 {
        return (0, 0);
    }
    let min_len = members.iter().map(|m| m.len()).min().unwrap_or(0);
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

/// Rejoin the kept `words[lead..len-trail]` range, placing the original
/// separator (`seps[i - 1]`) between consecutive kept words so dashes survive.
fn join_trimmed(words: &[String], seps: &[char], lead: usize, trail: usize) -> String {
    let end = words.len() - trail;
    let mut out = String::new();
    for i in lead..end {
        if i > lead {
            out.push(seps[i - 1]);
        }
        out.push_str(&words[i]);
    }
    out
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
    fn formats_common_types_into_short_uppercase_tokens() {
        assert_eq!(
            disambiguating_tag_labels(&[
                tag("us", "region"),
                tag("ja", "lang"),
                tag("2", "disc"),
                tag("1", "rev"),
            ]),
            vec!["US", "JA", "D2", "R1"]
        );
        assert_eq!(
            disambiguating_tag_labels(&[
                tag("2", "players"),
                tag("1996-10-04", "builddate"),
                tag("hack", "unlicensed"),
            ]),
            // Curated codes uppercase; free-text flag stays lowercase.
            vec!["2P", "'96", "hack"]
        );
    }

    #[test]
    fn region_world_uses_mister_shorthand() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("world", "region")]),
            vec!["W"]
        );
    }

    #[test]
    fn mixed_case_values_are_normalized_before_compaction() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("World", "region")]),
            vec!["W"]
        );
        assert_eq!(
            disambiguating_tag_labels(&[tag("Revision-A", "rev")]),
            vec!["RA"]
        );
    }

    #[test]
    fn rev_strips_spelled_out_prefix() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("revision-a", "rev")]),
            vec!["RA"]
        );
        assert_eq!(
            disambiguating_tag_labels(&[tag("1-2", "rev")]),
            vec!["R1-2"]
        );
    }

    #[test]
    fn region_multi_value_joins_with_slash() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("us,eu", "region")]),
            vec!["US/EU"]
        );
    }

    #[test]
    fn free_text_keeps_dashes_lowercase_and_is_capped() {
        assert_eq!(
            disambiguating_tag_labels(&[tag("atari-lightgun", "unknown")]),
            vec!["atari-lightgun"]
        );
        // "homebrew-translation" (20 chars) -> hard cut to 14 with the dash kept.
        assert_eq!(
            disambiguating_tag_labels(&[tag("homebrew-translation", "unknown")]),
            vec!["homebrew-trans"]
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
            ("Crossbow".into(), vec!["atari-joystick".into()]),
            ("Crossbow".into(), vec!["atari-lightgun".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["joystick", "lightgun"]
        );
    }

    #[test]
    fn sibling_diff_keeps_each_member_nonempty_and_preserves_dashes() {
        let rows = vec![
            ("Arkanoid".into(), vec!["unl-lives-slow".into()]),
            ("Arkanoid".into(), vec!["unl-lives".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["lives-slow", "lives"]
        );
    }

    #[test]
    fn sibling_diff_drops_shared_trailing_token() {
        let rows = vec![
            ("Game".into(), vec!["US".into(), "R1".into()]),
            ("Game".into(), vec!["EU".into(), "R1".into()]),
        ];
        assert_eq!(sibling_disambiguation_displays(&rows), vec!["US", "EU"]);
    }

    #[test]
    fn sibling_diff_singleton_is_unchanged() {
        let rows = vec![("Solo".into(), vec!["US".into(), "R1".into()])];
        assert_eq!(sibling_disambiguation_displays(&rows), vec!["US R1"]);
    }

    #[test]
    fn sibling_diff_distinct_names_not_grouped() {
        let rows = vec![
            ("A".into(), vec!["atari-joystick".into()]),
            ("B".into(), vec!["atari-lightgun".into()]),
        ];
        assert_eq!(
            sibling_disambiguation_displays(&rows),
            vec!["atari-joystick", "atari-lightgun"]
        );
    }
}
