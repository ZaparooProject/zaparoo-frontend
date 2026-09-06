// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Every face the frontend can render with travels inside the binary, as in
// the Qt build: one static file on the SD card, nothing to sync or lose.
// The base faces (Noto Sans and the CRT bitmap face) are imported by
// `ui/app.slint`; the six script faces below are embedded here and handed
// to Slint's shared font collection at startup, zero-copy (fontique keeps a
// reference to the static bytes). They are static Regular instances
// (`resources/fonts/runtime/`, see its README) rather than the variable
// files the Qt build embeds: Slint's runtime text path renders a variable
// font's default instance, and four of the six default to Thin.
//
// Registration alone is not enough for fallback. Slint resolves a run of
// Arabic inside a "Noto Sans" label through fontique's per-script fallback
// table and the generic sans-serif families, and both are empty on a
// device without system fonts. So each face is also appended as the
// fallback for the scripts it covers and to the generic families.

use slint::fontique_010::fontique::{self, FallbackKey, GenericFamily, Script};

struct Face {
    bytes: &'static [u8],
    /// ISO 15924 script codes this face is the fallback for.
    scripts: &'static [&'static str],
}

// Order matters for scripts several faces cover: Han ideographs come from
// the JP face first, then TC, then KR. Slint does not tag runs with a
// language yet, so this is a compromise between the ja, zh_CN and ko
// catalogs (the Qt build had the same three faces and the same ambiguity).
static FACES: [Face; 6] = [
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansArabic-Regular.ttf"),
        scripts: &["Arab"],
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansDevanagari-Regular.ttf"),
        scripts: &["Deva"],
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansHebrew-Regular.ttf"),
        scripts: &["Hebr"],
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansJP-Regular.ttf"),
        scripts: &["Hira", "Kana", "Hani"],
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansTC-Regular.ttf"),
        scripts: &["Hani", "Bopo"],
    },
    Face {
        bytes: include_bytes!("../../../resources/fonts/runtime/NotoSansKR-Regular.ttf"),
        scripts: &["Hang", "Hani"],
    },
];

/// Registers the embedded script faces with Slint's font collection and
/// wires them into script and generic-family fallback. Call once, after the
/// platform exists (the first component has been created) and before the
/// first frame.
pub fn register_embedded_fonts() {
    let mut collection = slint::fontique_010::shared_collection();
    let mut all_families = Vec::new();
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
        all_families.extend(families);
    }
    for generic in [
        GenericFamily::SansSerif,
        GenericFamily::SystemUi,
        GenericFamily::UiSansSerif,
    ] {
        collection.append_generic_families(generic, all_families.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::FACES;

    #[test]
    fn embedded_faces_are_truetype() {
        for face in &FACES {
            assert!(face.bytes.len() > 10_000);
            assert_eq!(&face.bytes[..4], &[0, 1, 0, 0], "TrueType sfnt header");
        }
    }
}
