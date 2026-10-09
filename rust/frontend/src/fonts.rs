// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Every face the frontend can render with travels inside the binary: one
// static file on the SD card, nothing to sync or lose.
// The base faces (Noto Sans and the CRT bitmap face) are imported by
// `ui/app.slint`; the seven script faces below are embedded here and handed
// to Slint's shared font collection at startup, zero-copy (fontique keeps a
// reference to the static bytes). They are static Regular instances
// (`resources/fonts/runtime/`, see its README) rather than the variable
// files: Slint's runtime text path renders a variable font's default
// instance, and five of the seven default to Thin.
//
// Registration alone is not enough for fallback. Slint resolves a run of
// Arabic inside a "Noto Sans" label through fontique's per-script fallback
// table and the generic sans-serif families, and both are empty on a
// device without system fonts. So each face is also made the fallback for
// the scripts it covers and listed in the generic families.
//
// Han ideographs are the exception: four faces draw them, each with its
// own glyph shapes and none covering all of the others. Slint does not tag
// runs with a language, so the interface language picks the order
// (`zaparoo_app::han_fonts`) and `set_han_preference` reorders them
// whenever the language changes.

use std::sync::Mutex;

use slint::fontique_011::fontique::{self, FallbackKey, FamilyId, GenericFamily, Script};
use zaparoo_app::han_fonts::{han_face_order, HanFace};

struct Face {
    bytes: &'static [u8],
    /// ISO 15924 script codes this face is the fallback for, Han aside.
    scripts: &'static [&'static str],
    han: Option<HanFace>,
}

static FACES: [Face; 7] = [
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansArabic-Regular.ttf"),
        scripts: &["Arab"],
        han: None,
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansDevanagari-Regular.ttf"),
        scripts: &["Deva"],
        han: None,
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansHebrew-Regular.ttf"),
        scripts: &["Hebr"],
        han: None,
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansJP-Regular.ttf"),
        scripts: &["Hira", "Kana"],
        han: Some(HanFace::Japanese),
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansSC-Regular.ttf"),
        scripts: &[],
        han: Some(HanFace::SimplifiedChinese),
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansTC-Regular.ttf"),
        scripts: &["Bopo"],
        han: Some(HanFace::TraditionalChinese),
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansKR-Regular.ttf"),
        scripts: &["Hang"],
        han: Some(HanFace::Korean),
    },
];

const GENERIC_FAMILIES: [GenericFamily; 3] = [
    GenericFamily::SansSerif,
    GenericFamily::SystemUi,
    GenericFamily::UiSansSerif,
];

/// The families registration produced, kept so the Han faces can be put in
/// another order.
struct Registered {
    /// Families of the faces that draw no Han, in `FACES` order.
    other: Vec<FamilyId>,
    han: Vec<(HanFace, Vec<FamilyId>)>,
}

static REGISTERED: Mutex<Option<Registered>> = Mutex::new(None);

/// Registers the embedded script faces with Slint's font collection and
/// wires them into script and generic-family fallback. Call once, after the
/// platform exists (the first component has been created) and before the
/// first frame. Han starts in the default order; `set_han_preference`
/// follows the language.
pub fn register_embedded_fonts() {
    let mut registered = Registered {
        other: Vec::new(),
        han: Vec::new(),
    };
    {
        let mut collection = slint::fontique_011::shared_collection();
        for face in &FACES {
            let blob = fontique::Blob::new(std::sync::Arc::new(face.bytes));
            let families: Vec<_> = collection
                .register_fonts(blob, None)
                .into_iter()
                .map(|(family, _)| family)
                .collect();
            for script in face.scripts {
                collection.append_fallbacks(
                    FallbackKey::new(Script::from_str_unchecked(script), None),
                    families.iter().copied(),
                );
            }
            match face.han {
                Some(han) => registered.han.push((han, families)),
                None => registered.other.extend(families),
            }
        }
    }
    *REGISTERED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(registered);
    set_han_preference("");
}

/// Orders the Han faces for a language or locale tag, so shared ideographs
/// take that language's glyph shapes. A no-op until the faces are
/// registered.
///
/// Both lists are rewritten. A label's font stack is its family and then
/// the generic families, and only a character none of those cover reaches
/// the per-script fallback, so the generic order is the one that decides
/// for a "Noto Sans" label.
pub fn set_han_preference(language: &str) {
    let (han, generic): (Vec<FamilyId>, Vec<FamilyId>) = {
        let registered = REGISTERED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(registered) = registered.as_ref() else {
            return;
        };
        let han: Vec<FamilyId> = han_face_order(language)
            .iter()
            .flat_map(|wanted| {
                registered
                    .han
                    .iter()
                    .filter(move |(face, _)| face == wanted)
                    .flat_map(|(_, families)| families.iter().copied())
            })
            .collect();
        let generic = registered.other.iter().copied().chain(han.iter().copied());
        (han.clone(), generic.collect())
    };
    let mut collection = slint::fontique_011::shared_collection();
    collection.set_fallbacks(
        FallbackKey::new(Script::from_str_unchecked("Hani"), None),
        han.into_iter(),
    );
    for family in GENERIC_FAMILIES {
        collection.set_generic_families(family, generic.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests should fail fast")]

    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use zaparoo_app::han_fonts::han_face_order;

    use super::FACES;

    // The Latin, Greek and Cyrillic face `ui/app.slint` imports.
    static BASE: &[u8] = include_bytes!("../../../resources/fonts/NotoSans.ttf");

    fn be16(data: &[u8], at: usize) -> u32 {
        u32::from(u16::from_be_bytes([data[at], data[at + 1]]))
    }

    fn be32(data: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    /// Inclusive code point ranges a face maps, read from its `cmap`
    /// format 12 subtable, or format 4 when it has no format 12. A format 4
    /// range can hold unmapped holes; none of the faces checked here rely
    /// on one.
    fn coverage(font: &[u8]) -> Vec<(u32, u32)> {
        let tables = be16(font, 4) as usize;
        let cmap = (0..tables)
            .map(|i| 12 + i * 16)
            .find(|at| &font[*at..*at + 4] == b"cmap")
            .map(|at| be32(font, at + 8) as usize)
            .expect("cmap table");
        let subtables: Vec<usize> = (0..be16(font, cmap + 2) as usize)
            .map(|i| cmap + be32(font, cmap + 4 + i * 8 + 4) as usize)
            .collect();
        if let Some(at) = subtables.iter().find(|at| be16(font, **at) == 12) {
            return (0..be32(font, at + 12) as usize)
                .map(|i| at + 16 + i * 12)
                .map(|group| (be32(font, group), be32(font, group + 4)))
                .collect();
        }
        let at = subtables
            .iter()
            .find(|at| be16(font, **at) == 4)
            .expect("cmap format 4 or 12");
        let segments = be16(font, at + 6) as usize / 2;
        (0..segments)
            .map(|i| {
                let end = be16(font, at + 14 + i * 2);
                let start = be16(font, at + 16 + segments * 2 + i * 2);
                (start, end)
            })
            .filter(|(start, _)| *start != 0xFFFF)
            .collect()
    }

    fn covers(ranges: &[(u32, u32)], c: char) -> bool {
        let c = u32::from(c);
        ranges
            .iter()
            .any(|(start, end)| (*start..=*end).contains(&c))
    }

    /// Every non-ASCII character a catalog can put on screen, by language.
    fn catalogs() -> Vec<(String, BTreeSet<char>)> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("translations");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(root).expect("translations directory") {
            let entry = entry.expect("directory entry");
            let po = entry.path().join("LC_MESSAGES/frontend.po");
            let Ok(text) = std::fs::read_to_string(po) else {
                continue;
            };
            let chars = text
                .lines()
                .filter(|line| !line.starts_with('#'))
                .flat_map(str::chars)
                .filter(|c| !c.is_ascii())
                .collect();
            out.push((entry.file_name().to_string_lossy().into_owned(), chars));
        }
        assert!(out.len() >= 17, "catalogs found: {}", out.len());
        out
    }

    fn is_han(c: char) -> bool {
        matches!(u32::from(c), 0x3400..=0x4DBF | 0x4E00..=0x9FFF)
    }

    #[test]
    fn embedded_faces_are_truetype() {
        for face in &FACES {
            assert!(face.bytes.len() > 10_000);
            assert_eq!(&face.bytes[..4], &[0, 1, 0, 0], "TrueType sfnt header");
        }
    }

    #[test]
    fn every_han_face_in_an_order_is_embedded() {
        for wanted in han_face_order("") {
            assert_eq!(
                FACES.iter().filter(|f| f.han == Some(wanted)).count(),
                1,
                "{wanted:?}"
            );
        }
    }

    // The shared collection needs a platform, and the probe platform is
    // the one that needs no display.
    #[cfg(feature = "mister")]
    #[test]
    fn the_language_orders_the_han_faces_in_both_lists() {
        use slint::fontique_011::fontique::{FallbackKey, GenericFamily, Script};

        assert!(
            slint::platform::set_platform(Box::new(crate::route_motion::ProbePlatform)).is_ok()
        );

        // One test, so no other thread reorders the shared collection
        // between the call and the read.
        for (language, expected) in [
            (
                "zh_CN",
                [
                    "Noto Sans SC",
                    "Noto Sans TC",
                    "Noto Sans JP",
                    "Noto Sans KR",
                ],
            ),
            (
                "zh_TW",
                [
                    "Noto Sans TC",
                    "Noto Sans SC",
                    "Noto Sans JP",
                    "Noto Sans KR",
                ],
            ),
            (
                "ko",
                [
                    "Noto Sans KR",
                    "Noto Sans JP",
                    "Noto Sans SC",
                    "Noto Sans TC",
                ],
            ),
            (
                "ja",
                [
                    "Noto Sans JP",
                    "Noto Sans SC",
                    "Noto Sans TC",
                    "Noto Sans KR",
                ],
            ),
            (
                "de",
                [
                    "Noto Sans JP",
                    "Noto Sans SC",
                    "Noto Sans TC",
                    "Noto Sans KR",
                ],
            ),
        ] {
            super::register_embedded_fonts();
            super::set_han_preference(language);
            let mut collection = slint::fontique_011::shared_collection();
            let expected: Vec<_> = expected
                .iter()
                .map(|name| collection.family_id(name).expect("registered family"))
                .collect();
            let fallbacks: Vec<_> = collection
                .fallback_families(FallbackKey::new(Script::from_str_unchecked("Hani"), None))
                .collect();
            assert_eq!(fallbacks, expected, "{language}: Han fallback");
            let generic: Vec<_> = collection
                .generic_families(GenericFamily::SansSerif)
                .filter(|family| expected.contains(family))
                .collect();
            assert_eq!(generic, expected, "{language}: generic sans-serif");
        }
    }

    #[test]
    fn every_catalog_character_has_a_glyph() {
        let mut faces: Vec<Vec<(u32, u32)>> = FACES.iter().map(|f| coverage(f.bytes)).collect();
        faces.push(coverage(BASE));
        for (language, chars) in catalogs() {
            let missing: String = chars
                .iter()
                .filter(|c| !faces.iter().any(|ranges| covers(ranges, **c)))
                .collect();
            assert!(missing.is_empty(), "{language}: no glyph for {missing}");
        }
    }

    #[test]
    fn a_language_draws_its_han_text_from_its_preferred_face() {
        for (language, chars) in catalogs() {
            let preferred = han_face_order(&language)[0];
            let face = FACES
                .iter()
                .find(|f| f.han == Some(preferred))
                .expect("preferred face");
            let ranges = coverage(face.bytes);
            if !["zh_CN", "zh_TW", "ja", "ko"].contains(&language.as_str()) {
                continue;
            }
            let missing: String = chars
                .iter()
                .filter(|c| is_han(**c) && !covers(&ranges, **c))
                .collect();
            assert!(
                missing.is_empty(),
                "{language}: {preferred:?} face lacks {missing}"
            );
        }
    }
}
