// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// HTTPS POSTs for standalone builds. The frontend has no HTTPS client of
// its own: it shells out to curl, which Core's installer requires. The
// body always goes in on stdin, never on the command line, so nothing in
// it (a password included) shows in the process list. Runs on a blocking
// thread, never the UI thread.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The CA bundle the `MiSTer` downloader keeps current. The stock bundle
/// dates from 2021 and no longer verifies current certificates, so curl is
/// pointed here when the file exists, as Core does for its own requests.
const MISTER_DOWNLOADER_CA_BUNDLE: &str = "/media/fat/Scripts/.config/downloader/cacert.pem";

/// One POST.
pub(crate) struct Post<'a> {
    pub url: &'a str,
    pub content_type: &'a str,
    pub body: &'a [u8],
    pub timeout: Duration,
    /// An HTTP error is a failure. Off, an error status still returns its
    /// body, for an API that answers a refusal in JSON.
    pub fail_with_body: bool,
    pub user_agent: Option<&'a str>,
}

/// The bundle curl should trust instead of its default, when there is one.
fn ca_bundle() -> Option<PathBuf> {
    let path = Path::new(MISTER_DOWNLOADER_CA_BUNDLE);
    (cfg!(feature = "mister") && path.is_file()).then(|| path.to_path_buf())
}

/// curl's arguments for one POST; the body arrives on stdin.
fn args(post: &Post<'_>, ca_bundle: Option<&Path>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--silent".into(), "--show-error".into()];
    if post.fail_with_body {
        args.push("--fail-with-body".into());
    }
    args.extend([
        "--max-time".into(),
        post.timeout.as_secs().to_string().into(),
        "-H".into(),
        format!("Content-Type: {}", post.content_type).into(),
    ]);
    if let Some(agent) = post.user_agent {
        args.push("--user-agent".into());
        args.push(agent.into());
    }
    args.extend(["--data-binary".into(), "@-".into()]);
    if let Some(path) = ca_bundle {
        args.push("--cacert".into());
        args.push(path.into());
    }
    args.push(post.url.into());
    args
}

/// Send the POST and return the response body, or why it failed (for the
/// log, never shown).
pub(crate) fn post(post: &Post<'_>) -> Result<String, String> {
    let mut child = Command::new("curl")
        .args(args(post, ca_bundle().as_deref()))
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
            .write_all(post.body)
            .map_err(|e| format!("curl write failed: {e}"))?;
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a missing argument is a test failure")]
mod tests {
    use super::*;

    fn upload() -> Post<'static> {
        Post {
            url: "https://logs.example/",
            content_type: "multipart/form-data; boundary=b",
            body: b"",
            timeout: Duration::from_secs(30),
            fail_with_body: true,
            user_agent: None,
        }
    }

    #[test]
    fn curl_uses_its_default_roots_without_a_bundle() {
        assert_eq!(
            args(&upload(), None),
            [
                "--silent",
                "--show-error",
                "--fail-with-body",
                "--max-time",
                "30",
                "-H",
                "Content-Type: multipart/form-data; boundary=b",
                "--data-binary",
                "@-",
                "https://logs.example/",
            ]
        );
    }

    #[test]
    fn curl_trusts_the_given_bundle() {
        let args = args(&upload(), Some(Path::new("/roots/cacert.pem")));
        let at = args.iter().position(|a| a == "--cacert");
        assert_eq!(at, Some(args.len() - 3));
        assert_eq!(args[args.len() - 2], "/roots/cacert.pem");
        assert_eq!(args[args.len() - 1], "https://logs.example/");
    }

    #[test]
    fn an_api_post_keeps_error_bodies_and_names_itself() {
        let post = Post {
            fail_with_body: false,
            user_agent: Some("ZaparooFrontend/1.0"),
            ..upload()
        };
        let args = args(&post, None);
        assert!(!args.iter().any(|a| a == "--fail-with-body"));
        let at = args.iter().position(|a| a == "--user-agent").unwrap();
        assert_eq!(args[at + 1], "ZaparooFrontend/1.0");
    }
}
