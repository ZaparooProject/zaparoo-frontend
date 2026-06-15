// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Region resolution for system names and logo variants.
//
// One shared `Region` value drives both the localized name shown in the
// systems grid and (when regional logo art exists) the logo artwork key
// selected for that system. The user picks `auto`/`us`/`eu`/`jp` from
// Settings; `auto` derives the region from the effective UI locale pushed
// by C++ at startup via `zaparoo_rust_set_effective_locale`.
//
// Effective locale is pushed once by `main.cpp` after `QLocale(langCode)`
// is constructed (line ~192), before the QML engine and any model
// `Initialize` callbacks run. The OnceLock-backed global is therefore
// always populated before `current_region()` is called.

use std::sync::OnceLock;

use crate::models::with_persist_read;

/// Display region. Values map to the three `Names_MiSTer` locale sets
/// (US / EU / JP) that are hardcoded in `system_names.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Us,
    Eu,
    Jp,
}

/// The effective system locale name resolved by C++ before any model
/// initialises. Set exactly once via `zaparoo_rust_set_effective_locale`.
/// An empty string (the default) falls through to the EU fallback in
/// `resolve_region`.
static EFFECTIVE_LOCALE: OnceLock<String> = OnceLock::new();

/// Push the effective locale from C++ (`locale.name()`, e.g. `"en_US"`,
/// `"ja_JP"`, `"de_DE"`). Call once during `main.cpp` before the QML
/// engine starts. Later calls are silent no-ops (`OnceLock` semantics).
///
/// # Safety
///
/// `locale_ptr` must point to `locale_len` bytes of valid UTF-8 for the
/// duration of this call, unless `locale_len` is zero.
#[no_mangle]
pub unsafe extern "C" fn zaparoo_rust_set_effective_locale(
    locale_ptr: *const u8,
    locale_len: usize,
) {
    let locale = if locale_ptr.is_null() || locale_len == 0 {
        String::new()
    } else {
        // SAFETY: caller guarantees a valid UTF-8 slice.
        let bytes = unsafe { std::slice::from_raw_parts(locale_ptr, locale_len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    let _ = EFFECTIVE_LOCALE.set(locale);
}

/// Resolve a `Region` from the persisted setting and the effective locale.
///
/// - Explicit `"us"` / `"eu"` / `"jp"` → direct.
/// - `"auto"` (or unknown/empty) → derived from `effective_locale`:
///   - `"en"` prefix → US
///   - `"ja"` prefix → JP
///   - anything else → EU
pub fn resolve_region(setting: &str, effective_locale: &str) -> Region {
    match setting.trim() {
        "us" => Region::Us,
        "eu" => Region::Eu,
        "jp" => Region::Jp,
        _ => {
            // Strip the region tag; use only the language prefix.
            let lang = effective_locale
                .split_once(['_', '-'])
                .map_or(effective_locale, |(lang, _)| lang)
                .to_ascii_lowercase();
            match lang.as_str() {
                "en" => Region::Us,
                "ja" => Region::Jp,
                _ => Region::Eu,
            }
        }
    }
}

/// Read the current region from persisted settings and the effective locale.
/// Callers that want the region for a row-projection pass should call this
/// once and thread the value through rather than calling it per-row.
pub fn current_region() -> Region {
    let setting = with_persist_read(|s| s.settings.region.clone());
    let effective_locale = EFFECTIVE_LOCALE
        .get()
        .map_or("", |s| s.as_str())
        .to_string();
    resolve_region(&setting, &effective_locale)
}

#[cfg(test)]
mod tests {
    use super::{resolve_region, Region};

    #[test]
    fn explicit_us_resolves_to_us() {
        assert_eq!(resolve_region("us", "de_DE"), Region::Us);
    }

    #[test]
    fn explicit_eu_resolves_to_eu() {
        assert_eq!(resolve_region("eu", "en_US"), Region::Eu);
    }

    #[test]
    fn explicit_jp_resolves_to_jp() {
        assert_eq!(resolve_region("jp", "en_US"), Region::Jp);
    }

    #[test]
    fn auto_english_locale_is_us() {
        assert_eq!(resolve_region("auto", "en_US"), Region::Us);
        assert_eq!(resolve_region("auto", "en_GB"), Region::Us);
        assert_eq!(resolve_region("auto", "en"), Region::Us);
    }

    #[test]
    fn auto_japanese_locale_is_jp() {
        assert_eq!(resolve_region("auto", "ja_JP"), Region::Jp);
        assert_eq!(resolve_region("auto", "ja"), Region::Jp);
    }

    #[test]
    fn auto_other_locales_are_eu() {
        for locale in ["de_DE", "fr_FR", "es_ES", "it_IT", "ko_KR", "zh_CN", ""] {
            assert_eq!(
                resolve_region("auto", locale),
                Region::Eu,
                "locale={locale}"
            );
        }
    }

    #[test]
    fn unknown_setting_falls_back_to_auto_logic() {
        // An unrecognized setting string is treated as auto.
        assert_eq!(resolve_region("", "en_US"), Region::Us);
        assert_eq!(resolve_region("unknown", "de_DE"), Region::Eu);
    }
}
