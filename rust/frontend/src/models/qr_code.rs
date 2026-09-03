// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::models::action_error::report_action_error;
use cxx_qt_lib::{QString, QStringList};
use qrcode::{Color, QrCode as RustQrCode};
use std::pin::Pin;
use tracing::error;

#[derive(Debug, Default)]
pub struct QrCodeRust {
    content: QString,
    size: i32,
    /// One bit-string per matrix row, '1' = dark module.
    ///
    /// A `QStringList` qproperty rather than a `row_at(row)` invokable:
    /// `QrMatrix.qml` binds each delegate's bits to this, and a plain
    /// invokable carries no notify signal, so the binding's only
    /// dependency was the delegate's own row index. Two payloads that
    /// produce the same QR version leave `size` unchanged, the `Repeater`
    /// never rebuilds, and the matrix kept painting whichever content was
    /// generated first in the session -- so the log-upload code rendered
    /// the documentation URL, and two games could share one "Write with
    /// App" code. A notifying property makes the repaint automatic and
    /// removes the write-order trap along with it.
    rows: QStringList,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(QString, content)]
        #[qproperty(i32, size)]
        #[qproperty(QStringList, rows)]
        type QrCode = super::QrCodeRust;

        #[qinvokable]
        fn generate(self: Pin<&mut QrCode>, content: QString);
    }
}

impl ffi::QrCode {
    fn generate(mut self: Pin<&mut Self>, content: QString) {
        let content_string = content.to_string();
        let rows = generate_rows(&content_string);
        if rows.is_empty() && !content_string.is_empty() {
            report_action_error("qr_code", "");
        }
        let size = i32::try_from(rows.len()).unwrap_or(i32::MAX);
        self.as_mut().set_rows(vec_to_qstringlist(&rows));
        self.as_mut().set_size(size);
        self.as_mut().set_content(content);
    }
}

fn vec_to_qstringlist(v: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for s in v {
        list.append(QString::from(s.as_str()));
    }
    list
}

fn generate_rows(content: &str) -> Vec<String> {
    let code = match RustQrCode::new(content.as_bytes()) {
        Ok(code) => code,
        Err(e) => {
            error!("failed to generate QR code: {e}");
            return Vec::new();
        }
    };
    let width = code.width();
    let colors = code.to_colors();
    (0..width)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if colors[y * width + x] == Color::Dark {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::generate_rows;

    #[test]
    fn generated_matrix_is_square() {
        let rows = generate_rows("http://www.zaparoo.org");
        assert!(!rows.is_empty());
        let width = rows.len();
        assert!(rows.iter().all(|row| row.len() == width));
    }

    /// The shape that made the stale-matrix bug invisible: the docs URL and
    /// a log-upload URL are close enough in length to land on the same QR
    /// version, so `size` alone cannot tell a consumer that the payload
    /// changed. `rows` is the only signal, which is why it has to be a
    /// notifying property rather than an invokable read.
    #[test]
    fn same_version_payloads_differ_only_in_rows() {
        let docs = generate_rows("https://zaparoo.org/docs/frontend/");
        let log = generate_rows("https://logs.zaparoo.org/kf8s2xqv");
        assert!(!docs.is_empty() && !log.is_empty());
        assert_eq!(
            docs.len(),
            log.len(),
            "fixture payloads must share a QR version for this test to mean anything"
        );
        assert_ne!(
            docs, log,
            "different payloads must produce different modules"
        );
    }
}
