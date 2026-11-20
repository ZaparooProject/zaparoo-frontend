// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The header clock: the `clock_format` setting picks 12h or 24h outright,
// `off` hides the header clock, and `auto` follows the effective locale
// (the language setting, else the host locale). With no CLDR tables to
// consult, the locale rule is a small list of the 12-hour languages the
// app ships.

/// The first year a clock reading is taken as set. A device with no clock
/// battery boots in 1970 and only learns the time from the network; this
/// is the same floor Core uses for its own `clockReliable`.
pub const MIN_SET_YEAR: i32 = 2024;

/// Whether a reading in `year` is a real time rather than the boot default
/// of a device that has not synced yet.
pub fn is_set(year: i32) -> bool {
    year >= MIN_SET_YEAR
}

/// Whether the header shows the clock for `clock_format`.
pub fn shown(clock_format: &str) -> bool {
    clock_format != "off"
}

/// Whether the clock shows 12-hour time for `clock_format` (`auto`, `12h`,
/// `24h` or `off`), given the language setting (`auto` or empty defers to
/// the host locale tag, if any). `off` only hides the header clock; the
/// screensaver's clock styles still follow the language, as `auto` does.
pub fn uses_twelve_hour(clock_format: &str, language: &str, host_locale: Option<&str>) -> bool {
    match clock_format {
        "12h" => true,
        "24h" => false,
        _ => {
            let tag = if language.is_empty() || language == "auto" {
                host_locale.unwrap_or_default()
            } else {
                language
            };
            locale_uses_twelve_hour(tag)
        }
    }
}

/// CLDR short time pattern class for a locale tag (`en_AU`, `de-DE`,
/// `ar_EG.UTF-8`, ...). English defaults to 12-hour except the European
/// and southern African variants; Arabic, Greek, Hindi, Korean, Mexican
/// Spanish and traditional Chinese are 12-hour; everything else is 24.
pub fn locale_uses_twelve_hour(tag: &str) -> bool {
    let tag = tag.split(['.', '@']).next().unwrap_or_default();
    let mut parts = tag.split(['_', '-']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = parts.next().unwrap_or_default().to_ascii_uppercase();
    match language.as_str() {
        "en" => !matches!(region.as_str(), "GB" | "IE" | "ZA" | "DK" | "150"),
        "ar" | "el" | "hi" | "ko" | "bn" | "ur" => true,
        "es" => matches!(region.as_str(), "MX" | "US"),
        "zh" => matches!(region.as_str(), "TW" | "HK" | "HANT"),
        _ => false,
    }
}

/// `12:05 AM` in 12-hour form, `00:05` in 24-hour form.
pub fn format(hour: u8, minute: u8, twelve_hour: bool) -> String {
    if twelve_hour {
        let (h, suffix) = match hour {
            0 => (12, "AM"),
            h @ 1..=11 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{h}:{minute:02} {suffix}")
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// The widest reading for the format, measured once so the clock's slot
/// never reflows on a minute boundary.
pub fn widest_sample(twelve_hour: bool) -> &'static str {
    if twelve_hour {
        "12:59 PM"
    } else {
        "23:59"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_formats_win() {
        assert!(uses_twelve_hour("12h", "de", None));
        assert!(!uses_twelve_hour("24h", "en_US", None));
    }

    #[test]
    fn auto_follows_the_language_then_the_host() {
        assert!(uses_twelve_hour("auto", "en", None));
        assert!(!uses_twelve_hour("auto", "en_GB", None));
        assert!(!uses_twelve_hour("auto", "de", Some("en_US.UTF-8")));
        assert!(uses_twelve_hour("auto", "auto", Some("en_AU.UTF-8")));
        assert!(uses_twelve_hour("auto", "", Some("ar_EG")));
        assert!(!uses_twelve_hour("auto", "", None));
        assert!(!uses_twelve_hour("auto", "ja", None));
        assert!(uses_twelve_hour("auto", "ko", None));
        assert!(uses_twelve_hour("auto", "zh-TW", None));
        assert!(!uses_twelve_hour("auto", "zh_CN", None));
    }

    #[test]
    fn off_hides_the_header_clock_and_formats_like_auto() {
        assert!(!shown("off"));
        assert!(shown("auto"));
        assert!(shown("12h"));
        assert!(shown("24h"));
        assert!(uses_twelve_hour("off", "en", None));
        assert!(!uses_twelve_hour("off", "de", None));
    }

    #[test]
    fn a_boot_default_year_is_not_a_set_clock() {
        assert!(!is_set(1970));
        assert!(!is_set(2023));
        assert!(is_set(2024));
        assert!(is_set(2026));
    }

    #[test]
    fn formats_match_the_qt_patterns() {
        assert_eq!(format(0, 5, true), "12:05 AM");
        assert_eq!(format(11, 59, true), "11:59 AM");
        assert_eq!(format(12, 0, true), "12:00 PM");
        assert_eq!(format(23, 7, true), "11:07 PM");
        assert_eq!(format(9, 3, false), "09:03");
        assert_eq!(widest_sample(true), "12:59 PM");
    }
}
