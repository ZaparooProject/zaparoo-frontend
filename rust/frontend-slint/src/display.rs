// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Display policy from Qt's `mister_runtime.rs` and `main.cpp`. Output timing,
//! framebuffer render size, and CRT scene size are separate quantities.

#[cfg(any(feature = "mister", test))]
pub fn automatic_size((width, height): (u32, u32)) -> (u32, u32) {
    const MAX_PIXELS: u64 = 1366 * 768;
    for divisor in 1..=4 {
        let size = ((width / divisor).max(1), (height / divisor).max(1));
        if u64::from(size.0) * u64::from(size.1) <= MAX_PIXELS || divisor == 4 {
            return size;
        }
    }
    unreachable!()
}

pub fn selectable_sizes(output: (u32, u32)) -> Vec<(u32, u32)> {
    (1..=4)
        .rev()
        .filter(|divisor| output.0.is_multiple_of(*divisor) && output.1.is_multiple_of(*divisor))
        .map(|divisor| (output.0 / divisor, output.1 / divisor))
        .filter(|&(width, height)| width > 0 && width <= 1920 && (720..=1080).contains(&height))
        .collect()
}

pub fn resolution_options(output: Option<(u32, u32)>) -> Vec<String> {
    std::iter::once(String::new())
        .chain(
            output
                .into_iter()
                .flat_map(selectable_sizes)
                .map(|(w, h)| format!("{w}x{h}")),
        )
        .collect()
}

pub fn bitmap_type(embedded: bool, crt: bool, height: u32) -> bool {
    crt || (embedded && height > 0 && height < 400)
}

pub fn motion_enabled(reduce_motion: bool, embedded: bool, crt: bool, height: u32) -> bool {
    !(reduce_motion || embedded && !crt && height >= 1080)
}

/// Keep numeric components untranslated; Slint owns the surrounding message.
pub fn register_labels(app: &crate::App) {
    use slint::ComponentHandle;
    let labels = app.global::<crate::SettingsLabels>();
    labels.on_resolution_width(|value| value.split_once('x').map_or("", |v| v.0).into());
    labels.on_resolution_height(|value| value.split_once('x').map_or("", |v| v.1).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_and_picker_sizes_match_qt_output_matrix() {
        for (output, automatic, choices) in [
            ((1920, 1080), (960, 540), vec!["", "1920x1080"]),
            ((2560, 1440), (1280, 720), vec!["", "1280x720"]),
            ((3840, 2160), (1280, 720), vec!["", "1280x720", "1920x1080"]),
            ((1366, 768), (1366, 768), vec!["", "1366x768"]),
            ((1280, 720), (1280, 720), vec!["", "1280x720"]),
            ((640, 480), (640, 480), vec![""]),
            ((352, 240), (352, 240), vec![""]),
        ] {
            assert_eq!(automatic_size(output), automatic);
            assert_eq!(resolution_options(Some(output)), choices);
        }
        assert_eq!(resolution_options(None), [""]);
    }

    #[test]
    fn font_and_motion_modes_match_qt_without_changing_preferences() {
        for height in [240, 399, 400, 480, 540, 720, 1080] {
            assert_eq!(bitmap_type(true, false, height), height < 400);
            assert!(!bitmap_type(false, false, height));
            assert!(bitmap_type(false, true, height));
            assert_eq!(motion_enabled(false, true, false, height), height < 1080);
            assert!(motion_enabled(false, false, false, height));
            assert!(motion_enabled(false, true, true, height));
            assert!(!motion_enabled(true, true, true, height));
        }
        assert!(!bitmap_type(true, false, 0));
    }

    #[cfg(feature = "mister")]
    #[test]
    fn resolution_labels_are_translated_and_automatic_is_not_blank(
    ) -> Result<(), slint::PlatformError> {
        use slint::ComponentHandle;
        assert!(
            slint::platform::set_platform(Box::new(crate::route_motion::ProbePlatform)).is_ok()
        );
        let app = crate::App::new()?;
        register_labels(&app);
        let labels = app.global::<crate::SettingsLabels>();
        assert_eq!(
            labels.invoke_value("resolution".into(), "".into()),
            "Automatic"
        );
        assert_eq!(
            labels.invoke_picker_value("resolution".into(), "".into()),
            "Automatic (Recommended)"
        );
        assert_eq!(
            labels.invoke_value("resolution".into(), "1920x1080".into()),
            "1920 × 1080"
        );
        assert_eq!(
            labels.invoke_picker_value("resolution".into(), "1920x1080".into()),
            "1920 × 1080 (Animations off)"
        );
        assert_eq!(
            labels.invoke_picker_value("resolution".into(), "1280x720".into()),
            "1280 × 720"
        );
        Ok(())
    }

    #[cfg(feature = "mister")]
    #[test]
    #[allow(clippy::float_cmp, reason = "integer font roles must match Qt exactly")]
    fn hdmi_font_roles_use_qt_ladder_while_crt_keeps_bitmap_type(
    ) -> Result<(), slint::PlatformError> {
        use slint::ComponentHandle;
        assert!(
            slint::platform::set_platform(Box::new(crate::route_motion::ProbePlatform)).is_ok()
        );
        let app = crate::App::new()?;
        let mut state = zaparoo_core::persist::PersistedState::default();
        for (width, height, crt, body, title) in [
            (960, 540, false, 17.0, 20.0),
            (1920, 1080, false, 28.0, 35.0),
            (352, 240, false, 8.0, 8.0),
            (352, 240, true, 8.0, 8.0),
        ] {
            for orientation in [
                crate::Orientation::Horizontal,
                crate::Orientation::Cw,
                crate::Orientation::Ccw,
            ] {
                state.settings.orientation = orientation.token().into();
                crate::seed_display_globals(&app, &state, crt, crt, (width, height));
                let (w, h) =
                    crate::scene_size(f64::from(width), f64::from(height), orientation, crt);
                crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, w, h, crt));
                let sizing = app.global::<crate::Sizing>();
                assert_eq!(sizing.get_font_body(), body);
                assert_eq!(sizing.get_font_title(), title);
                assert_eq!(
                    app.global::<crate::Theme>().get_font_ui(),
                    if height < 400 {
                        "MxPlus HP 100LX 6x8"
                    } else {
                        "Noto Sans"
                    }
                );
                assert_eq!(
                    app.global::<crate::Motion>().get_enabled(),
                    height < 1080 || crt
                );
                assert!(!state.settings.reduce_motion);
            }
        }
        Ok(())
    }
}
