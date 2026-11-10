// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

fn main() -> Result<(), slint::PlatformError> {
    // Answered before anything starts, so asking never opens a second
    // frontend next to a running one.
    if std::env::args().skip(1).any(|a| a == "--version") {
        use std::io::Write as _;
        // A closed stdout leaves nothing to report to.
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            version_line(
                env!("CARGO_PKG_VERSION"),
                zaparoo_build_info::COMMIT,
                zaparoo_build_info::BUILD_DATE,
                zaparoo_build_info::CHANNEL,
            )
        );
        return Ok(());
    }
    frontend::run()
}

/// What `--version` prints.
fn version_line(version: &str, commit: &str, date: &str, channel: &str) -> String {
    format!("zaparoo frontend {version} ({commit}, {date}, {channel})")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_line_names_the_build() {
        assert_eq!(
            super::version_line("2.2.0", "f2e2b57", "2026-11-10", "release"),
            "zaparoo frontend 2.2.0 (f2e2b57, 2026-11-10, release)"
        );
    }
}
