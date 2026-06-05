// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.LogUpload` — drives the "Upload log" affordance in Settings.
//
// The user kicks the flow with `upload()`; the singleton spawns a worker
// thread that builds one capped payload from a support summary,
// `frontend.log`, and Core's API-downloaded log, then POSTs it as
// multipart/form-data to `https://logs.zaparoo.org/` and queues the result
// back onto the Qt thread.
//
// HTTP via shelled-out curl: the frontend otherwise has no HTTPS client
// (tokio-tungstenite is built without TLS to keep the MiSTer ARM32 binary
// small) and curl is a hard requirement of Core's installer, so it's
// reliably present everywhere this frontend runs.
//
// The QML side observes `state` (idle / uploading / success / error) and
// renders one of three views in `LogUploadModal.qml`. On success the URL
// is plain text plus a QR code generated through `Browse.QrCode`.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::thread;
use tracing::{error, info, warn};
use zaparoo_core::client::Client;
use zaparoo_core::media_types::{LaunchEntry, MediaResult, ScrapingStatusResponse, TokenInfo};
use zaparoo_core::platform_paths::log_file_path;

use crate::models::{build_info, global_handle, global_store, system_status};

/// Public endpoint used by Core's TUI for the same flow. Hardcoded
/// because it is the only place this frontend uploads anything and the
/// destination is part of the user contract.
const UPLOAD_URL: &str = "https://logs.zaparoo.org/";

/// curl wall-clock cap. Matches Core's TUI (`30 * time.Second`); the
/// log file is small but the upload service occasionally stalls and we
/// don't want to block the modal forever.
const UPLOAD_TIMEOUT_SECS: u32 = 30;

const SUPPORT_SUMMARY_LIMIT_BYTES: usize = 64 * 1024;
const PER_LOG_LIMIT_BYTES: usize = 256 * 1024;
const UPLOAD_LIMIT_BYTES: usize = 768 * 1024;
const UPLOAD_HEADROOM_BYTES: usize = 4 * 1024;
const PAYLOAD_LIMIT_BYTES: usize = UPLOAD_LIMIT_BYTES - UPLOAD_HEADROOM_BYTES;

/// State machine values exposed to QML. Kept as integers because
/// cxx-qt 0.8 does not surface Rust enums to QML cleanly; the QML side
/// has matching readonly properties on the modal.
const STATE_IDLE: i32 = 0;
const STATE_UPLOADING: i32 = 1;
const STATE_SUCCESS: i32 = 2;
const STATE_ERROR: i32 = 3;

#[derive(Default)]
pub struct LogUploadRust {
    state: i32,
    url: QString,
    error_message: QString,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(i32, state)]
        #[qproperty(QString, url)]
        #[qproperty(QString, error_message)]
        type LogUpload = super::LogUploadRust;

        #[qinvokable]
        fn upload(self: Pin<&mut LogUpload>);

        #[qinvokable]
        fn reset(self: Pin<&mut LogUpload>);
    }

    impl cxx_qt::Threading for LogUpload {}
}

impl ffi::LogUpload {
    fn upload(mut self: Pin<&mut Self>) {
        if self.state == STATE_UPLOADING {
            // Already in flight — debounce repeated Accept presses while
            // the modal is still painting "Uploading…".
            return;
        }
        self.as_mut().set_state(STATE_UPLOADING);
        self.as_mut().set_url(QString::default());
        self.as_mut().set_error_message(QString::default());

        let qt_thread = self.qt_thread();
        let runtime = global_handle();
        let client = global_store().client();
        if let Err(e) = thread::Builder::new()
            .name("zaparoo-log-upload".into())
            .spawn(move || {
                let outcome = run_upload(&runtime, &client);
                let _ = qt_thread.queue(move |model| apply_outcome(model, outcome));
            })
        {
            error!("failed to spawn log upload thread: {e}");
            self.as_mut()
                .set_error_message(QString::from("Could not start upload."));
            self.as_mut().set_state(STATE_ERROR);
        }
    }

    fn reset(mut self: Pin<&mut Self>) {
        self.as_mut().set_state(STATE_IDLE);
        self.as_mut().set_url(QString::default());
        self.as_mut().set_error_message(QString::default());
    }
}

#[derive(Debug)]
enum UploadOutcome {
    Success(String),
    Error(String),
}

fn apply_outcome(mut model: Pin<&mut ffi::LogUpload>, outcome: UploadOutcome) {
    match outcome {
        UploadOutcome::Success(url) => {
            info!("log upload succeeded: {url}");
            model.as_mut().set_url(QString::from(url.as_str()));
            model.as_mut().set_state(STATE_SUCCESS);
        }
        UploadOutcome::Error(message) => {
            warn!("log upload failed: {message}");
            model
                .as_mut()
                .set_error_message(QString::from(message.as_str()));
            model.as_mut().set_state(STATE_ERROR);
        }
    }
}

fn run_upload(runtime: &tokio::runtime::Handle, client: &Client) -> UploadOutcome {
    let log_path = log_file_path();
    let frontend_log = match read_file_tail(&log_path, PER_LOG_LIMIT_BYTES) {
        Ok(bytes) => bytes,
        Err(message) => return UploadOutcome::Error(message),
    };

    let (support_summary, core_log) = runtime.block_on(async {
        let summary = gather_support_summary(client).await;
        let core_log = match client.settings_logs_download().await {
            Ok(result) => {
                match decode_base64_tail(result.content.as_bytes(), PER_LOG_LIMIT_BYTES) {
                    Ok(bytes) => CoreLogSource::Available(bytes),
                    Err(e) => {
                        let message = format!("core log decode failed: {e}");
                        warn!("{message}");
                        CoreLogSource::Unavailable(message)
                    }
                }
            }
            Err(e) => {
                let message = format!("core log download failed: {e}");
                warn!("{message}");
                CoreLogSource::Unavailable(message)
            }
        };
        (summary, core_log)
    });

    upload_payload(&build_upload_payload(
        support_summary.as_bytes(),
        &frontend_log,
        core_log,
    ))
}

#[derive(Debug)]
enum CoreLogSource {
    Available(Vec<u8>),
    Unavailable(String),
}

fn read_file_tail(path: &std::path::Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut file =
        File::open(path).map_err(|e| format!("Log file not found at {}: {e}", path.display()))?;
    let len = file
        .metadata()
        .map_err(|e| format!("Could not inspect log file at {}: {e}", path.display()))?
        .len();
    let start = len.saturating_sub(max_bytes as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("Could not read log file at {}: {e}", path.display()))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read log file at {}: {e}", path.display()))?;
    Ok(bytes)
}

fn tail_bytes(bytes: &[u8], max_bytes: usize) -> &[u8] {
    if bytes.len() <= max_bytes {
        bytes
    } else {
        &bytes[bytes.len() - max_bytes..]
    }
}

fn decode_base64_tail(encoded: &[u8], max_bytes: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut reader = base64::read::DecoderReader::new(encoded, &BASE64_STANDARD);
    let mut tail = Vec::with_capacity(max_bytes.min(8 * 1024));
    let mut chunk = [0_u8; 8 * 1024];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..read]);
        if tail.len() > max_bytes {
            let excess = tail.len() - max_bytes;
            tail.drain(..excess);
        }
    }

    Ok(tail)
}

async fn gather_support_summary(client: &Client) -> String {
    let mut lines = vec!["===== support summary =====".to_string()];

    lines.push(format!(
        "Frontend: {}",
        build_info::provenance_string(env!("CARGO_PKG_VERSION"))
    ));

    match client.health().await {
        Ok(result) => push_non_empty(&mut lines, "Core API", &result.status),
        Err(e) => lines.push(format!("Core API: unavailable ({})", e.message)),
    }
    match client.version().await {
        Ok(result) => {
            push_non_empty(&mut lines, "Core version", &result.version);
            push_non_empty(&mut lines, "Core platform", &result.platform);
        }
        Err(e) => lines.push(format!("Core version: unavailable ({})", e.message)),
    }

    lines.push("-- system --".into());
    lines.extend(system_status::support_summary_lines());

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

    lines.push("-- library --".into());
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

fn build_upload_payload(
    support_summary: &[u8],
    frontend_log: &[u8],
    core_log: CoreLogSource,
) -> Vec<u8> {
    let mut payload = Vec::with_capacity(PAYLOAD_LIMIT_BYTES.min(frontend_log.len() * 2));
    payload.extend_from_slice(tail_bytes(support_summary, SUPPORT_SUMMARY_LIMIT_BYTES));
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    payload.extend_from_slice(b"===== frontend.log (tail) =====\n");
    let remaining = PAYLOAD_LIMIT_BYTES.saturating_sub(payload.len());
    let frontend_limit = PER_LOG_LIMIT_BYTES.min(remaining);
    payload.extend_from_slice(tail_bytes(frontend_log, frontend_limit));
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    payload.extend_from_slice(b"\n===== core.log (tail) =====\n");

    match core_log {
        CoreLogSource::Available(bytes) => {
            let remaining = PAYLOAD_LIMIT_BYTES.saturating_sub(payload.len());
            let core_limit = PER_LOG_LIMIT_BYTES.min(remaining);
            payload.extend_from_slice(tail_bytes(&bytes, core_limit));
        }
        CoreLogSource::Unavailable(message) => {
            payload.extend_from_slice(format!("core.log unavailable: {message}\n").as_bytes());
        }
    }

    payload.truncate(PAYLOAD_LIMIT_BYTES);
    payload
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
    format!("{text} ({state})")
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn upload_payload(payload: &[u8]) -> UploadOutcome {
    let payload_len = payload.len();
    info!(payload_len, "log upload payload prepared");
    let timeout_arg = UPLOAD_TIMEOUT_SECS.to_string();
    // MiSTer commonly has an outdated CA bundle and incorrect clock before NTP sync.
    // Keep support log upload usable there.
    let mut child = match Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--insecure",
            "--max-time",
            timeout_arg.as_str(),
            "-F",
            "file=@-;filename=zaparoo.log",
            UPLOAD_URL,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return UploadOutcome::Error(format!("curl failed to start: {e}")),
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(payload) {
            drop(stdin);
            reap_child_after_stdin_failure(&mut child);
            return UploadOutcome::Error(format!("curl upload write failed: {e}"));
        }
    } else {
        reap_child_after_stdin_failure(&mut child);
        return UploadOutcome::Error("curl stdin was not available".into());
    }

    let output = match child.wait_with_output() {
        Ok(out) => out,
        Err(e) => return UploadOutcome::Error(format!("curl failed: {e}")),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("curl exited with status {}", output.status)
        } else {
            stderr
        };
        warn!(payload_len, "log upload curl failed: {message}");
        return UploadOutcome::Error(format!("{message} (payload {payload_len} bytes)"));
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return UploadOutcome::Error("Upload service returned an empty response.".into());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return UploadOutcome::Error(format!("Unexpected upload response: {url}"));
    }
    UploadOutcome::Success(url)
}

fn reap_child_after_stdin_failure(child: &mut Child) {
    if let Err(e) = child.kill() {
        warn!("failed to stop curl after stdin failure: {e}");
    }
    if let Err(e) = child.wait() {
        warn!("failed to reap curl after stdin failure: {e}");
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        build_upload_payload, decode_base64_tail, read_file_tail, CoreLogSource,
        PAYLOAD_LIMIT_BYTES, PER_LOG_LIMIT_BYTES, SUPPORT_SUMMARY_LIMIT_BYTES,
    };
    use base64::Engine as _;
    use std::io::Write;

    #[test]
    fn read_file_tail_returns_full_small_file() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(b"short log").expect("write log");

        let bytes = read_file_tail(file.path(), 512).expect("read tail");

        assert_eq!(bytes, b"short log");
    }

    #[test]
    fn read_file_tail_returns_last_bytes_for_large_file() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(b"0123456789").expect("write log");

        let bytes = read_file_tail(file.path(), 4).expect("read tail");

        assert_eq!(bytes, b"6789");
    }

    #[test]
    fn payload_places_summary_frontend_core_in_order() {
        let payload = build_upload_payload(
            b"summary",
            b"front",
            CoreLogSource::Available(b"core".to_vec()),
        );
        let text = String::from_utf8(payload).expect("utf8 payload");

        let summary_pos = text.find("summary").expect("summary");
        let frontend_pos = text.find("front").expect("frontend log");
        let core_pos = text.find("core").expect("core log");
        assert!(summary_pos < frontend_pos);
        assert!(frontend_pos < core_pos);
    }

    #[test]
    fn payload_stays_below_limit_with_full_logs() {
        let frontend = vec![b'f'; PER_LOG_LIMIT_BYTES];
        let core = vec![b'c'; PER_LOG_LIMIT_BYTES];

        let summary = vec![b's'; SUPPORT_SUMMARY_LIMIT_BYTES];

        let payload = build_upload_payload(&summary, &frontend, CoreLogSource::Available(core));

        assert!(payload.len() <= PAYLOAD_LIMIT_BYTES);
    }

    #[test]
    fn payload_marks_unavailable_core_log() {
        let payload = build_upload_payload(
            b"summary",
            b"front",
            CoreLogSource::Unavailable("not connected".into()),
        );
        let text = String::from_utf8(payload).expect("utf8 payload");

        assert!(text.contains("front"));
        assert!(text.contains("core.log unavailable: not connected"));
    }

    #[test]
    fn decode_base64_tail_keeps_only_capped_decoded_tail() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"0123456789");

        let bytes = decode_base64_tail(encoded.as_bytes(), 4).expect("decode tail");

        assert_eq!(bytes, b"6789");
    }
}
