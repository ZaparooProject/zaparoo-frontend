// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.LogUpload` — drives the "Upload log" affordance in Settings.
//
// The user kicks the flow with `upload()`; the singleton spawns a worker
// thread that POSTs `frontend.log` as multipart/form-data to
// `https://logs.zaparoo.org/` (mirroring Core's TUI exportlog flow), then
// queues the result back onto the Qt thread.
//
// HTTP via shelled-out curl: the frontend otherwise has no HTTPS client
// (tokio-tungstenite is built without TLS to keep the MiSTer ARM32 binary
// small) and curl is a hard requirement of Core's installer, so it's
// reliably present everywhere this frontend runs.
//
// The QML side observes `state` (idle / uploading / success / error) and
// renders one of three views in `LogUploadModal.qml`. On success the URL
// is plain text plus a QR code generated through `Browse.QrCode`.

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use std::pin::Pin;
use std::process::{Command, Stdio};
use std::thread;
use tracing::{error, info, warn};
use zaparoo_core::platform_paths::log_file_path;

/// Public endpoint used by Core's TUI for the same flow. Hardcoded
/// because it is the only place this frontend uploads anything and the
/// destination is part of the user contract.
const UPLOAD_URL: &str = "https://logs.zaparoo.org/";

/// curl wall-clock cap. Matches Core's TUI (`30 * time.Second`); the
/// log file is small but the upload service occasionally stalls and we
/// don't want to block the modal forever.
const UPLOAD_TIMEOUT_SECS: u32 = 30;

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
        if let Err(e) = thread::Builder::new()
            .name("zaparoo-log-upload".into())
            .spawn(move || {
                let outcome = run_upload();
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

fn run_upload() -> UploadOutcome {
    let log_path = log_file_path();
    if !log_path.exists() {
        return UploadOutcome::Error(format!("Log file not found at {}", log_path.display()));
    }

    let log_arg = format!("file=@{}", log_path.display());
    let timeout_arg = UPLOAD_TIMEOUT_SECS.to_string();
    // `--insecure` skips TLS verification. MiSTer ships with an outdated
    // CA bundle and a clock that's wrong until NTP syncs minutes after
    // boot, both of which make standard verification fail. The frontend
    // is uploading a log to a known endpoint owned by the project — the
    // payload is not sensitive and the destination is hard-coded — so
    // accepting any cert is the right trade-off.
    let output = match Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--insecure",
            "--max-time",
            timeout_arg.as_str(),
            "-F",
            log_arg.as_str(),
            UPLOAD_URL,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            return UploadOutcome::Error(format!("curl failed to start: {e}"));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("curl exited with status {}", output.status)
        } else {
            stderr
        };
        return UploadOutcome::Error(message);
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
