// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The log uploader behind Settings > About > Upload log file: builds the
// support bundle (a summary plus both log tails), posts it, and shows the
// resulting link as a scannable code. The bundle rules live in
// `zaparoo_app::log_upload`.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::Arc;

use base64::Engine as _;
use slint::{ComponentHandle, SharedString};
use zaparoo_app::log_upload::{self as rules, CoreLog, Phase};
use zaparoo_core::client::Client;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{LaunchEntry, MediaResult, ScrapingStatusResponse, TokenInfo};

use crate::router::{lock, Ctx};
use crate::{App, LogUploadView};

/// Where the bundle goes. The frontend has no HTTPS client of its own,
/// and curl is a hard requirement of Core's installer, so the upload
/// shells out to it.
const UPLOAD_URL: &str = "https://logs.zaparoo.org/";

/// The panel's state.
#[derive(Debug, Clone)]
pub struct LogUploadModel {
    pub open: bool,
    pub phase: Phase,
    pub url: String,
    /// Bumped per run so a late reply cannot land on a newer one.
    pub seq: u64,
}

impl LogUploadModel {
    pub fn new() -> Self {
        Self {
            open: false,
            phase: Phase::Uploading,
            url: String::new(),
            seq: 0,
        }
    }
}

impl Default for LogUploadModel {
    fn default() -> Self {
        Self::new()
    }
}

fn render(ctx: &Ctx, app: &App) {
    let view = app.global::<LogUploadView>();
    let shared = lock(&ctx.shared);
    let model = &shared.log_upload;
    view.set_open(model.open);
    view.set_phase(model.phase.into());
    view.set_url(SharedString::from(model.url.as_str()));
}

/// Open the panel and start a run.
pub fn open(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.log_upload.open = true;
        shared.log_upload.url.clear();
    }
    start(ctx, app);
}

pub fn close(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.log_upload.open = false;
        // Any reply still in flight belongs to a run nobody is watching.
        shared.log_upload.seq += 1;
    }
    app.global::<LogUploadView>().set_qr_modules(0);
    render(ctx, app);
    crate::settings::refresh(ctx, app);
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let phase = lock(&ctx.shared).log_upload.phase;
    match action {
        actions::ACCEPT => match phase {
            Phase::Uploading => {}
            Phase::Done => close(ctx, app),
            Phase::Failed => start(ctx, app),
        },
        actions::CANCEL => close(ctx, app),
        _ => {}
    }
}

/// The identity block at the top of the bundle: what this build is, what
/// it is talking to, and what it is showing.
fn support_summary(ctx: &Ctx, app: &App) -> String {
    use std::fmt::Write as _;
    let shared = lock(&ctx.shared);
    let s = &shared.persist.settings;
    let mut out = String::from("===== zaparoo frontend support summary =====\n");
    // A write to a String cannot fail; the results are discarded.
    let _ = writeln!(out, "frontend: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "runtime: {}",
        if ctx.is_mister { "mister" } else { "desktop" }
    );
    let _ = writeln!(out, "crt path: {}", ctx.crt_enabled);
    let _ = writeln!(
        out,
        "core: {}",
        if shared.core_version.is_empty() {
            "unknown"
        } else {
            shared.core_version.as_str()
        }
    );
    let _ = writeln!(
        out,
        "screen: {} ({}x{})",
        app.global::<crate::Shell>().get_active_screen().token(),
        app.global::<crate::Sizing>().get_screen_width(),
        app.global::<crate::Sizing>().get_screen_height()
    );
    let _ = writeln!(
        out,
        "catalog: {} systems, {} categories",
        shared.systems.len(),
        shared.categories.len()
    );
    let _ = writeln!(
        out,
        "settings: resolution={} orientation={} profile={} language={} region={}",
        s.resolution, s.orientation, s.interface_profile, s.language, s.region
    );
    let _ = writeln!(
        out,
        "settings: scheme={} intensity={} logos={} artwork={}",
        s.color_scheme, s.color_intensity, s.system_logo_style, s.media_image_type
    );
    let _ = writeln!(
        out,
        "settings: systems={} games={} hidden={} filenames={}",
        s.systems_browse_layout, s.games_browse_layout, s.show_hidden, s.show_original_filenames
    );
    out
}

/// The live half of the summary: what Core says about itself right now,
/// asked over the API at upload time. Every probe degrades to an
/// "unavailable" line so a half-reachable Core still yields a bundle.
async fn live_summary(client: &Client) -> String {
    let mut lines = vec!["-- core --".to_string()];
    match client.health().await {
        Ok(result) => push_non_empty(&mut lines, "Core API", &result.status),
        Err(e) => lines.push(format!("Core API: unavailable ({})", e.message)),
    }
    match client.readers().await {
        Ok(result) => lines.push(format!("Readers: {}", result.readers.len())),
        Err(e) => lines.push(format!("Readers: unavailable ({})", e.message)),
    }
    match client.tokens().await {
        Ok(result) => {
            if let Some(token) = result.last.as_ref() {
                lines.push(format!("Last token: {}", token_display(token)));
            }
        }
        Err(e) => lines.push(format!("Last token: unavailable ({})", e.message)),
    }
    match client.tokens_history().await {
        Ok(result) => {
            if let Some(entry) = result.entries.first() {
                lines.push(format!("Last launch: {}", launch_display(entry)));
            }
        }
        Err(e) => lines.push(format!("Last launch: unavailable ({})", e.message)),
    }
    lines.push("-- library --".to_string());
    match client.media().await {
        Ok(result) => lines.extend(media_summary(&result)),
        Err(e) => lines.push(format!("Media: unavailable ({})", e.message)),
    }
    match client.media_scrape_status().await {
        Ok(result) => lines.extend(scrape_summary(&result)),
        Err(e) => lines.push(format!("Scrape status: unavailable ({})", e.message)),
    }
    lines.push(String::new());
    lines.join("\n")
}

fn push_non_empty(lines: &mut Vec<String>, label: &str, value: &str) {
    if !value.is_empty() {
        lines.push(format!("{label}: {value}"));
    }
}

fn push_non_zero(lines: &mut Vec<String>, label: &str, value: i32) {
    if value != 0 {
        lines.push(format!("{label}: {value}"));
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn media_summary(result: &MediaResult) -> Vec<String> {
    let db = &result.database;
    let mut lines = vec![
        format!("Media DB exists: {}", yes_no(db.exists)),
        format!("Indexing: {}", yes_no(db.indexing)),
        format!("Optimizing: {}", yes_no(db.optimizing)),
        format!("Active media: {}", yes_no(!result.active.is_empty())),
    ];
    if let Some(total_files) = db.total_files {
        lines.push(format!("Total files: {total_files}"));
    }
    if let Some(total_media) = db.total_media {
        lines.push(format!("Total media: {total_media}"));
    }
    if let Some(active) = result.active.first() {
        push_non_empty(&mut lines, "Active game", &active.media_name);
        push_non_empty(&mut lines, "Active system", &active.system_name);
        push_non_empty(&mut lines, "Active launcher", &active.launcher_id);
    }
    lines
}

fn scrape_summary(result: &ScrapingStatusResponse) -> Vec<String> {
    let mut lines = vec![
        format!("Scraping: {}", yes_no(result.scraping)),
        format!("Scrape done: {}", yes_no(result.done)),
        format!("Scrape paused: {}", yes_no(result.paused)),
    ];
    push_non_empty(&mut lines, "Scrape state", &result.state);
    push_non_empty(&mut lines, "Scraper", &result.scraper_id);
    push_non_empty(&mut lines, "Scrape system", &result.system_id);
    push_non_zero(&mut lines, "Scrape processed", result.processed);
    push_non_zero(&mut lines, "Scrape total", result.total);
    push_non_zero(&mut lines, "Scrape matched", result.matched);
    push_non_zero(&mut lines, "Scrape skipped", result.skipped);
    push_non_zero(&mut lines, "Total scraped", result.total_scraped);
    push_non_empty(&mut lines, "Scrape error", &result.error);
    lines
}

fn token_display(token: &TokenInfo) -> String {
    if !token.text.is_empty() {
        token.text.clone()
    } else if !token.uid.is_empty() {
        format!("uid:{}", token.uid)
    } else if !token.data.is_empty() {
        format!("data:{}", token.data)
    } else {
        "(empty)".into()
    }
}

fn launch_display(entry: &LaunchEntry) -> String {
    let state = if entry.success { "success" } else { "failed" };
    let text = if entry.text.is_empty() {
        "(empty)"
    } else {
        entry.text.as_str()
    };
    format!("{text} ({state}, {})", entry.time)
}

/// Build the bundle and post it, then land the result on the panel.
fn start(ctx: &Ctx, app: &App) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        shared.log_upload.seq += 1;
        shared.log_upload.phase = Phase::Uploading;
        shared.log_upload.url.clear();
        shared.log_upload.seq
    };
    app.global::<LogUploadView>().set_qr_modules(0);
    render(ctx, app);

    let mut summary = support_summary(ctx, app);
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        summary.push_str(&live_summary(&client).await);
        // Core's own log comes over the API; a frontend that cannot
        // reach Core still uploads its own half.
        let core_log = match client.settings_logs_download().await {
            Ok(result) => {
                match base64::engine::general_purpose::STANDARD.decode(result.content.as_bytes()) {
                    Ok(bytes) => CoreLog::Available(
                        rules::tail_bytes(&bytes, rules::PER_LOG_LIMIT_BYTES).to_vec(),
                    ),
                    Err(e) => CoreLog::Unavailable(format!("core log decode failed: {e}")),
                }
            }
            Err(e) => CoreLog::Unavailable(format!("core log download failed: {e}")),
        };
        let frontend_log =
            std::fs::read(zaparoo_core::platform_paths::log_file_path()).unwrap_or_default();
        let payload = rules::build_payload(summary.as_bytes(), &frontend_log, &core_log);
        let outcome = tokio::task::spawn_blocking(move || post(&payload))
            .await
            .unwrap_or_else(|e| Err(format!("upload task failed: {e}")));
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx2.shared);
                if shared.log_upload.seq != seq {
                    return;
                }
                match &outcome {
                    Ok(url) => {
                        shared.log_upload.phase = Phase::Done;
                        shared.log_upload.url.clone_from(url);
                    }
                    Err(message) => {
                        tracing::warn!("log upload failed: {message}");
                        shared.log_upload.phase = Phase::Failed;
                        shared.log_upload.url.clear();
                    }
                }
            }
            if let Ok(url) = &outcome {
                if let Some((image, modules)) =
                    crate::qr::qr_image(url, crate::qr::code_colors(&app))
                {
                    let view = app.global::<LogUploadView>();
                    view.set_qr(image);
                    view.set_qr_modules(i32::try_from(modules).unwrap_or(0));
                }
            }
            render(&ctx2, &app);
        });
    });
}

/// Post the bundle with curl and return the link it answers with.
fn post(payload: &[u8]) -> Result<String, String> {
    let timeout = rules::UPLOAD_TIMEOUT_SECS.to_string();
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            timeout.as_str(),
            "-F",
            "file=@-;filename=zaparoo.log",
            UPLOAD_URL,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl failed to start: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "curl stdin was not available".to_string())?;
        stdin
            .write_all(payload)
            .map_err(|e| format!("curl upload write failed: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("curl failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("curl exited with status {}", output.status)
        } else {
            stderr
        });
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err("the upload service returned an empty response".to_string());
    }
    Ok(url)
}

pub fn bind_input(ctx: &Arc<Ctx>, app: &App) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.global::<crate::LogUploadInput>().on_confirmed(move || {
        if let Some(app) = weak.upgrade() {
            crate::router::handle_action(&ctx, &app, actions::ACCEPT);
        }
    });
}
