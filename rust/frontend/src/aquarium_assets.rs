// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The embedded Aquarium collection. Borrow a PNG by archive path without
//! allocating an image cache, reading disk, or unpacking the whole collection.
//! `just aquarium-assets` packs the source folders into the restricted USTAR
//! format this reader accepts: regular files with names of at most 100 bytes.

static PACK: &[u8] = include_bytes!("../assets/aquarium.tar");
const BLOCK: usize = 512;

#[derive(Debug)]
pub(crate) struct FishAsset {
    pub path: &'static str,
    pub crop: [u32; 4],
    pub faces_right: bool,
}

impl FishAsset {
    pub fn size(&self) -> (u32, u32) {
        (self.crop[2], self.crop[3])
    }
}

/// Metadata only; actual PNGs are decoded just for the fish selected for a run.
pub(crate) fn fish_catalog() -> &'static [FishAsset] {
    static CATALOG: std::sync::OnceLock<Vec<FishAsset>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let text = get("fish-catalog.tsv")
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or_default();
        text.lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let path = fields.next()?;
                let crop = [
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                    fields.next()?.parse().ok()?,
                ];
                let faces_right = fields.next()? == "1";
                Some(FishAsset {
                    path,
                    crop,
                    faces_right,
                })
            })
            .collect()
    })
}

pub(crate) fn get(name: &str) -> Option<&'static [u8]> {
    Entries(PACK).find_map(|(path, bytes)| (path == name).then_some(bytes))
}

struct Entries<'a>(&'a [u8]);

impl<'a> Iterator for Entries<'a> {
    type Item = (&'a str, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let remaining = std::mem::take(&mut self.0);
        let header = remaining.get(..BLOCK)?;
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        // Only our packer's simple regular-file headers are supported.
        if !matches!(header[156], 0 | b'0')
            || header.get(257..263)? != b"ustar\0"
            || header.get(345..500)?.iter().any(|byte| *byte != 0)
        {
            return None;
        }
        let name_end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        let name = std::str::from_utf8(header.get(..name_end)?).ok()?;
        let size_text = std::str::from_utf8(header.get(124..136)?).ok()?;
        let size = usize::from_str_radix(size_text.trim_matches(['\0', ' ']), 8).ok()?;
        let padded = size.checked_add(BLOCK - 1)? / BLOCK * BLOCK;
        let next = BLOCK.checked_add(padded)?;
        let bytes = remaining.get(BLOCK..BLOCK.checked_add(size)?)?;
        self.0 = remaining.get(next..)?;
        Some((name, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_is_addressable_and_every_payload_is_a_valid_png() {
        let mut counts = std::collections::BTreeMap::new();
        for (path, bytes) in Entries(PACK) {
            if path == "fish-catalog.tsv" {
                continue;
            }
            let category = path.split('/').next().unwrap_or_default();
            *counts.entry(category).or_insert(0) += 1;
            let decoded = image::load_from_memory(bytes);
            assert!(decoded.is_ok(), "invalid image: {path}");
            assert_eq!(get(path), Some(bytes));
        }
        assert_eq!(counts.get("fish"), Some(&178));
        for category in ["coral", "scenery", "ruins", "relics", "wreckage"] {
            assert_eq!(counts.get(category), Some(&12), "{category}");
        }
        assert_eq!(counts.get("runtime"), Some(&8));
        assert!(get("fish/missing.png").is_none());
    }

    #[test]
    fn catalog_covers_every_fish_and_crops_fit_the_originals() {
        let catalog = fish_catalog();
        assert_eq!(catalog.len(), 178);
        let paths: std::collections::HashSet<_> = catalog.iter().map(|fish| fish.path).collect();
        assert_eq!(paths.len(), catalog.len());
        for fish in catalog {
            let image = image::load_from_memory(get(fish.path).unwrap_or_default());
            assert!(image.is_ok(), "{}: invalid PNG", fish.path);
            if let Ok(image) = image {
                let [x, y, w, h] = fish.crop;
                assert!(w > 0 && h > 0 && x + w <= image.width() && y + h <= image.height());
                assert_eq!(fish.size(), (w, h));
            }
        }
        assert_eq!(catalog.iter().filter(|fish| fish.faces_right).count(), 12);
    }

    #[test]
    fn reads_borrow_the_embedded_payload_and_truncation_is_safe() {
        let (name, bytes) = Entries(PACK).next().unwrap_or(("", &[]));
        assert!(!bytes.is_empty());
        assert!(std::ptr::eq(
            get(name).unwrap_or_default().as_ptr(),
            bytes.as_ptr()
        ));
        assert!(Entries(&PACK[..BLOCK - 1]).next().is_none());
        assert!(Entries(&PACK[..BLOCK]).next().is_none());
        let mut unsupported = PACK[..BLOCK].to_vec();
        unsupported[156] = b'2';
        assert!(Entries(&unsupported).next().is_none());
    }
}
