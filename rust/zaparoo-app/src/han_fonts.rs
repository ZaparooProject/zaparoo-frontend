// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Han ideographs are one Unicode block drawn four ways. Japanese, Korean,
// Simplified Chinese and Traditional Chinese each have their own glyph
// shapes for the same code point, and no single face covers every
// character the other three use. The text renderer does not tag runs with
// a language, so the interface language decides which face is tried
// first; the rest follow so a character the preferred face lacks still
// renders.

/// One of the embedded faces that can draw Han ideographs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HanFace {
    Japanese,
    Korean,
    SimplifiedChinese,
    TraditionalChinese,
}

/// The order Han faces are tried in for a language or locale tag (`ja`,
/// `zh_CN`, `zh-Hant`, `ko_KR.UTF-8`, ...). Anything that is not
/// Chinese, Japanese or Korean keeps the Japanese face first.
pub fn han_face_order(tag: &str) -> [HanFace; 4] {
    use HanFace::{Japanese, Korean, SimplifiedChinese, TraditionalChinese};
    let tag = tag.split(['.', '@']).next().unwrap_or_default();
    let mut parts = tag.split(['_', '-']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    match language.as_str() {
        "zh" => {
            // An explicit script subtag wins over the region.
            let parts: Vec<&str> = parts.collect();
            let has = |wanted: &[&str]| {
                parts
                    .iter()
                    .any(|part| wanted.iter().any(|w| part.eq_ignore_ascii_case(w)))
            };
            let traditional = has(&["hant"]) || (!has(&["hans"]) && has(&["tw", "hk", "mo"]));
            if traditional {
                [TraditionalChinese, SimplifiedChinese, Japanese, Korean]
            } else {
                [SimplifiedChinese, TraditionalChinese, Japanese, Korean]
            }
        }
        "ko" => [Korean, Japanese, SimplifiedChinese, TraditionalChinese],
        _ => [Japanese, SimplifiedChinese, TraditionalChinese, Korean],
    }
}

#[cfg(test)]
mod tests {
    use super::han_face_order;
    use super::HanFace::{Japanese, Korean, SimplifiedChinese, TraditionalChinese};

    #[test]
    fn each_cjk_language_prefers_its_own_face() {
        assert_eq!(han_face_order("zh_CN")[0], SimplifiedChinese);
        assert_eq!(han_face_order("zh_TW")[0], TraditionalChinese);
        assert_eq!(han_face_order("ja")[0], Japanese);
        assert_eq!(han_face_order("ko")[0], Korean);
    }

    #[test]
    fn full_orders_match_the_agreed_table() {
        assert_eq!(
            han_face_order("zh_CN"),
            [SimplifiedChinese, TraditionalChinese, Japanese, Korean]
        );
        assert_eq!(
            han_face_order("zh_TW"),
            [TraditionalChinese, SimplifiedChinese, Japanese, Korean]
        );
        assert_eq!(
            han_face_order("ja"),
            [Japanese, SimplifiedChinese, TraditionalChinese, Korean]
        );
        assert_eq!(
            han_face_order("ko"),
            [Korean, Japanese, SimplifiedChinese, TraditionalChinese]
        );
    }

    #[test]
    fn other_languages_keep_japanese_first() {
        for tag in ["", "auto", "en", "de_AT", "ar", "he"] {
            assert_eq!(
                han_face_order(tag),
                [Japanese, SimplifiedChinese, TraditionalChinese, Korean],
                "{tag}"
            );
        }
    }

    #[test]
    fn locale_tags_resolve_by_language_script_and_region() {
        assert_eq!(han_face_order("ja_JP.UTF-8")[0], Japanese);
        assert_eq!(han_face_order("ko-KR")[0], Korean);
        assert_eq!(han_face_order("zh")[0], SimplifiedChinese);
        assert_eq!(han_face_order("zh_SG")[0], SimplifiedChinese);
        assert_eq!(han_face_order("zh-Hans-HK")[0], SimplifiedChinese);
        assert_eq!(han_face_order("zh-Hant-CN")[0], TraditionalChinese);
        assert_eq!(han_face_order("zh_HK")[0], TraditionalChinese);
        assert_eq!(han_face_order("zh-Hant")[0], TraditionalChinese);
        assert_eq!(han_face_order("ZH_tw")[0], TraditionalChinese);
    }

    #[test]
    fn every_order_lists_each_face_once() {
        for tag in ["zh_CN", "zh_TW", "ja", "ko", "en"] {
            let order = han_face_order(tag);
            for face in [Japanese, Korean, SimplifiedChinese, TraditionalChinese] {
                assert_eq!(order.iter().filter(|f| **f == face).count(), 1, "{tag}");
            }
        }
    }
}
