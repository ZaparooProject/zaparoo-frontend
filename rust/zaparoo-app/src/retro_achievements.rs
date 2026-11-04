// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! `RetroAchievements` on `MiSTer`, through odelot's `RetroAchievements` Main
//! (`MiSTer_RA`): reading and editing its `retroachievements.cfg`, telling
//! whether MiSTer.ini hands the `RetroAchievements` cores to it, and the
//! `RetroAchievements` login check.
//!
//! The cfg is parsed the way `MiSTer_RA` parses it (`ra_load_credentials`
//! in its `achievements.cpp`): one `key=value` per line, a `#` in the first
//! column or an empty line skipped, the key compared case-insensitively
//! with nothing trimmed from it, only leading spaces and tabs trimmed from
//! the value, integers read the way `atoi` reads them, and the last copy of
//! a key winning. It reads the file once each time a `RetroAchievements` core
//! starts.

/// Longest username or password `MiSTer_RA` keeps.
pub const FIELD_MAX_CHARS: usize = 127;

/// Where achievement popups appear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupPosition {
    #[default]
    Left,
    Center,
    Right,
}

impl PopupPosition {
    /// `left`, `center` or `centre`, `right`, or `0`/`1`/`2`, with trailing
    /// spaces ignored; anything else is Left, as on `MiSTer_RA`.
    pub fn parse(value: &str) -> Self {
        let v = value.trim_end_matches([' ', '\t']);
        if v.eq_ignore_ascii_case("center") || v.eq_ignore_ascii_case("centre") || v == "1" {
            Self::Center
        } else if v.eq_ignore_ascii_case("right") || v == "2" {
            Self::Right
        } else {
            Self::Left
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// The settings the frontend shows, with `MiSTer_RA`'s own defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent on/off keys of MiSTer_RA's cfg"
)]
pub struct RaConfig {
    pub username: String,
    pub password: String,
    pub hardcore: bool,
    pub progress_popups: bool,
    pub challenge_popups: bool,
    pub leaderboard_updates: bool,
    pub leaderboard_submission: bool,
    pub popup_position: PopupPosition,
    pub list_hotkey: bool,
}

impl Default for RaConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            hardcore: false,
            progress_popups: true,
            challenge_popups: true,
            leaderboard_updates: true,
            leaderboard_submission: true,
            popup_position: PopupPosition::Left,
            list_hotkey: false,
        }
    }
}

impl RaConfig {
    /// Both halves of a login are there; `MiSTer_RA` stays off otherwise.
    pub fn logged_in(&self) -> bool {
        !self.username.is_empty() && !self.password.is_empty()
    }
}

// Canonical key names, and the other spellings `MiSTer_RA` accepts for the
// same setting.
pub const USERNAME: &str = "username";
pub const PASSWORD: &str = "password";
pub const HARDCORE: &str = "hardcore";
pub const PROGRESS_POPUPS: &str = "show_progress_popups";
pub const CHALLENGE_POPUPS: &str = "show_challenge_show_popup";
pub const LEADERBOARD_UPDATES: &str = "show_leaderboards_updates";
pub const LEADERBOARD_SUBMISSION: &str = "show_leaderboards_submission";
pub const POPUP_POSITION: &str = "popup_position";
pub const LIST_HOTKEY: &str = "list_hotkey";

fn aliases(key: &str) -> &'static [&'static str] {
    match key {
        LEADERBOARD_UPDATES => &[LEADERBOARD_UPDATES, "show-leaderboards-updates"],
        LEADERBOARD_SUBMISSION => &[LEADERBOARD_SUBMISSION, "show-leaderboards-submission"],
        USERNAME => &[USERNAME],
        PASSWORD => &[PASSWORD],
        HARDCORE => &[HARDCORE],
        PROGRESS_POPUPS => &[PROGRESS_POPUPS],
        CHALLENGE_POPUPS => &[CHALLENGE_POPUPS],
        POPUP_POSITION => &[POPUP_POSITION],
        LIST_HOTKEY => &[LIST_HOTKEY],
        _ => &[],
    }
}

/// The key and value of a setting line, or `None` for a comment, a blank
/// line or a line without `=`. `line` has no line ending.
fn split_line(line: &[u8]) -> Option<(&[u8], &[u8])> {
    if line.first().is_none_or(|b| *b == b'#') {
        return None;
    }
    let eq = line.iter().position(|b| *b == b'=')?;
    let value = &line[eq + 1..];
    let start = value
        .iter()
        .position(|b| *b != b' ' && *b != b'\t')
        .unwrap_or(value.len());
    Some((&line[..eq], &value[start..]))
}

/// A line's content as `MiSTer_RA` sees it: up to the first `\n`, then cut
/// at the first `\r`.
fn content(line: &[u8]) -> &[u8] {
    let line = line.split(|b| *b == b'\n').next().unwrap_or(line);
    line.split(|b| *b == b'\r').next().unwrap_or(line)
}

fn is_key(key: &[u8], canonical: &str) -> bool {
    aliases(canonical)
        .iter()
        .any(|alias| key.eq_ignore_ascii_case(alias.as_bytes()))
}

/// `atoi`: optional leading whitespace and sign, then digits.
fn atoi(value: &[u8]) -> i64 {
    let text = String::from_utf8_lossy(value);
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let end = digits
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len());
    let n: i64 = digits[..end].parse().unwrap_or(0);
    if negative {
        -n
    } else {
        n
    }
}

fn text(value: &[u8]) -> String {
    let value = String::from_utf8_lossy(value);
    value.chars().take(FIELD_MAX_CHARS).collect()
}

/// Lines of the file, each with its own line ending.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split_inclusive(|b| *b == b'\n')
}

pub fn parse(bytes: &[u8]) -> RaConfig {
    let mut cfg = RaConfig::default();
    for line in lines(bytes) {
        let Some((key, value)) = split_line(content(line)) else {
            continue;
        };
        let on = || atoi(value) != 0;
        if is_key(key, USERNAME) {
            cfg.username = text(value);
        } else if is_key(key, PASSWORD) {
            cfg.password = text(value);
        } else if is_key(key, HARDCORE) {
            cfg.hardcore = on();
        } else if is_key(key, PROGRESS_POPUPS) {
            cfg.progress_popups = on();
        } else if is_key(key, CHALLENGE_POPUPS) {
            cfg.challenge_popups = on();
        } else if is_key(key, LEADERBOARD_UPDATES) {
            cfg.leaderboard_updates = on();
        } else if is_key(key, LEADERBOARD_SUBMISSION) {
            cfg.leaderboard_submission = on();
        } else if is_key(key, POPUP_POSITION) {
            cfg.popup_position = PopupPosition::parse(&String::from_utf8_lossy(value));
        } else if is_key(key, LIST_HOTKEY) {
            cfg.list_hotkey = on();
        }
    }
    cfg
}

/// One change to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Write this key with this value.
    Set(&'static str, String),
    /// Delete every line for this key.
    Remove(&'static str),
}

impl Edit {
    fn key(&self) -> &'static str {
        match self {
            Self::Set(key, _) | Self::Remove(key) => key,
        }
    }
}

/// The file with `edits` applied. Every line no edit names is kept byte for
/// byte, comments and unknown keys included. A set key's lines are
/// rewritten in place, keeping how the key was spelled and the line's
/// ending; one that is not there yet is appended in the file's own line
/// ending.
pub fn rewrite(original: &[u8], edits: &[Edit]) -> Vec<u8> {
    let crlf = original.windows(2).any(|w| w == b"\r\n");
    let newline: &[u8] = if crlf { b"\r\n" } else { b"\n" };
    let mut written = vec![false; edits.len()];
    let mut out = Vec::with_capacity(original.len() + 64);
    for line in lines(original) {
        let edit = split_line(content(line)).and_then(|(key, _)| {
            edits
                .iter()
                .position(|edit| is_key(key, edit.key()))
                .map(|i| (i, key))
        });
        let Some((i, key)) = edit else {
            out.extend_from_slice(line);
            continue;
        };
        match &edits[i] {
            Edit::Remove(_) => {}
            Edit::Set(_, value) => {
                written[i] = true;
                out.extend_from_slice(key);
                out.push(b'=');
                out.extend_from_slice(value.as_bytes());
                out.extend_from_slice(&line[content(line).len()..]);
            }
        }
    }
    for (edit, done) in edits.iter().zip(written) {
        if let (Edit::Set(key, value), false) = (edit, done) {
            if !out.is_empty() && !out.ends_with(b"\n") {
                out.extend_from_slice(newline);
            }
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(newline);
        }
    }
    out
}

fn flag(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

/// The Settings rows for the cfg's options, and what each one writes.
pub fn option_edits(id: &str, on: bool) -> Vec<Edit> {
    match id {
        "raHardcore" => vec![Edit::Set(HARDCORE, flag(on))],
        "raProgressPopups" => vec![Edit::Set(PROGRESS_POPUPS, flag(on))],
        "raChallengePopups" => vec![Edit::Set(CHALLENGE_POPUPS, flag(on))],
        "raLeaderboardPopups" => vec![
            Edit::Set(LEADERBOARD_UPDATES, flag(on)),
            Edit::Set(LEADERBOARD_SUBMISSION, flag(on)),
        ],
        "raListHotkey" => vec![Edit::Set(LIST_HOTKEY, flag(on))],
        _ => Vec::new(),
    }
}

/// Whether an option row reads as on. The leaderboard row covers two keys
/// and reads on only when both are.
pub fn option_checked(cfg: &RaConfig, id: &str) -> bool {
    match id {
        "raHardcore" => cfg.hardcore,
        "raProgressPopups" => cfg.progress_popups,
        "raChallengePopups" => cfg.challenge_popups,
        "raLeaderboardPopups" => cfg.leaderboard_updates && cfg.leaderboard_submission,
        "raListHotkey" => cfg.list_hotkey,
        _ => false,
    }
}

/// Apply an edit to the parsed config the way writing it would.
pub fn apply(cfg: &mut RaConfig, edits: &[Edit]) {
    *cfg = parse(&rewrite(&render(cfg), edits));
}

fn render(cfg: &RaConfig) -> Vec<u8> {
    let edits = [
        Edit::Set(USERNAME, cfg.username.clone()),
        Edit::Set(PASSWORD, cfg.password.clone()),
        Edit::Set(HARDCORE, flag(cfg.hardcore)),
        Edit::Set(PROGRESS_POPUPS, flag(cfg.progress_popups)),
        Edit::Set(CHALLENGE_POPUPS, flag(cfg.challenge_popups)),
        Edit::Set(LEADERBOARD_UPDATES, flag(cfg.leaderboard_updates)),
        Edit::Set(LEADERBOARD_SUBMISSION, flag(cfg.leaderboard_submission)),
        Edit::Set(POPUP_POSITION, cfg.popup_position.token().to_string()),
        Edit::Set(LIST_HOTKEY, flag(cfg.list_hotkey)),
    ];
    rewrite(b"", &edits)
}

/// A username or password `MiSTer_RA` can read back as typed: short
/// enough, on one line, and not starting with a space it would trim.
pub fn value_supported(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= FIELD_MAX_CHARS
        && !value.contains(['\r', '\n'])
        && !value.starts_with([' ', '\t'])
}

/// MiSTer.ini sends the `RetroAchievements` cores to `MiSTer_RA`: a section
/// for `RA_` core names (`[RA_*]`, or any `RA_` name, alone or in a `+`
/// list) with `main=MiSTer_RA`. Comments start with `;` or `#`.
pub fn mister_ini_has_ra(ini: &str) -> bool {
    let mut in_ra = false;
    for raw in ini.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            let name = section.split(']').next().unwrap_or("");
            in_ra = name.split('+').any(|part| {
                let part = part.trim();
                part.len() >= 3 && part[..3].eq_ignore_ascii_case("ra_")
            });
            continue;
        }
        if !in_ra {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("main") {
            continue;
        }
        let value = value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        if value.eq_ignore_ascii_case("MiSTer_RA") {
            return true;
        }
    }
    false
}

/// `RetroAchievements` is ready to play: the Main binary, a core for it, and
/// the MiSTer.ini section that switches to it.
pub fn installed(ra_main: bool, ini_ra: bool, launcher: bool) -> bool {
    ra_main && ini_ra && launcher
}

/// The form body of `RetroAchievements`' `login2` request.
pub fn login_form(username: &str, password: &str) -> String {
    format!(
        "r=login2&u={}&p={}",
        form_encode(username),
        form_encode(password)
    )
}

fn form_encode(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// What `RetroAchievements` answered a login with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Logged in; the username as `RetroAchievements` spells it.
    LoggedIn(String),
    WrongCredentials,
    /// Refused for some other reason (an expired or banned account).
    Refused,
    /// No answer, or one that was not a login result.
    Unreachable,
}

/// Read a `login2` answer's `Success`, `User` and `Code` fields.
pub fn login_outcome(
    success: Option<bool>,
    user: Option<&str>,
    code: Option<&str>,
) -> LoginOutcome {
    match success {
        Some(true) => match user.filter(|u| !u.is_empty()) {
            Some(user) => LoginOutcome::LoggedIn(user.to_string()),
            None => LoginOutcome::Unreachable,
        },
        Some(false) if code == Some("invalid_credentials") => LoginOutcome::WrongCredentials,
        Some(false) => LoginOutcome::Refused,
        None => LoginOutcome::Unreachable,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a test fails fast on bad bytes")]
mod tests {
    use super::*;

    const SAMPLE: &str = "# RetroAchievements\nusername=odelot\npassword=\n\n# popups\nshow_challenge_hide_popup=0\nsmart_cache=-1\nhardcore=0\n";

    #[test]
    fn parse_reads_like_mister_ra() {
        let cfg = parse(SAMPLE.as_bytes());
        assert_eq!(cfg.username, "odelot");
        assert_eq!(cfg.password, "");
        assert!(!cfg.logged_in());
        assert!(cfg.progress_popups, "an absent key keeps its default");

        let cfg = parse(b"USERNAME=  Me\npassword=\tp w \nHardcore=1x\npopup_position=Centre  \n");
        assert_eq!(cfg.username, "Me");
        assert_eq!(cfg.password, "p w ", "only leading spaces are trimmed");
        assert!(cfg.hardcore);
        assert_eq!(cfg.popup_position, PopupPosition::Center);
        assert!(cfg.logged_in());

        // Spaces around the key make it another key, as on MiSTer_RA.
        let cfg = parse(b"username = me\n #password=x\npassword=x\npassword=y\n");
        assert_eq!(cfg.username, "");
        assert_eq!(cfg.password, "y", "the last copy wins");

        let cfg = parse(b"show-leaderboards-updates=0\r\nshow_leaderboards_submission=1\r\n");
        assert!(!cfg.leaderboard_updates);
        assert!(cfg.leaderboard_submission);
        assert!(!option_checked(&cfg, "raLeaderboardPopups"));
    }

    #[test]
    fn popup_positions_fall_back_to_left() {
        assert_eq!(PopupPosition::parse("right"), PopupPosition::Right);
        assert_eq!(PopupPosition::parse("2"), PopupPosition::Right);
        assert_eq!(PopupPosition::parse("1"), PopupPosition::Center);
        assert_eq!(PopupPosition::parse("middle"), PopupPosition::Left);
        assert_eq!(PopupPosition::Center.token(), "center");
    }

    #[test]
    fn rewrite_changes_only_the_lines_it_owns() {
        let out = rewrite(
            SAMPLE.as_bytes(),
            &[
                Edit::Set(USERNAME, "Callan".into()),
                Edit::Set(PASSWORD, "s3cret".into()),
                Edit::Set(LIST_HOTKEY, "1".into()),
            ],
        );
        assert_eq!(
            String::from_utf8(out.clone()).unwrap(),
            "# RetroAchievements\nusername=Callan\npassword=s3cret\n\n# popups\nshow_challenge_hide_popup=0\nsmart_cache=-1\nhardcore=0\nlist_hotkey=1\n"
        );
        let cfg = parse(&out);
        assert_eq!(cfg.username, "Callan");
        assert!(cfg.list_hotkey);

        let out = rewrite(&out, &[Edit::Remove(USERNAME), Edit::Remove(PASSWORD)]);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("username") && !text.contains("password="));
        assert!(text.contains("# RetroAchievements\n\n# popups"));
    }

    #[test]
    fn rewrite_keeps_key_spelling_line_endings_and_other_bytes() {
        let original =
            b"Hardcore=0\r\nusername_extra=1\r\n\xff\xfe=raw\r\nshow-leaderboards-updates=1";
        let out = rewrite(
            original,
            &[
                Edit::Set(HARDCORE, "1".into()),
                Edit::Set(LEADERBOARD_UPDATES, "0".into()),
                Edit::Set(POPUP_POSITION, "right".into()),
            ],
        );
        assert_eq!(
            out,
            b"Hardcore=1\r\nusername_extra=1\r\n\xff\xfe=raw\r\nshow-leaderboards-updates=0\r\npopup_position=right\r\n"
        );
        assert_eq!(rewrite(b"", &[Edit::Remove(USERNAME)]), b"");
        assert_eq!(
            rewrite(b"", &[Edit::Set(HARDCORE, "1".into())]),
            b"hardcore=1\n"
        );
        // Every copy of a key changes, so no older copy can win.
        assert_eq!(
            rewrite(
                b"hardcore=0\nhardcore=0\n",
                &[Edit::Set(HARDCORE, "1".into())]
            ),
            b"hardcore=1\nhardcore=1\n"
        );
    }

    #[test]
    fn options_map_to_their_keys() {
        assert_eq!(
            option_edits("raLeaderboardPopups", false),
            vec![
                Edit::Set(LEADERBOARD_UPDATES, "0".into()),
                Edit::Set(LEADERBOARD_SUBMISSION, "0".into())
            ]
        );
        assert!(option_edits("raAccount", true).is_empty());
        let mut cfg = RaConfig::default();
        apply(&mut cfg, &option_edits("raHardcore", true));
        assert!(option_checked(&cfg, "raHardcore"));
        apply(&mut cfg, &[Edit::Set(POPUP_POSITION, "right".into())]);
        assert_eq!(cfg.popup_position, PopupPosition::Right);
    }

    #[test]
    fn unusable_values_are_refused() {
        assert!(value_supported("hunter2"));
        assert!(value_supported("a b"));
        assert!(!value_supported(""));
        assert!(!value_supported(" lead"));
        assert!(!value_supported("two\nlines"));
        assert!(!value_supported(&"x".repeat(FIELD_MAX_CHARS + 1)));
    }

    #[test]
    fn mister_ini_section_is_found_only_where_it_switches_main() {
        assert!(mister_ini_has_ra(
            "[MiSTer]\nvideo_mode=8\n\n[RA_*]\nmain=MiSTer_RA\n"
        ));
        assert!(mister_ini_has_ra(
            "[ra_*]\nMAIN = mister_ra ; RetroAchievements\n"
        ));
        assert!(mister_ini_has_ra("[SNES+RA_NES]\nmain=\"MiSTer_RA\"\n"));
        assert!(!mister_ini_has_ra("[MiSTer]\nmain=MiSTer_RA\n"));
        assert!(!mister_ini_has_ra("[RA_*]\n;main=MiSTer_RA\n"));
        assert!(!mister_ini_has_ra("[RA_*]\nmain=MiSTer\n"));
        assert!(!mister_ini_has_ra(""));
        assert!(installed(true, true, true));
        assert!(!installed(true, false, true));
    }

    #[test]
    fn login_form_encodes_every_reserved_byte() {
        assert_eq!(
            login_form("Me_1", "a b&c=d+e%f/é"),
            "r=login2&u=Me_1&p=a%20b%26c%3Dd%2Be%25f%2F%C3%A9"
        );
    }

    #[test]
    fn login_answers_map_to_outcomes() {
        assert_eq!(
            login_outcome(Some(true), Some("Callan"), None),
            LoginOutcome::LoggedIn("Callan".into())
        );
        assert_eq!(
            login_outcome(Some(true), None, None),
            LoginOutcome::Unreachable
        );
        assert_eq!(
            login_outcome(Some(false), None, Some("invalid_credentials")),
            LoginOutcome::WrongCredentials
        );
        assert_eq!(
            login_outcome(Some(false), None, Some("access_denied")),
            LoginOutcome::Refused
        );
        assert_eq!(login_outcome(None, None, None), LoginOutcome::Unreachable);
    }
}
