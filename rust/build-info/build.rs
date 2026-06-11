// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Bakes build provenance into the binary as `cargo:rustc-env` values.
//! Deliberately the ONLY build script that watches `.git/` — see
//! `src/lib.rs` for why this lives in a leaf crate.

fn main() {
    println!("cargo:rerun-if-env-changed=ZAPAROO_OFFICIAL_BUILD");
    println!("cargo:rerun-if-env-changed=ZAPAROO_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=ZAPAROO_BUILD_DATE");

    // Rerun when HEAD or any branch ref moves so ZAPAROO_BUILD_COMMIT /
    // ZAPAROO_BUILD_DATE refresh after rebases, branch switches, and
    // commits that don't otherwise touch this crate. Emitting any
    // rerun-if-* directive disables Cargo's "rerun on any package
    // file change" default, which is why these are needed alongside
    // the env-changed lines above.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");

    // Build provenance — baked into the binary and surfaced through the
    // `Browse.BuildInfo` singleton plus the startup log. Goal is
    // "this binary is from this source tree at this date, and it is /
    // is not an official package", not DRM. Failures fall back to
    // "unknown" / "dev"; the build still succeeds.
    //
    // Prefer values supplied via env so cross-builds that don't have
    // `.git/` in their build context (e.g. the ARM32 Docker build,
    // which COPYs only source dirs) can be told the commit and date by
    // the host. Fall back to running `git` / `date` when the env vars
    // are absent or empty, which is the common path for host builds.
    let commit = std::env::var("ZAPAROO_BUILD_COMMIT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ZAPAROO_BUILD_COMMIT={commit}");

    let build_date = std::env::var("ZAPAROO_BUILD_DATE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::process::Command::new("date")
                .args(["-u", "+%Y-%m-%d"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ZAPAROO_BUILD_DATE={build_date}");

    let channel = if std::env::var("ZAPAROO_OFFICIAL_BUILD").is_ok() {
        "official"
    } else {
        "dev"
    };
    println!("cargo:rustc-env=ZAPAROO_BUILD_CHANNEL={channel}");
}
