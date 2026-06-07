// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use zaparoo_core::media_types::TagInfo;

pub fn tag_display_value(tag: &TagInfo) -> String {
    let label = tag.label.trim();
    if label.is_empty() {
        tag.tag.trim().to_string()
    } else {
        label.to_string()
    }
}
