// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.ImageOverrides` — a stateless singleton that exposes the
// `crate::image_overrides` lookup to QML. The Hub builds its icon cover keys
// in QML (dynamic categories plus static action tiles), so it needs a
// QML-callable way to ask "is there a user override image for this id?".
//
// System artwork resolves entirely in Rust (`models::systems`) and does not
// go through this singleton.

use cxx_qt_lib::QString;

#[derive(Default)]
pub struct ImageOverridesRust;

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
        type ImageOverrides = super::ImageOverridesRust;

        /// Return the `"custom-image/{path}"` cover key for an override file
        /// in namespace `ns` (e.g. `"hub"`) matching `id`, or an empty string
        /// when no override is present. QML callers fall back to the bundled
        /// cover key on empty. The returned key is served as-is (no tint) by
        /// the `custom-image` image provider. (`ns`, not `namespace`, because
        /// the latter is a C++ keyword in the generated wrapper.)
        #[qinvokable]
        fn override_cover_key(self: &ImageOverrides, ns: &QString, id: &QString) -> QString;
    }
}

impl ffi::ImageOverrides {
    fn override_cover_key(&self, ns: &QString, id: &QString) -> QString {
        crate::image_overrides::override_path(&ns.to_string(), &id.to_string())
            .map_or_else(QString::default, |path| {
                QString::from(format!("custom-image/{}", path.display()).as_str())
            })
    }
}
