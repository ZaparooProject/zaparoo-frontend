// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use qrcode::{Color, QrCode as RustQrCode};
use std::pin::Pin;
use tracing::error;

#[derive(Debug, Default)]
pub struct QrCodeRust {
    content: QString,
    size: i32,
    rows: Vec<String>,
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
        #[qproperty(QString, content)]
        #[qproperty(i32, size)]
        type QrCode = super::QrCodeRust;

        #[qinvokable]
        fn generate(self: Pin<&mut QrCode>, content: QString);

        #[qinvokable]
        fn row_at(self: &QrCode, row: i32) -> QString;
    }
}

impl ffi::QrCode {
    fn generate(mut self: Pin<&mut Self>, content: QString) {
        let content_string = content.to_string();
        let rows = generate_rows(&content_string);
        let size = rows.len() as i32;
        self.as_mut().set_content(content);
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().set_size(size);
    }

    fn row_at(&self, row: i32) -> QString {
        if row < 0 {
            return QString::default();
        }
        self.rust()
            .rows
            .get(row as usize)
            .map_or_else(QString::default, |s| QString::from(s.as_str()))
    }
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
}
