// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The support bundle the log uploader sends: how much of each log it
//! keeps, the order the sections go in, and the overall cap. Ported from
//! `models/log_upload.rs`.

/// The support summary is trimmed to this before the logs are added.
pub const SUPPORT_SUMMARY_LIMIT_BYTES: usize = 64 * 1024;
/// Each log contributes at most this much of its tail.
pub const PER_LOG_LIMIT_BYTES: usize = 256 * 1024;
/// What the upload service accepts, less the multipart headroom.
pub const UPLOAD_LIMIT_BYTES: usize = 768 * 1024;
const UPLOAD_HEADROOM_BYTES: usize = 4 * 1024;
pub const PAYLOAD_LIMIT_BYTES: usize = UPLOAD_LIMIT_BYTES - UPLOAD_HEADROOM_BYTES;
/// Wall-clock cap on the upload itself, matching Core's own TUI.
pub const UPLOAD_TIMEOUT_SECS: u32 = 30;

/// The last `max_bytes` of a buffer; the whole thing when it is shorter.
/// A log's tail is the part that explains what just went wrong.
pub fn tail_bytes(bytes: &[u8], max_bytes: usize) -> &[u8] {
    if bytes.len() <= max_bytes {
        bytes
    } else {
        &bytes[bytes.len() - max_bytes..]
    }
}

/// Core's own log, when it could be fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreLog {
    Available(Vec<u8>),
    /// Why it is missing; the reason goes in the bundle rather than
    /// leaving a reader to guess.
    Unavailable(String),
}

/// Build the bundle: the summary, the frontend log's tail, then Core's,
/// each capped, and the whole thing truncated to the service's limit.
pub fn build_payload(support_summary: &[u8], frontend_log: &[u8], core_log: &CoreLog) -> Vec<u8> {
    let mut payload = Vec::with_capacity(PAYLOAD_LIMIT_BYTES.min(frontend_log.len().max(1) * 2));
    payload.extend_from_slice(tail_bytes(support_summary, SUPPORT_SUMMARY_LIMIT_BYTES));
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    payload.extend_from_slice(b"===== frontend.log (tail) =====\n");
    let remaining = PAYLOAD_LIMIT_BYTES.saturating_sub(payload.len());
    payload.extend_from_slice(tail_bytes(frontend_log, PER_LOG_LIMIT_BYTES.min(remaining)));
    if !payload.ends_with(b"\n") {
        payload.push(b'\n');
    }
    payload.extend_from_slice(b"\n===== core.log (tail) =====\n");
    match core_log {
        CoreLog::Available(bytes) => {
            let remaining = PAYLOAD_LIMIT_BYTES.saturating_sub(payload.len());
            payload.extend_from_slice(tail_bytes(bytes, PER_LOG_LIMIT_BYTES.min(remaining)));
        }
        CoreLog::Unavailable(message) => {
            payload.extend_from_slice(format!("core.log unavailable: {message}\n").as_bytes());
        }
    }
    payload.truncate(PAYLOAD_LIMIT_BYTES);
    payload
}

/// Where the modal is in the upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Uploading,
    Done,
    Failed,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    /// The confirm button's label key; the run itself has none.
    pub fn action_key(self) -> Option<&'static str> {
        match self {
            Self::Uploading => None,
            Self::Done => Some("done"),
            Self::Failed => Some("retry"),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "the fixtures are ASCII; a decode failure is a test failure"
)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_keeps_the_end_of_the_buffer() {
        assert_eq!(tail_bytes(b"abcdef", 3), b"def");
        assert_eq!(tail_bytes(b"abc", 10), b"abc");
        assert_eq!(tail_bytes(b"", 10), b"");
        assert_eq!(tail_bytes(b"abc", 0), b"");
    }

    #[test]
    fn the_bundle_orders_its_sections_and_separates_them() {
        let payload = build_payload(
            b"summary",
            b"frontend line",
            &CoreLog::Available(b"core line".to_vec()),
        );
        let text = String::from_utf8(payload).expect("the fixture is text");
        assert_eq!(
            text,
            "summary\n===== frontend.log (tail) =====\nfrontend line\n\n===== core.log (tail) =====\ncore line"
        );
    }

    #[test]
    fn a_missing_core_log_says_why() {
        let payload = build_payload(
            b"summary\n",
            b"log\n",
            &CoreLog::Unavailable("not connected".into()),
        );
        let text = String::from_utf8(payload).expect("the fixture is text");
        assert!(text.ends_with("core.log unavailable: not connected\n"));
        // A summary that already ends in a newline gains no second one.
        assert!(text.starts_with("summary\n====="));
    }

    #[test]
    fn every_section_is_capped_and_the_bundle_fits_the_service_limit() {
        let big = vec![b'x'; PAYLOAD_LIMIT_BYTES];
        let payload = build_payload(&big, &big, &CoreLog::Available(big.clone()));
        assert!(payload.len() <= PAYLOAD_LIMIT_BYTES);
        // Summary, both log tails, the two separators and their newlines.
        let separators =
            "===== frontend.log (tail) =====\n".len() + "\n===== core.log (tail) =====\n".len() + 2;
        assert_eq!(
            payload.len(),
            SUPPORT_SUMMARY_LIMIT_BYTES + 2 * PER_LOG_LIMIT_BYTES + separators
        );
    }

    #[test]
    fn each_phase_names_its_button() {
        assert_eq!(Phase::Uploading.action_key(), None);
        assert_eq!(Phase::Done.action_key(), Some("done"));
        assert_eq!(Phase::Failed.action_key(), Some("retry"));
        assert_eq!(Phase::Uploading.as_str(), "uploading");
    }
}
